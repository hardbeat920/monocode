//! Port of src/features/workspace/model/tabKeys.ts.
//!
//! Workspace keybindings:
//!
//! | Command | Keys |
//! | --- | --- |
//! | New tab | cmd-t |
//! | Close other tabs | cmd-opt-t |
//! | Close tab | cmd-w |
//! | Close all tabs | shift-cmd-w |
//! | Split pane right | cmd-d |
//! | Split pane down | shift-cmd-d |
//! | Next tab | shift-cmd-} |
//! | Previous tab | shift-cmd-{ |
//! | Back in tab history | cmd-[ |
//! | Forward in history | cmd-] |
//! | Activate tab 1 to 8 | cmd-1 to cmd-8 |
//! | Last tab | cmd-9 |
//! | Cycle next tab | ctrl-tab |
//! | Cycle previous tab | ctrl-shift-tab |
//! | Focus pane | cmd-opt-arrows |
//! | New terminal | cmd-` |
//! | New terminal tab | shift-cmd-` |
//! | Toggle terminal | cmd-j |
//! | Zoom in | cmd-= (cmd-+ on shift layouts) |
//! | Zoom out | cmd-- |
//! | Reset zoom | cmd-0 |
//! | Previous session | shift-cmd-up |
//! | Next session | shift-cmd-down |
//! | Previous in tab | cmd-up |
//! | Next in tab | cmd-down |
//! | Archive session | shift-cmd-a |
//! | Previous project | shift-cmd-left |
//! | Next project | shift-cmd-right |
//! | Stop focused turn | escape |
//!
//! The TypeScript read a DOM `KeyboardEvent`; `KeyEvent` carries the same
//! fields, with `key` and `code` holding the DOM key and code names.

use monocode_core::Session;

use crate::layout::{FocusDir, WorkspaceTab};

/// The `KeyboardEvent` fields the workspace shortcuts read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyEvent {
    /// DOM `key`, for example `"t"`, `"ArrowUp"`, `"Escape"`.
    pub key: String,
    /// DOM `code`, for example `"Backquote"`, `"Digit3"`.
    pub code: String,
    pub meta_key: bool,
    pub ctrl_key: bool,
    pub alt_key: bool,
    pub shift_key: bool,
    pub is_composing: bool,
    pub repeat: bool,
    pub default_prevented: bool,
}

/// `TabCommand`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TabCommand {
    New,
    CloseOthers,
    CloseAll,
    Close,
    Next,
    Prev,
    CycleNext,
    CyclePrev,
    Back,
    Forward,
    SplitRight,
    SplitDown,
    NewTerminal,
    NewTerminalTab,
    ToggleTerminal,
    PrevSession,
    NextSession,
    PrevSessionInTab,
    NextSessionInTab,
    ArchiveSession,
    PrevProject,
    NextProject,
    /// `{ activate: n }`: tab index, or -1 for the last tab.
    Activate(i64),
    /// `{ focus: dir }`.
    Focus(FocusDir),
}

/// The named commands and their `TAB_COMMAND_KEYBINDINGS` rows.
const TAB_COMMAND_KEYBINDINGS: [(TabCommand, &str, &str); 22] = [
    (TabCommand::New, "new", "Tab: New"),
    (TabCommand::CloseOthers, "close-others", "Tab: Close Others"),
    (TabCommand::CloseAll, "close-all", "Tab: Close All"),
    (TabCommand::Close, "close", "Pane: Close"),
    (TabCommand::Next, "next", "Tab: Next"),
    (TabCommand::Prev, "prev", "Tab: Previous"),
    (TabCommand::CycleNext, "cycle-next", "Tab: Cycle Next"),
    (TabCommand::CyclePrev, "cycle-prev", "Tab: Cycle Previous"),
    (TabCommand::Back, "back", "Tab: Back"),
    (TabCommand::Forward, "forward", "Tab: Forward"),
    (TabCommand::SplitRight, "split-right", "Pane: Split Right"),
    (TabCommand::SplitDown, "split-down", "Pane: Split Down"),
    (TabCommand::NewTerminal, "new-terminal", "Terminal: New"),
    (
        TabCommand::NewTerminalTab,
        "new-terminal-tab",
        "Terminal: New Tab",
    ),
    (
        TabCommand::ToggleTerminal,
        "toggle-terminal",
        "Terminal: Toggle Dock",
    ),
    (TabCommand::PrevSession, "prev-session", "Session: Previous"),
    (TabCommand::NextSession, "next-session", "Session: Next"),
    (
        TabCommand::PrevSessionInTab,
        "prev-session-in-tab",
        "Session: Previous in Current Tab",
    ),
    (
        TabCommand::NextSessionInTab,
        "next-session-in-tab",
        "Session: Next in Current Tab",
    ),
    (
        TabCommand::ArchiveSession,
        "archive-session",
        "Session: Archive",
    ),
    (TabCommand::PrevProject, "prev-project", "Project: Previous"),
    (TabCommand::NextProject, "next-project", "Project: Next"),
];

/// The keybinding row for `{ activate: 0..7 }`.
pub const ACTIVATE_1_TO_8_KEYBINDING: &str = "Tab: Activate 1–8";
/// The keybinding row for `{ activate: -1 }`.
pub const ACTIVATE_LAST_KEYBINDING: &str = "Tab: Activate Last";
const FOCUS_KEYBINDING_PREFIX: &str = "Pane: Focus ";

impl TabCommand {
    /// The string the TypeScript used for a named command, for example
    /// `"close-others"`. `None` for `Activate` and `Focus`.
    pub fn name(self) -> Option<&'static str> {
        TAB_COMMAND_KEYBINDINGS
            .iter()
            .find(|(command, _, _)| *command == self)
            .map(|(_, name, _)| *name)
    }
}

/// `tabCommand`: the workspace command for a key press, if any.
pub fn tab_command(e: &KeyEvent) -> Option<TabCommand> {
    if e.is_composing {
        return None;
    }

    let modifier = e.meta_key || e.ctrl_key;

    if modifier && e.alt_key && !e.shift_key {
        if e.key.to_lowercase() == "t" {
            return Some(TabCommand::CloseOthers);
        }
        return match e.key.as_str() {
            "ArrowLeft" => Some(TabCommand::Focus(FocusDir::Left)),
            "ArrowRight" => Some(TabCommand::Focus(FocusDir::Right)),
            "ArrowUp" => Some(TabCommand::Focus(FocusDir::Up)),
            "ArrowDown" => Some(TabCommand::Focus(FocusDir::Down)),
            _ => None,
        };
    }

    if e.key == "Tab" && e.ctrl_key && !e.meta_key && !e.alt_key {
        return Some(if e.shift_key {
            TabCommand::CyclePrev
        } else {
            TabCommand::CycleNext
        });
    }

    if !modifier || e.alt_key {
        return None;
    }

    if e.key == "`" || e.code == "Backquote" {
        return Some(if e.shift_key {
            TabCommand::NewTerminalTab
        } else {
            TabCommand::NewTerminal
        });
    }

    let key = e.key.to_lowercase();

    if e.shift_key {
        if key == "a" && !e.repeat {
            return Some(TabCommand::ArchiveSession);
        }
        return match e.key.as_str() {
            "]" | "}" => Some(TabCommand::Next),
            "[" | "{" => Some(TabCommand::Prev),
            "ArrowUp" => Some(TabCommand::PrevSession),
            "ArrowDown" => Some(TabCommand::NextSession),
            "ArrowLeft" => Some(TabCommand::PrevProject),
            "ArrowRight" => Some(TabCommand::NextProject),
            _ if key == "d" => Some(TabCommand::SplitDown),
            _ if key == "w" => Some(TabCommand::CloseAll),
            _ => None,
        };
    }

    if key == "t" {
        return Some(TabCommand::New);
    }
    if e.key == "ArrowUp" {
        return Some(TabCommand::PrevSessionInTab);
    }
    if e.key == "ArrowDown" {
        return Some(TabCommand::NextSessionInTab);
    }
    if key == "w" {
        return Some(TabCommand::Close);
    }
    if key == "d" {
        return Some(TabCommand::SplitRight);
    }
    if key == "j" {
        return Some(TabCommand::ToggleTerminal);
    }
    if e.key == "[" || e.code == "BracketLeft" {
        return Some(TabCommand::Back);
    }
    if e.key == "]" || e.code == "BracketRight" {
        return Some(TabCommand::Forward);
    }
    if key.as_str() >= "1" && key.as_str() <= "8" {
        // TODO(port): the TypeScript compared strings, so a key such as "1a"
        // passes and yields `{ activate: NaN }`. NaN has no i64 form, so
        // that case returns None here.
        return monocode_core::js::parse_number(&key).map(|n| TabCommand::Activate(n as i64 - 1));
    }
    if key == "9" {
        return Some(TabCommand::Activate(-1));
    }
    None
}

/// `tabCommandKeybinding`: the configurable keybinding row for a command.
pub fn tab_command_keybinding(command: TabCommand) -> String {
    match command {
        TabCommand::Focus(dir) => {
            let name = dir.as_str();
            format!(
                "{FOCUS_KEYBINDING_PREFIX}{}{}",
                name[..1].to_uppercase(),
                &name[1..]
            )
        }
        TabCommand::Activate(index) => {
            if index < 0 {
                ACTIVATE_LAST_KEYBINDING.into()
            } else {
                ACTIVATE_1_TO_8_KEYBINDING.into()
            }
        }
        named => TAB_COMMAND_KEYBINDINGS
            .iter()
            .find(|(command, _, _)| *command == named)
            .map(|(_, _, binding)| (*binding).to_string())
            .unwrap_or_default(),
    }
}

/// `tabCommandForKeybinding`: map a custom keybinding row back to its
/// command. `code` is the DOM `code` of the key that fired it.
pub fn tab_command_for_keybinding(binding: &str, code: &str) -> Option<TabCommand> {
    if binding == ACTIVATE_1_TO_8_KEYBINDING {
        let digit = code.strip_prefix("Digit")?;
        let value = match digit.as_bytes() {
            [d @ b'1'..=b'8'] => i64::from(d - b'0'),
            _ => return None,
        };
        return Some(TabCommand::Activate(value - 1));
    }
    if binding == ACTIVATE_LAST_KEYBINDING {
        return Some(TabCommand::Activate(-1));
    }
    if let Some(dir) = binding.strip_prefix(FOCUS_KEYBINDING_PREFIX) {
        // TODO(port): the TypeScript cast any suffix to FocusDir, so
        // "Pane: Focus Sideways" produced `{ focus: "sideways" }`. Unknown
        // directions return None here.
        return FocusDir::parse(&dir.to_lowercase()).map(TabCommand::Focus);
    }
    TAB_COMMAND_KEYBINDINGS
        .iter()
        .find(|(_, _, row)| *row == binding)
        .map(|(command, _, _)| *command)
}

/// `adjacentItemId`: the next or previous id, wrapping at both ends.
pub fn adjacent_item_id(ids: &[String], current: Option<&str>, delta: i64) -> Option<String> {
    if ids.is_empty() {
        return None;
    }
    let index = current
        .filter(|current| !current.is_empty())
        .and_then(|current| ids.iter().position(|id| id == current));
    let Some(index) = index else {
        return if delta < 0 {
            ids.last().cloned()
        } else {
            ids.first().cloned()
        };
    };
    let len = ids.len() as i64;
    let next = (index as i64 + if delta < 0 { -1 } else { 1 } + len) % len;
    ids.get(next as usize).cloned()
}

/// Input to `shouldHandleListNavigation`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ListNavigationInput {
    pub blocked_target: bool,
    pub empty_composer_target: bool,
    pub surface_open: bool,
}

/// `shouldHandleListNavigation`.
pub fn should_handle_list_navigation(input: ListNavigationInput) -> bool {
    !input.surface_open && (!input.blocked_target || input.empty_composer_target)
}

/// `isPlainEscape`.
fn is_plain_escape(e: &KeyEvent) -> bool {
    e.key == "Escape"
        && !e.is_composing
        && !e.repeat
        && !e.meta_key
        && !e.ctrl_key
        && !e.alt_key
        && !e.shift_key
}

/// `shouldStopFocusedTurnOnEscape`.
pub fn should_stop_focused_turn_on_escape(
    e: &KeyEvent,
    in_terminal: bool,
    focused_session_busy: bool,
) -> bool {
    is_plain_escape(e) && !e.default_prevented && !in_terminal && focused_session_busy
}

/// `focusedBusyAgentSessionId`: the busy agent session focused in exactly
/// the active tab.
pub fn focused_busy_agent_session_id(
    active_tab_id: &str,
    tabs: &[WorkspaceTab],
    sessions: &[Session],
    project_terminal_focused: bool,
) -> Option<String> {
    if project_terminal_focused {
        return None;
    }
    let tab = tabs.iter().find(|entry| entry.id == active_tab_id)?;
    if tab.diff_focused == Some(true) {
        return None;
    }
    let session = sessions.iter().find(|entry| entry.id == tab.focused_id)?;
    session.is_busy().then(|| session.id.clone())
}

/// `deferUnhandledEscape`: run `run` after the current key dispatch, unless
/// a later handler marks the Escape handled first.
///
/// `default_prevented` reads the live "handled" flag of the event, both now
/// and when the deferred callback runs. `defer` schedules the callback; the
/// TypeScript default was `setTimeout(callback, 0)`, because a microtask can
/// run between window keydown listeners before a later surface (such as
/// Settings) has prevented the same Escape.
pub fn defer_unhandled_escape(
    e: &KeyEvent,
    default_prevented: impl Fn() -> bool + 'static,
    run: impl FnOnce() + 'static,
    defer: impl FnOnce(Box<dyn FnOnce()>),
) {
    if !is_plain_escape(e) || default_prevented() {
        return;
    }
    defer(Box::new(move || {
        if !default_prevented() {
            run();
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::leaf;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    fn key(key: &str) -> KeyEvent {
        KeyEvent {
            key: key.into(),
            ..Default::default()
        }
    }

    fn meta(k: &str) -> KeyEvent {
        KeyEvent {
            meta_key: true,
            ..key(k)
        }
    }

    fn meta_shift(k: &str) -> KeyEvent {
        KeyEvent {
            shift_key: true,
            ..meta(k)
        }
    }

    #[test]
    fn archives_with_cmd_shift_a_or_ctrl_shift_a() {
        assert_eq!(
            tab_command(&meta_shift("A")),
            Some(TabCommand::ArchiveSession)
        );
        assert_eq!(
            tab_command(&KeyEvent {
                ctrl_key: true,
                shift_key: true,
                ..key("a")
            }),
            Some(TabCommand::ArchiveSession)
        );
    }

    #[test]
    fn leaves_other_a_key_events_alone() {
        let cases = [
            key("a"),
            meta("a"),
            KeyEvent {
                ctrl_key: true,
                ..key("a")
            },
            KeyEvent {
                shift_key: true,
                ..key("a")
            },
            KeyEvent {
                alt_key: true,
                ..meta_shift("a")
            },
            KeyEvent {
                is_composing: true,
                ..meta_shift("a")
            },
            KeyEvent {
                repeat: true,
                ..meta_shift("a")
            },
        ];
        for event in cases {
            assert_eq!(tab_command(&event), None, "{event:?}");
        }
    }

    #[test]
    fn opens_a_terminal_pane_with_cmd_backtick() {
        assert_eq!(
            tab_command(&KeyEvent {
                code: "Backquote".into(),
                ..meta("`")
            }),
            Some(TabCommand::NewTerminal)
        );
    }

    #[test]
    fn opens_a_terminal_workspace_tab_with_shift_cmd_backtick() {
        assert_eq!(
            tab_command(&KeyEvent {
                code: "Backquote".into(),
                ..meta_shift("~")
            }),
            Some(TabCommand::NewTerminalTab)
        );
    }

    #[test]
    fn closes_all_tabs_with_cmd_shift_w_or_ctrl_shift_w() {
        assert_eq!(tab_command(&meta_shift("W")), Some(TabCommand::CloseAll));
        assert_eq!(
            tab_command(&KeyEvent {
                ctrl_key: true,
                shift_key: true,
                ..key("w")
            }),
            Some(TabCommand::CloseAll)
        );
        assert_eq!(tab_command(&meta("w")), Some(TabCommand::Close));
    }

    #[test]
    fn keeps_ctrl_tab_separate_from_the_adjacent_tab_shortcut() {
        let ctrl_tab = KeyEvent {
            ctrl_key: true,
            ..key("Tab")
        };
        assert_eq!(tab_command(&ctrl_tab), Some(TabCommand::CycleNext));
        assert_eq!(
            tab_command(&KeyEvent {
                shift_key: true,
                ..ctrl_tab
            }),
            Some(TabCommand::CyclePrev)
        );
    }

    #[test]
    fn keeps_existing_tab_chrome_bindings() {
        assert_eq!(tab_command(&meta("t")), Some(TabCommand::New));
        assert_eq!(
            tab_command(&KeyEvent {
                alt_key: true,
                ..meta("t")
            }),
            Some(TabCommand::CloseOthers)
        );
        assert_eq!(
            tab_command(&KeyEvent {
                ctrl_key: true,
                alt_key: true,
                ..key("t")
            }),
            Some(TabCommand::CloseOthers)
        );
        assert_eq!(tab_command(&meta("d")), Some(TabCommand::SplitRight));
        assert_eq!(tab_command(&meta("j")), Some(TabCommand::ToggleTerminal));
    }

    #[test]
    fn walks_tab_visit_history_with_cmd_brackets() {
        assert_eq!(
            tab_command(&KeyEvent {
                code: "BracketLeft".into(),
                ..meta("[")
            }),
            Some(TabCommand::Back)
        );
        assert_eq!(
            tab_command(&KeyEvent {
                code: "BracketRight".into(),
                ..meta("]")
            }),
            Some(TabCommand::Forward)
        );
    }

    #[test]
    fn keeps_shift_cmd_brackets_as_adjacent_tab_cycle() {
        assert_eq!(
            tab_command(&KeyEvent {
                code: "BracketLeft".into(),
                ..meta_shift("{")
            }),
            Some(TabCommand::Prev)
        );
        assert_eq!(
            tab_command(&KeyEvent {
                code: "BracketRight".into(),
                ..meta_shift("}")
            }),
            Some(TabCommand::Next)
        );
    }

    #[test]
    fn uses_shift_mod_arrows_for_session_and_project_navigation() {
        assert_eq!(
            tab_command(&meta_shift("ArrowUp")),
            Some(TabCommand::PrevSession)
        );
        assert_eq!(
            tab_command(&meta_shift("ArrowDown")),
            Some(TabCommand::NextSession)
        );
        assert_eq!(
            tab_command(&meta_shift("ArrowLeft")),
            Some(TabCommand::PrevProject)
        );
        assert_eq!(
            tab_command(&meta_shift("ArrowRight")),
            Some(TabCommand::NextProject)
        );
    }

    #[test]
    fn uses_unshifted_mod_arrows_to_switch_sessions_in_the_current_tab() {
        assert_eq!(
            tab_command(&meta("ArrowUp")),
            Some(TabCommand::PrevSessionInTab)
        );
        assert_eq!(
            tab_command(&KeyEvent {
                ctrl_key: true,
                ..key("ArrowDown")
            }),
            Some(TabCommand::NextSessionInTab)
        );
        assert_eq!(
            tab_command(&KeyEvent {
                alt_key: true,
                ..meta_shift("ArrowUp")
            }),
            None
        );
    }

    #[test]
    fn activates_tabs_by_digit() {
        assert_eq!(tab_command(&meta("1")), Some(TabCommand::Activate(0)));
        assert_eq!(tab_command(&meta("8")), Some(TabCommand::Activate(7)));
        assert_eq!(tab_command(&meta("9")), Some(TabCommand::Activate(-1)));
        assert_eq!(tab_command(&meta("0")), None);
    }

    #[test]
    fn maps_parsed_commands_to_configurable_keybinding_rows() {
        assert_eq!(tab_command_keybinding(TabCommand::New), "Tab: New");
        assert_eq!(
            tab_command_keybinding(TabCommand::CycleNext),
            "Tab: Cycle Next"
        );
        assert_eq!(
            tab_command_keybinding(TabCommand::Activate(0)),
            "Tab: Activate 1–8"
        );
        assert_eq!(
            tab_command_keybinding(TabCommand::Activate(-1)),
            "Tab: Activate Last"
        );
        assert_eq!(
            tab_command_keybinding(TabCommand::Focus(FocusDir::Left)),
            "Pane: Focus Left"
        );
        assert_eq!(TabCommand::CloseOthers.name(), Some("close-others"));
    }

    #[test]
    fn maps_custom_keybinding_rows_back_to_commands() {
        assert_eq!(
            tab_command_for_keybinding("Tab: New", ""),
            Some(TabCommand::New)
        );
        assert_eq!(
            tab_command_for_keybinding("Tab: Activate 1–8", "Digit3"),
            Some(TabCommand::Activate(2))
        );
        assert_eq!(
            tab_command_for_keybinding("Tab: Activate 1–8", "Digit9"),
            None
        );
        assert_eq!(
            tab_command_for_keybinding("Pane: Focus Down", ""),
            Some(TabCommand::Focus(FocusDir::Down))
        );
        assert_eq!(tab_command_for_keybinding("Tab: Unknown", ""), None);
    }

    #[test]
    fn cycles_ordered_item_ids_and_wraps_at_both_ends() {
        let ids: Vec<String> = ["a", "b", "c"].iter().map(|id| id.to_string()).collect();
        assert_eq!(adjacent_item_id(&ids, Some("b"), 1).as_deref(), Some("c"));
        assert_eq!(adjacent_item_id(&ids, Some("c"), 1).as_deref(), Some("a"));
        assert_eq!(adjacent_item_id(&ids, Some("a"), -1).as_deref(), Some("c"));
        assert_eq!(
            adjacent_item_id(&ids, Some("missing"), 1).as_deref(),
            Some("a")
        );
        assert_eq!(
            adjacent_item_id(&ids, Some("missing"), -1).as_deref(),
            Some("c")
        );
        assert_eq!(adjacent_item_id(&[], Some("a"), 1), None);
    }

    #[test]
    fn allows_navigation_from_an_empty_composer() {
        assert!(should_handle_list_navigation(ListNavigationInput {
            blocked_target: true,
            empty_composer_target: true,
            surface_open: false,
        }));
    }

    #[test]
    fn blocks_list_navigation_while_another_text_or_app_surface_owns_focus() {
        let input = |blocked_target, empty_composer_target, surface_open| ListNavigationInput {
            blocked_target,
            empty_composer_target,
            surface_open,
        };
        assert!(should_handle_list_navigation(input(false, false, false)));
        assert!(!should_handle_list_navigation(input(true, false, false)));
        assert!(!should_handle_list_navigation(input(false, false, true)));
        assert!(!should_handle_list_navigation(input(true, true, true)));
    }

    fn escape() -> KeyEvent {
        key("Escape")
    }

    #[test]
    fn stops_a_busy_focused_agent_turn_on_plain_escape() {
        assert!(should_stop_focused_turn_on_escape(&escape(), false, true));
    }

    #[test]
    fn does_not_steal_escape_that_another_surface_already_handled() {
        let handled = KeyEvent {
            default_prevented: true,
            ..escape()
        };
        assert!(!should_stop_focused_turn_on_escape(&handled, false, true));
    }

    #[test]
    fn leaves_terminal_escape_and_idle_sessions_alone() {
        assert!(!should_stop_focused_turn_on_escape(&escape(), true, true));
        assert!(!should_stop_focused_turn_on_escape(&escape(), false, false));
    }

    #[test]
    fn ignores_modified_composing_or_repeated_escape() {
        let base = escape();
        let cases = [
            KeyEvent {
                meta_key: true,
                ..base.clone()
            },
            KeyEvent {
                ctrl_key: true,
                ..base.clone()
            },
            KeyEvent {
                alt_key: true,
                ..base.clone()
            },
            KeyEvent {
                shift_key: true,
                ..base.clone()
            },
            KeyEvent {
                is_composing: true,
                ..base.clone()
            },
            KeyEvent {
                repeat: true,
                ..base.clone()
            },
        ];
        for event in cases {
            assert!(
                !should_stop_focused_turn_on_escape(&event, false, true),
                "{event:?}"
            );
        }
    }

    fn escape_tab(focused: &str) -> WorkspaceTab {
        WorkspaceTab::new("tab-a", leaf(focused), focused)
    }

    fn session(id: &str, busy: bool) -> Session {
        let mut session = Session::blank(id, monocode_core::HarnessId::Claude, "", "/repo");
        session.busy = Some(busy);
        session
    }

    #[test]
    fn returns_only_the_busy_agent_session_in_the_exact_active_tab() {
        let tabs = [escape_tab("session-a")];
        let sessions = [session("session-a", true)];
        assert_eq!(
            focused_busy_agent_session_id("tab-a", &tabs, &sessions, false).as_deref(),
            Some("session-a")
        );
        assert_eq!(
            focused_busy_agent_session_id("missing", &tabs, &sessions, false),
            None
        );
    }

    #[test]
    fn does_not_stop_through_diff_terminal_dock_editor_or_idle_focus() {
        let tabs = [escape_tab("session-a")];
        let sessions = [session("session-a", true)];
        let diff = [WorkspaceTab {
            diff_focused: Some(true),
            ..escape_tab("session-a")
        }];
        assert_eq!(
            focused_busy_agent_session_id("tab-a", &diff, &sessions, false),
            None
        );
        assert_eq!(
            focused_busy_agent_session_id("tab-a", &tabs, &sessions, true),
            None
        );
        assert_eq!(
            focused_busy_agent_session_id("tab-a", &[escape_tab("editor-pane")], &sessions, false),
            None
        );
        assert_eq!(
            focused_busy_agent_session_id("tab-a", &tabs, &[session("session-a", false)], false),
            None
        );
    }

    type Deferred = Rc<RefCell<Option<Box<dyn FnOnce()>>>>;

    fn capture(deferred: &Deferred) -> impl FnOnce(Box<dyn FnOnce()>) + use<> {
        let deferred = deferred.clone();
        move |callback| *deferred.borrow_mut() = Some(callback)
    }

    #[test]
    fn waits_for_later_keydown_handlers_before_stopping_the_session() {
        let prevented = Rc::new(Cell::new(false));
        let stopped = Rc::new(Cell::new(false));
        let deferred: Deferred = Rc::default();
        let (flag, stop) = (prevented.clone(), stopped.clone());
        defer_unhandled_escape(
            &escape(),
            move || flag.get(),
            move || stop.set(true),
            capture(&deferred),
        );
        assert!(!stopped.get());

        prevented.set(true);
        let callback = deferred.borrow_mut().take().unwrap();
        callback();
        assert!(!stopped.get());
    }

    #[test]
    fn runs_after_the_keydown_dispatch_when_escape_stays_unhandled() {
        let stopped = Rc::new(Cell::new(false));
        let deferred: Deferred = Rc::default();
        let stop = stopped.clone();
        defer_unhandled_escape(
            &escape(),
            || false,
            move || stop.set(true),
            capture(&deferred),
        );
        assert!(!stopped.get());
        let callback = deferred.borrow_mut().take().unwrap();
        callback();
        assert!(stopped.get());
    }

    #[test]
    fn yields_to_a_later_same_dispatch_escape_handler() {
        let prevented = Rc::new(Cell::new(false));
        let stopped = Rc::new(Cell::new(false));
        let deferred: Deferred = Rc::default();
        let (flag, stop) = (prevented.clone(), stopped.clone());
        defer_unhandled_escape(
            &escape(),
            move || flag.get(),
            move || stop.set(true),
            capture(&deferred),
        );
        prevented.set(true);
        let callback = deferred.borrow_mut().take().unwrap();
        callback();
        assert!(!stopped.get());
    }

    #[test]
    fn does_not_schedule_an_already_handled_or_repeated_escape() {
        let scheduled = Rc::new(Cell::new(0));
        let count = |scheduled: &Rc<Cell<i32>>| {
            let scheduled = scheduled.clone();
            move |_: Box<dyn FnOnce()>| scheduled.set(scheduled.get() + 1)
        };
        defer_unhandled_escape(&escape(), || true, || {}, count(&scheduled));
        defer_unhandled_escape(
            &KeyEvent {
                repeat: true,
                ..escape()
            },
            || false,
            || {},
            count(&scheduled),
        );
        assert_eq!(scheduled.get(), 0);
    }
}
