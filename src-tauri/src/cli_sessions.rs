//! Sessions that the provider CLIs wrote on their own, outside MonoCode.
//!
//! Claude Code, Codex and Grok each keep every conversation on disk. This
//! module finds the ones that belong to a project folder and turns a chosen
//! transcript into a flat list of [`Entry`] values that the app can replay
//! into its own blocks. Resuming an imported session goes through the CLI's
//! normal resume path, so the conversation continues in the same provider
//! session the terminal used.
//!
//! The module only reads files. Every function takes its roots explicitly so
//! tests can point it at a temporary directory.

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Longest tool output kept per call. The CLI keeps the full text; the
/// imported transcript only needs enough to read what happened.
const MAX_TOOL_OUTPUT: usize = 8_000;
/// Longest single message kept.
const MAX_TEXT: usize = 200_000;
/// Longest title shown in the picker and used as the session title.
const MAX_TITLE: usize = 120;
/// Bytes read from the start and end of a Claude transcript while listing.
const CLAUDE_HEAD_BYTES: u64 = 256 * 1024;
const CLAUDE_TAIL_BYTES: u64 = 64 * 1024;
/// Newest sessions returned per provider.
const MAX_LISTED: usize = 500;
/// The `CLAUDE_CODE_ENTRYPOINT` MonoCode gives the Claude Code it spawns.
const MONOCODE_CLAUDE_ENTRYPOINT: &str = "monocode";

/// Where each CLI keeps its data. Every list may hold more than one root,
/// for example `~/.claude` and a `CLAUDE_CONFIG_DIR` override.
#[derive(Debug, Clone, Default)]
pub struct Roots {
    pub claude: Vec<PathBuf>,
    pub codex: Vec<PathBuf>,
    pub grok: Vec<PathBuf>,
}

impl Roots {
    /// The default locations under `home`, plus the env overrides each CLI
    /// honors.
    pub fn from_env(home: Option<&Path>) -> Self {
        let mut roots = Roots::default();
        let env_dir = |key: &str| {
            std::env::var_os(key)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        roots.claude.extend(env_dir("CLAUDE_CONFIG_DIR"));
        roots.codex.extend(env_dir("CODEX_HOME"));
        roots.grok.extend(env_dir("GROK_HOME"));
        if let Some(home) = home {
            push_unique(&mut roots.claude, home.join(".claude"));
            push_unique(&mut roots.codex, home.join(".codex"));
            push_unique(&mut roots.grok, home.join(".grok"));
        }
        roots
    }

    fn for_harness(&self, harness: &str) -> &[PathBuf] {
        match harness {
            "claude" => &self.claude,
            "codex" => &self.codex,
            "grok" => &self.grok,
            _ => &[],
        }
    }
}

fn push_unique(list: &mut Vec<PathBuf>, path: PathBuf) {
    if !list.contains(&path) {
        list.push(path);
    }
}

/// One CLI session found on disk.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CliSession {
    pub harness: String,
    pub provider_session_id: String,
    pub cwd: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Epoch milliseconds.
    pub created_at: i64,
    pub updated_at: i64,
    pub path: String,
}

/// One step of an imported conversation, in order.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
// An import holds one short-lived list of these; boxing the tool variant
// would only add an allocation per call.
#[allow(clippy::large_enum_variant)]
pub enum Entry {
    User {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        at: Option<i64>,
    },
    Assistant {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        at: Option<i64>,
    },
    Reasoning {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        at: Option<i64>,
    },
    Tool(ToolEntry),
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolEntry {
    pub id: String,
    /// The provider's tool name, for example `Bash` or `exec_command`.
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Activity kind such as `execute`, `edit` or `search`. Serialized as
    /// `toolKind` because `kind` is the enum tag.
    #[serde(rename = "toolKind", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    pub failed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<i64>,
}

/// Every session the three CLIs recorded for `cwd`, newest first. Sessions
/// the CLI's own resume picker hides (subagents, SDK runs) are left out.
pub fn list_sessions(roots: &Roots, cwd: &str) -> Vec<CliSession> {
    let mut sessions = Vec::new();
    for root in &roots.claude {
        sessions.extend(list_claude(root, cwd));
    }
    let mut titles = HashMap::new();
    for root in &roots.codex {
        titles.extend(codex_thread_names(root));
    }
    for root in &roots.codex {
        sessions.extend(list_codex(root, cwd, &titles));
    }
    for root in &roots.grok {
        sessions.extend(list_grok(root, cwd));
    }
    let mut seen = std::collections::HashSet::new();
    sessions.retain(|session| {
        seen.insert((session.harness.clone(), session.provider_session_id.clone()))
    });
    sessions.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.provider_session_id.cmp(&b.provider_session_id))
    });
    sessions
}

/// The conversation stored at `path`. The path must sit inside one of the
/// roots for `harness`; anything else is refused.
pub fn read_session(roots: &Roots, harness: &str, path: &Path) -> Result<Vec<Entry>, String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("Could not open {}: {error}", path.display()))?;
    let inside = roots.for_harness(harness).iter().any(|root| {
        root.canonicalize()
            .map(|root| canonical.starts_with(root))
            .unwrap_or(false)
    });
    if !inside {
        return Err("That file is not a CLI session".into());
    }
    match harness {
        "claude" => read_claude(&canonical),
        "codex" => read_codex(&canonical),
        "grok" => read_grok(&canonical),
        _ => Err(format!("Importing {harness} sessions is not supported")),
    }
}

// ---------------------------------------------------------------------------
// Claude Code: <config>/projects/<slug>/<session id>.jsonl

/// Claude names a project folder after its path with every character other
/// than ASCII letters and digits replaced by `-`. Long names are cut at 200
/// characters and get a hash suffix.
fn claude_project_slug(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn list_claude(root: &Path, cwd: &str) -> Vec<CliSession> {
    let slug = claude_project_slug(cwd);
    let prefix: String = slug.chars().take(200).collect();
    let Ok(projects) = std::fs::read_dir(root.join("projects")) else {
        return Vec::new();
    };
    let mut sessions = Vec::new();
    for project in projects.flatten() {
        let name = project.file_name().to_string_lossy().into_owned();
        let matches = name == slug
            || (slug.len() > 200 && name.starts_with(&prefix))
            // Paths under /private (macOS /tmp, /var) appear both ways.
            || claude_project_slug(&strip_private(cwd)) == name;
        if !matches {
            continue;
        }
        let Ok(files) = std::fs::read_dir(project.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(session) = claude_summary(&path, cwd) {
                sessions.push(session);
            }
        }
    }
    sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at));
    sessions.truncate(MAX_LISTED);
    sessions
}

fn claude_summary(path: &Path, cwd: &str) -> Option<CliSession> {
    let id = path.file_stem()?.to_str()?.to_owned();
    if !is_session_id(&id) {
        return None;
    }
    let file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut bytes = Vec::new();
    file.take(CLAUDE_HEAD_BYTES).read_to_end(&mut bytes).ok()?;
    // The limit can cut a character or a line in half; both are harmless.
    let head = String::from_utf8_lossy(&bytes);

    let mut session_cwd = None;
    let mut entrypoint = None;
    let mut prompt = None;
    let mut model = None;
    let mut created_at = None;
    for line in head.lines() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if session_cwd.is_none() {
            session_cwd = str_field(&record, "cwd").map(str::to_owned);
        }
        if entrypoint.is_none() {
            entrypoint = str_field(&record, "entrypoint").map(str::to_owned);
        }
        if created_at.is_none() {
            created_at = record.get("timestamp").and_then(timestamp_ms);
        }
        match str_field(&record, "type") {
            Some("user") if prompt.is_none() => {
                if record.get("isSidechain").and_then(Value::as_bool) == Some(true) {
                    return None;
                }
                if record.get("isMeta").and_then(Value::as_bool) != Some(true) {
                    prompt = claude_user_text(&record);
                }
            }
            Some("assistant") if model.is_none() => {
                model = record
                    .pointer("/message/model")
                    .and_then(Value::as_str)
                    .filter(|model| !model.starts_with('<'))
                    .map(str::to_owned);
            }
            _ => {}
        }
        if session_cwd.is_some() && entrypoint.is_some() && prompt.is_some() && model.is_some() {
            break;
        }
    }
    // Claude Code's own picker hides SDK runs; so does this list. Sessions
    // MonoCode started are already in its history, or were deleted there.
    if matches!(
        entrypoint.as_deref(),
        Some("sdk-cli" | "sdk-ts" | "sdk-py" | MONOCODE_CLAUDE_ENTRYPOINT)
    ) {
        return None;
    }
    let session_cwd = session_cwd?;
    if !same_dir(&session_cwd, cwd) {
        return None;
    }
    let prompt = prompt?;

    let tail = read_tail(path, len, CLAUDE_TAIL_BYTES).unwrap_or_default();
    let mut custom_title = None;
    let mut ai_title = None;
    let mut summary = None;
    for line in tail.lines() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match str_field(&record, "type") {
            Some("custom-title") => {
                custom_title = str_field(&record, "customTitle").map(str::to_owned)
            }
            Some("ai-title") => ai_title = str_field(&record, "aiTitle").map(str::to_owned),
            Some("summary") => summary = str_field(&record, "summary").map(str::to_owned),
            _ => {}
        }
    }
    let title = custom_title
        .or(ai_title)
        .or(summary)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or(prompt);
    let updated_at = modified_ms(path);
    Some(CliSession {
        harness: "claude".into(),
        provider_session_id: id,
        cwd: session_cwd,
        title: title_from(&title),
        model,
        created_at: created_at.unwrap_or(updated_at),
        updated_at,
        path: path.to_string_lossy().into_owned(),
    })
}

/// The visible text of a user record, or `None` for tool results, slash
/// command echoes and other records the user did not type.
fn claude_user_text(record: &Value) -> Option<String> {
    let content = record.pointer("/message/content")?;
    let text = match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => {
            if blocks
                .iter()
                .any(|block| str_field(block, "type") == Some("tool_result"))
            {
                return None;
            }
            blocks
                .iter()
                .filter(|block| str_field(block, "type") == Some("text"))
                .filter_map(|block| str_field(block, "text"))
                .collect::<Vec<_>>()
                .join("\n")
        }
        _ => return None,
    };
    claude_visible_prompt(&text)
}

fn claude_visible_prompt(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("<local-command")
        || trimmed.starts_with("Caveat:")
        || trimmed.starts_with("[Request interrupted")
        || trimmed.starts_with("<system-reminder>")
    {
        return None;
    }
    if trimmed.contains("<command-name>") {
        let name = between(trimmed, "<command-name>", "</command-name>")?;
        let args = between(trimmed, "<command-args>", "</command-args>").unwrap_or("");
        let command = format!("{} {}", name.trim(), args.trim());
        return Some(command.trim().to_owned());
    }
    Some(trimmed.to_owned())
}

fn read_claude(path: &Path) -> Result<Vec<Entry>, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut entries = Vec::new();
    let mut tools: HashMap<String, usize> = HashMap::new();
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { continue };
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if record.get("isSidechain").and_then(Value::as_bool) == Some(true)
            || record.get("isMeta").and_then(Value::as_bool) == Some(true)
            || record.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
        {
            continue;
        }
        let at = record.get("timestamp").and_then(timestamp_ms);
        match str_field(&record, "type") {
            Some("assistant") => {
                let Some(blocks) = record.pointer("/message/content").and_then(Value::as_array)
                else {
                    continue;
                };
                for block in blocks {
                    match str_field(block, "type") {
                        Some("text") => {
                            push_text(&mut entries, "assistant", str_field(block, "text"), at)
                        }
                        Some("thinking") => {
                            push_text(&mut entries, "reasoning", str_field(block, "thinking"), at)
                        }
                        Some("tool_use") => {
                            let Some(id) = str_field(block, "id") else {
                                continue;
                            };
                            let name = str_field(block, "name").unwrap_or("Tool").to_owned();
                            let input = block.get("input").cloned();
                            let command = input
                                .as_ref()
                                .and_then(|input| str_field(input, "command"))
                                .filter(|_| name == "Bash")
                                .map(str::to_owned);
                            tools.insert(id.to_owned(), entries.len());
                            entries.push(Entry::Tool(ToolEntry {
                                id: id.to_owned(),
                                name,
                                input,
                                command,
                                at,
                                ..ToolEntry::default()
                            }));
                        }
                        _ => {}
                    }
                }
            }
            Some("user") => match record.pointer("/message/content") {
                Some(Value::Array(blocks)) => {
                    for block in blocks {
                        match str_field(block, "type") {
                            Some("tool_result") => {
                                let Some(index) =
                                    str_field(block, "tool_use_id").and_then(|id| tools.get(id))
                                else {
                                    continue;
                                };
                                if let Some(Entry::Tool(tool)) = entries.get_mut(*index) {
                                    tool.output = Some(clip(
                                        &content_text(block.get("content")),
                                        MAX_TOOL_OUTPUT,
                                    ));
                                    tool.failed = block.get("is_error").and_then(Value::as_bool)
                                        == Some(true);
                                }
                            }
                            Some("text") => {
                                let text = str_field(block, "text").and_then(claude_visible_prompt);
                                push_text(&mut entries, "user", text.as_deref(), at);
                            }
                            _ => {}
                        }
                    }
                }
                Some(Value::String(text)) => {
                    let text = claude_visible_prompt(text);
                    push_text(&mut entries, "user", text.as_deref(), at);
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok(entries)
}

// ---------------------------------------------------------------------------
// Codex: <home>/sessions/YYYY/MM/DD/rollout-<time>-<thread id>.jsonl

/// Thread names set with `/rename` or picked by Codex. The last line wins.
fn codex_thread_names(root: &Path) -> HashMap<String, String> {
    let mut names = HashMap::new();
    let Ok(file) = File::open(root.join("session_index.jsonl")) else {
        return names;
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let (Some(id), Some(name)) =
            (str_field(&record, "id"), str_field(&record, "thread_name"))
        {
            if !name.trim().is_empty() {
                names.insert(id.to_owned(), name.to_owned());
            }
        }
    }
    names
}

fn list_codex(root: &Path, cwd: &str, titles: &HashMap<String, String>) -> Vec<CliSession> {
    let mut files = Vec::new();
    collect_files(&root.join("sessions"), 4, &mut |path| {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if name.starts_with("rollout-") && name.ends_with(".jsonl") {
            files.push(path.to_path_buf());
        }
    });
    let mut sessions = Vec::new();
    for path in files {
        let Some(meta) =
            first_line(&path).and_then(|line| serde_json::from_str::<Value>(&line).ok())
        else {
            continue;
        };
        if str_field(&meta, "type") != Some("session_meta") {
            continue;
        }
        let Some(payload) = meta.get("payload") else {
            continue;
        };
        let Some(session_cwd) = str_field(payload, "cwd") else {
            continue;
        };
        if !same_dir(session_cwd, cwd) {
            continue;
        }
        // `codex resume` lists interactive threads only. Subagents carry an
        // object here and `codex exec` runs carry "exec".
        if !matches!(
            payload.get("source").and_then(Value::as_str),
            Some("cli" | "vscode")
        ) {
            continue;
        }
        let Some(id) = str_field(payload, "id").filter(|id| is_session_id(id)) else {
            continue;
        };
        // MonoCode's own threads (`monocode`, `monocode-text`) are already in
        // its history, or were deleted there.
        if str_field(payload, "originator")
            .is_some_and(|originator| originator.starts_with("monocode"))
        {
            continue;
        }
        let (prompt, model) = codex_prompt_and_model(&path);
        let Some(prompt) = prompt else { continue };
        let title = titles.get(id).cloned().unwrap_or(prompt);
        let updated_at = modified_ms(&path);
        let created_at = payload
            .get("timestamp")
            .or_else(|| meta.get("timestamp"))
            .and_then(timestamp_ms)
            .unwrap_or(updated_at);
        sessions.push(CliSession {
            harness: "codex".into(),
            provider_session_id: id.to_owned(),
            cwd: session_cwd.to_owned(),
            title: title_from(&title),
            model,
            created_at,
            updated_at,
            path: path.to_string_lossy().into_owned(),
        });
    }
    sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at));
    sessions.truncate(MAX_LISTED);
    sessions
}

fn codex_prompt_and_model(path: &Path) -> (Option<String>, Option<String>) {
    let Ok(file) = File::open(path) else {
        return (None, None);
    };
    let mut prompt = None;
    let mut model = None;
    for line in BufReader::new(file).lines().take(400).map_while(Result::ok) {
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let payload = record.get("payload");
        match str_field(&record, "type") {
            Some("turn_context") if model.is_none() => {
                model = payload
                    .and_then(|p| str_field(p, "model"))
                    .map(str::to_owned);
            }
            Some("event_msg") if prompt.is_none() => {
                let Some(payload) = payload else { continue };
                prompt = match str_field(payload, "type") {
                    Some("user_message") => {
                        str_field(payload, "message").and_then(codex_visible_prompt)
                    }
                    Some("item_completed") => payload
                        .get("item")
                        .filter(|item| str_field(item, "type") == Some("UserMessage"))
                        .map(|item| content_text(item.get("content")))
                        .and_then(|text| codex_visible_prompt(&text)),
                    _ => None,
                };
            }
            Some("response_item") if prompt.is_none() => {
                let Some(payload) = payload else { continue };
                if str_field(payload, "type") == Some("message")
                    && str_field(payload, "role") == Some("user")
                {
                    prompt = codex_visible_prompt(&content_text(payload.get("content")));
                }
            }
            _ => {}
        }
        if prompt.is_some() && model.is_some() {
            break;
        }
    }
    (prompt, model)
}

/// Codex injects AGENTS.md, environment context and similar wrappers as user
/// messages. Those all start with a tag or a fixed heading.
fn codex_visible_prompt(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('<')
        || trimmed.starts_with("# AGENTS.md instructions")
    {
        return None;
    }
    Some(trimmed.to_owned())
}

fn read_codex(path: &Path) -> Result<Vec<Entry>, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let records: Vec<Value> = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect();
    // Newer Codex builds log each finished turn item, which carries the real
    // shell command behind code-mode scripts. Older builds only have the raw
    // model items. A thread started on an older build and resumed on a newer
    // one has both, so the turns before the first item log are read the old
    // way and the rest the new way.
    let first_item = records.iter().position(|record| {
        str_field(record, "type") == Some("event_msg")
            && record.pointer("/payload/type").and_then(Value::as_str) == Some("item_completed")
    });
    let Some(first_item) = first_item else {
        return Ok(codex_entries_from_responses(&records));
    };
    let split = records[..first_item]
        .iter()
        .rposition(is_codex_turn_start)
        .unwrap_or(0);
    let mut entries = codex_entries_from_responses(&records[..split]);
    entries.extend(codex_entries_from_items(&records[split..]));
    Ok(entries)
}

/// The record that opens a turn: `turn_context`, or the `task_started` event.
fn is_codex_turn_start(record: &Value) -> bool {
    match str_field(record, "type") {
        Some("turn_context") => true,
        Some("event_msg") => {
            record.pointer("/payload/type").and_then(Value::as_str) == Some("task_started")
        }
        _ => false,
    }
}

fn codex_entries_from_items(records: &[Value]) -> Vec<Entry> {
    let mut entries = Vec::new();
    for record in records {
        if str_field(record, "type") != Some("event_msg") {
            continue;
        }
        let Some(payload) = record.get("payload") else {
            continue;
        };
        if str_field(payload, "type") != Some("item_completed") {
            continue;
        }
        let Some(item) = payload.get("item") else {
            continue;
        };
        let at = payload
            .get("completed_at_ms")
            .and_then(Value::as_i64)
            .or_else(|| record.get("timestamp").and_then(timestamp_ms));
        let id = str_field(item, "id").unwrap_or("").to_owned();
        match str_field(item, "type") {
            Some("UserMessage") => {
                let text = codex_visible_prompt(&content_text(item.get("content")));
                push_text(&mut entries, "user", text.as_deref(), at);
            }
            Some("AgentMessage") => {
                let text = content_text(item.get("content"));
                push_text(&mut entries, "assistant", Some(&text), at);
            }
            Some("Reasoning") => {
                let text = string_list(item.get("summary_text")).join("\n\n");
                push_text(&mut entries, "reasoning", Some(&text), at);
            }
            Some("CommandExecution") => {
                let command = codex_command(item.get("command")).unwrap_or_default();
                let output = str_field(item, "aggregated_output")
                    .or_else(|| str_field(item, "formatted_output"))
                    .unwrap_or("");
                let failed = str_field(item, "status") == Some("failed")
                    || item
                        .get("exit_code")
                        .and_then(Value::as_i64)
                        .is_some_and(|code| code != 0);
                entries.push(Entry::Tool(ToolEntry {
                    id,
                    name: "exec_command".into(),
                    title: Some(command.clone()),
                    kind: Some("execute".into()),
                    command: Some(command),
                    output: Some(clip(output, MAX_TOOL_OUTPUT)),
                    failed,
                    at,
                    ..ToolEntry::default()
                }));
            }
            Some("FileChange") => {
                let paths: Vec<String> = item
                    .get("changes")
                    .and_then(Value::as_object)
                    .map(|changes| changes.keys().cloned().collect())
                    .unwrap_or_default();
                entries.push(Entry::Tool(ToolEntry {
                    id,
                    name: "apply_patch".into(),
                    title: Some(edit_title(&paths)),
                    kind: Some("edit".into()),
                    paths,
                    output: str_field(item, "stdout").map(|text| clip(text, MAX_TOOL_OUTPUT)),
                    failed: str_field(item, "status") == Some("failed"),
                    at,
                    ..ToolEntry::default()
                }));
            }
            Some("McpToolCall") => {
                let server = str_field(item, "server").unwrap_or("mcp");
                let tool = str_field(item, "tool").unwrap_or("tool");
                entries.push(Entry::Tool(ToolEntry {
                    id,
                    name: format!("{server}.{tool}"),
                    title: Some(format!("{server}: {tool}")),
                    kind: Some("mcp".into()),
                    input: item.get("arguments").cloned(),
                    output: item
                        .get("result")
                        .map(|result| clip(&content_text(result.get("content")), MAX_TOOL_OUTPUT)),
                    failed: str_field(item, "status") == Some("failed"),
                    at,
                    ..ToolEntry::default()
                }));
            }
            Some("Extension") if str_field(item, "kind") == Some("web.search") => {
                let query = str_field(item, "query").unwrap_or("").to_owned();
                entries.push(Entry::Tool(ToolEntry {
                    id,
                    name: "web_search".into(),
                    title: Some(format!("Search {query}").trim().to_owned()),
                    kind: Some("search".into()),
                    at,
                    ..ToolEntry::default()
                }));
            }
            Some("WebSearch") => {
                let query = str_field(item, "query").unwrap_or("").to_owned();
                entries.push(Entry::Tool(ToolEntry {
                    id,
                    name: "web_search".into(),
                    title: Some(format!("Search {query}").trim().to_owned()),
                    kind: Some("search".into()),
                    at,
                    ..ToolEntry::default()
                }));
            }
            Some("ImageView") => {
                let path = str_field(item, "path").unwrap_or("").to_owned();
                entries.push(Entry::Tool(ToolEntry {
                    id,
                    name: "view_image".into(),
                    title: Some(format!("View {}", file_name(&path)).trim().to_owned()),
                    kind: Some("read".into()),
                    paths: if path.is_empty() {
                        Vec::new()
                    } else {
                        vec![path]
                    },
                    at,
                    ..ToolEntry::default()
                }));
            }
            _ => {}
        }
    }
    entries
}

fn codex_entries_from_responses(records: &[Value]) -> Vec<Entry> {
    // Older builds log the user's typed text as an event as well as a model
    // input item. The event never carries injected context, so prefer it.
    let has_user_events = records.iter().any(|record| {
        str_field(record, "type") == Some("event_msg")
            && record.pointer("/payload/type").and_then(Value::as_str) == Some("user_message")
    });
    let mut entries = Vec::new();
    let mut tools: HashMap<String, usize> = HashMap::new();
    for record in records {
        let Some(payload) = record.get("payload") else {
            continue;
        };
        let at = record.get("timestamp").and_then(timestamp_ms);
        match (str_field(record, "type"), str_field(payload, "type")) {
            (Some("event_msg"), Some("user_message")) => {
                let text = str_field(payload, "message").and_then(codex_visible_prompt);
                push_text(&mut entries, "user", text.as_deref(), at);
            }
            (Some("response_item"), Some("message")) => {
                let text = content_text(payload.get("content"));
                match str_field(payload, "role") {
                    Some("user") if !has_user_events => {
                        let text = codex_visible_prompt(&text);
                        push_text(&mut entries, "user", text.as_deref(), at);
                    }
                    Some("assistant") => push_text(&mut entries, "assistant", Some(&text), at),
                    _ => {}
                }
            }
            (Some("response_item"), Some("reasoning")) => {
                let text = payload
                    .get("summary")
                    .and_then(Value::as_array)
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|part| str_field(part, "text"))
                            .collect::<Vec<_>>()
                            .join("\n\n")
                    })
                    .unwrap_or_default();
                push_text(&mut entries, "reasoning", Some(&text), at);
            }
            (Some("response_item"), Some("function_call")) => {
                let id = str_field(payload, "call_id").unwrap_or("").to_owned();
                let name = str_field(payload, "name").unwrap_or("tool").to_owned();
                let arguments = str_field(payload, "arguments")
                    .and_then(|text| serde_json::from_str::<Value>(text).ok());
                let command = arguments.as_ref().and_then(|args| {
                    str_field(args, "cmd")
                        .map(str::to_owned)
                        .or_else(|| codex_command(args.get("command")))
                });
                let tool = match command {
                    Some(command) => ToolEntry {
                        title: Some(command.clone()),
                        kind: Some("execute".into()),
                        command: Some(command),
                        ..ToolEntry::default()
                    },
                    None => ToolEntry {
                        title: Some(name.clone()),
                        input: arguments,
                        ..ToolEntry::default()
                    },
                };
                tools.insert(id.clone(), entries.len());
                entries.push(Entry::Tool(ToolEntry {
                    id,
                    name,
                    at,
                    ..tool
                }));
            }
            (Some("response_item"), Some("local_shell_call")) => {
                let id = str_field(payload, "call_id").unwrap_or("").to_owned();
                let command = codex_command(payload.pointer("/action/command")).unwrap_or_default();
                tools.insert(id.clone(), entries.len());
                entries.push(Entry::Tool(ToolEntry {
                    id,
                    name: "shell".into(),
                    title: Some(command.clone()),
                    kind: Some("execute".into()),
                    command: Some(command),
                    at,
                    ..ToolEntry::default()
                }));
            }
            (Some("response_item"), Some("custom_tool_call")) => {
                let id = str_field(payload, "call_id").unwrap_or("").to_owned();
                let name = str_field(payload, "name").unwrap_or("tool").to_owned();
                let input = str_field(payload, "input").unwrap_or("");
                let tool = if name == "apply_patch" {
                    let paths = patch_paths(input);
                    ToolEntry {
                        title: Some(edit_title(&paths)),
                        kind: Some("edit".into()),
                        paths,
                        ..ToolEntry::default()
                    }
                } else {
                    ToolEntry {
                        title: Some(if name == "exec" {
                            "Run script".into()
                        } else {
                            name.clone()
                        }),
                        kind: Some(if name == "exec" {
                            "execute".into()
                        } else {
                            name.clone()
                        }),
                        input: Some(Value::String(clip(input, MAX_TOOL_OUTPUT))),
                        ..ToolEntry::default()
                    }
                };
                tools.insert(id.clone(), entries.len());
                entries.push(Entry::Tool(ToolEntry {
                    id,
                    name,
                    at,
                    ..tool
                }));
            }
            (Some("response_item"), Some("function_call_output" | "custom_tool_call_output")) => {
                let Some(index) = str_field(payload, "call_id").and_then(|id| tools.get(id)) else {
                    continue;
                };
                let (output, failed) = codex_output(payload.get("output"));
                if let Some(Entry::Tool(tool)) = entries.get_mut(*index) {
                    tool.output = Some(clip(&output, MAX_TOOL_OUTPUT));
                    tool.failed = failed;
                }
            }
            (Some("response_item"), Some("web_search_call")) => {
                let query = payload
                    .pointer("/action/query")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                entries.push(Entry::Tool(ToolEntry {
                    id: str_field(payload, "id").unwrap_or("").to_owned(),
                    name: "web_search".into(),
                    title: Some(format!("Search {query}").trim().to_owned()),
                    kind: Some("search".into()),
                    at,
                    ..ToolEntry::default()
                }));
            }
            _ => {}
        }
    }
    entries
}

/// `["/bin/zsh", "-lc", "git status"]` reads as `git status`.
fn codex_command(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(command) => Some(command.clone()),
        Value::Array(parts) => {
            let parts: Vec<&str> = parts.iter().filter_map(Value::as_str).collect();
            if parts.len() >= 3 && matches!(parts[1], "-lc" | "-c") {
                return Some(parts[2..].join(" "));
            }
            Some(parts.join(" ")).filter(|command| !command.is_empty())
        }
        _ => None,
    }
}

/// Older tool outputs are a JSON string like `{"output": ..., "metadata":
/// {"exit_code": 1}}`; newer ones are plain text or content parts.
fn codex_output(value: Option<&Value>) -> (String, bool) {
    match value {
        Some(Value::String(text)) => match serde_json::from_str::<Value>(text) {
            Ok(parsed) if parsed.get("output").is_some() => {
                let output = str_field(&parsed, "output").unwrap_or("").to_owned();
                let failed = parsed
                    .pointer("/metadata/exit_code")
                    .and_then(Value::as_i64)
                    .is_some_and(|code| code != 0);
                (output, failed)
            }
            _ => (text.clone(), false),
        },
        Some(other) => (content_text(Some(other)), false),
        None => (String::new(), false),
    }
}

fn patch_paths(patch: &str) -> Vec<String> {
    patch
        .lines()
        .filter_map(|line| {
            ["*** Update File: ", "*** Add File: ", "*** Delete File: "]
                .iter()
                .find_map(|prefix| line.strip_prefix(prefix))
        })
        .map(|path| path.trim().to_owned())
        .collect()
}

// ---------------------------------------------------------------------------
// Grok: <home>/sessions/<url-encoded cwd>/<session id>/

fn list_grok(root: &Path, cwd: &str) -> Vec<CliSession> {
    let Ok(dirs) = std::fs::read_dir(root.join("sessions")) else {
        return Vec::new();
    };
    let mut sessions = Vec::new();
    for dir in dirs.flatten() {
        let name = dir.file_name().to_string_lossy().into_owned();
        if !same_dir(&percent_decode(&name), cwd) {
            continue;
        }
        let Ok(children) = std::fs::read_dir(dir.path()) else {
            continue;
        };
        for child in children.flatten() {
            if let Some(session) = grok_summary(&child.path(), cwd) {
                sessions.push(session);
            }
        }
    }
    sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at));
    sessions.truncate(MAX_LISTED);
    sessions
}

fn grok_summary(dir: &Path, cwd: &str) -> Option<CliSession> {
    let summary: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("summary.json")).ok()?).ok()?;
    let id = summary
        .pointer("/info/id")
        .and_then(Value::as_str)
        .or_else(|| dir.file_name()?.to_str())?
        .to_owned();
    if !is_session_id(&id) {
        return None;
    }
    // Subagent sessions point at their parent; Grok's own list skips them.
    if summary
        .get("parent_session_id")
        .is_some_and(|value| !value.is_null())
    {
        return None;
    }
    let session_cwd = summary
        .pointer("/info/cwd")
        .and_then(Value::as_str)
        .unwrap_or(cwd)
        .to_owned();
    let prompt = grok_first_prompt(dir)?;
    let title = str_field(&summary, "title")
        .or_else(|| str_field(&summary, "session_summary"))
        .filter(|title| !title.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or(prompt);
    let updated_at = summary
        .get("last_active_at")
        .or_else(|| summary.get("updated_at"))
        .and_then(timestamp_ms)
        .unwrap_or_else(|| modified_ms(&dir.join("summary.json")));
    Some(CliSession {
        harness: "grok".into(),
        provider_session_id: id,
        cwd: session_cwd,
        title: title_from(&title),
        model: str_field(&summary, "current_model_id").map(str::to_owned),
        created_at: summary
            .get("created_at")
            .and_then(timestamp_ms)
            .unwrap_or(updated_at),
        updated_at,
        path: dir.join("summary.json").to_string_lossy().into_owned(),
    })
}

fn grok_first_prompt(dir: &Path) -> Option<String> {
    let file = File::open(dir.join("chat_history.jsonl")).ok()?;
    for line in BufReader::new(file).lines().take(200).map_while(Result::ok) {
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if str_field(&record, "type") == Some("user") && record.get("synthetic_reason").is_none() {
            if let Some(text) = grok_visible_prompt(&content_text(record.get("content"))) {
                return Some(text);
            }
        }
    }
    None
}

fn grok_visible_prompt(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let trimmed = between(trimmed, "<user_query>", "</user_query>")
        .unwrap_or(trimmed)
        .trim();
    if trimmed.is_empty() || trimmed.starts_with("<system-reminder>") {
        return None;
    }
    Some(trimmed.to_owned())
}

fn read_grok(summary_path: &Path) -> Result<Vec<Entry>, String> {
    let dir = summary_path.parent().ok_or("Missing Grok session folder")?;
    let from_updates = read_grok_updates(&dir.join("updates.jsonl"));
    if from_updates
        .iter()
        .any(|entry| matches!(entry, Entry::User { .. }))
    {
        return Ok(from_updates);
    }
    read_grok_history(&dir.join("chat_history.jsonl"))
}

/// Grok logs the same ACP `session/update` notifications it streams to a
/// client, user prompts included.
fn read_grok_updates(path: &Path) -> Vec<Entry> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = Vec::new();
    let mut tools: HashMap<String, usize> = HashMap::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if str_field(&record, "method") != Some("session/update") {
            continue;
        }
        let Some(update) = record.pointer("/params/update") else {
            continue;
        };
        let at = record.get("timestamp").and_then(timestamp_ms);
        let chunk = || update.pointer("/content/text").and_then(Value::as_str);
        match str_field(update, "sessionUpdate") {
            Some("user_message_chunk") => append_text(&mut entries, "user", chunk(), at),
            Some("agent_message_chunk") => append_text(&mut entries, "assistant", chunk(), at),
            Some("agent_thought_chunk") => append_text(&mut entries, "reasoning", chunk(), at),
            Some("tool_call") => {
                let Some(id) = str_field(update, "toolCallId") else {
                    continue;
                };
                let tool = ToolEntry {
                    id: id.to_owned(),
                    name: str_field(update, "kind").unwrap_or("tool").to_owned(),
                    title: str_field(update, "title").map(str::to_owned),
                    kind: str_field(update, "kind").map(str::to_owned),
                    input: update.get("rawInput").cloned(),
                    at,
                    ..ToolEntry::default()
                };
                match tools.get(id) {
                    Some(index) => entries[*index] = Entry::Tool(tool),
                    None => {
                        tools.insert(id.to_owned(), entries.len());
                        entries.push(Entry::Tool(tool));
                    }
                }
                apply_acp_tool_update(&mut entries, &tools, id, update);
            }
            Some("tool_call_update") => {
                let Some(id) = str_field(update, "toolCallId") else {
                    continue;
                };
                apply_acp_tool_update(&mut entries, &tools, id, update);
            }
            _ => {}
        }
    }
    for entry in &mut entries {
        if let Entry::User { text, .. } = entry {
            if let Some(visible) = grok_visible_prompt(text) {
                *text = visible;
            }
        }
    }
    entries.retain(|entry| !matches!(entry, Entry::User { text, .. } if text.trim().is_empty()));
    entries
}

fn apply_acp_tool_update(
    entries: &mut [Entry],
    tools: &HashMap<String, usize>,
    id: &str,
    update: &Value,
) {
    let Some(Entry::Tool(tool)) = tools.get(id).and_then(|index| entries.get_mut(*index)) else {
        return;
    };
    if let Some(title) = str_field(update, "title") {
        tool.title = Some(title.to_owned());
    }
    if let Some(kind) = str_field(update, "kind") {
        tool.kind = Some(kind.to_owned());
    }
    if let Some(input) = update.get("rawInput") {
        tool.input = Some(input.clone());
    }
    if str_field(update, "status") == Some("failed") {
        tool.failed = true;
    }
    if let Some(content) = update.get("content").and_then(Value::as_array) {
        let mut texts = Vec::new();
        for item in content {
            match str_field(item, "type") {
                Some("content") => {
                    if let Some(text) = item.pointer("/content/text").and_then(Value::as_str) {
                        texts.push(text.to_owned());
                    }
                }
                Some("diff") => {
                    if let Some(path) = str_field(item, "path") {
                        if !tool.paths.iter().any(|known| known == path) {
                            tool.paths.push(path.to_owned());
                        }
                    }
                }
                _ => {}
            }
        }
        if !texts.is_empty() {
            tool.output = Some(clip(&texts.join("\n"), MAX_TOOL_OUTPUT));
        }
    }
    if tool.output.is_none() {
        if let Some(raw) = update.get("rawOutput") {
            let text = match raw {
                Value::String(text) => text.clone(),
                other => str_field(other, "output")
                    .map(str::to_owned)
                    .unwrap_or_else(|| other.to_string()),
            };
            tool.output = Some(clip(&text, MAX_TOOL_OUTPUT));
        }
    }
}

/// Fallback for sessions without an update log: the model-facing history.
fn read_grok_history(path: &Path) -> Result<Vec<Entry>, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut entries = Vec::new();
    let mut tools: HashMap<String, usize> = HashMap::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let Ok(record) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match str_field(&record, "type") {
            Some("user") if record.get("synthetic_reason").is_none() => {
                let text = grok_visible_prompt(&content_text(record.get("content")));
                push_text(&mut entries, "user", text.as_deref(), None);
            }
            Some("reasoning") => {
                let text = record
                    .get("summary")
                    .and_then(Value::as_array)
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|part| str_field(part, "text"))
                            .collect::<Vec<_>>()
                            .join("\n\n")
                    })
                    .unwrap_or_default();
                push_text(&mut entries, "reasoning", Some(&text), None);
            }
            Some("assistant") => {
                let text = content_text(record.get("content"));
                push_text(&mut entries, "assistant", Some(&text), None);
                for call in record
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let Some(id) = str_field(call, "id") else {
                        continue;
                    };
                    let name = str_field(call, "name").unwrap_or("tool").to_owned();
                    let input = str_field(call, "arguments")
                        .and_then(|text| serde_json::from_str(text).ok());
                    tools.insert(id.to_owned(), entries.len());
                    entries.push(Entry::Tool(ToolEntry {
                        id: id.to_owned(),
                        title: Some(name.clone()),
                        name,
                        input,
                        ..ToolEntry::default()
                    }));
                }
            }
            Some("tool_result") => {
                let Some(index) = str_field(&record, "tool_call_id").and_then(|id| tools.get(id))
                else {
                    continue;
                };
                if let Some(Entry::Tool(tool)) = entries.get_mut(*index) {
                    tool.output = Some(clip(&content_text(record.get("content")), MAX_TOOL_OUTPUT));
                }
            }
            _ => {}
        }
    }
    Ok(entries)
}

// ---------------------------------------------------------------------------
// Helpers

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .or_else(|| str_field(item, "text").map(str::to_owned))
            })
            .collect(),
        Some(Value::String(text)) => vec![text.clone()],
        _ => Vec::new(),
    }
}

/// Text from a string, or from content parts shaped like `{type, text}`.
fn content_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| match part {
                Value::String(text) => Some(text.as_str()),
                _ => str_field(part, "text"),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn push_text(entries: &mut Vec<Entry>, role: &str, text: Option<&str>, at: Option<i64>) {
    let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
        return;
    };
    let text = clip(text, MAX_TEXT);
    entries.push(match role {
        "user" => Entry::User { text, at },
        "reasoning" => Entry::Reasoning { text, at },
        _ => Entry::Assistant { text, at },
    });
}

/// Streamed chunks join onto the entry before them when the role matches.
fn append_text(entries: &mut Vec<Entry>, role: &str, chunk: Option<&str>, at: Option<i64>) {
    let Some(chunk) = chunk.filter(|chunk| !chunk.is_empty()) else {
        return;
    };
    let last = entries.last_mut();
    let target = match (role, last) {
        ("user", Some(Entry::User { text, .. }))
        | ("assistant", Some(Entry::Assistant { text, .. }))
        | ("reasoning", Some(Entry::Reasoning { text, .. })) => Some(text),
        _ => None,
    };
    match target {
        Some(text) => {
            if text.len() < MAX_TEXT {
                text.push_str(chunk);
            }
        }
        None => entries.push(match role {
            "user" => Entry::User {
                text: chunk.to_owned(),
                at,
            },
            "reasoning" => Entry::Reasoning {
                text: chunk.to_owned(),
                at,
            },
            _ => Entry::Assistant {
                text: chunk.to_owned(),
                at,
            },
        }),
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n… (truncated)", &text[..end])
}

fn title_from(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    if line.chars().count() <= MAX_TITLE {
        return line.to_owned();
    }
    let cut: String = line.chars().take(MAX_TITLE - 1).collect();
    format!("{}…", cut.trim_end())
}

fn edit_title(paths: &[String]) -> String {
    match paths {
        [] => "Edit files".into(),
        [one] => format!("Edit {}", file_name(one)),
        many => format!("Edit {} files", many.len()),
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let end = text[start..].find(close)? + start;
    Some(&text[start..end])
}

/// Provider ids end up in `sessions.provider_session_id`, which only takes
/// ASCII letters, digits, `-` and `_`.
fn is_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn same_dir(a: &str, b: &str) -> bool {
    normalize_dir(a) == normalize_dir(b)
}

fn normalize_dir(path: &str) -> String {
    let mut path = strip_private(path).replace('\\', "/");
    while path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    if cfg!(windows) {
        path = path.to_lowercase();
    }
    path
}

/// macOS reports `/tmp` and `/var` as `/private/...` once resolved.
fn strip_private(path: &str) -> String {
    for dir in ["/private/tmp", "/private/var", "/private/etc"] {
        if path == dir || path.starts_with(&format!("{dir}/")) {
            return path["/private".len()..].to_owned();
        }
    }
    path.to_owned()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(value) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                out.push(value);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn first_line(path: &Path) -> Option<String> {
    let mut line = String::new();
    BufReader::new(File::open(path).ok()?)
        .read_line(&mut line)
        .ok()?;
    Some(line)
}

fn read_tail(path: &Path, len: u64, bytes: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    file.seek(SeekFrom::Start(len.saturating_sub(bytes))).ok()?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).ok()?;
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

fn collect_files(dir: &Path, depth: usize, visit: &mut dyn FnMut(&Path)) {
    let Ok(children) = std::fs::read_dir(dir) else {
        return;
    };
    for child in children.flatten() {
        let path = child.path();
        match child.file_type() {
            Ok(kind) if kind.is_dir() && depth > 0 => collect_files(&path, depth - 1, visit),
            Ok(kind) if kind.is_file() => visit(&path),
            _ => {}
        }
    }
}

fn modified_ms(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

/// Epoch milliseconds from an RFC 3339 string or a number of seconds or
/// milliseconds.
fn timestamp_ms(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => {
            let raw = number.as_f64()?;
            Some(if raw > 1e11 {
                raw as i64
            } else {
                (raw * 1000.0) as i64
            })
        }
        Value::String(text) => parse_rfc3339(text),
        _ => None,
    }
}

fn parse_rfc3339(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let num = |range: std::ops::Range<usize>| -> Option<i64> { text.get(range)?.parse().ok() };
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let mut rest = &text[19..];
    let mut millis = 0;
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        let padded = format!("{:0<3}", &fraction[..digits.min(3)]);
        millis = padded.parse().ok()?;
        rest = &fraction[digits..];
    }
    let offset_minutes = match rest {
        "" | "Z" | "z" => 0,
        zone => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let zone = zone.get(1..)?;
            let hours: i64 = zone.get(0..2)?.parse().ok()?;
            let minutes: i64 = zone
                .get(3..5)
                .or_else(|| zone.get(2..4))
                .and_then(|m| m.parse().ok())
                .unwrap_or(0);
            sign * (hours * 60 + minutes)
        }
    };
    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second - offset_minutes * 60;
    Some(seconds * 1000 + millis)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "monocode-cli-sessions-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_lines(path: &Path, lines: &[Value]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = File::create(path).unwrap();
        for line in lines {
            writeln!(file, "{line}").unwrap();
        }
    }

    const CWD: &str = "/Users/me/code/app";

    #[test]
    fn parses_rfc3339_timestamps() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_rfc3339("2026-10-01T16:52:21.001667Z"),
            Some(1_790_873_541_001)
        );
        assert_eq!(
            parse_rfc3339("2026-10-01T18:52:21.5+02:00"),
            Some(1_790_873_541_500)
        );
        assert_eq!(
            timestamp_ms(&serde_json::json!(1_790_873_541)),
            Some(1_790_873_541_000)
        );
    }

    #[test]
    fn serializes_tool_kind_apart_from_the_tag() {
        let entry = Entry::Tool(ToolEntry {
            id: "t".into(),
            name: "Bash".into(),
            kind: Some("execute".into()),
            ..ToolEntry::default()
        });
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["kind"], "tool");
        assert_eq!(json["toolKind"], "execute");
    }

    #[test]
    fn decodes_grok_folder_names() {
        assert_eq!(
            percent_decode("%2FUsers%2Fme%2Fmy%20app"),
            "/Users/me/my app"
        );
        assert_eq!(percent_decode("100%"), "100%");
    }

    #[test]
    fn matches_private_tmp_paths() {
        assert!(same_dir("/private/tmp/app/", "/tmp/app"));
        assert!(!same_dir("/tmp/app", "/tmp/app2"));
    }

    fn claude_fixture(root: &Path, id: &str, entrypoint: &str) -> PathBuf {
        let path = root
            .join("projects")
            .join(claude_project_slug(CWD))
            .join(format!("{id}.jsonl"));
        write_lines(
            &path,
            &[
                serde_json::json!({"type": "queue-operation"}),
                serde_json::json!({"type": "user", "cwd": CWD, "entrypoint": entrypoint, "timestamp": "2026-01-01T00:00:00Z", "message": {"role": "user", "content": "Fix the login bug"}}),
                serde_json::json!({"type": "assistant", "cwd": CWD, "entrypoint": entrypoint, "timestamp": "2026-01-01T00:00:01Z", "message": {"model": "claude-opus-5-5", "content": [
                    {"type": "thinking", "thinking": "Look at auth.ts"},
                    {"type": "text", "text": "Checking the code."},
                    {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "git status"}}
                ]}}),
                serde_json::json!({"type": "user", "cwd": CWD, "timestamp": "2026-01-01T00:00:02Z", "message": {"content": [
                    {"type": "tool_result", "tool_use_id": "toolu_1", "content": "clean", "is_error": false}
                ]}}),
                serde_json::json!({"type": "user", "isSidechain": true, "message": {"content": "subagent prompt"}}),
                serde_json::json!({"type": "user", "isMeta": true, "message": {"content": "meta"}}),
                serde_json::json!({"type": "user", "message": {"content": "<command-name>/review</command-name><command-args>42</command-args>"}}),
                serde_json::json!({"type": "ai-title", "aiTitle": "Login bug fix"}),
            ],
        );
        path
    }

    #[test]
    fn lists_terminal_claude_sessions_and_hides_sdk_runs() {
        let root = temp_root("claude-list");
        claude_fixture(&root, "11111111-1111-1111-1111-111111111111", "cli");
        claude_fixture(&root, "22222222-2222-2222-2222-222222222222", "sdk-cli");
        claude_fixture(&root, "55555555-5555-5555-5555-555555555555", "monocode");
        let roots = Roots {
            claude: vec![root.clone()],
            ..Roots::default()
        };
        let sessions = list_sessions(&roots, CWD);
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.harness, "claude");
        assert_eq!(
            session.provider_session_id,
            "11111111-1111-1111-1111-111111111111"
        );
        assert_eq!(session.title, "Login bug fix");
        assert_eq!(session.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(
            session.created_at,
            parse_rfc3339("2026-01-01T00:00:00Z").unwrap()
        );
        assert!(list_sessions(&roots, "/Users/me/code/other").is_empty());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn reads_a_claude_transcript() {
        let root = temp_root("claude-read");
        let path = claude_fixture(&root, "33333333-3333-3333-3333-333333333333", "cli");
        let roots = Roots {
            claude: vec![root.clone()],
            ..Roots::default()
        };
        let entries = read_session(&roots, "claude", &path).unwrap();
        assert!(matches!(&entries[0], Entry::User { text, .. } if text == "Fix the login bug"));
        assert!(matches!(&entries[1], Entry::Reasoning { text, .. } if text == "Look at auth.ts"));
        assert!(
            matches!(&entries[2], Entry::Assistant { text, .. } if text == "Checking the code.")
        );
        let Entry::Tool(tool) = &entries[3] else {
            panic!("expected a tool")
        };
        assert_eq!(tool.name, "Bash");
        assert_eq!(tool.command.as_deref(), Some("git status"));
        assert_eq!(tool.output.as_deref(), Some("clean"));
        assert!(matches!(&entries[4], Entry::User { text, .. } if text == "/review 42"));
        assert_eq!(entries.len(), 5);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn refuses_files_outside_the_roots() {
        let root = temp_root("claude-outside");
        let other = temp_root("claude-elsewhere");
        let path = claude_fixture(&other, "44444444-4444-4444-4444-444444444444", "cli");
        let roots = Roots {
            claude: vec![root.clone()],
            ..Roots::default()
        };
        assert!(read_session(&roots, "claude", &path).is_err());
        std::fs::remove_dir_all(root).ok();
        std::fs::remove_dir_all(other).ok();
    }

    fn codex_meta(id: &str, source: Value) -> Value {
        serde_json::json!({"timestamp": "2026-02-01T10:00:00Z", "type": "session_meta", "payload": {
            "id": id, "cwd": CWD, "source": source, "originator": "codex-tui", "timestamp": "2026-02-01T10:00:00Z"
        }})
    }

    #[test]
    fn lists_and_reads_older_codex_rollouts() {
        let root = temp_root("codex-old");
        let id = "019e0df8-c566-7360-87c2-3f5440d5f2b4";
        let path = root.join(format!(
            "sessions/2026/02/01/rollout-2026-02-01T10-00-00-{id}.jsonl"
        ));
        write_lines(
            &path,
            &[
                codex_meta(id, serde_json::json!("cli")),
                serde_json::json!({"type": "turn_context", "payload": {"model": "gpt-5.5"}}),
                serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "<environment_context>x</environment_context>"}]}}),
                serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Add a test"}]}}),
                serde_json::json!({"type": "event_msg", "payload": {"type": "user_message", "message": "Add a test"}}),
                serde_json::json!({"type": "response_item", "payload": {"type": "reasoning", "summary": [{"type": "summary_text", "text": "Plan it"}]}}),
                serde_json::json!({"type": "response_item", "payload": {"type": "function_call", "name": "shell", "call_id": "c1", "arguments": "{\"command\":[\"bash\",\"-lc\",\"cargo test\"]}"}}),
                serde_json::json!({"type": "response_item", "payload": {"type": "function_call_output", "call_id": "c1", "output": "{\"output\":\"1 failed\",\"metadata\":{\"exit_code\":101}}"}}),
                serde_json::json!({"type": "response_item", "payload": {"type": "custom_tool_call", "name": "apply_patch", "call_id": "c2", "input": "*** Begin Patch\n*** Update File: src/lib.rs\n*** End Patch"}}),
                serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Done."}]}}),
            ],
        );
        let own = "019e0df8-c566-7360-87c2-3f5440d5f2b6";
        let mut own_meta = codex_meta(own, serde_json::json!("vscode"));
        own_meta["payload"]["originator"] = serde_json::json!("monocode");
        write_lines(
            &root.join(format!(
                "sessions/2026/02/01/rollout-2026-02-01T10-00-02-{own}.jsonl"
            )),
            &[
                own_meta,
                serde_json::json!({"type": "event_msg", "payload": {"type": "user_message", "message": "Hi"}}),
            ],
        );
        let subagent = "019e0df8-c566-7360-87c2-3f5440d5f2b5";
        write_lines(
            &root.join(format!(
                "sessions/2026/02/01/rollout-2026-02-01T10-00-01-{subagent}.jsonl"
            )),
            &[codex_meta(
                subagent,
                serde_json::json!({"subagent": "review"}),
            )],
        );
        write_lines(
            &root.join("session_index.jsonl"),
            &[serde_json::json!({"id": id, "thread_name": "Test coverage"})],
        );
        let roots = Roots {
            codex: vec![root.clone()],
            ..Roots::default()
        };
        let sessions = list_sessions(&roots, CWD);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "Test coverage");
        assert_eq!(sessions[0].model.as_deref(), Some("gpt-5.5"));

        let entries = read_session(&roots, "codex", Path::new(&sessions[0].path)).unwrap();
        assert!(matches!(&entries[0], Entry::User { text, .. } if text == "Add a test"));
        assert!(matches!(&entries[1], Entry::Reasoning { text, .. } if text == "Plan it"));
        let Entry::Tool(shell) = &entries[2] else {
            panic!("expected a tool")
        };
        assert_eq!(shell.command.as_deref(), Some("cargo test"));
        assert!(shell.failed);
        assert_eq!(shell.output.as_deref(), Some("1 failed"));
        let Entry::Tool(edit) = &entries[3] else {
            panic!("expected an edit")
        };
        assert_eq!(edit.paths, vec!["src/lib.rs".to_owned()]);
        assert_eq!(edit.title.as_deref(), Some("Edit lib.rs"));
        assert!(matches!(&entries[4], Entry::Assistant { text, .. } if text == "Done."));
        assert_eq!(entries.len(), 5);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn reads_newer_codex_items() {
        let root = temp_root("codex-new");
        let id = "01a0fe87-a600-7432-af6c-182db08f7c4e";
        let path = root.join(format!(
            "sessions/2026/10/02/rollout-2026-10-02T17-31-37-{id}.jsonl"
        ));
        let item = |item: Value| serde_json::json!({"type": "event_msg", "payload": {"type": "item_completed", "item": item, "completed_at_ms": 1_000}});
        write_lines(
            &path,
            &[
                codex_meta(id, serde_json::json!("vscode")),
                serde_json::json!({"type": "response_item", "payload": {"type": "custom_tool_call", "name": "exec", "call_id": "x", "input": "await tools.exec_command({cmd: 'ls'})"}}),
                item(
                    serde_json::json!({"type": "UserMessage", "id": "u", "content": [{"type": "text", "text": "List files"}]}),
                ),
                item(
                    serde_json::json!({"type": "CommandExecution", "id": "e", "command": ["/bin/zsh", "-lc", "ls"], "aggregated_output": "a\nb", "exit_code": 0, "status": "completed"}),
                ),
                item(
                    serde_json::json!({"type": "FileChange", "id": "f", "changes": {"/a/b.rs": {"type": "update"}}, "status": "completed"}),
                ),
                item(
                    serde_json::json!({"type": "AgentMessage", "id": "m", "content": [{"type": "Text", "text": "Two files."}]}),
                ),
            ],
        );
        let roots = Roots {
            codex: vec![root.clone()],
            ..Roots::default()
        };
        let entries = read_session(&roots, "codex", &path).unwrap();
        assert_eq!(entries.len(), 4);
        assert!(
            matches!(&entries[0], Entry::User { text, at: Some(1_000) } if text == "List files")
        );
        let Entry::Tool(command) = &entries[1] else {
            panic!("expected a command")
        };
        assert_eq!(command.command.as_deref(), Some("ls"));
        assert_eq!(command.output.as_deref(), Some("a\nb"));
        let Entry::Tool(edit) = &entries[2] else {
            panic!("expected an edit")
        };
        assert_eq!(edit.paths, vec!["/a/b.rs".to_owned()]);
        assert!(matches!(&entries[3], Entry::Assistant { text, .. } if text == "Two files."));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn keeps_older_turns_of_a_thread_resumed_on_a_newer_codex() {
        let root = temp_root("codex-mixed");
        let id = "01a0fe87-a600-7432-af6c-182db08f7c4f";
        let path = root.join(format!(
            "sessions/2026/10/02/rollout-2026-10-02T17-31-38-{id}.jsonl"
        ));
        let item = |item: Value| serde_json::json!({"type": "event_msg", "payload": {"type": "item_completed", "item": item}});
        write_lines(
            &path,
            &[
                codex_meta(id, serde_json::json!("cli")),
                serde_json::json!({"type": "turn_context", "payload": {"model": "gpt-5"}}),
                serde_json::json!({"type": "event_msg", "payload": {"type": "user_message", "message": "Old question"}}),
                serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Old answer"}]}}),
                serde_json::json!({"type": "turn_context", "payload": {"model": "gpt-6"}}),
                serde_json::json!({"type": "response_item", "payload": {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "New answer"}]}}),
                item(
                    serde_json::json!({"type": "UserMessage", "id": "u", "content": [{"type": "text", "text": "New question"}]}),
                ),
                item(
                    serde_json::json!({"type": "AgentMessage", "id": "m", "content": [{"type": "Text", "text": "New answer"}]}),
                ),
            ],
        );
        let roots = Roots {
            codex: vec![root.clone()],
            ..Roots::default()
        };
        let entries = read_session(&roots, "codex", &path).unwrap();
        let texts: Vec<&str> = entries
            .iter()
            .map(|entry| match entry {
                Entry::User { text, .. } | Entry::Assistant { text, .. } => text.as_str(),
                _ => "",
            })
            .collect();
        assert_eq!(
            texts,
            vec!["Old question", "Old answer", "New question", "New answer"]
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn lists_and_reads_grok_sessions() {
        let root = temp_root("grok");
        let id = "01a0f861-9962-71b2-a630-47b9e2280a45";
        let dir = root.join("sessions/%2FUsers%2Fme%2Fcode%2Fapp").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("summary.json"),
            serde_json::json!({"info": {"id": id, "cwd": CWD}, "created_at": "2026-10-01T16:52:21Z", "updated_at": "2026-10-01T17:13:04Z", "current_model_id": "grok-4.7"}).to_string(),
        )
        .unwrap();
        write_lines(
            &dir.join("chat_history.jsonl"),
            &[
                serde_json::json!({"type": "system", "content": "You are Grok"}),
                serde_json::json!({"type": "user", "content": [{"type": "text", "text": "Rename the flag"}], "prompt_index": 0}),
            ],
        );
        let update = |update: Value| serde_json::json!({"timestamp": 1_790_873_541, "method": "session/update", "params": {"sessionId": id, "update": update}});
        write_lines(
            &dir.join("updates.jsonl"),
            &[
                serde_json::json!({"timestamp": 1_790_873_541, "method": "_x.ai/session/update", "params": {"update": {"sessionUpdate": "hook_execution"}}}),
                update(
                    serde_json::json!({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "Rename "}}),
                ),
                update(
                    serde_json::json!({"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "the flag"}}),
                ),
                update(
                    serde_json::json!({"sessionUpdate": "agent_thought_chunk", "content": {"type": "text", "text": "Search first"}}),
                ),
                update(
                    serde_json::json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "grep flag", "kind": "search", "status": "pending"}),
                ),
                update(
                    serde_json::json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed", "content": [{"type": "content", "content": {"type": "text", "text": "src/flag.rs"}}]}),
                ),
                update(
                    serde_json::json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Renamed "}}),
                ),
                update(
                    serde_json::json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "it."}}),
                ),
            ],
        );
        let roots = Roots {
            grok: vec![root.clone()],
            ..Roots::default()
        };
        let sessions = list_sessions(&roots, CWD);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "Rename the flag");
        assert_eq!(sessions[0].model.as_deref(), Some("grok-4.7"));

        let entries = read_session(&roots, "grok", Path::new(&sessions[0].path)).unwrap();
        assert_eq!(entries.len(), 4);
        assert!(matches!(&entries[0], Entry::User { text, .. } if text == "Rename the flag"));
        assert!(matches!(&entries[1], Entry::Reasoning { text, .. } if text == "Search first"));
        let Entry::Tool(tool) = &entries[2] else {
            panic!("expected a tool")
        };
        assert_eq!(tool.title.as_deref(), Some("grep flag"));
        assert_eq!(tool.output.as_deref(), Some("src/flag.rs"));
        assert!(matches!(&entries[3], Entry::Assistant { text, .. } if text == "Renamed it."));
        std::fs::remove_dir_all(root).ok();
    }
}
