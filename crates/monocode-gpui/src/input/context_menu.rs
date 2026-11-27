//! Shared by composers, message editing, search fields and the file editor.
//! GPUI draws these fields itself, so there is no webview editing menu.
use super::*;
use crate::popover::{Anchor, Menu, MenuAction, MenuEvent, MenuItem, menu_popover};

pub(super) struct EditingMenu {
    menu: Entity<Menu>,
    position: Point<Pixels>,
    _events: gpui::Subscription,
}

impl Input {
    fn editing_menu_items(&self, clipboard: bool) -> Vec<MenuItem> {
        let modifier = if cfg!(target_os = "macos") {
            "⌘"
        } else {
            "Ctrl+"
        };
        let selected = !self.secret && !self.selection.is_empty();
        let action = |id, label, key, enabled: bool| {
            MenuAction::new(id, label)
                .shortcut(format!("{modifier}{key}"))
                .disabled(!enabled)
                .into()
        };
        let mut items = Vec::new();
        if self.code.is_some() {
            items.extend([
                action("undo", "Undo", "Z", self.history_available(false)),
                action(
                    "redo",
                    "Redo",
                    if cfg!(target_os = "macos") {
                        "⇧Z"
                    } else {
                        "Shift+Z"
                    },
                    self.history_available(true),
                ),
                MenuItem::Separator,
            ]);
        }
        items.extend([
            action("cut", "Cut", "X", selected && !self.read_only),
            action("copy", "Copy", "C", selected),
            // Dispatch Paste through the focused field's ancestors, just as
            // the shortcut does: composers also accept clipboard images.
            action("paste", "Paste", "V", clipboard && !self.read_only),
            MenuItem::Separator,
            action("select-all", "Select All", "A", !self.content.is_empty()),
        ]);
        items
    }

    pub(super) fn open_context_menu(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(target_os = "macos")]
        crate::traffic_lights::focus_content(window);
        self.end_drag();
        window.focus(&self.focus, cx);
        let index = self.mouse_index(event.position);
        // Preserve a selection clicked inside; otherwise move the insertion
        // point so Paste applies where the user opened the menu.
        if self.selection.is_empty() || !self.selection.contains(&index) {
            self.move_to(index, cx);
        }
        let items = self.editing_menu_items(cx.read_from_clipboard().is_some());
        let menu = cx.new(|cx| Menu::new("Text editing", items, window, cx));
        let events = cx.subscribe_in(&menu, window, |this, _, event, window, cx| {
            this.context_menu = None;
            if let MenuEvent::Picked(id, _) = event {
                let action: Box<dyn gpui::Action> = match id.as_ref() {
                    "undo" => Box::new(Undo),
                    "redo" => Box::new(Redo),
                    "cut" => Box::new(Cut),
                    "copy" => Box::new(Copy),
                    "paste" => Box::new(Paste),
                    "select-all" => Box::new(SelectAll),
                    _ => return,
                };
                window.focus(&this.focus, cx);
                window.dispatch_action(action, cx);
            }
            cx.notify();
        });
        self.context_menu = Some(EditingMenu {
            menu,
            position: event.position,
            _events: events,
        });
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn editing_menu(&self, cx: &App) -> Option<impl IntoElement> {
        self.context_menu.as_ref().map(|state| {
            menu_popover(&state.menu, Anchor::Point(state.position), cx).without_animation()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn editing_actions_respect_selection_clipboard_read_only_and_secrets(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|cx| {
            let input = cx.new(Input::new);
            input.update(cx, |input, _| {
                let enabled = |input: &Input, clipboard| {
                    input
                        .editing_menu_items(clipboard)
                        .iter()
                        .filter_map(|item| match item {
                            MenuItem::Action(action) if !action.disabled => {
                                Some(action.id.to_string())
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                };
                assert!(enabled(input, false).is_empty());
                input.content = "hello".into();
                input.selection = 0..5;
                assert_eq!(enabled(input, true), ["cut", "copy", "paste", "select-all"]);
                input.read_only = true;
                assert_eq!(enabled(input, true), ["copy", "select-all"]);
                input.secret = true;
                input.read_only = false;
                assert_eq!(enabled(input, true), ["paste", "select-all"]);
                input.secret = false;
                input.selection = 5..5;
                assert_eq!(enabled(input, false), ["select-all"]);
            });
        });
    }
}
