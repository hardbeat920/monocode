//! Port of src/features/sessions/ui/TranscriptFind.tsx: the find bar over a
//! conversation. Cmd-F (or the "Editor: Find" binding) opens it, Enter and
//! Shift-Enter, F3, and Cmd-G step through the matches, Escape closes it.
//!
//! The bar takes the session's blocks and reports the match to show as a
//! [`TranscriptFindEvent`]; the host passes it to
//! [`crate::transcript::TranscriptView::navigate_to_block`]. The host also
//! forwards key presses from its pane through [`TranscriptFind::handle_key`],
//! the way the React component listened on the window.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    Keystroke, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_core::transcript::BlockRef;
use monocode_core::transcript::find::find_transcript_blocks;
use monocode_ui::styled::{UiStyled as _, glass_backdrop};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, icon, u};

/// Which edge of the pane the bar sits at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FindSide {
    Left,
    #[default]
    Right,
}

/// What the bar asks the transcript to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptFindEvent {
    /// `onNavigate(blockId, query)`. `None` clears the search.
    Navigate {
        block_id: Option<String>,
        query: String,
    },
}

/// The count label: "2 of 5", "No results", or nothing for an empty query.
pub fn match_label(query: &str, matches: usize, active: usize) -> String {
    if monocode_core::js::trim(query).is_empty() {
        return String::new();
    }
    if matches == 0 {
        return "No results".into();
    }
    format!("{} of {matches}", active.min(matches - 1) + 1)
}

/// `<TranscriptFind blocks visible focused onNavigate side />`.
pub struct TranscriptFind {
    blocks: Vec<BlockRef>,
    visible: bool,
    open: bool,
    query: String,
    active: usize,
    matches: Vec<String>,
    side: FindSide,
    input: Entity<InputState>,
    /// The last selection reported, so a redraw does not repeat it.
    reported: Option<(Option<String>, String)>,
    _input: Subscription,
}

impl EventEmitter<TranscriptFindEvent> for TranscriptFind {}

impl TranscriptFind {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in conversation"));
        let subscription =
            cx.subscribe_in(&input, window, |this, input, event, _, cx| match event {
                InputEvent::Change => {
                    let query = input.read(cx).value().to_string();
                    this.set_query(query, cx);
                }
                InputEvent::PressEnter { shift, .. } => {
                    this.step(if *shift { -1 } else { 1 }, cx);
                }
                InputEvent::Focus | InputEvent::Blur => {}
            });
        Self {
            blocks: Vec::new(),
            visible: true,
            open: false,
            query: String::new(),
            active: 0,
            matches: Vec::new(),
            side: FindSide::Right,
            input,
            reported: None,
            _input: subscription,
        }
    }

    pub fn set_blocks(&mut self, blocks: Vec<BlockRef>, cx: &mut Context<Self>) {
        self.blocks = blocks;
        self.refresh(cx);
    }

    /// False while another tab is in front.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible != visible {
            self.visible = visible;
            self.report(cx);
            cx.notify();
        }
    }

    pub fn set_side(&mut self, side: FindSide, cx: &mut Context<Self>) {
        self.side = side;
        cx.notify();
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn matches(&self) -> &[String] {
        &self.matches
    }

    /// The match on screen.
    pub fn selected(&self) -> Option<&str> {
        if self.matches.is_empty() {
            return None;
        }
        self.matches
            .get(self.active.min(self.matches.len() - 1))
            .map(String::as_str)
    }

    /// `openFind`: show the bar with the query selected.
    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open(cx);
        self.input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }

    /// Show the bar without moving focus.
    pub fn open(&mut self, cx: &mut Context<Self>) {
        self.open = true;
        self.report(cx);
        cx.notify();
    }

    /// `closeFind`.
    pub fn close_find(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        self.reported = None;
        cx.emit(TranscriptFindEvent::Navigate {
            block_id: None,
            query: String::new(),
        });
        cx.notify();
    }

    /// `step`.
    pub fn step(&mut self, direction: i32, cx: &mut Context<Self>) {
        let count = self.matches.len() as i32;
        if count == 0 {
            return;
        }
        self.active = ((self.active as i32 + direction + count) % count) as usize;
        self.report(cx);
        cx.notify();
    }

    /// Put `query` in the field and search for it.
    pub fn search(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        let text = query.to_string();
        self.input
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.set_query(query.to_string(), cx);
    }

    /// The query typed into the field.
    pub fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        self.query = query;
        self.active = 0;
        self.refresh(cx);
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.matches = find_transcript_blocks(&self.blocks, &self.query);
        self.report(cx);
        cx.notify();
    }

    /// The effect that navigates whenever the selection or query changes
    /// while the bar is open and visible.
    fn report(&mut self, cx: &mut Context<Self>) {
        if !self.open || !self.visible {
            return;
        }
        let current = (self.selected().map(str::to_string), self.query.clone());
        if self.reported.as_ref() == Some(&current) {
            return;
        }
        self.reported = Some(current.clone());
        cx.emit(TranscriptFindEvent::Navigate {
            block_id: current.0,
            query: current.1,
        });
    }

    /// The window key listener: returns whether the key was used. `focused`
    /// is whether the pane has focus; `find_binding` is the user's
    /// "Editor: Find" override, if any.
    pub fn handle_key(
        &mut self,
        keystroke: &Keystroke,
        focused: bool,
        find_binding: Option<&Keystroke>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.visible || !focused {
            return false;
        }
        let modifiers = keystroke.modifiers;
        let command = modifiers.platform || modifiers.control;
        let key = keystroke.key.to_lowercase();
        let default_find = command && !modifiers.alt && !modifiers.shift && key == "f";
        let find_pressed = match find_binding {
            Some(binding) => binding.modifiers == modifiers && binding.key == keystroke.key,
            None => default_find,
        };
        if find_pressed {
            self.open_find(window, cx);
            return true;
        }
        if default_find {
            return true;
        }
        if self.open && (key == "f3" || (command && !modifiers.alt && key == "g")) {
            self.step(if modifiers.shift { -1 } else { 1 }, cx);
            return true;
        }
        if self.open && key == "escape" {
            self.close_find(cx);
            return true;
        }
        false
    }
}

impl Render for TranscriptFind {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible || !self.open {
            return div().into_any_element();
        }
        let theme = Theme::of(cx).clone();
        let label = match_label(&self.query, self.matches.len(), self.active);
        let has_matches = !self.matches.is_empty();
        let button = |id: &'static str, glyph: IconName, label: &'static str, enabled: bool| {
            div()
                .id(id)
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(u(24.))
                .rounded(u(4.))
                .tooltip(tooltip(label))
                .when(!enabled, |el| el.opacity(0.3))
                .when(enabled, |el| {
                    el.cursor_pointer().hover(|s| s.bg(theme.content(0.1)))
                })
                .child(icon(glyph).size(u(14.)).text_color(theme.content(0.55)))
        };
        let bar = div()
            .relative()
            .flex()
            .items_center()
            .gap(u(4.))
            .w(u(360.))
            .rounded(u(8.))
            .border_1()
            .border_color(theme.content(0.1))
            .p(u(4.))
            .shadow_lg()
            .child(glass_backdrop(8., 24., theme.content(0.05)))
            .child(
                icon(IconName::Search)
                    .ml(u(4.))
                    .size(u(14.))
                    .flex_none()
                    .text_color(theme.content(0.5)),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .w(u(176.))
                    .text_px(12.)
                    .line_height(u(16.))
                    .child(super::util::bare_input(
                        &self.input,
                        theme.content(0.4),
                        &theme,
                        cx,
                    )),
            )
            .child(
                div()
                    .relative()
                    .flex_none()
                    .min_w(u(66.))
                    .flex()
                    .justify_end()
                    .font_family(theme.fonts.mono.clone())
                    .text_px(11.)
                    .tabular()
                    .text_color(theme.content(0.5))
                    .child(label),
            )
            .child(
                button(
                    "find-previous",
                    IconName::ChevronUp,
                    "Previous match",
                    has_matches,
                )
                .when(has_matches, |el| {
                    el.on_click(cx.listener(|this, _, _, cx| this.step(-1, cx)))
                }),
            )
            .child(
                button(
                    "find-next",
                    IconName::ChevronDown,
                    "Next match",
                    has_matches,
                )
                .when(has_matches, |el| {
                    el.on_click(cx.listener(|this, _, _, cx| this.step(1, cx)))
                }),
            )
            .child(
                button("find-close", IconName::X, "Close find", true)
                    .on_click(cx.listener(|this, _, _, cx| this.close_find(cx))),
            );
        let side = self.side;
        div()
            .absolute()
            .top(u(8.))
            .map(|el| match side {
                FindSide::Left => el.left(u(12.)),
                FindSide::Right => el.right(u(12.)),
            })
            .font_family(theme.fonts.sans.clone())
            .child(bar)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_the_match_count() {
        assert_eq!(match_label("", 3, 0), "");
        assert_eq!(match_label("  ", 3, 0), "");
        assert_eq!(match_label("cat", 0, 0), "No results");
        assert_eq!(match_label("cat", 3, 1), "2 of 3");
        assert_eq!(match_label("cat", 3, 7), "3 of 3");
    }
}
