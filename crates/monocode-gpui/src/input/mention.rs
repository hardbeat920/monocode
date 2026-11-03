//! Composer.tsx's file mention: the `@` is drawn as the file's icon, with a
//! little air before the path. The editor keeps the original `@` byte.
use super::*;

// An em blank for the icon and a thin one for the gap replace the `@` when
// shaping. Like the command slot, every caret, selection and hit test maps
// through `DisplayLayout`.
const MENTION_SLOT: &str = "\u{2003}\u{2009}";
const GROWTH: usize = MENTION_SLOT.len() - 1;
pub(super) const ICON_SIDE: f32 = 13.;
/// Bytes of the em blank the icon is centered in.
pub(super) const ICON_LEN: usize = '\u{2003}'.len_utf8();

#[derive(Clone)]
pub(super) enum MentionIcon {
    File(SharedString),
    Note,
}
impl MentionIcon {
    /// `@path`, `@"spaced path"`, or `@note/slug`.
    pub fn of(token: &str) -> Option<Self> {
        let path = token.strip_prefix('@')?;
        if path.starts_with("note/") {
            return Some(Self::Note);
        }
        let path = path
            .strip_prefix('"')
            .and_then(|p| p.strip_suffix('"'))
            .map_or_else(|| path.to_owned(), |p| p.replace("\\\"", "\""));
        let name = path.rsplit('/').next().unwrap_or(&path);
        Some(Self::File(
            format!("mi/{}.svg", crate::file_icons::file_icon_name(name)).into(),
        ))
    }
}

/// Source ↔ shaped-text offsets for the command slot and mention slots.
#[derive(Clone, Default)]
pub(super) struct DisplayLayout {
    pub command: Option<command::CommandLayout>,
    /// Source offsets of mention `@`s, ascending.
    pub mentions: Vec<(usize, MentionIcon)>,
}
impl DisplayLayout {
    fn pre(&self, at: usize) -> usize {
        self.command.map_or(at, |command| command.to_display(at))
    }
    pub fn to_display(&self, at: usize) -> usize {
        let passed = self.mentions.iter().filter(|(m, _)| *m < at).count();
        self.pre(at) + GROWTH * passed
    }
    pub fn to_source(&self, at: usize) -> usize {
        let mut shift = 0;
        for (m, _) in &self.mentions {
            let start = self.pre(*m) + shift;
            if at <= start {
                break;
            }
            if at < start + MENTION_SLOT.len() {
                // Inside the blank: snap to the nearer side of the `@`.
                return if at - start <= MENTION_SLOT.len() / 2 {
                    *m
                } else {
                    m + 1
                };
            }
            shift += GROWTH;
        }
        let at = at - shift;
        self.command.map_or(at, |command| command.to_source(at))
    }
    /// Shaped text with each mention `@` swapped for its slot.
    pub fn apply(&self, display: SharedString) -> SharedString {
        if self.mentions.is_empty() {
            return display;
        }
        let mut text = String::with_capacity(display.len() + GROWTH * self.mentions.len());
        let mut cursor = 0;
        for (m, _) in &self.mentions {
            let at = self.pre(*m);
            text.push_str(&display[cursor..at]);
            text.push_str(MENTION_SLOT);
            cursor = at + 1;
        }
        text.push_str(&display[cursor..]);
        text.into()
    }
}

impl Input {
    pub(super) fn display_layout(&self) -> DisplayLayout {
        let command = self.command_layout();
        if self.secret || self.content.is_empty() {
            // The placeholder still sits after a retained mode's slot.
            return DisplayLayout {
                command,
                mentions: Vec::new(),
            };
        }
        let mut mentions: Vec<_> = self
            .composer_tokens
            .iter()
            .filter(|range| {
                range.end <= self.content.len()
                    && self.content.is_char_boundary(range.start)
                    && self.content.is_char_boundary(range.end)
                    // A leading `/command` owns byte 0.
                    && !(range.start == 0 && command.is_some())
            })
            .filter_map(|range| Some((range.start, MentionIcon::of(&self.content[range.clone()])?)))
            .collect();
        mentions.sort_by_key(|(at, _)| *at);
        DisplayLayout { command, mentions }
    }
}
