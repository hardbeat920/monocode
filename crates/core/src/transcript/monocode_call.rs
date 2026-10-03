//! Port of src/features/sessions/model/monocodeToolCall.ts: recognize the
//! app's own CLI (`monocode app <action>`) in a shell tool row, so the
//! transcript can show the action instead of a long binary path.

use std::borrow::Borrow;

use crate::block::ToolPreviewKind;
use crate::{Block, BlockRole};

/// `MonoCodeToolCall`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonoCodeToolCall {
    pub action: String,
    pub label: &'static str,
    pub command: String,
}

/// `ACTION_LABELS`.
const ACTION_LABELS: &[(&str, &str)] = &[
    ("models.list", "List models"),
    ("sessions.list", "List sessions"),
    ("sessions.read", "Read a session"),
    ("sessions.send", "Continue a session"),
    ("sessions.draft", "Save a draft"),
    ("sessions.start", "Start a session"),
    ("folders.list", "List folders"),
    ("folders.move", "Move a session"),
    ("notes.list", "List notes"),
    ("notes.read", "Read a note"),
    ("notes.write", "Write a note"),
];

fn is_js_space(c: char) -> bool {
    crate::js::is_space(c) || crate::js::is_line_terminator(c)
}

/// `shellWords`: conservatively split one shell invocation. Compound commands
/// return `None` so they keep the ordinary shell row.
fn shell_words(command: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = command.chars().collect();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if quote == Some('\'') {
            if ch == '\'' {
                quote = None;
            } else {
                word.push(ch);
            }
            index += 1;
            continue;
        }
        if ch == '\'' && quote.is_none() {
            quote = Some('\'');
            started = true;
            index += 1;
            continue;
        }
        if ch == '"' {
            quote = if quote == Some('"') { None } else { Some('"') };
            started = true;
            index += 1;
            continue;
        }
        if ch == '\\' {
            let next = *chars.get(index + 1)?;
            // Preserve ordinary path separators, including quoted Windows paths.
            let literal = if quote == Some('"') {
                !matches!(next, '\\' | '"' | '$' | '`')
            } else {
                !(is_js_space(next)
                    || matches!(
                        next,
                        '\'' | '"'
                            | '\\'
                            | ';'
                            | '&'
                            | '|'
                            | '<'
                            | '>'
                            | '('
                            | ')'
                            | '['
                            | ']'
                            | '{'
                            | '}'
                            | '$'
                            | '`'
                            | '#'
                    ))
            };
            if literal {
                word.push(ch);
            } else {
                word.push(next);
                index += 1;
            }
            started = true;
            index += 1;
            continue;
        }
        if ch == '$' || ch == '`' {
            return None;
        }
        if quote.is_none()
            && matches!(
                ch,
                '\r' | '\n' | ';' | '&' | '|' | '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}' | '#'
            )
        {
            return None;
        }
        if quote.is_none() && is_js_space(ch) {
            if started {
                words.push(std::mem::take(&mut word));
            }
            word.clear();
            started = false;
            index += 1;
            continue;
        }
        word.push(ch);
        started = true;
        index += 1;
    }
    if quote.is_some() {
        return None;
    }
    if started {
        words.push(word);
    }
    Some(words)
}

/// `/(?:^|[/\\])monocode(?:\.exe)?$/i`.
fn is_monocode_binary(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    let base = lower.strip_suffix(".exe").unwrap_or(&lower);
    match base.strip_suffix("monocode") {
        Some(prefix) => prefix.is_empty() || prefix.ends_with('/') || prefix.ends_with('\\'),
        None => false,
    }
}

/// `/^Run(?:ning)?\s+command:\s*/i`.
fn strip_run_command(command: &str) -> &str {
    let lower = command.to_ascii_lowercase();
    let after_run = if lower.starts_with("running") {
        7
    } else if lower.starts_with("run") {
        3
    } else {
        return command;
    };
    let rest = &command[after_run..];
    let spaced = rest.trim_start_matches(is_js_space);
    if spaced.len() == rest.len() {
        return command;
    }
    let Some(after) = spaced
        .get(..8)
        .filter(|word| word.eq_ignore_ascii_case("command:"))
        .map(|_| &spaced[8..])
    else {
        return command;
    };
    after.trim_start_matches(is_js_space)
}

/// `monoCodeToolCall`: the app CLI command a tool row runs, not a mention of
/// it in prose or output.
pub fn monocode_tool_call(block: &Block) -> Option<MonoCodeToolCall> {
    if block.role != BlockRole::Tool && block.role != BlockRole::Approval {
        return None;
    }
    let tool = block.tool.as_ref();
    // A shell preview keeps the original command when the display title was
    // simplified. Never accept a shorter title in place of that command.
    let candidate = tool
        .and_then(|tool| tool.preview.as_ref())
        .filter(|preview| preview.kind == ToolPreviewKind::Shell)
        .and_then(|preview| preview.title.as_deref())
        .or_else(|| tool.and_then(|tool| tool.title.as_deref()))
        .unwrap_or(&block.text);
    let command = strip_run_command(crate::js::trim(candidate));
    if command.is_empty() {
        return None;
    }
    let words = shell_words(command)?;
    if !words.first().is_some_and(|word| is_monocode_binary(word)) {
        return None;
    }
    if words.get(1).map(String::as_str) != Some("app") {
        return None;
    }
    let action = words.get(2).map(String::as_str).unwrap_or("--help");
    if matches!(action, "--help" | "help" | "-h") {
        return (words.len() <= 3).then(|| MonoCodeToolCall {
            action: "--help".into(),
            label: "View CLI commands",
            command: command.to_string(),
        });
    }
    let label = ACTION_LABELS
        .iter()
        .find(|(name, _)| *name == action)
        .map(|(_, label)| *label)?;
    let mut index = 3;
    while index < words.len() {
        if !matches!(words[index].as_str(), "--json" | "--input" | "--request-id")
            || words.get(index + 1).is_none_or(|value| value.is_empty())
        {
            return None;
        }
        index += 2;
    }
    Some(MonoCodeToolCall {
        action: action.to_string(),
        label,
        command: command.to_string(),
    })
}

/// `monoCodeWorkSummary`: a group of only MonoCode calls is named for the app.
pub fn monocode_work_summary<B: Borrow<Block>>(steps: &[B], live: bool) -> Option<&'static str> {
    if steps.iter().any(|block| {
        let block: &Block = block.borrow();
        block.interjection.is_some() || block.role == BlockRole::System
    }) {
        return None;
    }
    let calls: Vec<&Block> = steps
        .iter()
        .map(Borrow::borrow)
        .filter(|block| block.role == BlockRole::Tool || block.role == BlockRole::Approval)
        .collect();
    if calls.is_empty()
        || calls
            .iter()
            .any(|block| monocode_tool_call(block).is_none())
    {
        return None;
    }
    Some(if live {
        "Using MonoCode"
    } else {
        "Used MonoCode"
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BlockTool;

    fn shell(text: &str) -> Block {
        let mut block = Block::new("t", BlockRole::Tool, text);
        block.tool = Some(BlockTool {
            kind: Some("shell".into()),
            status: Some("completed".into()),
            ..Default::default()
        });
        block
    }

    #[test]
    fn recognizes_app_commands_by_their_binary() {
        let call = monocode_tool_call(&shell(
            "/repo/target/debug/MonoCode.app/Contents/MacOS/monocode app notes.list --json '{}'",
        ))
        .unwrap();
        assert_eq!(call.action, "notes.list");
        assert_eq!(call.label, "List notes");
        let help = monocode_tool_call(&shell("monocode app --help")).unwrap();
        assert_eq!(help.action, "--help");
        assert_eq!(help.label, "View CLI commands");
        assert_eq!(
            monocode_tool_call(&shell("Run command: monocode app sessions.list"))
                .unwrap()
                .action,
            "sessions.list"
        );
        assert!(monocode_tool_call(&shell("monocode.exe app models.list")).is_some());
    }

    #[test]
    fn leaves_compound_and_unknown_commands_to_the_shell_row() {
        assert!(monocode_tool_call(&shell("monocode app notes.list && echo extra")).is_none());
        assert!(monocode_tool_call(&shell("monocode app notes.delete")).is_none());
        assert!(monocode_tool_call(&shell("monocode app notes.list --force x")).is_none());
        assert!(monocode_tool_call(&shell("echo monocode app notes.list")).is_none());
        assert!(monocode_tool_call(&shell("monocode app $(whoami)")).is_none());
        assert!(monocode_tool_call(&shell("notmonocode app notes.list")).is_none());
        assert!(
            monocode_tool_call(&Block::new(
                "a",
                BlockRole::Assistant,
                "monocode app notes.list"
            ))
            .is_none()
        );
    }

    #[test]
    fn keeps_quoted_json_whole() {
        let call = monocode_tool_call(&shell(
            r#"monocode app sessions.send --json '{"prompt":"private-marker"}'"#,
        ))
        .unwrap();
        assert_eq!(call.action, "sessions.send");
        assert!(call.command.contains("private-marker"));
    }

    #[test]
    fn names_groups_of_only_app_calls() {
        let steps = vec![
            shell("monocode app --help"),
            shell("monocode app notes.list"),
        ];
        assert_eq!(monocode_work_summary(&steps, true), Some("Using MonoCode"));
        assert_eq!(monocode_work_summary(&steps, false), Some("Used MonoCode"));
        let mixed = vec![shell("monocode app --help"), shell("ls")];
        assert_eq!(monocode_work_summary(&mixed, false), None);
        assert_eq!(monocode_work_summary::<Block>(&[], false), None);
    }
}
