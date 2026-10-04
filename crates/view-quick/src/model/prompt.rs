//! Port of the prompt helpers in src/features/quick-composer/ui/QuickComposer.tsx:
//! `quickPromptMode`, the commands the floating composer offers after a
//! leading `/`, and the prompt's sizes.

use std::collections::HashSet;

use monocode_core::js;
use monocode_view_composer::composer::model::commands::{
    DRAFT, OPERATOR, ORCHESTRATOR, PLAN, draft_command, operator_command, orchestrator_command,
    plan_command,
};
use monocode_view_composer::composer::model::skills::Skill;

/// `PROMPT_MAX_HEIGHT`: the tallest the prompt grows before it scrolls, in
/// CSS px.
pub const PROMPT_MAX_HEIGHT: f32 = 220.0;

/// `MODE_INDENT`: sized for the 16px prompt, as the main composer's indent
/// is for 14px.
pub const MODE_INDENT: f32 = 15.0;

/// `MODE_COMMANDS`: commands the floating composer offers after a leading
/// `/`.
pub fn mode_commands() -> Vec<Skill> {
    vec![
        plan_command(),
        operator_command(),
        orchestrator_command(),
        draft_command(),
    ]
}

/// `MODE_NAMES`.
pub fn mode_names() -> HashSet<String> {
    [PLAN, OPERATOR, ORCHESTRATOR, DRAFT]
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn mode_name(name: &str) -> Option<&'static str> {
    [PLAN, OPERATOR, ORCHESTRATOR, DRAFT]
        .into_iter()
        .find(|mode| *mode == name)
}

/// What `quickPromptMode` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickPromptMode {
    /// The prompt the session should get.
    pub prompt: String,
    /// The mode the prompt starts with.
    pub mode: Option<&'static str>,
}

/// `quickPromptMode`: the mode a prompt starts with
/// (`/^\/([a-z]+)(?=\s|$)\s*/`), and the prompt the session should get.
/// The workspace reads Operator from the prompt itself, so it stays.
pub fn quick_prompt_mode(text: &str) -> QuickPromptMode {
    let plain = || QuickPromptMode {
        prompt: text.to_string(),
        mode: None,
    };
    let Some(rest) = text.strip_prefix('/') else {
        return plain();
    };
    let len = rest.bytes().take_while(u8::is_ascii_lowercase).count();
    if len == 0 {
        return plain();
    }
    let after = &rest[len..];
    if after.chars().next().is_some_and(|c| !js::is_space(c)) {
        return plain();
    }
    let Some(mode) = mode_name(&rest[..len]) else {
        return plain();
    };
    if mode == OPERATOR {
        return QuickPromptMode {
            prompt: text.to_string(),
            mode: Some(mode),
        };
    }
    let spaces: usize = after
        .chars()
        .take_while(|c| js::is_space(*c))
        .map(char::len_utf8)
        .sum();
    QuickPromptMode {
        prompt: after[spaces..].to_string(),
        mode: Some(mode),
    }
}

/// `CommandLabel`: the command name with its first letter capitalized.
pub fn command_label(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_plan_orchestrator_and_draft_but_keeps_operator() {
        assert_eq!(
            quick_prompt_mode("/plan sketch the refactor"),
            QuickPromptMode {
                prompt: "sketch the refactor".into(),
                mode: Some(PLAN)
            }
        );
        assert_eq!(quick_prompt_mode("/orchestrator ship it").prompt, "ship it");
        assert_eq!(quick_prompt_mode("/draft\nremember").prompt, "remember");
        assert_eq!(
            quick_prompt_mode("/operator list my notes"),
            QuickPromptMode {
                prompt: "/operator list my notes".into(),
                mode: Some(OPERATOR)
            }
        );
        assert_eq!(quick_prompt_mode("/plan").prompt, "");
    }

    #[test]
    fn ignores_unknown_partial_and_inner_commands() {
        for text in [
            "/planning",
            "/unknown x",
            "say /plan",
            " /plan",
            "/Plan",
            "/op",
            "/",
        ] {
            assert_eq!(quick_prompt_mode(text).mode, None, "{text}");
            assert_eq!(quick_prompt_mode(text).prompt, text);
        }
    }

    #[test]
    fn lists_the_four_mode_commands() {
        let names: Vec<String> = mode_commands().into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["plan", "operator", "orchestrator", "draft"]);
        assert_eq!(mode_names().len(), 4);
        assert_eq!(command_label("orchestrator"), "Orchestrator");
    }
}
