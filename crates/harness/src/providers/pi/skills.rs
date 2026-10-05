//! Port of src/integrations/harness/providers/pi/piSkills.ts: Pi skill and
//! omp command discovery over a short-lived `--mode rpc` probe.
//!
//! `PiSkillCommand` was a `NativeCommand` with `source: "pi"`, so the Pi
//! parser returns `NativeCommand` directly.

use std::collections::HashSet;

use anyhow::{Result, anyhow};
use futures::FutureExt;
use monocode_core::HarnessId;
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::js;
use serde_json::{Value, json};

use crate::core::child::{ChildEvent, Children};

use super::client::PiRpc;
use super::deps::{NativeCommand, NativeSubcommand, Rec, native_command_invocation};
use super::flavor::{OMP_FLAVOR, PI_FLAVOR, PiFlavor};
use super::protocol::{
    PiSpawnOptions, as_record, build_pi_spawn_args, extension_ui_response,
    needs_extension_ui_reply, parse_extension_ui_request,
};

/// Request timeout for the discovery probe.
pub const REQUEST_TIMEOUT_MS: u64 = 45_000;

/// `discoverPiSkills`.
pub async fn discover_pi_skills(children: &Children, cwd: &str) -> Result<Vec<NativeCommand>> {
    let data = discover_commands(children, &PI_FLAVOR, cwd, "get_commands").await?;
    pi_skills_from_rpc_data(data.as_ref()).map_err(|error| anyhow!(error))
}

/// `discoverOmpCommands`.
pub async fn discover_omp_commands(children: &Children, cwd: &str) -> Result<Vec<NativeCommand>> {
    let data = discover_commands(children, &OMP_FLAVOR, cwd, "get_available_commands").await?;
    omp_commands_from_rpc_data(data.as_ref()).map_err(|error| anyhow!(error))
}

/// `discoverCommands`: run one sessionless probe in `cwd`, with the user's
/// config, skills, and extensions loaded, and return the reply's `data`.
async fn discover_commands(
    children: &Children,
    flavor: &PiFlavor,
    cwd: &str,
    command: &str,
) -> Result<Option<Value>> {
    let path = children.resolve_binary(flavor.id).await?.path;
    let bridge = children.acquire_harness_bridge().await?;
    let child_id = format!("monocode-{}-skills-{}", flavor.id, uuid::Uuid::new_v4());
    let rpc = PiRpc::new(children, &child_id, flavor.label);
    let label = flavor.label;
    let result: Result<Option<Value>> = async {
        let events = children.watch_child(&child_id);
        let pump = rpc.clone();
        children.spawner().spawn(
            async move {
                while let Ok(event) = events.recv().await {
                    match event {
                        ChildEvent::Stdout(line) => {
                            // `replyToUi`: deny every extension dialog that waits for an answer.
                            let Some(frame) = pump.push_line(&line) else {
                                continue;
                            };
                            let Some(request) = parse_extension_ui_request(&frame) else {
                                continue;
                            };
                            if !needs_extension_ui_reply(&request) {
                                continue;
                            }
                            let deny = extension_ui_response(&request, ApprovalDecision::Deny);
                            let _ = pump.write_line(deny.to_string()).await;
                        }
                        ChildEvent::Exit(_) => {
                            pump.close(Some(format!("{label} skill probe exited")))
                        }
                        ChildEvent::Stderr(_) => {}
                    }
                }
            }
            .boxed(),
        );
        let args = build_pi_spawn_args(
            flavor,
            &PiSpawnOptions {
                no_session: true,
                ..Default::default()
            },
        );
        children
            .spawn_child(&child_id, &path, args, cwd, None, Some(flavor.id))
            .await?;
        let request: Rec = json!({ "type": command })
            .as_object()
            .cloned()
            .unwrap_or_default();
        let response = rpc.request(request, REQUEST_TIMEOUT_MS as i64).await?;
        Ok(response.get("data").cloned())
    }
    .await;
    rpc.close(None);
    children.unwatch_child(&child_id);
    let _ = children.kill_child(&child_id).await;
    drop(bridge);
    result
}

fn has_reserved_char(name: &str) -> bool {
    name.chars()
        .any(|c| js::is_space(c) || c == '/' || c == '\\')
}

/// `ompCommandsFromRpcData`.
pub fn omp_commands_from_rpc_data(data: Option<&Value>) -> Result<Vec<NativeCommand>, String> {
    let Some(Value::Array(commands)) = as_record(data).and_then(|data| data.get("commands")) else {
        return Err("OMP get_available_commands returned no commands array".into());
    };
    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = Vec::new();
    for value in commands {
        let row = value.as_object();
        let Some(Value::String(name)) = row.and_then(|row| row.get("name")) else {
            continue;
        };
        if name.is_empty() || has_reserved_char(name) || seen.contains(name.as_str()) {
            continue;
        }
        seen.insert(name);
        let row = row.expect("a named row is an object");
        let aliases: Vec<String> = match row.get("aliases") {
            Some(Value::Array(aliases)) => aliases
                .iter()
                .filter_map(Value::as_str)
                .filter(|alias| !alias.is_empty() && !has_reserved_char(alias))
                .map(str::to_string)
                .collect(),
            _ => Vec::new(),
        };
        let hint = as_record(row.get("input")).and_then(|input| input.get("hint"));
        let subcommands: Vec<NativeSubcommand> = match row.get("subcommands") {
            Some(Value::Array(subcommands)) => subcommands
                .iter()
                .filter_map(|value| {
                    let sub = value.as_object()?;
                    let name = sub.get("name")?.as_str().filter(|name| !name.is_empty())?;
                    Some(NativeSubcommand {
                        name: name.to_string(),
                        description: sub
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        usage: sub.get("usage").and_then(Value::as_str).map(str::to_string),
                    })
                })
                .collect(),
            _ => Vec::new(),
        };
        out.push(NativeCommand {
            name: name.clone(),
            invocation: native_command_invocation(HarnessId::Omp, name),
            source: HarnessId::Omp,
            description: row
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            origin: row
                .get("source")
                .and_then(Value::as_str)
                .map(str::to_string),
            aliases: (!aliases.is_empty()).then_some(aliases),
            input_hint: hint.and_then(Value::as_str).map(str::to_string),
            subcommands: (!subcommands.is_empty()).then_some(subcommands),
        });
    }
    Ok(out)
}

/// `piSkillsFromRpcData`: the first valid row for each `skill:` invocation.
pub fn pi_skills_from_rpc_data(data: Option<&Value>) -> Result<Vec<NativeCommand>, String> {
    let Some(Value::Array(commands)) = as_record(data).and_then(|data| data.get("commands")) else {
        return Err("Pi get_commands returned no commands array".into());
    };
    let mut seen: HashSet<&str> = HashSet::new();
    let mut skills = Vec::new();
    for value in commands {
        let Some(command) = value.as_object() else {
            continue;
        };
        let Some(Value::String(invocation)) = command.get("name") else {
            continue;
        };
        if command.get("source").and_then(Value::as_str) != Some("skill")
            || !invocation.starts_with("skill:")
            || seen.contains(invocation.as_str())
        {
            continue;
        }
        let name = &invocation["skill:".len()..];
        if name.is_empty() {
            continue;
        }
        seen.insert(invocation);
        skills.push(NativeCommand {
            name: name.to_string(),
            description: command
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            invocation: invocation.clone(),
            source: HarnessId::Pi,
            origin: None,
            aliases: None,
            input_hint: None,
            subcommands: None,
        });
    }
    Ok(skills)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn omp(name: &str, invocation: &str, origin: &str) -> NativeCommand {
        NativeCommand {
            name: name.into(),
            invocation: invocation.into(),
            description: String::new(),
            source: HarnessId::Omp,
            origin: Some(origin.into()),
            aliases: None,
            input_hint: None,
            subcommands: None,
        }
    }

    #[test]
    fn preserves_metadata_from_all_command_origins_and_escapes_reserved_monocode_commands() {
        let data = json!({
            "commands": [
                { "name": "plan", "source": "builtin" },
                { "name": "compact", "source": "builtin" },
                { "name": "new", "source": "builtin" },
                { "name": "new-session", "source": "builtin" },
                { "name": "add-to-folder", "source": "builtin" },
                {
                    "name": "Review_Code",
                    "source": "custom",
                    "aliases": ["review", null],
                    "description": "Choose reviewer",
                    "input": { "hint": "<reviewer> [path]" },
                    "subcommands": [
                        { "name": "list", "description": "List reviewers", "usage": "list --all" },
                        null,
                    ],
                },
                { "name": "Review_Code", "description": "duplicate" },
                { "name": "skill:design", "source": "skill" },
                { "name": "mcp:search", "source": "mcp" },
                { "name": "file", "source": "file" },
                { "name": "extension", "source": "extension" },
                { "name": "" },
                { "name": "bad command" },
                null,
            ],
        });
        assert_eq!(
            omp_commands_from_rpc_data(Some(&data)).unwrap(),
            vec![
                omp("plan", "omp:plan", "builtin"),
                omp("compact", "omp:compact", "builtin"),
                omp("new", "new", "builtin"),
                omp("new-session", "new-session", "builtin"),
                omp("add-to-folder", "omp:add-to-folder", "builtin"),
                NativeCommand {
                    description: "Choose reviewer".into(),
                    aliases: Some(vec!["review".into()]),
                    input_hint: Some("<reviewer> [path]".into()),
                    subcommands: Some(vec![NativeSubcommand {
                        name: "list".into(),
                        description: Some("List reviewers".into()),
                        usage: Some("list --all".into()),
                    }]),
                    ..omp("Review_Code", "Review_Code", "custom")
                },
                omp("skill:design", "skill:design", "skill"),
                omp("mcp:search", "mcp:search", "mcp"),
                omp("file", "file", "file"),
                omp("extension", "extension", "extension"),
            ]
        );
    }

    #[test]
    fn rejects_malformed_omp_inventories() {
        for value in [None, Some(json!({})), Some(json!({ "commands": null }))] {
            let error = omp_commands_from_rpc_data(value.as_ref()).unwrap_err();
            assert!(error.contains("commands"), "{error}");
        }
    }

    #[test]
    fn keeps_the_first_valid_row_for_each_pi_skill_invocation() {
        let data = json!({
            "commands": [
                { "name": "help", "description": "Help", "source": "builtin" },
                {
                    "name": "skill:architect",
                    "description": "Design before implementation.",
                    "source": "skill",
                    "sourceInfo": { "path": "/tmp/architect/SKILL.md" },
                },
                { "name": "skill:architect", "description": "Duplicate", "source": "skill" },
                { "name": "skill:", "source": "skill" },
                { "name": 42, "source": "skill" },
            ],
        });
        assert_eq!(
            pi_skills_from_rpc_data(Some(&data)).unwrap(),
            vec![NativeCommand {
                name: "architect".into(),
                description: "Design before implementation.".into(),
                invocation: "skill:architect".into(),
                source: HarnessId::Pi,
                origin: None,
                aliases: None,
                input_hint: None,
                subcommands: None,
            }]
        );
    }

    #[test]
    fn rejects_a_malformed_pi_commands_envelope() {
        for data in [
            None,
            Some(Value::Null),
            Some(json!({})),
            Some(json!({ "commands": null })),
        ] {
            let error = pi_skills_from_rpc_data(data.as_ref()).unwrap_err();
            assert!(error.contains("commands"), "{error}");
        }
    }
}
