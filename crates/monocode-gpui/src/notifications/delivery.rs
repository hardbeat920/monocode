//! Shared, side-effect-free notification delivery policy. The host owns OS
//! permission requests, dispatch, settings persistence, and opening sessions.

use std::collections::HashSet;

use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Permission {
    Prompt,
    Granted,
    Denied,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectRule {
    pub enabled: bool,
    /// Last suppressed millisecond. A scheduled mute expires at its deadline.
    pub after: u64,
}

impl Default for ProjectRule {
    fn default() -> Self {
        Self {
            enabled: true,
            after: 0,
        }
    }
}

impl ProjectRule {
    /// Read the exact React preference schema, including indefinite null mutes.
    /// Invalid fields are ignored, as in loadNotificationPreferences.
    pub fn from_preferences(preferences: &Value, project: &str, category: &str) -> Self {
        let preference = &preferences[project];
        let disabled = preference["disabled"]
            .as_array()
            .is_some_and(|values| values.iter().any(|value| value.as_str() == Some(category)));
        Self {
            enabled: preference.get("mutedUntil") != Some(&Value::Null) && !disabled,
            after: timestamp(&preference["resumedAt"])
                .max(timestamp(&preference["mutedUntil"]).saturating_sub(1))
                .max(timestamp(&preference["enabledAfter"][category])),
        }
    }

    pub fn allows(self, occurred_at: u64, now: u64) -> bool {
        self.enabled && now > self.after && occurred_at > self.after
    }
}

fn timestamp(value: &Value) -> u64 {
    value
        .as_f64()
        .filter(|number| number.is_finite() && *number >= 0.0)
        .map(|number| number.min(u64::MAX as f64) as u64)
        .unwrap_or(0)
}

pub fn should_notify(
    enabled: bool,
    permission: Permission,
    window_focused: bool,
    session_visible: bool,
) -> bool {
    enabled
        && !(window_focused && session_visible)
        && matches!(permission, Permission::Prompt | Permission::Granted)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    Session(i64),
    /// Preserve the due timestamp when clearing: stale clicks must never clear a
    /// newer reminder. The host must clear with Some(due_at) before opening.
    Reminder {
        session_id: i64,
        due_at: u64,
    },
}

impl Target {
    /// Sequence prevents replacement of an earlier banner for this session.
    pub fn tag(self, sequence: u64) -> String {
        match self {
            Self::Session(id) => format!("monocode-session:{id}:{sequence}"),
            Self::Reminder { session_id, due_at } => {
                format!("monocode-reminder:{session_id}:{due_at}")
            }
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        if let Some(rest) = tag.strip_prefix("monocode-session:") {
            let (id, sequence) = rest.split_once(':')?;
            sequence.parse::<u64>().ok()?;
            return Some(Self::Session(id.parse().ok()?));
        }
        let (id, due) = tag.strip_prefix("monocode-reminder:")?.split_once(':')?;
        Some(Self::Reminder {
            session_id: id.parse().ok()?,
            due_at: due.parse().ok()?,
        })
    }
}

/// First nonempty paragraph with collapsed whitespace, capped at 240 UTF-16
/// code units like the web UI. Never split a Unicode scalar while truncating.
pub fn clip_body(text: &str) -> String {
    let mut paragraph = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !paragraph.is_empty() {
                break;
            }
        } else {
            paragraph.extend(line.split_whitespace());
        }
    }
    let body = paragraph.join(" ");
    if body.encode_utf16().count() <= 240 {
        return body;
    }
    let mut used = 0;
    let mut clipped = String::new();
    for character in body.chars() {
        used += character.len_utf16();
        if used > 239 {
            break;
        }
        clipped.push(character);
    }
    clipped.push('…');
    clipped
}

/// Count sessions that need input, never individual requests. The badge is
/// independent of notification opt-in, project muting, and visible sessions.
pub fn dock_badge_count(sessions: impl IntoIterator<Item = (i64, bool)>) -> usize {
    sessions
        .into_iter()
        .filter_map(|(id, needs_input)| needs_input.then_some(id))
        .collect::<HashSet<_>>()
        .len()
}
