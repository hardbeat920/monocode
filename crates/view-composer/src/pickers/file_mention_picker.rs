//! Port of src/features/sessions/ui/FileMentionPicker.tsx: the `@` mention
//! list of files, folders, and notes, with the fuzzy-matched characters in
//! the accent color.
//!
//! It is a controlled element: the composer owns the query, the ranked
//! files, and the highlighted row. Hovering a row only highlights it after
//! the pointer moves, which GPUI's hover events already guarantee: they
//! fire on mouse moves, not when keyboard scrolling slides a row under a
//! still pointer.

use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use monocode_ui::styled::glass_backdrop;
use monocode_ui::{IconName, Theme, UiStyled as _, file_type_icon, folder_type_icon, icon, u};

use super::match_text::match_text;

/// `NOTE_PATH_PREFIX`.
pub const NOTE_PATH_PREFIX: &str = "note:";

/// `isNoteMentionPath`.
pub fn is_note_mention_path(path: &str) -> bool {
    path.starts_with(NOTE_PATH_PREFIX)
}

/// A ranked file, from `RankedFile`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionFile {
    /// Absolute path, or `note:<id>` for a note.
    pub path: SharedString,
    /// Path relative to the project, or the note's mention label.
    pub relative: SharedString,
    pub name: SharedString,
    pub is_dir: bool,
    /// Matched positions in `relative`, in UTF-16 units.
    pub positions: Vec<usize>,
}

/// How one row splits into a name and a directory, with the matched
/// positions of each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionRowParts {
    pub note: bool,
    pub dir: String,
    pub name_positions: Vec<usize>,
    pub dir_positions: Vec<usize>,
}

/// The row split `FileMentionPicker` renders.
pub fn mention_row_parts(file: &MentionFile) -> MentionRowParts {
    let note = is_note_mention_path(&file.path);
    let units: Vec<u16> = file.relative.encode_utf16().collect();
    let slash = units.iter().rposition(|unit| *unit == u16::from(b'/'));
    let dir = match (note, slash) {
        (false, Some(slash)) => String::from_utf16_lossy(&units[..slash]),
        _ => String::new(),
    };
    let name_offset = slash.map(|slash| slash + 1).unwrap_or(0);
    let name_positions = if note {
        file.positions.clone()
    } else {
        file.positions
            .iter()
            .filter(|pos| **pos >= name_offset)
            .map(|pos| pos - name_offset)
            .collect()
    };
    let dir_positions = match slash {
        Some(slash) => file
            .positions
            .iter()
            .copied()
            .filter(|pos| *pos < slash)
            .collect(),
        None => Vec::new(),
    };
    MentionRowParts {
        note,
        dir,
        name_positions,
        dir_positions,
    }
}

/// The line shown when nothing matches.
pub fn empty_mention_message(query: &str, loading: bool, include_notes: bool) -> &'static str {
    if loading {
        "Indexing files…"
    } else if !monocode_core::js::trim(query).is_empty() {
        if include_notes {
            "No matching files or notes"
        } else {
            "No matching files or folders"
        }
    } else if include_notes {
        "No files or notes found"
    } else {
        "No files or folders found"
    }
}

type IndexFn = Rc<dyn Fn(usize, &mut Window, &mut App)>;
type FileFn = Rc<dyn Fn(&MentionFile, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct FileMentionPicker {
    id: ElementId,
    files: Vec<MentionFile>,
    query: SharedString,
    active: usize,
    loading: bool,
    include_notes: bool,
    on_active: Option<IndexFn>,
    on_pick: Option<FileFn>,
}

pub fn file_mention_picker(
    id: impl Into<ElementId>,
    files: Vec<MentionFile>,
    query: impl Into<SharedString>,
    active: usize,
) -> FileMentionPicker {
    FileMentionPicker {
        id: id.into(),
        files,
        query: query.into(),
        active,
        loading: false,
        include_notes: false,
        on_active: None,
        on_pick: None,
    }
}

impl FileMentionPicker {
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn include_notes(mut self, include: bool) -> Self {
        self.include_notes = include;
        self
    }

    pub fn on_active(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_active = Some(Rc::new(f));
        self
    }

    pub fn on_pick(mut self, f: impl Fn(&MentionFile, &mut Window, &mut App) + 'static) -> Self {
        self.on_pick = Some(Rc::new(f));
        self
    }
}

/// Scroll state kept across frames for a controlled list.
pub(crate) struct ListScroll {
    pub handle: ScrollHandle,
    pub last_active: Option<usize>,
}

/// Scrolls the active row into view when it changes.
pub(crate) fn follow_active(
    id: &ElementId,
    active: usize,
    window: &mut Window,
    cx: &mut App,
) -> ScrollHandle {
    let state = window.use_keyed_state(id.clone(), cx, |_, _| ListScroll {
        handle: ScrollHandle::new(),
        last_active: None,
    });
    state.update(cx, |state, _| {
        if state.last_active != Some(active) {
            state.last_active = Some(active);
            state.handle.scroll_to_item(active);
        }
        state.handle.clone()
    })
}

impl RenderOnce for FileMentionPicker {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let frame = div()
            .relative()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(u(theme.radius.lg))
            .border_1()
            .border_color(theme.content(0.10))
            .font_family(theme.fonts.sans.clone())
            .debug_selector(|| "file-mention-picker".into())
            .child(glass_backdrop(theme.radius.lg, 24., theme.content(0.05)));
        if self.files.is_empty() {
            return frame.child(
                div()
                    .relative()
                    .px(u(12.))
                    .py(u(10.))
                    .text_px(theme.text.label)
                    .text_color(theme.content(0.50))
                    .child(empty_mention_message(
                        &self.query,
                        self.loading,
                        self.include_notes,
                    )),
            );
        }
        let scroll = follow_active(&self.id, self.active, window, cx);
        let searching = !monocode_core::js::trim(&self.query).is_empty();
        let mut list = div()
            .id(self.id)
            .relative()
            .flex()
            .flex_col()
            .max_h(u(240.))
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .px(u(4.))
            .py(u(4.));
        for (index, file) in self.files.into_iter().enumerate() {
            let highlighted = index == self.active;
            let parts = mention_row_parts(&file);
            let glyph = if parts.note {
                icon(IconName::StickyNote)
                    .size(u(14.))
                    .text_color(theme.colors.content)
                    .into_any_element()
            } else if file.is_dir {
                folder_type_icon(file.name.clone(), false, false)
                    .size(15.)
                    .into_any_element()
            } else {
                file_type_icon(file.name.clone())
                    .size(15.)
                    .into_any_element()
            };
            let label = match_text(file.name.clone(), parts.name_positions, searching)
                .suffix(if file.is_dir { "/" } else { "" });
            let mut name = div().flex_1().min_w_0().truncate().child(label);
            if highlighted {
                name = name.text_color(theme.colors.mention);
            }
            let mut row = div()
                .id(("mention-row", index))
                .debug_selector({
                    let path = file.relative.clone();
                    move || format!("mention-row-{path}")
                })
                .flex()
                .flex_none()
                .w_full()
                .items_center()
                .gap(u(8.))
                .h(u(32.))
                .px(u(8.))
                .rounded(u(theme.radius.md))
                .text_px(theme.text.body)
                .leading(theme.leading.none)
                .text_color(theme.colors.content)
                .child(
                    div()
                        .flex_none()
                        .text_color(theme.colors.content)
                        .child(glyph),
                )
                .child(name);
            if highlighted {
                row = row.bg(theme.colors.selection);
            }
            if parts.note {
                row = row.child(
                    div()
                        .flex_none()
                        .font_family(theme.fonts.mono.clone())
                        .text_px(theme.text.caption)
                        .text_color(theme.content(0.40))
                        .child("Note"),
                );
            } else if !parts.dir.is_empty() {
                row = row.child(
                    div()
                        .min_w_0()
                        .max_w(gpui::relative(0.45))
                        .truncate()
                        .font_family(theme.fonts.mono.clone())
                        .text_px(theme.text.caption)
                        .text_color(theme.content(0.40))
                        .child(match_text(parts.dir, parts.dir_positions, searching)),
                );
            }
            if let Some(on_active) = self.on_active.clone() {
                row = row.on_hover(move |hovered, window, cx| {
                    if *hovered {
                        on_active(index, window, cx);
                    }
                });
            }
            if let Some(on_pick) = self.on_pick.clone() {
                row = row.on_click(move |_, window, cx| on_pick(&file, window, cx));
            }
            list = list.child(row);
        }
        frame.child(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(relative: &str, positions: Vec<usize>) -> MentionFile {
        let name = relative.rsplit('/').next().unwrap_or(relative);
        MentionFile {
            path: format!("/repo/{relative}").into(),
            relative: relative.to_string().into(),
            name: name.to_string().into(),
            is_dir: false,
            positions,
        }
    }

    #[test]
    fn splits_matches_between_the_name_and_the_directory() {
        let parts = mention_row_parts(&file("src/lib/main.rs", vec![0, 4, 8, 9]));
        assert!(!parts.note);
        assert_eq!(parts.dir, "src/lib");
        assert_eq!(parts.name_positions, vec![0, 1]);
        assert_eq!(parts.dir_positions, vec![0, 4]);
    }

    #[test]
    fn notes_keep_every_position_on_the_name() {
        let note = MentionFile {
            path: "note:abc".into(),
            relative: "note/auth".into(),
            name: "note/auth".into(),
            is_dir: false,
            positions: vec![0, 5],
        };
        let parts = mention_row_parts(&note);
        assert!(parts.note);
        assert_eq!(parts.dir, "");
        assert_eq!(parts.name_positions, vec![0, 5]);
    }

    #[test]
    fn empty_messages_cover_loading_search_and_notes() {
        assert_eq!(empty_mention_message("x", true, false), "Indexing files…");
        assert_eq!(
            empty_mention_message(" a ", false, false),
            "No matching files or folders"
        );
        assert_eq!(
            empty_mention_message("a", false, true),
            "No matching files or notes"
        );
        assert_eq!(
            empty_mention_message("", false, true),
            "No files or notes found"
        );
        assert_eq!(
            empty_mention_message("", false, false),
            "No files or folders found"
        );
    }
}
