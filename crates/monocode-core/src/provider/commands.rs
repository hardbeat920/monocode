//! Provider-owned commands. Local composer commands never become ordinary prompts.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeCommand {
    pub name: String,
    pub description: String,
    pub invocation: String,
    pub source: Kind,
    pub origin: Option<String>,
    pub aliases: Vec<String>,
    pub input_hint: Option<String>,
    pub subcommands: Vec<NativeSubcommand>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeSubcommand {
    pub name: String,
    pub description: Option<String>,
    pub usage: Option<String>,
}

pub fn native_invocation(kind: Kind, name: &str) -> String {
    if matches!(name, "plan" | "compact" | "add-to-folder") {
        format!("{}:{name}", kind.id())
    } else {
        name.into()
    }
}

pub fn native_prompt(kind: Kind, text: &str) -> String {
    let leading = text.len() - text.trim_start().len();
    let body = &text[leading..];
    let end = body.find(char::is_whitespace).unwrap_or(body.len());
    let prefix = format!("/{}:", kind.id());
    if let Some(name) = body[..end]
        .strip_prefix(prefix.as_str())
        .filter(|name| matches!(*name, "plan" | "compact" | "add-to-folder"))
    {
        return format!("{}/{}{}", &text[..leading], name, &body[end..]);
    }
    text.into()
}

/// OMP and Devin own slash arguments, including @file-like text. Expanding a
/// local skill or mention ahead of them would turn a command into normal text.
pub fn prepare_prompt(
    kind: Kind,
    text: &str,
    cwd: &str,
    attachments: &[PathBuf],
) -> Result<String, String> {
    let text = native_prompt(kind, text);
    if matches!(kind, Kind::Omp | Kind::Devin) && text.trim_start().starts_with('/') {
        crate::composer::validate_attachments(attachments)?;
        return Ok(text);
    }
    crate::composer::prepare_prompt(&text, cwd, attachments)
}

pub(super) fn parse_commands(kind: Kind, rows: &Value) -> Vec<NativeCommand> {
    let mut seen = std::collections::HashSet::new();
    rows.as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let name = row
                .as_str()
                .or_else(|| row["name"].as_str())?
                .trim_start_matches('/');
            if name.is_empty()
                || name
                    .chars()
                    .any(|c| c.is_whitespace() || matches!(c, '/' | '\\'))
                || !seen.insert(name.to_owned())
            {
                return None;
            }
            // Pi's command API advertises only skill commands to the shared picker.
            if kind == Kind::Pi && (row["source"] != "skill" || !name.starts_with("skill:")) {
                return None;
            }
            Some(NativeCommand {
                name: name
                    .strip_prefix("skill:")
                    .filter(|_| kind == Kind::Pi)
                    .unwrap_or(name)
                    .into(),
                description: row["description"].as_str().unwrap_or("").into(),
                invocation: native_invocation(kind, name),
                source: kind,
                origin: row["source"].as_str().map(str::to_owned),
                aliases: row["aliases"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter(|alias| {
                        !alias.is_empty()
                            && !alias
                                .chars()
                                .any(|c| c.is_whitespace() || matches!(c, '/' | '\\'))
                    })
                    .map(str::to_owned)
                    .collect(),
                input_hint: row["input"]["hint"].as_str().map(str::to_owned),
                subcommands: row["subcommands"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|subcommand| {
                        let name = subcommand["name"]
                            .as_str()
                            .filter(|name| !name.is_empty())?;
                        Some(NativeSubcommand {
                            name: name.into(),
                            description: subcommand["description"].as_str().map(str::to_owned),
                            usage: subcommand["usage"].as_str().map(str::to_owned),
                        })
                    })
                    .collect(),
            })
        })
        .collect()
}

pub fn supports_compact(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Codex
            | Kind::Claude
            | Kind::Pi
            | Kind::Omp
            | Kind::Grok
            | Kind::Devin
            | Kind::OpenCode
    )
}

/// Compaction owns the provider transport, but creates no user or assistant turn.
pub fn compact(
    mut request: Request,
    cancel: Arc<AtomicBool>,
    emit: impl FnMut(Vec<Event>) -> Result<(), String>,
) -> Result<(), String> {
    if !supports_compact(request.kind) {
        return Err(format!(
            "Context compaction is not supported by the native {} transport.",
            request.kind.label()
        ));
    }
    if request.provider_id.is_none() {
        return Err("Send a message before compacting this chat.".into());
    }
    request.prompt = "/compact".into();
    request.settings.attachments.clear();
    run_operation(request, cancel, None, false, true, false, emit).map(|_| ())
}

/// Pi-family command discovery is an isolated no-session RPC, matching
/// piSkills.ts. It never submits an empty prompt or resumes the main session.
pub fn discover_commands(
    mut request: Request,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<NativeCommand>, String> {
    if !request.kind.is_pi() {
        return Ok(Vec::new());
    }
    request.provider_id = None;
    request.prompt.clear();
    request.settings.attachments.clear();
    let mut commands = Vec::new();
    run_operation(request, cancel, None, false, false, true, |events| {
        for event in events {
            if let Event::Commands(discovered) = event {
                commands = discovered;
            }
        }
        Ok(())
    })?;
    Ok(commands)
}

impl Protocol {
    pub(super) fn command_updates(&self, v: &Value) -> Option<Vec<NativeCommand>> {
        let rows =
            if self.request.kind == Kind::Claude && v["type"] == "system" && v["subtype"] == "init"
            {
                v.get("slash_commands")?
            } else if self.request.kind.is_acp() {
                let params = v.get("params")?;
                let update = params.get("update").unwrap_or(params);
                if update["sessionUpdate"] != "available_commands_update" {
                    return None;
                }
                update.get("availableCommands")?
            } else if self.request.kind == Kind::Omp && v["type"] == "available_commands_update" {
                v.get("commands")?
            } else {
                return None;
            };
        Some(parse_commands(self.request.kind, rows))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_escape_only_rewrites_reserved_leading_commands() {
        assert_eq!(
            native_prompt(Kind::Omp, " /omp:compact tail"),
            " /compact tail"
        );
        assert_eq!(
            native_prompt(Kind::Omp, "/omp:custom tail"),
            "/omp:custom tail"
        );
        assert_eq!(native_prompt(Kind::Pi, "/omp:compact"), "/omp:compact");
    }
    #[test]
    fn catalogs_validate_deduplicate_and_preserve_provider_ownership() {
        let commands = parse_commands(
            Kind::Omp,
            &json!([{"name":"compact"},{"name":"compact"},{"name":"bad/name"},{"name":"custom","description":"Run custom","source":"builtin","aliases":["c","bad/name"],"input":{"hint":"<target>"},"subcommands":[{"name":"list","usage":"list [filter]"}]}]),
        );
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].invocation, "omp:compact");
        assert_eq!(commands[1].description, "Run custom");
        assert_eq!(commands[1].aliases, vec!["c"]);
        assert_eq!(commands[1].input_hint.as_deref(), Some("<target>"));
        assert_eq!(
            commands[1].subcommands[0].usage.as_deref(),
            Some("list [filter]")
        );
        assert!(
            parse_commands(Kind::Pi, &json!([{"name":"compact","source":"builtin"}])).is_empty()
        );
    }
    #[test]
    fn raw_provider_commands_preserve_their_arguments_without_local_expansion() {
        assert_eq!(
            prepare_prompt(
                Kind::Omp,
                "/omp:plan @missing-file",
                "/missing-project",
                &[]
            )
            .unwrap(),
            "/plan @missing-file"
        );
        assert_eq!(
            prepare_prompt(
                Kind::Devin,
                "/custom @missing-file",
                "/missing-project",
                &[]
            )
            .unwrap(),
            "/custom @missing-file"
        );
    }
    #[test]
    fn pi_command_probe_never_starts_or_resumes_a_user_prompt() {
        for kind in [Kind::Pi, Kind::Omp] {
            let mut protocol = Protocol::new(Request {
                kind,
                binary: "unused".into(),
                cwd: "/tmp".into(),
                prompt: String::new(),
                provider_id: None,
                model: None,
                settings: Settings::default(),
            });
            protocol.command_probe = true;
            let mut output = Vec::new();
            protocol.pi_start(&mut output).unwrap();
            let wire = String::from_utf8(output).unwrap();
            assert!(wire.contains(if kind == Kind::Pi {
                "get_commands"
            } else {
                "get_available_commands"
            }));
            assert!(!wire.contains("native-state"));
            assert!(!wire.contains("native-prompt"));
            let events = protocol.frame(json!({"id":"native-commands","type":"response","success":true,"data":{"commands":[]}}), &mut Vec::new()).unwrap();
            assert_eq!(events, vec![Event::Commands(Vec::new())]);
            assert!(protocol.done);
        }
    }
    #[test]
    fn pi_compaction_is_an_rpc_and_its_acknowledgement_reports_context() {
        for kind in [Kind::Pi, Kind::Omp] {
            let mut protocol = Protocol::new(Request {
                kind,
                binary: "unused".into(),
                cwd: "/tmp".into(),
                prompt: "/compact".into(),
                provider_id: Some("session".into()),
                model: None,
                settings: Settings::default(),
            });
            protocol.compact = true;
            let mut output = Vec::new();
            protocol
                .frame(
                    json!({"type":"response","id":"native-state","success":true,"data":{}}),
                    &mut output,
                )
                .unwrap();
            if kind == Kind::Omp {
                protocol
                    .frame(
                        json!({"type":"response","id":"native-fast","success":true}),
                        &mut output,
                    )
                    .unwrap();
            }
            let wire = String::from_utf8(output).unwrap();
            assert!(wire.contains("native-compact"));
            assert!(!wire.contains("native-prompt"));
            let events = protocol.frame(json!({"type":"response","id":"native-compact","success":true,"data":{"estimatedTokensAfter":42}}), &mut Vec::new()).unwrap();
            assert!(events.contains(&Event::Context {
                used: Some(42),
                window: None
            }));
            assert!(events.contains(&Event::Done));
            assert!(protocol.done);
        }
    }
    #[test]
    fn codex_compaction_resumes_without_starting_a_prompt_turn() {
        let mut protocol = Protocol::new(Request {
            kind: Kind::Codex,
            binary: "unused".into(),
            cwd: "/tmp".into(),
            prompt: "/compact".into(),
            provider_id: Some("thread".into()),
            model: None,
            settings: Settings::default(),
        });
        protocol.compact = true;
        let mut output = Vec::new();
        protocol
            .frame(
                json!({"id":2,"result":{"thread":{"id":"thread"}}}),
                &mut output,
            )
            .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("thread/compact/start"));
        assert!(!output.contains("turn/start"));
    }
}
