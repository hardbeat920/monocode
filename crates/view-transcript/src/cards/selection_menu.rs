//! Port of src/features/sessions/ui/TranscriptSelectionMenu.tsx: a small
//! toolbar over selected transcript text with "Add to chat" and "Add to
//! notes".
//!
//! Adding to chat is immediate. Saving a note can fail, so "Add to notes"
//! waits for the host's [`TranscriptSelectionMenu::finish_note`]: success
//! closes the menu; an error stays on screen with the button usable again,
//! so the selected text is not lost.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Anchor, AnyElement, Bounds, Context, EventEmitter, InteractiveElement as _, IntoElement,
    KeyDownEvent, ParentElement as _, Pixels, Render, StatefulInteractiveElement as _, Styled as _,
    Window, div, point, px,
};
use monocode_ui::styled::UiStyled as _;
use monocode_ui::widgets::{PopoverSide, popover_at, popover_frame};
use monocode_ui::{IconName, Theme, icon, u};

/// `TranscriptSelection`: the selected text and where it sits.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptSelection {
    pub text: String,
    /// The selection's box in window coordinates. The menu sits above it.
    pub rect: Bounds<Pixels>,
}

/// Which action a button runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionAction {
    AddToChat,
    AddToNotes,
}

/// What the reader picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionMenuEvent {
    /// `onAddToChat(text)`.
    AddToChat { text: String },
    /// `onAddToNotes(text)`. Answer with [`TranscriptSelectionMenu::finish_note`].
    AddToNotes { text: String },
    /// `onDismiss`: the menu closed. Clear the text selection when
    /// `clear_selection` (Escape, or an action finished).
    Dismiss { clear_selection: bool },
}

/// `<TranscriptSelectionMenu selection onAddToChat onAddToNotes onDismiss />`.
pub struct TranscriptSelectionMenu {
    selection: Option<TranscriptSelection>,
    chat: bool,
    notes: bool,
    pending: Option<SelectionAction>,
    error: Option<String>,
}

impl EventEmitter<SelectionMenuEvent> for TranscriptSelectionMenu {}

impl TranscriptSelectionMenu {
    /// `chat` and `notes`: which actions the host offers.
    pub fn new(chat: bool, notes: bool) -> Self {
        Self {
            selection: None,
            chat,
            notes,
            pending: None,
            error: None,
        }
    }

    pub fn set_actions(&mut self, chat: bool, notes: bool, cx: &mut Context<Self>) {
        self.chat = chat;
        self.notes = notes;
        cx.notify();
    }

    /// Show the menu for a selection, or hide it with `None`.
    pub fn set_selection(
        &mut self,
        selection: Option<TranscriptSelection>,
        cx: &mut Context<Self>,
    ) {
        if self.selection != selection {
            self.pending = None;
            self.error = None;
        }
        self.selection = selection;
        cx.notify();
    }

    pub fn selection(&self) -> Option<&TranscriptSelection> {
        self.selection.as_ref()
    }

    pub fn is_open(&self) -> bool {
        self.selection.is_some() && (self.chat || self.notes)
    }

    pub fn pending(&self) -> Option<SelectionAction> {
        self.pending
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Run an action, as clicking its button does.
    pub fn pick(&mut self, action: SelectionAction, cx: &mut Context<Self>) {
        if self.pending.is_some() {
            return;
        }
        let Some(selection) = self.selection.clone() else {
            return;
        };
        self.error = None;
        match action {
            SelectionAction::AddToChat if self.chat => {
                cx.emit(SelectionMenuEvent::AddToChat {
                    text: selection.text,
                });
                self.dismiss(true, cx);
            }
            SelectionAction::AddToNotes if self.notes => {
                self.pending = Some(action);
                cx.emit(SelectionMenuEvent::AddToNotes {
                    text: selection.text,
                });
                cx.notify();
            }
            _ => {}
        }
    }

    /// The note save finished. An error keeps the menu and the selection.
    pub fn finish_note(&mut self, result: Result<(), String>, cx: &mut Context<Self>) {
        if self.pending != Some(SelectionAction::AddToNotes) {
            return;
        }
        self.pending = None;
        match result {
            Ok(()) => self.dismiss(true, cx),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    /// Close the menu. Scrolling and resizing move the text out from under
    /// it, so the host dismisses it then too.
    pub fn dismiss(&mut self, clear_selection: bool, cx: &mut Context<Self>) {
        self.selection = None;
        self.pending = None;
        self.error = None;
        cx.emit(SelectionMenuEvent::Dismiss { clear_selection });
        cx.notify();
    }

    fn action_button(
        &self,
        action: SelectionAction,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (id, label, glyph) = match action {
            SelectionAction::AddToChat => (
                "selection-add-to-chat",
                "Add to chat",
                IconName::MessageSquarePlus,
            ),
            SelectionAction::AddToNotes => (
                "selection-add-to-notes",
                "Add to notes",
                IconName::FilePlusCorner,
            ),
        };
        let disabled = self.pending == Some(action);
        div()
            .id(id)
            .flex()
            .w_full()
            .h(u(32.))
            .items_center()
            .gap(u(8.))
            .whitespace_nowrap()
            .rounded(u(8.))
            .px(u(10.))
            .font_family(theme.fonts.sans.clone())
            .text_px(13.)
            .line_height(u(13.))
            .text_color(theme.colors.content)
            .when(disabled, |el| el.opacity(0.5))
            .when(!disabled, |el| {
                el.cursor_pointer()
                    .hover(|s| s.bg(theme.content(0.05)))
                    .on_click(cx.listener(move |this, _, _, cx| this.pick(action, cx)))
            })
            .child(icon(glyph).size(u(14.)).text_color(theme.colors.content))
            .child(label)
            .into_any_element()
    }
}

impl Render for TranscriptSelectionMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(selection) = self.selection.clone().filter(|_| self.chat || self.notes) else {
            return div().into_any_element();
        };
        let theme = Theme::of(cx).clone();
        let mut list = div()
            .id("selection-actions")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    this.dismiss(true, cx);
                }
            }))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.dismiss(false, cx)))
            .p(u(4.))
            .flex()
            .flex_col()
            .items_stretch()
            .gap(u(2.))
            .min_w(u(144.));
        if self.chat {
            list = list.child(self.action_button(SelectionAction::AddToChat, &theme, cx));
        }
        if self.notes {
            list = list.child(self.action_button(SelectionAction::AddToNotes, &theme, cx));
            if let Some(error) = self.error.clone() {
                list = list.child(
                    div()
                        .max_w(u(320.))
                        .px(u(10.))
                        .py(u(4.))
                        .font_family(theme.fonts.sans.clone())
                        .text_px(12.)
                        .line_height(u(16.))
                        .text_color(theme.content(0.7))
                        .child(format!("Could not save note. {error}")),
                );
            }
        }
        let frame = popover_frame("transcript-selection-menu")
            .side(PopoverSide::Top)
            .animate(!cx.reduce_motion())
            .child(list);
        let position = point(selection.rect.center().x, selection.rect.top() - px(6.));
        popover_at(position, Anchor::BottomCenter, frame, cx).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{AppContext as _, TestAppContext, size};

    fn selection(text: &str) -> TranscriptSelection {
        TranscriptSelection {
            text: text.into(),
            rect: Bounds::new(point(px(10.), px(20.)), size(px(100.), px(20.))),
        }
    }

    #[gpui::test]
    fn keeps_selected_text_available_when_saving_a_note_fails(cx: &mut TestAppContext) {
        let menu = cx.new(|_| TranscriptSelectionMenu::new(false, true));
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = events.clone();
        cx.update(|cx| {
            cx.subscribe(&menu, move |_, event: &SelectionMenuEvent, _| {
                sink.borrow_mut().push(event.clone())
            })
            .detach();
        });
        menu.update(cx, |menu, cx| {
            menu.set_selection(Some(selection("Keep this")), cx);
            menu.pick(SelectionAction::AddToNotes, cx);
        });
        menu.read_with(cx, |menu, _| {
            assert_eq!(menu.pending(), Some(SelectionAction::AddToNotes));
        });
        assert_eq!(
            events.borrow().as_slice(),
            [SelectionMenuEvent::AddToNotes {
                text: "Keep this".into()
            }]
        );
        // A second click while pending does nothing.
        menu.update(cx, |menu, cx| menu.pick(SelectionAction::AddToNotes, cx));
        assert_eq!(events.borrow().len(), 1);
        menu.update(cx, |menu, cx| menu.finish_note(Err("Disk full".into()), cx));
        menu.read_with(cx, |menu, _| {
            assert_eq!(menu.error(), Some("Disk full"));
            assert_eq!(menu.pending(), None);
            assert!(menu.is_open());
        });
        assert!(
            !events
                .borrow()
                .iter()
                .any(|event| matches!(event, SelectionMenuEvent::Dismiss { .. }))
        );
        menu.update(cx, |menu, cx| {
            menu.pick(SelectionAction::AddToNotes, cx);
            menu.finish_note(Ok(()), cx);
        });
        let dismissals = events
            .borrow()
            .iter()
            .filter(|event| matches!(event, SelectionMenuEvent::Dismiss { .. }))
            .count();
        assert_eq!(dismissals, 1);
        menu.read_with(cx, |menu, _| assert!(!menu.is_open()));
    }

    #[gpui::test]
    fn offers_the_selected_text_to_both_chat_and_notes(cx: &mut TestAppContext) {
        let menu = cx.new(|_| TranscriptSelectionMenu::new(true, true));
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = events.clone();
        cx.update(|cx| {
            cx.subscribe(&menu, move |_, event: &SelectionMenuEvent, _| {
                sink.borrow_mut().push(event.clone())
            })
            .detach();
        });
        menu.update(cx, |menu, cx| {
            menu.set_selection(Some(selection("A useful link")), cx);
            menu.pick(SelectionAction::AddToNotes, cx);
            menu.finish_note(Ok(()), cx);
        });
        assert_eq!(
            events.borrow().as_slice(),
            [
                SelectionMenuEvent::AddToNotes {
                    text: "A useful link".into()
                },
                SelectionMenuEvent::Dismiss {
                    clear_selection: true
                },
            ]
        );
        events.borrow_mut().clear();
        menu.update(cx, |menu, cx| {
            menu.set_selection(Some(selection("Quote me")), cx);
            menu.pick(SelectionAction::AddToChat, cx);
        });
        assert_eq!(
            events.borrow().as_slice(),
            [
                SelectionMenuEvent::AddToChat {
                    text: "Quote me".into()
                },
                SelectionMenuEvent::Dismiss {
                    clear_selection: true
                },
            ]
        );
    }
}
