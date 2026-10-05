//! Port of src/integrations/harness/core/nativeCommands.ts: provider-owned
//! slash commands share one picker but run inside their harness.

use std::sync::{Arc, LazyLock};

use regex::Regex;
use serde::{Deserialize, Serialize};

use monocode_core::harness::HarnessId;

use super::task::BoxFuture;

/// `NativeCommand`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeCommand {
    pub name: String,
    pub description: String,
    pub invocation: String,
    pub source: HarnessId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subcommands: Option<Vec<NativeSubcommand>>,
}

/// One entry of `NativeCommand.subcommands`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSubcommand {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<String>,
}

/// `CommandContext`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandContext {
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Receives a provider's command list each time it changes.
pub type CommandsListener = Arc<dyn Fn(Vec<NativeCommand>) + Send + Sync>;

/// Ends a subscription.
pub type Unsubscribe = Box<dyn FnOnce() + Send>;

/// `NativeCommandProvider`.
pub trait NativeCommandProvider: Send + Sync {
    fn discover(
        &self,
        context: CommandContext,
    ) -> BoxFuture<'_, anyhow::Result<Vec<NativeCommand>>>;

    /// `subscribe`. `None` means the provider has no live updates, which is
    /// the TypeScript `subscribe` being absent.
    fn subscribe(
        &self,
        _context: CommandContext,
        _on_commands: CommandsListener,
    ) -> Option<Unsubscribe> {
        None
    }

    /// Full command runtimes own slash arguments, including @file-like text.
    fn raw_slash_commands(&self) -> bool {
        false
    }
}

const RESERVED_COMMANDS: [&str; 3] = ["plan", "compact", "add-to-folder"];

fn is_reserved(name: &str) -> bool {
    RESERVED_COMMANDS.contains(&name)
}

/// `nativeCommandInvocation`.
pub fn native_command_invocation(harness: HarnessId, name: &str) -> String {
    if is_reserved(name) {
        format!("{harness}:{name}")
    } else {
        name.to_string()
    }
}

static SLASH_COMMAND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\s*)/(\S+)").unwrap());

/// `nativeCommandPrompt`. Only the reserved-command escape is rewritten;
/// custom names stay exact.
pub fn native_command_prompt(harness: HarnessId, text: &str) -> String {
    let Some(captures) = SLASH_COMMAND.captures(text) else {
        return text.to_string();
    };
    let whole = captures.get(0).unwrap();
    let space = &captures[1];
    let name = &captures[2];
    let prefix = format!("{harness}:");
    match name.strip_prefix(&prefix) {
        Some(rest) if is_reserved(rest) => {
            format!("{space}/{rest}{}", &text[whole.end()..])
        }
        _ => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_only_reserved_names() {
        assert_eq!(
            native_command_invocation(HarnessId::Claude, "plan"),
            "claude:plan"
        );
        assert_eq!(
            native_command_invocation(HarnessId::Claude, "review"),
            "review"
        );
    }

    #[test]
    fn rewrites_the_reserved_escape_back() {
        assert_eq!(
            native_command_prompt(HarnessId::Codex, "  /codex:compact now"),
            "  /compact now"
        );
        assert_eq!(
            native_command_prompt(HarnessId::Codex, "/codex:review now"),
            "/codex:review now"
        );
        assert_eq!(
            native_command_prompt(HarnessId::Codex, "/claude:plan"),
            "/claude:plan"
        );
        assert_eq!(
            native_command_prompt(HarnessId::Codex, "no command"),
            "no command"
        );
    }

    #[test]
    fn native_command_json_matches_typescript() {
        let value = serde_json::json!({
            "name": "review", "description": "Review", "invocation": "review",
            "source": "claude", "inputHint": "[path]",
            "subcommands": [{ "name": "all" }]
        });
        let command: NativeCommand = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&command).unwrap(), value);
    }
}
