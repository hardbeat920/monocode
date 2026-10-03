//! Port of src/features/inbox/model/linkedSessionSeen.ts: the remote
//! snapshot the user acknowledged per linked session, stored under
//! `monocode.linkedSessionSeen` and capped at 500 sessions.
//!
//! localStorage becomes `Kv` with the same key and JSON. Listeners become
//! `InboxSignal::LinkedSessionSeen` on the `InboxClient`.

use monocode_settings::Kv;
use serde_json::Value;

use super::client::{InboxClient, InboxSignal};

/// The localStorage key.
pub const LINKED_SESSION_SEEN_KEY: &str = "monocode.linkedSessionSeen";
const MAX_ENTRIES: usize = 500;

fn load_seen_map(kv: &Kv) -> Vec<(String, f64)> {
    let Some(raw) = kv
        .get_item(LINKED_SESSION_SEEN_KEY)
        .filter(|raw| !raw.is_empty())
    else {
        return Vec::new();
    };
    let Ok(Value::Object(object)) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    object
        .into_iter()
        .filter(|(key, _)| !key.is_empty())
        .filter_map(|(key, value)| {
            value
                .as_f64()
                .filter(|value| value.is_finite())
                .map(|value| (key, value))
        })
        .collect()
}

fn number_json(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 9.0e15 {
        (value as i64).to_string()
    } else {
        serde_json::Number::from_f64(value)
            .map(|number| number.to_string())
            .unwrap_or_else(|| "null".into())
    }
}

/// `linkedSessionSeenAt`.
pub fn linked_session_seen_at(kv: &Kv, session_id: &str) -> i64 {
    load_seen_map(kv)
        .into_iter()
        .find(|(key, _)| key == session_id)
        .map(|(_, value)| value as i64)
        .unwrap_or(0)
}

/// `markLinkedSessionUpdateSeen` without the listener call: remember the
/// exact remote snapshot acknowledged for this session. Returns whether it
/// wrote.
pub fn mark_linked_session_update_seen(kv: &Kv, session_id: &str, remote_updated_at: i64) -> bool {
    if session_id.is_empty() {
        return false;
    }
    let mut entries = load_seen_map(kv);
    let remote = remote_updated_at as f64;
    match entries.iter_mut().find(|(key, _)| key == session_id) {
        Some(entry) => entry.1 = entry.1.max(remote),
        None => entries.push((session_id.to_string(), remote.max(0.0))),
    }
    entries.sort_by(|left, right| right.1.total_cmp(&left.1));
    entries.truncate(MAX_ENTRIES);
    let body: Vec<String> = entries
        .iter()
        .map(|(key, value)| format!("{}:{}", Value::String(key.clone()), number_json(*value)))
        .collect();
    kv.set_item(LINKED_SESSION_SEEN_KEY, &format!("{{{}}}", body.join(",")));
    true
}

impl InboxClient {
    /// `linkedSessionSeenAt`.
    pub fn linked_session_seen_at(&self, session_id: &str) -> i64 {
        linked_session_seen_at(self.kv(), session_id)
    }

    /// `markLinkedSessionUpdateSeen`.
    pub fn mark_linked_session_update_seen(&self, session_id: &str, remote_updated_at: i64) {
        if mark_linked_session_update_seen(self.kv(), session_id, remote_updated_at) {
            self.emit(InboxSignal::LinkedSessionSeen);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_the_newest_acknowledged_remote_update_per_session() {
        let kv = Kv::in_memory();
        mark_linked_session_update_seen(&kv, "session-1", 200);
        mark_linked_session_update_seen(&kv, "session-1", 150);
        mark_linked_session_update_seen(&kv, "session-2", 300);
        assert_eq!(linked_session_seen_at(&kv, "session-1"), 200);
        assert_eq!(linked_session_seen_at(&kv, "session-2"), 300);
        assert_eq!(linked_session_seen_at(&kv, "other"), 0);
        assert_eq!(
            kv.get_item(LINKED_SESSION_SEEN_KEY).unwrap(),
            r#"{"session-2":300,"session-1":200}"#
        );
    }

    #[test]
    fn keeps_the_newest_five_hundred_sessions() {
        let kv = Kv::in_memory();
        for index in 0..505 {
            mark_linked_session_update_seen(&kv, &format!("s{index}"), index);
        }
        assert_eq!(linked_session_seen_at(&kv, "s4"), 0);
        assert_eq!(linked_session_seen_at(&kv, "s5"), 5);
        assert_eq!(load_seen_map(&kv).len(), 500);
    }
}
