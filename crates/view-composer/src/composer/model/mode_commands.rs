//! Port of the model half of src/features/sessions/ui/modeCommands.tsx:
//! which leading `/command` turns on a composer mode, and how each mode is
//! labeled. The view half (pill, highlight colors) is in `view::modes`.

use std::collections::HashSet;

use monocode_ui::IconName;

use super::commands;

/// A composer mode a leading `/command` can turn on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Plan,
    Operator,
    Orchestrator,
    Draft,
    Btw,
}

/// `pill` in `MODE_COMMAND_STYLES`: the chip under the prompt while a mode
/// is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModePill {
    pub label: &'static str,
    pub title: &'static str,
}

/// `menu` in `MODE_COMMAND_STYLES`: the row in the + menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeMenu {
    pub label: &'static str,
    pub description: &'static str,
}

impl Mode {
    pub const ALL: [Mode; 5] = [
        Mode::Plan,
        Mode::Operator,
        Mode::Orchestrator,
        Mode::Draft,
        Mode::Btw,
    ];

    /// The command name.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Plan => commands::PLAN,
            Mode::Operator => commands::OPERATOR,
            Mode::Orchestrator => commands::ORCHESTRATOR,
            Mode::Draft => commands::DRAFT,
            Mode::Btw => commands::BTW,
        }
    }

    pub fn from_name(name: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|mode| mode.name() == name)
    }

    pub fn icon(self) -> IconName {
        match self {
            Mode::Plan => IconName::AiIdea,
            Mode::Operator => IconName::CursorMagicSelection,
            Mode::Orchestrator => IconName::Share,
            Mode::Draft => IconName::CircleDashed,
            Mode::Btw => IconName::MessageSquare,
        }
    }

    pub fn pill(self) -> Option<ModePill> {
        Some(match self {
            Mode::Plan => ModePill {
                label: "Plan",
                title: "Plan mode",
            },
            Mode::Operator => ModePill {
                label: "Operator",
                title: "Operator",
            },
            Mode::Orchestrator => ModePill {
                label: "Orchestrator",
                title: "Orchestrator mode",
            },
            Mode::Draft => ModePill {
                label: "Draft",
                title: "Draft mode",
            },
            Mode::Btw => return None,
        })
    }

    pub fn menu(self) -> Option<ModeMenu> {
        Some(match self {
            Mode::Plan => ModeMenu {
                label: "Plan mode",
                description: "Review a plan before building",
            },
            Mode::Operator => ModeMenu {
                label: "Operator",
                description: "Give this thread access to MonoCode",
            },
            Mode::Orchestrator => ModeMenu {
                label: "Orchestrator",
                description: "Plan and coordinate agent work",
            },
            Mode::Draft => ModeMenu {
                label: "Draft",
                description: "Save this message without starting the agent",
            },
            Mode::Btw => return None,
        })
    }
}

/// `MODE_COMMAND_INDENT`: first-line indent, in CSS px, that with the `/`
/// makes room for a mode icon.
pub const MODE_COMMAND_INDENT: f32 = 13.0;

/// `ModeCommandToken`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeCommandToken {
    pub mode: Mode,
    /// Byte offset just past the command name.
    pub end: usize,
}

/// `leadingModeCommand`: mode commands only take effect at the start of a
/// prompt (`/^\/([a-z]+)(?=\s|$)/`), and only when the slash picker lists
/// them in `names`.
pub fn leading_mode_command(text: &str, names: &HashSet<String>) -> Option<ModeCommandToken> {
    let rest = text.strip_prefix('/')?;
    let len = rest.bytes().take_while(u8::is_ascii_lowercase).count();
    if len == 0 {
        return None;
    }
    let boundary = rest[len..]
        .chars()
        .next()
        .is_none_or(monocode_core::js::is_space);
    if !boundary {
        return None;
    }
    let name = &rest[..len];
    if !names.contains(name) {
        return None;
    }
    let mode = Mode::from_name(name)?;
    Some(ModeCommandToken { mode, end: len + 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> HashSet<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn reads_a_leading_mode_command() {
        let names = names(&["plan", "operator", "review-pr"]);
        assert_eq!(
            leading_mode_command("/plan fix it", &names),
            Some(ModeCommandToken {
                mode: Mode::Plan,
                end: 5
            })
        );
        assert_eq!(
            leading_mode_command("/operator", &names).map(|t| t.mode),
            Some(Mode::Operator)
        );
    }

    #[test]
    fn ignores_commands_that_are_not_leading_listed_or_modes() {
        let names = names(&["plan", "review-pr", "draft"]);
        assert_eq!(leading_mode_command(" /plan", &names), None);
        assert_eq!(leading_mode_command("say /plan", &names), None);
        assert_eq!(leading_mode_command("/planning", &names), None);
        assert_eq!(leading_mode_command("/review-pr", &names), None);
        assert_eq!(leading_mode_command("/operator", &names), None);
        assert_eq!(leading_mode_command("/Plan", &names), None);
        assert!(leading_mode_command("/draft\nnext", &names).is_some());
    }

    #[test]
    fn btw_has_no_pill_or_menu_row() {
        assert_eq!(Mode::Btw.pill(), None);
        assert_eq!(Mode::Btw.menu(), None);
        assert_eq!(Mode::Plan.pill().unwrap().title, "Plan mode");
    }
}
