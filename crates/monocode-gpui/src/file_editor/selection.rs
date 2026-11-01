//! EditorSelectionMenu + editorSelection.ts: share a compact file/line reference.
use super::{FileEditor, FileEvent};
use crate::{
    input::Input,
    popover::{Align, Anchor, AnchorSlot, Side, popover},
    theme::*,
};
use gpui::{
    AnyElement, Bounds, Context, Focusable, MouseButton, Pixels, Role, Size, Window, div,
    prelude::*, px,
};
use std::{ops::Range, path::Path};

#[derive(Clone)]
struct Target {
    range: Range<usize>,
    anchor: Bounds<Pixels>,
    reference: String,
}
#[derive(Default)]
pub(super) struct Selection {
    range: Range<usize>,
    target: Option<Target>,
    pending: bool,
    scheduled: bool,
    viewport: Option<Size<Pixels>>,
    surface: AnchorSlot,
}
impl Selection {
    pub(super) fn dismiss(&mut self) {
        self.target = None;
        self.pending = false;
    }
    fn observe(&mut self, input: &Input) {
        let range = input.selection_range();
        if self.range != range {
            self.range = range;
            self.target = None;
            self.pending = !self.range.is_empty();
        } else if self
            .target
            .as_ref()
            .is_some_and(|target| Some(target.anchor) != input.selection_anchor())
        {
            // Scrolling invalidates the captured viewport coordinates.
            self.dismiss();
        }
    }
}

fn format_reference(path: &str, start: usize, end: usize) -> String {
    let path = path.replace('\\', "/");
    let mentionable = !path.is_empty()
        && path.encode_utf16().count() <= 120
        && !path.starts_with('/')
        && !path.chars().any(|c| {
            c.is_whitespace()
                || c == '\u{feff}'
                || c == '@'
                || c <= '\u{1f}'
                || ('\u{7f}'..='\u{9f}').contains(&c)
        })
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    let file = if mentionable {
        format!("@{path}")
    } else {
        format!("`{}`", path.replace(['`', '\r', '\n'], "'"))
    };
    if start == end {
        format!("{file} (line {start})")
    } else {
        format!("{file} (lines {start}-{end})")
    }
}
fn reference_for_selection(path: &str, text: &str, range: Range<usize>) -> Option<String> {
    if range.is_empty()
        || range.end > text.len()
        || !text.is_char_boundary(range.start)
        || !text.is_char_boundary(range.end)
        || text[range.clone()].trim().is_empty()
    {
        return None;
    }
    let start = text.as_bytes()[..range.start]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1;
    // Count bytes rather than slicing at end-1, which can split a Unicode scalar.
    // Ending at the next line's column one does not include that next line.
    let end = text.as_bytes()[..range.end - 1]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1;
    Some(format_reference(path, start, end))
}

impl FileEditor {
    pub(super) fn observe_code_selection(&mut self, cx: &mut Context<Self>) {
        self.selection.observe(self.input.read(cx));
    }
    pub(super) fn finish_code_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.input.focus_handle(cx).is_focused(window) {
            return;
        }
        self.selection.observe(self.input.read(cx));
        // Existing pending work came from an actual range change. An outside
        // dismissal must not reopen on the following mouse-up.
        cx.notify();
    }
    fn capture_code_selection(&mut self, cx: &mut Context<Self>) {
        self.selection.scheduled = false;
        if !self.selection.pending {
            return;
        }
        self.selection.pending = false;
        if !self.visible || self.preview || self.document.is_none() {
            return;
        }
        let input = self.input.read(cx);
        if input.selection_dragging() {
            return;
        }
        let range = input.selection_range();
        let Some(anchor) = input.selection_anchor() else {
            return;
        };
        let path = Path::new(&self.path)
            .strip_prefix(&self.cwd)
            .unwrap_or_else(|_| Path::new(&self.path))
            .to_string_lossy();
        let Some(reference) = reference_for_selection(&path, &input.content, range.clone()) else {
            return;
        };
        self.selection.target = Some(Target {
            range,
            anchor,
            reference,
        });
        cx.notify();
    }
    pub(super) fn code_selection_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let viewport = window.viewport_size();
        if self
            .selection
            .viewport
            .is_some_and(|previous| previous != viewport)
        {
            self.selection.dismiss();
        }
        self.selection.viewport = Some(viewport);
        if !self.visible || self.preview || self.document.is_none() {
            return None;
        }
        if self.selection.pending
            && !self.selection.scheduled
            && !self.input.read(cx).selection_dragging()
        {
            // Layout first, then capture the caret after any scroll-to-selection.
            self.selection.scheduled = true;
            let view = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = view.update(cx, |this, cx| this.capture_code_selection(cx));
            });
        }
        let target = self.selection.target.clone()?;
        let anchor = AnchorSlot::default();
        anchor.set(Some(target.anchor));
        let weak = cx.entity().downgrade();
        Some(
            popover("editor-selection-menu", Anchor::Slot(anchor))
                .side(Side::Top)
                .align(Align::Center)
                .gap(6.)
                .child(
                    div()
                        .id("editor-selection-actions")
                        .role(Role::Toolbar)
                        .aria_label("Selected code actions")
                        .p_1()
                        .child(
                            div()
                                .id("editor-selection-chat")
                                .role(Role::Button)
                                .aria_label("Add to chat")
                                .tab_index(0)
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .h(px(28.))
                                .px_2()
                                .rounded(px(8.))
                                .text_size(px(13.))
                                .hover(|s| s.bg(content(0.05)))
                                .cursor_pointer()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(icon("comment-add", 14.))
                                .child("Add to chat")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    // The menu never mutates the file buffer or copies its contents.
                                    if this.input.read(cx).selection_range() == target.range {
                                        cx.emit(FileEvent::AddToChat(target.reference.clone()));
                                    }
                                    this.selection.dismiss();
                                    cx.notify();
                                })),
                        ),
                )
                .on_dismiss(move |_, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        this.selection.dismiss();
                        cx.notify();
                    });
                })
                .into_any_element(),
        )
    }
    pub(super) fn selection_surface(&self, cx: &Context<Self>) -> impl gpui::IntoElement + use<> {
        let slot = self.selection.surface.clone();
        let weak = cx.entity().downgrade();
        gpui::canvas(
            move |bounds, _, cx| {
                if slot.get().is_some_and(|old| old != bounds) {
                    let _ = weak.update(cx, |this, cx| {
                        let was_open = this.selection.target.is_some();
                        this.selection.dismiss();
                        if was_open {
                            cx.notify();
                        }
                    });
                }
                slot.set(Some(bounds));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn references_match_editor_selection_ts_for_mentions_and_quoted_paths() {
        assert_eq!(
            format_reference("src/FileEditor.tsx", 12, 13),
            "@src/FileEditor.tsx (lines 12-13)"
        );
        assert_eq!(
            format_reference("src/value.ts", 7, 7),
            "@src/value.ts (line 7)"
        );
        assert_eq!(
            format_reference("docs/read me.md", 3, 5),
            "`docs/read me.md` (lines 3-5)"
        );
        assert_eq!(
            format_reference("src\\value.ts", 1, 1),
            "@src/value.ts (line 1)"
        );
        assert_eq!(
            format_reference("/absolute/`file`\n.rs", 1, 2),
            "`/absolute/'file''.rs` (lines 1-2)"
        );
        assert_eq!(
            format_reference("../escape.rs", 1, 1),
            "`../escape.rs` (line 1)"
        );
        assert_eq!(format_reference("a@b.rs", 1, 1), "`a@b.rs` (line 1)");
    }
    #[test]
    fn exclusive_range_end_unicode_whitespace_and_unsaved_content_are_respected() {
        let text = "first\r\n🙂 selected\nlast\n";
        let start = text.find('🙂').unwrap();
        let end = text.find("last").unwrap();
        assert_eq!(
            reference_for_selection("src/current.rs", text, start..end).unwrap(),
            "@src/current.rs (line 2)"
        );
        assert_eq!(
            reference_for_selection("src/current.rs", text, 0..end + 1).unwrap(),
            "@src/current.rs (lines 1-3)"
        );
        assert!(reference_for_selection("file.rs", text, start..start + 1).is_none());
        assert!(reference_for_selection("file.rs", " \n\t", 0..3).is_none());
        assert!(reference_for_selection("file.rs", "text", 1..1).is_none());
        assert!(reference_for_selection("file.rs", "text", 0..10).is_none());
    }
}
