//! Port of src/features/sessions/model/monocodeToolCall.ts: recognize tool
//! calls to MonoCode's own app CLI so the transcript can name them.

use std::sync::LazyLock;

use monocode_core::block::ToolPreviewKind;
use monocode_core::{Block, BlockRole, js};
use regex::Regex;

/// `MonoCodeToolCall`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonoCodeToolCall {
    pub action: String,
    pub label: String,
    pub command: String,
}

/// `ACTION_LABELS`.
fn action_label(action: &str) -> Option<&'static str> {
    Some(match action {
        "models.list" => "List models",
        "sessions.list" => "List sessions",
        "sessions.read" => "Read a session",
        "sessions.send" => "Continue a session",
        "sessions.draft" => "Save a draft",
        "sessions.start" => "Start a session",
        "folders.list" => "List folders",
        "folders.move" => "Move a session",
        "notes.list" => "List notes",
        "notes.read" => "Read a note",
        "notes.write" => "Write a note",
        _ => return None,
    })
}

static RUN_COMMAND_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^Run(?:ning)?\s+command:\s*").unwrap());
static MONOCODE_BINARY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?:^|[/\\])monocode(?:\.exe)?$").unwrap());

/// `shellWords`: conservatively split one shell invocation. Compound
/// commands return `None` and keep the shell row.
fn shell_words(command: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = command.chars().collect();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if quote == Some('\'') {
            if c == '\'' {
                quote = None;
            } else {
                word.push(c);
            }
            index += 1;
            continue;
        }
        if c == '\'' && quote.is_none() {
            quote = Some('\'');
            started = true;
            index += 1;
            continue;
        }
        if c == '"' {
            quote = if quote == Some('"') { None } else { Some('"') };
            started = true;
            index += 1;
            continue;
        }
        if c == '\\' {
            let next = *chars.get(index + 1)?;
            // Preserve ordinary path separators, including quoted Windows paths.
            let literal = (quote == Some('"') && !matches!(next, '\\' | '"' | '$' | '`'))
                || (quote.is_none()
                    && !(js::is_space(next)
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
                        )));
            if literal {
                word.push(c);
            } else {
                word.push(next);
                index += 1;
            }
            started = true;
            index += 1;
            continue;
        }
        if c == '$' || c == '`' {
            return None;
        }
        if quote.is_none()
            && matches!(
                c,
                '\r' | '\n' | ';' | '&' | '|' | '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}' | '#'
            )
        {
            return None;
        }
        if quote.is_none() && js::is_space(c) {
            if started {
                words.push(std::mem::take(&mut word));
            }
            word.clear();
            started = false;
            index += 1;
            continue;
        }
        word.push(c);
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

/// `monoCodeToolCall`: recognize the actual app CLI command, not a mention
/// of it in prose or output.
pub fn monocode_tool_call(block: &Block) -> Option<MonoCodeToolCall> {
    if block.role != BlockRole::Tool && block.role != BlockRole::Approval {
        return None;
    }
    // A shell preview retains the original command when the display title was
    // simplified. Never accept a shorter title in place of that command.
    let tool = block.tool.as_ref();
    let shell_title = tool
        .and_then(|tool| tool.preview.as_ref())
        .filter(|preview| preview.kind == ToolPreviewKind::Shell)
        .and_then(|preview| preview.title.as_deref());
    let candidate = shell_title
        .or_else(|| tool.and_then(|tool| tool.title.as_deref()))
        .unwrap_or(&block.text);
    let command = RUN_COMMAND_PREFIX
        .replace(js::trim(candidate), "")
        .into_owned();
    if command.is_empty() {
        return None;
    }
    let words = shell_words(&command)?;
    if !MONOCODE_BINARY.is_match(words.first().map(String::as_str).unwrap_or("")) {
        return None;
    }
    if words.get(1).map(String::as_str) != Some("app") {
        return None;
    }
    let action = words.get(2).map(String::as_str).unwrap_or("--help");
    if matches!(action, "--help" | "help" | "-h") {
        if words.len() > 3 {
            return None;
        }
        return Some(MonoCodeToolCall {
            action: "--help".into(),
            label: "View CLI commands".into(),
            command,
        });
    }
    let label = action_label(action)?;
    let mut index = 3;
    while index < words.len() {
        let flag_ok = matches!(words[index].as_str(), "--json" | "--input" | "--request-id");
        let value_ok = words.get(index + 1).is_some_and(|value| !value.is_empty());
        if !flag_ok || !value_ok {
            return None;
        }
        index += 2;
    }
    Some(MonoCodeToolCall {
        action: action.to_string(),
        label: label.to_string(),
        command,
    })
}

/// `monoCodeWorkSummary`: a group of only MonoCode calls is named for the
/// app, not the shell.
pub fn monocode_work_summary(steps: &[Block], live: bool) -> Option<&'static str> {
    if steps
        .iter()
        .any(|block| block.interjection.is_some() || block.role == BlockRole::System)
    {
        return None;
    }
    let calls: Vec<&Block> = steps
        .iter()
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
    use monocode_core::block::{BlockTool, ToolPreview};

    fn tool(kind: &str, title: Option<&str>, preview_title: Option<&str>) -> BlockTool {
        BlockTool {
            kind: Some(kind.into()),
            title: title.map(str::to_string),
            preview: preview_title.map(|title| ToolPreview {
                title: Some(title.into()),
                ..ToolPreview::new(ToolPreviewKind::Shell)
            }),
            ..BlockTool::default()
        }
    }

    fn shell(text: &str) -> Block {
        Block {
            tool: Some(tool("shell", None, None)),
            ..Block::new(text, BlockRole::Tool, text)
        }
    }

    fn label(block: &Block) -> Option<String> {
        monocode_tool_call(block).map(|call| call.label)
    }

    #[test]
    fn recognizes_app_actions_with_absolute_quoted_or_bare_executables() {
        assert_eq!(
            label(&shell(
                "/repo/target/debug/MonoCode.app/Contents/MacOS/monocode app notes.list --json '{}'"
            ))
            .as_deref(),
            Some("List notes")
        );
        assert_eq!(
            label(&shell(
                "'/Applications/MonoCode App/monocode' app folders.move --input -"
            ))
            .as_deref(),
            Some("Move a session")
        );
        assert_eq!(
            label(&shell(
                "\"C:\\Program Files\\MonoCode\\monocode.exe\" app notes.list"
            ))
            .as_deref(),
            Some("List notes")
        );
        assert_eq!(
            label(&shell("monocode app --help")).as_deref(),
            Some("View CLI commands")
        );
        assert_eq!(
            label(&shell("monocode app sessions.read --json '{}'")).as_deref(),
            Some("Read a session")
        );
        assert_eq!(
            label(&shell("monocode app sessions.send --json '{}'")).as_deref(),
            Some("Continue a session")
        );
        assert_eq!(
            label(&shell("monocode app sessions.draft --json '{}'")).as_deref(),
            Some("Save a draft")
        );
        assert_eq!(
            label(&shell("monocode app notes.write --input -")).as_deref(),
            Some("Write a note")
        );
        let generic = Block {
            tool: Some(tool("other", Some("monocode app sessions.start"), None)),
            ..Block::new("generic", BlockRole::Tool, "Run command:")
        };
        assert_eq!(label(&generic).as_deref(), Some("Start a session"));
        let codex = Block {
            tool: Some(tool("execute", None, Some("monocode app notes.list"))),
            ..Block::new("codex-action", BlockRole::Tool, "List")
        };
        assert_eq!(label(&codex).as_deref(), Some("List notes"));
    }

    #[test]
    fn does_not_restyle_unrelated_commands_or_text_mentioning_the_cli() {
        assert_eq!(label(&shell("echo monocode app notes.list")), None);
        assert_eq!(label(&shell("monocode control list")), None);
        assert_eq!(
            label(&Block::new(
                "prose",
                BlockRole::Assistant,
                "monocode app notes.list"
            )),
            None
        );
    }

    #[test]
    fn does_not_compact_compound_shell_commands_or_hide_a_longer_shell_preview() {
        for command in [
            "monocode app notes.list && echo extra",
            "monocode app notes.list; echo extra",
            "monocode app notes.list | cat",
            "monocode app notes.list\necho extra",
            "monocode app notes.list --json \"$(echo extra)\"",
        ] {
            assert_eq!(label(&shell(command)), None, "{command}");
        }
        let short_title = Block {
            tool: Some(tool(
                "shell",
                Some("monocode app notes.list"),
                Some("monocode app notes.list && echo extra"),
            )),
            ..Block::new("short-title", BlockRole::Tool, "monocode app notes.list")
        };
        assert_eq!(label(&short_title), None);
        let command = "monocode app sessions.send --json '{\"prompt\":\"a; b\"}'";
        assert_eq!(
            monocode_tool_call(&shell(command)).unwrap().command,
            command
        );
    }

    #[test]
    fn names_a_group_only_when_all_its_tool_calls_use_monocode() {
        let calls = vec![
            shell("monocode app --help"),
            shell("monocode app notes.list"),
        ];
        assert_eq!(monocode_work_summary(&calls, true), Some("Using MonoCode"));
        assert_eq!(monocode_work_summary(&calls, false), Some("Used MonoCode"));
        let mut mixed = calls.clone();
        mixed.push(shell("git status"));
        assert_eq!(monocode_work_summary(&mixed, true), None);
    }
}
