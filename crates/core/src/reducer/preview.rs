//! Port of src/integrations/harness/core/preview.ts: tool preview
//! normalization and tool titles.
//!
//! Provider payloads arrive as `serde_json` values. `Record<string, unknown>`
//! becomes [`Record`], and `unknown` becomes `&Value`.
//!
//! `serde_json` maps keep keys sorted unless its `preserve_order` feature is
//! on, so the fallback that searches every field for a path
//! (`findPathInUnknown`) can visit fields in a different order than
//! JavaScript's insertion order.

use std::borrow::Cow;

use serde_json::{Map, Value};

use crate::block::{Extra, ToolPreview, ToolPreviewKind, ToolPreviewLine, ToolPreviewLineKind};
use crate::js;
use crate::paths::display_path;
use crate::reducer::js_regex::{
    decode_uri_component, is_word_byte, js_regex, nonempty, starts_with_word_ci,
    starts_with_word_then_text,
};
use crate::reducer::shell_intent::{
    ShellVerb, format_shell_intent, infer_shell_intent, rewrite_readable_title,
    unwrap_shell_command,
};

/// `Record<string, unknown>`.
pub type Record = Map<String, Value>;

pub const MAX_PREVIEW_LINES: usize = 6;
pub const MAX_LINE_CHARS: usize = 120;

/// `extractToolPreview`.
pub fn extract_tool_preview(update: &Record, tool: &Record) -> Option<ToolPreview> {
    let title = coerce_string(update.get("title"))
        .or_else(|| coerce_string(tool.get("title")))
        .or_else(|| coerce_string(update.get("name")))
        .or_else(|| coerce_string(tool.get("name")));
    let raw_kind = coerce_string(update.get("kind"))
        .or_else(|| coerce_string(tool.get("kind")))
        .unwrap_or_default()
        .to_lowercase();
    let inputs = input_records(&[
        nullish(update, &["rawInput", "raw_input", "input"]).map(Source::Value),
        nullish(tool, &["rawInput", "raw_input", "input"]).map(Source::Value),
        nullish(update, &["arguments", "args", "params"]).map(Source::Value),
        nullish(tool, &["arguments", "args", "params"]).map(Source::Value),
        Some(Source::Record(update)),
        Some(Source::Record(tool)),
    ]);
    let content = nullish(update, &["content"]).or_else(|| nullish(tool, &["content"]));
    let diff = extract_diff(content);
    let path = extract_path(
        update,
        tool,
        &inputs,
        diff.as_ref().and_then(|diff| diff.path.as_deref()),
    );
    let start_line = location_line(nullish(update, &["locations", "location"]))
        .or_else(|| location_line(nullish(tool, &["locations", "location"])))
        .or_else(|| first_number(&inputs, "line"))
        .or_else(|| first_number(&inputs, "offset"));
    let has_path = nonempty(path.as_deref()).is_some();
    let kind = preview_kind(&raw_kind, title.as_deref(), diff.is_some(), has_path);
    let query = search_query_in(&inputs);
    let title_text = title.as_deref().unwrap_or("");

    if kind == ToolPreviewKind::Search
        || (query.is_some()
            && kind != ToolPreviewKind::Write
            && raw_kind != "read"
            && !starts_with_word_ci(title_text, &["read"]))
    {
        let mut preview = new_preview(ToolPreviewKind::Search, title);
        preview.query = query;
        return Some(preview);
    }

    if kind == ToolPreviewKind::Shell {
        return shell_preview(title, update, tool);
    }

    let file_name = path
        .as_deref()
        .filter(|path| !path.is_empty())
        .map(basename);

    if kind == ToolPreviewKind::Write
        && let Some(diff) = &diff
    {
        if let Some(lines) = diff.lines.as_ref().filter(|lines| !lines.is_empty()) {
            let mut preview = new_preview(ToolPreviewKind::Write, title);
            preview.path = path;
            preview.file_name = file_name;
            preview.start_line = start_line;
            preview.additions = diff.additions;
            preview.deletions = diff.deletions;
            preview.lines = Some(lines.clone());
            return Some(preview);
        }
        if diff.old_text.is_some() || nonempty(diff.new_text.as_deref()).is_some() {
            let built = compact_diff(
                diff.old_text.as_deref(),
                diff.new_text.as_deref().unwrap_or(""),
            );
            let mut preview = new_preview(ToolPreviewKind::Write, title);
            preview.path = path;
            preview.file_name = file_name;
            preview.start_line = start_line;
            preview.lines = Some(built.lines);
            preview.additions = Some(built.additions);
            preview.deletions = Some(built.deletions);
            return Some(preview);
        }
    }

    if kind == ToolPreviewKind::Write {
        for input in &inputs {
            for (old_key, new_key) in [
                ("old_string", "new_string"),
                ("oldText", "newText"),
                ("old_text", "new_text"),
                ("oldString", "newString"),
            ] {
                let (Some(Value::String(before)), Some(Value::String(after))) =
                    (input.get(old_key), input.get(new_key))
                else {
                    continue;
                };
                let built = compact_diff(Some(before), after);
                let mut preview = new_preview(kind, title);
                preview.path = path;
                preview.file_name = file_name;
                preview.additions = Some(built.additions);
                preview.deletions = Some(built.deletions);
                // Replacement strings are excerpts, not the file from line one.
                preview.lines = Some(
                    built
                        .lines
                        .into_iter()
                        .map(|line| ToolPreviewLine {
                            number: None,
                            ..line
                        })
                        .collect(),
                );
                return Some(preview);
            }
            if let Some(Value::String(content)) = input.get("content")
                && (raw_kind == "write" || starts_with_word_ci(title_text, &["write"]))
                && !is_same_record(input, update)
                && !is_same_record(input, tool)
            {
                // Write may overwrite an existing file. Without its previous
                // contents these are the written lines, not a claim that every
                // line was added.
                let mut preview = new_preview(kind, title);
                preview.path = path;
                preview.file_name = file_name;
                preview.content_only = Some(true);
                preview.lines = Some(
                    text_lines(content)
                        .into_iter()
                        .take(MAX_PREVIEW_LINES)
                        .enumerate()
                        .map(|(index, text)| {
                            line(
                                Some(index as i64 + 1),
                                ToolPreviewLineKind::Context,
                                cap_line(&text),
                            )
                        })
                        .collect(),
                );
                return Some(preview);
            }
        }
    }

    if !has_path && kind != ToolPreviewKind::Read && kind != ToolPreviewKind::Write {
        return None;
    }

    let mut preview = new_preview(kind, title);
    preview.path = path;
    preview.file_name = file_name;
    preview.start_line = start_line;
    Some(preview)
}

fn shell_preview(title: Option<String>, update: &Record, tool: &Record) -> Option<ToolPreview> {
    let bags = input_records(&[Some(Source::Record(update)), Some(Source::Record(tool))]);
    let command = shell_command_in(&bags)?;
    let inferred = infer_shell_intent(&command)?;
    let inferred_path = nonempty(inferred.path.as_deref()).map(normalize_path);
    if inferred.verb == ShellVerb::Find && nonempty(inferred.query.as_deref()).is_some() {
        let mut preview = new_preview(ToolPreviewKind::Shell, title);
        preview.file_name = inferred_path
            .as_deref()
            .filter(|p| !p.is_empty())
            .map(basename);
        preview.path = inferred_path;
        preview.query = inferred.query;
        return Some(preview);
    }
    let inferred_path = inferred_path.filter(|path| !path.is_empty())?;
    let mut preview = new_preview(ToolPreviewKind::Shell, title);
    preview.file_name = Some(basename(&inferred_path));
    preview.path = Some(inferred_path);
    preview.start_line = inferred.start_line;
    Some(preview)
}

fn new_preview(kind: ToolPreviewKind, title: Option<String>) -> ToolPreview {
    let mut preview = ToolPreview::new(kind);
    preview.title = title;
    preview
}

fn line(number: Option<i64>, kind: ToolPreviewLineKind, text: String) -> ToolPreviewLine {
    ToolPreviewLine {
        number,
        kind,
        text,
        extra: Extra::new(),
    }
}

/// `kind?.trim().toLowerCase() ?? ""`.
fn kind_key(kind: Option<&str>) -> String {
    kind.map(|kind| js::trim(kind).to_lowercase())
        .unwrap_or_default()
}

/// `title?.trim() ?? ""`.
fn trimmed(value: Option<&str>) -> &str {
    value.map(js::trim).unwrap_or("")
}

/// `isEditTool`.
pub fn is_edit_tool(
    kind: Option<&str>,
    title: Option<&str>,
    preview: Option<&ToolPreview>,
) -> bool {
    if preview.is_some_and(|preview| preview.kind == ToolPreviewKind::Write) {
        return true;
    }
    let key = kind_key(kind);
    if matches!(key.as_str(), "edit" | "write" | "delete" | "move") {
        return true;
    }
    if !key.is_empty() && key != "other" {
        return false;
    }
    starts_with_word_ci(trimmed(title), &["edit", "write", "delete", "update"])
}

/// `isReadTool`.
pub fn is_read_tool(
    kind: Option<&str>,
    title: Option<&str>,
    preview: Option<&ToolPreview>,
) -> bool {
    if preview.is_some_and(|preview| preview.kind == ToolPreviewKind::Read) {
        return true;
    }
    let key = kind_key(kind);
    if key == "read" {
        return true;
    }
    if !key.is_empty() && key != "other" {
        return false;
    }
    starts_with_word_ci(trimmed(title), &["read"])
}

/// `isSearchTool`.
pub fn is_search_tool(
    kind: Option<&str>,
    title: Option<&str>,
    preview: Option<&ToolPreview>,
) -> bool {
    if preview.is_some_and(|preview| preview.kind == ToolPreviewKind::Search) {
        return true;
    }
    let key = kind_key(kind);
    if key == "search" {
        return true;
    }
    if !key.is_empty() && key != "other" {
        return false;
    }
    starts_with_word_ci(trimmed(title), &["find", "search", "grep", "glob"])
}

/// `isFileTool`.
pub fn is_file_tool(
    kind: Option<&str>,
    title: Option<&str>,
    preview: Option<&ToolPreview>,
) -> bool {
    is_read_tool(kind, title, preview) || is_edit_tool(kind, title, preview)
}

/// `isExecuteTool`.
pub fn is_execute_tool(kind: Option<&str>, title: Option<&str>) -> bool {
    let key = kind_key(kind);
    if matches!(key.as_str(), "execute" | "shell" | "bash") {
        return true;
    }
    if !key.is_empty() && key != "other" {
        return false;
    }
    // `/^(bash|shell|run(?:ning)?(?:\s+command)?)\b/i`: the optional
    // `\s+command` never changes whether the pattern matches.
    starts_with_word_ci(trimmed(title), &["bash", "shell", "run", "running"])
}

/// `extractShellCommand`: the argv or script a shell tool is about to run, if
/// the harness sent it.
pub fn extract_shell_command(values: &[&Value]) -> Option<String> {
    let sources: Vec<_> = values
        .iter()
        .map(|value| Some(Source::Value(value)))
        .collect();
    shell_command_in(&input_records(&sources))
}

fn shell_command_in(bags: &[Bag<'_>]) -> Option<String> {
    bags.iter().find_map(|raw| {
        ["command", "cmd", "script"]
            .iter()
            .find_map(|key| command_field(raw.get(*key)))
    })
}

/// `extractSkillName`: the skill a Skill tool is invoking, if the harness sent
/// it.
pub fn extract_skill_name(values: &[&Value]) -> Option<String> {
    let sources: Vec<_> = values
        .iter()
        .map(|value| Some(Source::Value(value)))
        .collect();
    for raw in input_records(&sources) {
        for key in ["skill", "skill_name", "skillName", "skill_id", "skillId"] {
            if let Some(found) = skill_name_field(raw.get(key)) {
                return Some(found);
            }
        }
        if let Some(named) = skill_name_field(raw.get("name"))
            && looks_like_skill_name(&named)
        {
            return Some(named);
        }
    }
    None
}

/// `isSkillTool`.
pub fn is_skill_tool(kind: Option<&str>, title: Option<&str>) -> bool {
    let key = kind_key(kind);
    if key == "skill" || key == "skills" {
        return true;
    }
    if !key.is_empty() && key != "other" {
        return false;
    }
    starts_with_word_ci(trimmed(title), &["skill"])
}

/// `isAgentToolName`: delegation tool names shared by the provider adapters.
pub fn is_agent_tool_name(name: &str) -> bool {
    matches!(
        js::trim(name).to_lowercase().as_str(),
        "agent" | "task" | "subagent"
    )
}

/// `isAgentTool`.
pub fn is_agent_tool(kind: Option<&str>, title: Option<&str>) -> bool {
    let key = kind_key(kind);
    if matches!(key.as_str(), "agent" | "task" | "subagent") {
        return true;
    }
    if !key.is_empty() && key != "other" {
        return false;
    }
    starts_with_word_ci(trimmed(title), &["agent", "task", "subagent"])
}

/// `agentToolTitle`: human label for a spawned subagent, the agent's
/// description, else its type. TypeScript defaulted `fallback` to
/// `"Subagent"`.
pub fn agent_tool_title(input: &Record, fallback: &str) -> String {
    if let Some(description) = coerce_string(input.get("description")) {
        return description;
    }
    if let Some(task) = coerce_string(input.get("task")) {
        return task;
    }
    let agent_type = [
        "subagent_type",
        "subagentType",
        "agent_type",
        "agentType",
        "agent",
    ]
    .iter()
    .find_map(|key| coerce_string(input.get(*key)));
    if let Some(agent_type) = agent_type {
        let label = format_agent_type(&agent_type);
        return if label.to_ascii_lowercase().contains("subagent") {
            label
        } else {
            format!("{label} subagent")
        };
    }
    if !fallback.is_empty() && !is_agent_tool_name(fallback) && !is_weak_tool_title(fallback) {
        return fallback.to_string();
    }
    "Subagent".into()
}

/// `formatAgentType`.
pub fn format_agent_type(value: &str) -> String {
    let text = js_regex!(r"[_-]+").replace_all(js::trim(value), " ");
    if text.is_empty() {
        return "Subagent".into();
    }
    // `/\b[a-z]/g` upper-cases a lowercase letter that starts a word.
    let mut out = String::with_capacity(text.len());
    let mut previous_word = false;
    for c in text.chars() {
        let word = c.is_ascii() && is_word_byte(c as u8);
        if c.is_ascii_lowercase() && !previous_word {
            out.push(c.to_ascii_uppercase());
        } else {
            out.push(c);
        }
        previous_word = word;
    }
    out
}

const SEARCH_QUERY_KEYS: [&str; 8] = [
    "pattern",
    "query",
    "glob",
    "glob_pattern",
    "globPattern",
    "search_term",
    "searchTerm",
    "regex",
];

/// `extractSearchQuery`.
pub fn extract_search_query(value: &Value) -> Option<String> {
    search_query_in(&input_records(&[Some(Source::Value(value))]))
}

/// `extractSearchQuery` over bags [`input_records`] already flattened.
/// Flattening them again visits the same bags first, so the first match is
/// the same.
fn search_query_in(bags: &[Bag<'_>]) -> Option<String> {
    bags.iter().find_map(|raw| {
        SEARCH_QUERY_KEYS
            .iter()
            .find_map(|key| coerce_string(raw.get(*key)))
    })
}

/// `titleFromToolInput`: shared title builder used by Claude, Pi, and
/// OpenCode tool mappers.
pub fn title_from_tool_input(name: &str, kind: &str, input: &Record) -> String {
    if is_agent_tool_name(name) || is_agent_tool(Some(kind), None) {
        return agent_tool_title(input, name);
    }
    let mut tool = Record::new();
    tool.insert("title".into(), Value::String(name.into()));
    tool.insert("name".into(), Value::String(name.into()));
    tool.insert("kind".into(), Value::String(kind.into()));
    tool.insert("rawInput".into(), Value::Object(input.clone()));
    let mut update = tool.clone();
    update.insert("input".into(), Value::Object(input.clone()));
    let preview = extract_tool_preview(&update, &tool);
    let input_value = Value::Object(input.clone());
    let command = extract_shell_command(&[&input_value]);
    let skill = extract_skill_name(&[&input_value]);
    let title = compose_tool_title(&ToolTitleInput {
        kind: Some(kind),
        title: Some(name),
        command: command.as_deref(),
        skill: skill.as_deref(),
        path: preview.as_ref().and_then(|preview| preview.path.as_deref()),
        query: preview
            .as_ref()
            .and_then(|preview| preview.query.as_deref()),
        preview_kind: preview.as_ref().map(|preview| preview.kind),
        cwd: None,
    });
    if title.is_empty() {
        name.to_string()
    } else {
        title
    }
}

/// The options `composeToolTitle` takes.
#[derive(Debug, Clone, Copy, Default)]
pub struct ToolTitleInput<'a> {
    pub kind: Option<&'a str>,
    pub title: Option<&'a str>,
    pub path: Option<&'a str>,
    pub query: Option<&'a str>,
    pub command: Option<&'a str>,
    pub skill: Option<&'a str>,
    pub preview_kind: Option<ToolPreviewKind>,
    pub cwd: Option<&'a str>,
}

/// `composeToolTitle`.
pub fn compose_tool_title(opts: &ToolTitleInput<'_>) -> String {
    let kind_owned = kind_key(opts.kind);
    let kind = Some(kind_owned.as_str());
    let title = trimmed(opts.title);
    let path = nonempty(opts.path.map(js::trim));
    let query = nonempty(opts.query.map(js::trim));
    let preview_kind = opts.preview_kind;
    let command = nonempty(opts.command.map(js::trim));
    let skill = format_skill_name(opts.skill);

    if is_agent_tool(kind, Some(title)) {
        let rest = js_regex!(r"^(?i-u:agent|task|subagent){B}{S}*").replace(title, "");
        let rest = js::trim(&rest);
        if !rest.is_empty() && !is_weak_tool_title(rest) {
            return rest.to_string();
        }
        return "Subagent".into();
    }

    if is_skill_tool(kind, Some(title))
        || (!skill.is_empty()
            && !is_execute_tool(kind, Some(title))
            && !is_read_tool(kind, Some(title), None)
            && !is_search_tool(kind, Some(title), None)
            && !is_edit_tool(kind, Some(title), None))
    {
        if !skill.is_empty() {
            return format!("Skill {skill}");
        }
        let rest = js_regex!(r"^(?i-u:skill){B}{S}*").replace(title, "");
        let rest = js::trim(&rest);
        if !rest.is_empty() && !is_weak_tool_title(rest) {
            let formatted = format_skill_name(Some(rest));
            return format!(
                "Skill {}",
                if formatted.is_empty() {
                    rest
                } else {
                    &formatted
                }
            );
        }
        return "Skill".into();
    }

    // Shell rows are one line. The command has to live in the title itself:
    // Codex already does this, Claude and Pi used to label the row "Bash" and
    // hide the argv in detail the activity stack never shows.
    if preview_kind == Some(ToolPreviewKind::Shell) || is_execute_tool(kind, Some(title)) {
        if let Some(rewritten) = rewrite_readable_title(title, path, query) {
            return rewritten;
        }
        let stripped;
        let source = match command {
            Some(command) => command,
            None => {
                stripped = strip_execute_prefix(title);
                &stripped
            }
        };
        let script = unwrap_shell_command(source);
        if let Some(inferred) = infer_shell_intent(&script) {
            let shown = path.map(str::to_string).or_else(|| {
                nonempty(inferred.path.as_deref()).map(|path| display_path(path, opts.cwd))
            });
            if let Some(readable) = format_shell_intent(&inferred, shown.as_deref(), query) {
                return readable;
            }
        }
        if command.is_some() {
            return first_line(Some(&script));
        }
        let rest = first_line(Some(&script));
        if !rest.is_empty() && !is_weak_tool_title(&rest) {
            return rest;
        }
        if !title.is_empty() && !is_weak_tool_title(title) {
            return title.to_string();
        }
        return "Shell".into();
    }

    // A title that already names a different action is finished. Re-prefixing
    // it with "Read" produces nonsense like "Read List ." when only a stale
    // read-shaped preview is pulling us into the branch below.
    if path.is_none()
        && starts_with_word_then_text(
            title,
            &["list", "write", "edit", "run", "delete", "move", "fetch"],
            true,
        )
    {
        return title.to_string();
    }

    if preview_kind == Some(ToolPreviewKind::Read) || is_read_tool(kind, Some(title), None) {
        if let Some(path) = path {
            return format!("Read {path}");
        }
        // `\b` keeps "Reading" from being sliced into "Read ing".
        let rest =
            js_regex!(r"^(?i-u:read)(?:(?i-u:ing))?(?:{S}+(?i-u:file))?{B}{S}*").replace(title, "");
        let rest = js::trim(&rest);
        if !rest.is_empty() && !is_weak_tool_title(rest) {
            return format!("Read {rest}");
        }
        return "Read".into();
    }

    if preview_kind == Some(ToolPreviewKind::Search) || is_search_tool(kind, Some(title), None) {
        let from_title;
        let q = match query {
            Some(query) => query,
            None => {
                from_title = js_regex!(r"^(?i-u:find|search|grep|glob)(?:(?i-u:ing))?{B}{S}*")
                    .replace(title, "")
                    .into_owned();
                js::trim(&from_title)
            }
        };
        if !q.is_empty() && !is_weak_tool_title(q) {
            return format!("Find {q}");
        }
        return "Find".into();
    }

    title.to_string()
}

/// `stubFilePreview`.
pub fn stub_file_preview(kind: Option<&str>, title: Option<&str>) -> ToolPreview {
    let inferred = preview_kind(kind.unwrap_or(""), title, false, false);
    let kind = if inferred == ToolPreviewKind::Write {
        ToolPreviewKind::Write
    } else {
        ToolPreviewKind::Read
    };
    new_preview(kind, title.map(str::to_string))
}

/// `isWeakToolTitle`: a placeholder label that a later, better one replaces.
pub fn is_weak_tool_title(value: &str) -> bool {
    const WEAK: [&str; 38] = [
        "tool",
        "shell",
        "bash",
        "execute",
        "command",
        "skill",
        "read",
        "edit",
        "search",
        "find",
        "grep",
        "glob",
        "fetch",
        "other",
        "write",
        "delete",
        "move",
        "think",
        "run",
        "list",
        "working",
        "reading",
        "editing",
        "searching",
        "writing",
        "running",
        "listing",
        "fetching",
        "thinking",
        "deleting",
        "moving",
        "read file",
        "edit file",
        "write file",
        "run command",
        "ran command",
        "unnamed",
        "mcp:tool",
    ];
    let lower = js::trim(value).to_ascii_lowercase();
    if WEAK.contains(&lower.as_str()) {
        return true;
    }
    // `mcp:\s*tool`.
    lower
        .strip_prefix("mcp:")
        .is_some_and(|rest| rest.trim_start_matches(js::is_space) == "tool")
}

/// `contextLines`.
pub fn context_lines(text: &str, start_line: Option<i64>) -> Vec<ToolPreviewLine> {
    let normalized = text.replace("\r\n", "\n");
    let raw: Vec<&str> = js::trim_end(&normalized).split('\n').collect();
    let start = start_line.unwrap_or(1).max(1);
    let from = ((start - 1).max(0) as usize).min(raw.len().saturating_sub(1));
    raw.iter()
        .skip(from)
        .take(MAX_PREVIEW_LINES)
        .enumerate()
        .map(|(index, text)| {
            line(
                Some((from + index + 1) as i64),
                ToolPreviewLineKind::Context,
                cap_line(text),
            )
        })
        .collect()
}

/// `mergeToolPreview`: fields from `next` win, but a weak title, a missing
/// path, or missing lines fall back to `prev`.
pub fn merge_tool_preview(
    next: Option<&ToolPreview>,
    prev: Option<&ToolPreview>,
) -> Option<ToolPreview> {
    let Some(next) = next else {
        return prev.cloned();
    };
    let Some(prev) = prev else {
        return Some(next.clone());
    };
    // `next.x || prev.x`.
    let pick = |a: &Option<String>, b: &Option<String>| {
        nonempty(a.as_deref()).or(b.as_deref()).map(str::to_string)
    };
    let read_or_search =
        |kind: ToolPreviewKind| matches!(kind, ToolPreviewKind::Read | ToolPreviewKind::Search);
    Some(ToolPreview {
        kind: next.kind,
        title: pick_strong(next.title.as_deref(), prev.title.as_deref()),
        path: pick(&next.path, &prev.path),
        file_name: pick(&next.file_name, &prev.file_name),
        start_line: next.start_line.or(prev.start_line),
        additions: next.additions.or(prev.additions),
        deletions: next.deletions.or(prev.deletions),
        content_only: if next.lines.is_some() {
            next.content_only
        } else {
            prev.content_only
        },
        query: pick(&next.query, &prev.query),
        lines: if read_or_search(next.kind) {
            None
        } else if next.lines.is_some() {
            next.lines.clone()
        } else if read_or_search(prev.kind) {
            None
        } else {
            prev.lines.clone()
        },
        output: pick(&next.output, &prev.output),
        extra: Extra::new(),
    })
}

fn pick_strong(next: Option<&str>, prev: Option<&str>) -> Option<String> {
    if let Some(next) = nonempty(next)
        && !is_weak_tool_title(next)
    {
        return Some(next.to_string());
    }
    if let Some(prev) = nonempty(prev)
        && !is_weak_tool_title(prev)
    {
        return Some(prev.to_string());
    }
    nonempty(next).or(prev).map(str::to_string)
}

fn preview_kind(raw: &str, title: Option<&str>, has_diff: bool, has_path: bool) -> ToolPreviewKind {
    match raw {
        "read" => ToolPreviewKind::Read,
        "search" => ToolPreviewKind::Search,
        "edit" | "write" | "delete" | "move" => ToolPreviewKind::Write,
        "execute" | "shell" => ToolPreviewKind::Shell,
        _ => {
            let title = title.unwrap_or("");
            if has_diff || starts_with_word_ci(title, &["edit", "write", "delete", "update"]) {
                ToolPreviewKind::Write
            } else if starts_with_word_ci(title, &["find", "search", "grep", "glob"]) {
                ToolPreviewKind::Search
            } else if starts_with_word_ci(title, &["read"]) || has_path {
                ToolPreviewKind::Read
            } else {
                ToolPreviewKind::Shell
            }
        }
    }
}

fn extract_path(
    update: &Record,
    tool: &Record,
    inputs: &[Bag<'_>],
    diff_path: Option<&str>,
) -> Option<String> {
    let content = || nullish(update, &["content"]).or_else(|| nullish(tool, &["content"]));
    location_path(nullish(update, &["locations", "location"]))
        .or_else(|| location_path(nullish(tool, &["locations", "location"])))
        .or_else(|| first_input_path(inputs))
        .or_else(|| diff_path.map(str::to_string))
        .or_else(|| content_path(content()))
        .or_else(|| find_path_in_record(update, 0))
        .or_else(|| find_path_in_record(tool, 0))
        .or_else(|| {
            let title =
                coerce_string(update.get("title")).or_else(|| coerce_string(tool.get("title")));
            path_from_title(title.as_deref())
        })
}

fn first_input_path(inputs: &[Bag<'_>]) -> Option<String> {
    inputs
        .iter()
        .find_map(|raw| input_path(raw).filter(|path| !path.is_empty()))
}

fn first_number(inputs: &[Bag<'_>], key: &str) -> Option<i64> {
    inputs
        .iter()
        .find_map(|raw| number_field(raw, key).filter(|found| *found != 0.0))
        .map(|found| found as i64)
}

fn input_path(raw: &Record) -> Option<String> {
    const KEYS: [&str; 10] = [
        "path",
        "filePath",
        "file_path",
        "targetFile",
        "target_file",
        "relative_workspace_path",
        "relativeWorkspacePath",
        "uri",
        "file",
        "absolutePath",
    ];
    KEYS.iter().find_map(|key| {
        coerce_string(raw.get(*key))
            .filter(|value| looks_like_tool_path(value))
            .map(|value| normalize_path(&value))
    })
}

fn path_from_title(title: Option<&str>) -> Option<String> {
    let found =
        js_regex!(r"^(?i-u:read|edit|write|delete|update){S}+(?:(?i-u:file){S}+)?({DOT}+)$")
            .captures(title?)?;
    let rest = js::trim(&found[1]);
    if rest.is_empty() || !looks_like_tool_path(rest) {
        return None;
    }
    Some(rest.to_string())
}

/// `locations` as a list: an array, one truthy value, or nothing.
fn location_items(locations: Option<&Value>) -> &[Value] {
    match locations {
        Some(Value::Array(items)) => items,
        Some(value) if truthy(value) => std::slice::from_ref(value),
        _ => &[],
    }
}

fn location_path(locations: Option<&Value>) -> Option<String> {
    for item in location_items(locations) {
        if let Value::String(text) = item
            && looks_like_tool_path(text)
        {
            return Some(normalize_path(text));
        }
        let path = item.as_object().and_then(|rec| {
            coerce_string(rec.get("path"))
                .or_else(|| coerce_string(rec.get("filePath")))
                .or_else(|| coerce_string(rec.get("uri")))
                .or_else(|| coerce_string(rec.get("file")))
        });
        if let Some(path) = path
            && looks_like_tool_path(&path)
        {
            return Some(normalize_path(&path));
        }
    }
    None
}

fn location_line(locations: Option<&Value>) -> Option<i64> {
    location_items(locations).iter().find_map(|item| {
        let line = number_field(item.as_object()?, "line")?;
        (line > 0.0).then_some(line as i64)
    })
}

fn content_path(content: Option<&Value>) -> Option<String> {
    for block in content_blocks(content) {
        if let Some(path) = coerce_string(block.get("path"))
            && looks_like_tool_path(&path)
        {
            return Some(normalize_path(&path));
        }
        let change_path = first_change(block).and_then(|change| coerce_string(change.get("path")));
        if let Some(path) = change_path
            && looks_like_tool_path(&path)
        {
            return Some(normalize_path(&path));
        }
    }
    None
}

/// What a provider `diff` content block said about the change.
struct Diff {
    path: Option<String>,
    old_text: Option<String>,
    new_text: Option<String>,
    lines: Option<Vec<ToolPreviewLine>>,
    additions: Option<i64>,
    deletions: Option<i64>,
}

fn extract_diff(content: Option<&Value>) -> Option<Diff> {
    for block in content_blocks(content) {
        let kind = coerce_string(block.get("type"))
            .unwrap_or_default()
            .to_lowercase();
        if kind != "diff" {
            continue;
        }
        let change = first_change(block);
        let path = coerce_string(block.get("path"))
            .or_else(|| change.and_then(|change| coerce_string(change.get("path"))));
        let patch = block.get("patch").and_then(Value::as_object);
        let patch_text = patch
            .and_then(|patch| coerce_string(patch.get("text")))
            .or_else(|| coerce_string(block.get("patch")));
        if let Some(patch_text) = patch_text.filter(|text| {
            let text = js::trim(text);
            text.starts_with("diff --git") || text.starts_with("@@ ")
        }) {
            let parsed = parse_git_patch(&patch_text);
            return Some(Diff {
                path: parsed.path.or(path),
                old_text: None,
                new_text: None,
                lines: Some(parsed.lines),
                additions: Some(parsed.additions),
                deletions: Some(parsed.deletions),
            });
        }
        let text_field = |key: &str| block.get(key).and_then(Value::as_str).map(str::to_string);
        return Some(Diff {
            path,
            old_text: text_field("oldText").or_else(|| text_field("old_text")),
            new_text: text_field("newText").or_else(|| text_field("new_text")),
            lines: None,
            additions: None,
            deletions: None,
        });
    }
    None
}

fn first_change(block: &Record) -> Option<&Record> {
    block.get("changes")?.as_array()?.first()?.as_object()
}

/// A content block and the record nested under its `content`, nested first.
fn with_nested(rec: &Record) -> Vec<&Record> {
    match rec.get("content").and_then(Value::as_object) {
        Some(nested) => vec![nested, rec],
        None => vec![rec],
    }
}

fn content_blocks(content: Option<&Value>) -> Vec<&Record> {
    match content {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_object)
            .flat_map(with_nested)
            .collect(),
        Some(value) => value.as_object().map(with_nested).unwrap_or_default(),
        None => Vec::new(),
    }
}

struct GitPatch {
    path: Option<String>,
    lines: Vec<ToolPreviewLine>,
    additions: i64,
    deletions: i64,
}

fn parse_git_patch(text: &str) -> GitPatch {
    let plus = crate::reducer::js_regex::multiline_captures(
        js_regex!(r"^\+\+\+{S}+(?:b/)?({DOT}+)"),
        text,
    );
    let git = crate::reducer::js_regex::multiline_captures(
        js_regex!(r"^diff --git{S}+{NS}+{S}+({NS}+)"),
        text,
    );
    let mut path = plus
        .or(git)
        .map(|found| {
            let raw = &found[1];
            js::trim(raw.strip_prefix("b/").unwrap_or(raw)).to_string()
        })
        .filter(|path| path != "/dev/null");

    let mut hunks: Vec<ToolPreviewLine> = Vec::new();
    let mut additions = 0;
    let mut deletions = 0;
    let mut new_num: i64 = 0;
    let normalized = text.replace("\r\n", "\n");
    for raw in normalized.split('\n') {
        if let Some(header) = js_regex!(r"^@@{S}+-([0-9]+)(?:,[0-9]+)?{S}+\+([0-9]+)").captures(raw)
        {
            new_num = header[2].parse::<f64>().unwrap_or(0.0) as i64;
            continue;
        }
        if raw.starts_with("diff ")
            || raw.starts_with("index ")
            || raw.starts_with("--- ")
            || raw.starts_with("+++ ")
        {
            continue;
        }
        if let Some(body) = raw.strip_prefix('+') {
            additions += 1;
            hunks.push(line(Some(new_num), ToolPreviewLineKind::Add, body.into()));
            new_num += 1;
        } else if let Some(body) = raw.strip_prefix('-') {
            deletions += 1;
            hunks.push(line(Some(new_num), ToolPreviewLineKind::Del, body.into()));
        } else if raw.starts_with('\\') {
            continue;
        } else {
            let body = raw.strip_prefix(' ').unwrap_or(raw);
            hunks.push(line(
                Some(new_num),
                ToolPreviewLineKind::Context,
                body.into(),
            ));
            new_num += 1;
        }
    }

    if let Some(found) = path.take() {
        path = Some(if !found.is_empty() && looks_like_path(&found) {
            normalize_path(&found)
        } else {
            found
        });
    }
    GitPatch {
        path,
        lines: preview_window(hunks),
        additions,
        deletions,
    }
}

/// Up to `MAX_PREVIEW_LINES` lines starting one line before the first change.
fn preview_window(hunks: Vec<ToolPreviewLine>) -> Vec<ToolPreviewLine> {
    let first = hunks
        .iter()
        .position(|line| line.kind != ToolPreviewLineKind::Context);
    let start = first.map_or(0, |first| first.saturating_sub(1));
    hunks
        .into_iter()
        .skip(start)
        .take(MAX_PREVIEW_LINES)
        .map(|line| ToolPreviewLine {
            text: cap_line(&line.text),
            ..line
        })
        .collect()
}

struct CompactDiff {
    lines: Vec<ToolPreviewLine>,
    additions: i64,
    deletions: i64,
}

fn compact_diff(old_text: Option<&str>, new_text: &str) -> CompactDiff {
    let old_lines = text_lines(old_text.unwrap_or(""));
    let new_lines = text_lines(new_text);
    let hunks = if old_text.is_none_or(str::is_empty) {
        new_lines
            .iter()
            .enumerate()
            .map(|(index, text)| {
                line(
                    Some(index as i64 + 1),
                    ToolPreviewLineKind::Add,
                    text.clone(),
                )
            })
            .collect()
    } else {
        greedy_diff(&old_lines, &new_lines)
    };
    let count = |kind| hunks.iter().filter(|line| line.kind == kind).count() as i64;
    let additions = count(ToolPreviewLineKind::Add);
    let deletions = count(ToolPreviewLineKind::Del);
    CompactDiff {
        lines: preview_window(hunks),
        additions,
        deletions,
    }
}

fn text_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = text
        .replace("\r\n", "\n")
        .split('\n')
        .map(str::to_string)
        .collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

fn greedy_diff(old_lines: &[String], new_lines: &[String]) -> Vec<ToolPreviewLine> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < old_lines.len() || j < new_lines.len() {
        if i < old_lines.len() && j < new_lines.len() && old_lines[i] == new_lines[j] {
            out.push(line(
                Some(j as i64 + 1),
                ToolPreviewLineKind::Context,
                old_lines[i].clone(),
            ));
            i += 1;
            j += 1;
            continue;
        }
        if let Some((sync_i, sync_j)) = find_sync(old_lines, new_lines, i, j, 8) {
            while i < sync_i {
                out.push(line(
                    Some(i as i64 + 1),
                    ToolPreviewLineKind::Del,
                    old_lines[i].clone(),
                ));
                i += 1;
            }
            while j < sync_j {
                out.push(line(
                    Some(j as i64 + 1),
                    ToolPreviewLineKind::Add,
                    new_lines[j].clone(),
                ));
                j += 1;
            }
            continue;
        }
        if i < old_lines.len() {
            out.push(line(
                Some(i as i64 + 1),
                ToolPreviewLineKind::Del,
                old_lines[i].clone(),
            ));
            i += 1;
        } else {
            out.push(line(
                Some(j as i64 + 1),
                ToolPreviewLineKind::Add,
                new_lines[j].clone(),
            ));
            j += 1;
        }
    }
    out
}

fn find_sync(
    old_lines: &[String],
    new_lines: &[String],
    i: usize,
    j: usize,
    window: usize,
) -> Option<(usize, usize)> {
    for di in 0..=window {
        for dj in 0..=window {
            if di == 0 && dj == 0 {
                continue;
            }
            let (oi, nj) = (i + di, j + dj);
            if oi < old_lines.len() && nj < new_lines.len() && old_lines[oi] == new_lines[nj] {
                return Some((oi, nj));
            }
        }
    }
    None
}

/// Fields `findPathInUnknown` never treats as a path.
const PATH_SEARCH_SKIP: [&str; 15] = [
    "toolCallId",
    "tool_call_id",
    "sessionUpdate",
    "status",
    "kind",
    "title",
    "content",
    "text",
    "rawOutput",
    "raw_output",
    "output",
    "result",
    "cwd",
    "workingDirectory",
    "working_directory",
];

fn find_path_in_value(value: &Value, depth: usize) -> Option<String> {
    if depth > 4 {
        return None;
    }
    match value {
        Value::String(text) => {
            let text = js::trim(text);
            if text.is_empty() {
                return None;
            }
            if text.starts_with('{') || text.starts_with('[') {
                let parsed: Value = serde_json::from_str(text).ok()?;
                return find_path_in_value(&parsed, depth + 1);
            }
            looks_like_tool_path(text).then(|| normalize_path(text))
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_path_in_value(item, depth + 1).filter(|found| !found.is_empty())),
        Value::Object(rec) => find_path_in_record(rec, depth),
        _ => None,
    }
}

fn find_path_in_record(rec: &Record, depth: usize) -> Option<String> {
    if depth > 4 {
        return None;
    }
    rec.iter()
        .filter(|(key, _)| !PATH_SEARCH_SKIP.contains(&key.as_str()))
        .find_map(|(_, nested)| {
            find_path_in_value(nested, depth + 1).filter(|found| !found.is_empty())
        })
}

fn looks_like_tool_path(value: &str) -> bool {
    let text = js::trim(value);
    if text.is_empty() || js::len(text) > 400 || text.contains(['\n', '\r']) {
        return false;
    }
    if is_weak_tool_title(text) {
        return false;
    }
    if text.starts_with("//") || text.starts_with("/*") || text.starts_with('*') {
        return false;
    }
    let lower = text.get(..6).unwrap_or(text).to_ascii_lowercase();
    if lower.starts_with("http:") || lower.starts_with("https:") {
        return false;
    }
    looks_like_path(text)
}

fn looks_like_path(value: &str) -> bool {
    let text = js::trim(value);
    looks_like_abs_path(text)
        || (text.contains('/') && !text.starts_with("//") && !text.starts_with("/*"))
        || has_short_extension(text)
}

/// `/\.[a-z0-9]{1,8}$/i`.
fn has_short_extension(text: &str) -> bool {
    text.rsplit_once('.').is_some_and(|(_, ext)| {
        (1..=8).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}

fn looks_like_abs_path(value: &str) -> bool {
    let text = js::trim(value);
    if text.starts_with("file://") {
        return true;
    }
    if text.starts_with("//") || text.starts_with("/*") {
        return false;
    }
    if text.starts_with('/') {
        // `/\/[^/\s]/`: some slash is followed by a real path character.
        let has_segment = text.match_indices('/').any(|(index, _)| {
            text[index + 1..]
                .chars()
                .next()
                .is_some_and(|next| next != '/' && !js::is_space(next))
        });
        return has_segment && js::len(text) < 512;
    }
    let bytes = text.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

fn cap_line(text: &str) -> String {
    if js::len(text) <= MAX_LINE_CHARS {
        return text.to_string();
    }
    format!("{}…", js::slice_prefix(text, MAX_LINE_CHARS - 1))
}

fn normalize_path(path: &str) -> String {
    match path.strip_prefix("file://") {
        Some(rest) => decode_uri_component(rest).unwrap_or_else(|| rest.to_string()),
        None => path.to_string(),
    }
}

/// The preview's own `basename`, which splits on both slash kinds.
fn basename(path: &str) -> String {
    let stripped = path.trim_end_matches(['/', '\\']);
    let trimmed = if stripped.is_empty() { path } else { stripped };
    trimmed
        .split(['/', '\\'])
        .rfind(|part| !part.is_empty())
        .unwrap_or(trimmed)
        .to_string()
}

/// Where [`input_records`] reads from: a value, or a record the caller holds.
#[derive(Clone, Copy)]
enum Source<'a> {
    Value(&'a Value),
    Record(&'a Record),
}

/// One flattened argument bag. Bags parsed from a JSON string are owned.
type Bag<'a> = Cow<'a, Record>;

/// Keys `inputRecords` descends into, in order.
const NESTED_INPUT_KEYS: [&str; 6] = [
    "arguments",
    "args",
    "params",
    "input",
    "rawInput",
    "raw_input",
];

/// `inputRecords`: flatten the nested argument bags ACP agents stuff under
/// args, input, or rawInput. A record reached twice by reference is kept
/// once, as the TypeScript's identity set did.
fn input_records<'a>(sources: &[Option<Source<'a>>]) -> Vec<Bag<'a>> {
    let mut out = Vec::new();
    let mut seen: Vec<*const Record> = Vec::new();
    for source in sources.iter().flatten() {
        match *source {
            Source::Value(value) => add_value(Cow::Borrowed(value), &mut out, &mut seen),
            Source::Record(rec) => add_record(Cow::Borrowed(rec), &mut out, &mut seen),
        }
    }
    out
}

fn add_value<'a>(value: Cow<'a, Value>, out: &mut Vec<Bag<'a>>, seen: &mut Vec<*const Record>) {
    match value {
        Cow::Borrowed(Value::Array(items)) => {
            for item in items {
                add_value(Cow::Borrowed(item), out, seen);
            }
        }
        Cow::Owned(Value::Array(items)) => {
            for item in items {
                add_value(Cow::Owned(item), out, seen);
            }
        }
        Cow::Borrowed(Value::Object(rec)) => add_record(Cow::Borrowed(rec), out, seen),
        Cow::Owned(Value::Object(rec)) => add_record(Cow::Owned(rec), out, seen),
        Cow::Borrowed(Value::String(text)) => {
            if let Some(rec) = parse_record(text) {
                add_record(Cow::Owned(rec), out, seen);
            }
        }
        Cow::Owned(Value::String(text)) => {
            if let Some(rec) = parse_record(&text) {
                add_record(Cow::Owned(rec), out, seen);
            }
        }
        _ => {}
    }
}

fn add_record<'a>(rec: Bag<'a>, out: &mut Vec<Bag<'a>>, seen: &mut Vec<*const Record>) {
    if rec.is_empty() {
        return;
    }
    match rec {
        Cow::Borrowed(borrowed) => {
            let pointer = borrowed as *const Record;
            if seen.contains(&pointer) {
                return;
            }
            seen.push(pointer);
            out.push(Cow::Borrowed(borrowed));
            for key in NESTED_INPUT_KEYS {
                if let Some(nested) = borrowed.get(key) {
                    add_value(Cow::Borrowed(nested), out, seen);
                }
            }
        }
        Cow::Owned(owned) => {
            let nested: Vec<Value> = NESTED_INPUT_KEYS
                .iter()
                .filter_map(|key| owned.get(*key).cloned())
                .collect();
            out.push(Cow::Owned(owned));
            for value in nested {
                add_value(Cow::Owned(value), out, seen);
            }
        }
    }
}

/// `parseRecord` for a string: the object a JSON string holds, if any.
fn parse_record(text: &str) -> Option<Record> {
    let text = js::trim(text);
    if !text.starts_with('{') && !text.starts_with('[') {
        return None;
    }
    match serde_json::from_str(text) {
        Ok(Value::Object(rec)) => Some(rec),
        _ => None,
    }
}

/// `input !== update`: the bag is the caller's record itself.
fn is_same_record(bag: &Bag<'_>, rec: &Record) -> bool {
    matches!(bag, Cow::Borrowed(borrowed) if std::ptr::eq(*borrowed, rec))
}

/// `rec.a ?? rec.b ?? ...`: the first key that is present and not null.
fn nullish<'a>(rec: &'a Record, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .find_map(|key| rec.get(*key).filter(|value| !value.is_null()))
}

/// JavaScript truthiness for a JSON value.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn command_field(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => nonempty(Some(js::trim(text))).map(str::to_string),
        Value::Array(items) if !items.is_empty() => {
            let parts: Option<Vec<&str>> = items.iter().map(Value::as_str).collect();
            let joined = parts?.join(" ");
            nonempty(Some(js::trim(&joined))).map(str::to_string)
        }
        _ => None,
    }
}

fn first_line(value: Option<&str>) -> String {
    let text = trimmed(value);
    if text.is_empty() {
        return String::new();
    }
    let first = text.split('\n').next().unwrap_or("");
    let first = first.strip_suffix('\r').unwrap_or(first);
    js::trim(first).to_string()
}

fn strip_execute_prefix(title: &str) -> String {
    let stripped = js_regex!(r"^(?i-u:bash|shell|execute){S}*[:\-]{S}+").replace(title, "");
    js::trim(&stripped).to_string()
}

fn skill_name_field(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?;
    let text = js::trim(text).trim_start_matches('/');
    if text.is_empty()
        || is_weak_tool_title(text)
        || js::len(text) > 80
        || text.contains(['\n', '\r'])
    {
        return None;
    }
    Some(text.to_string())
}

fn looks_like_skill_name(value: &str) -> bool {
    let text = js::trim(value).trim_start_matches('/');
    if text.is_empty() || text.contains('/') || text.contains('\\') {
        return false;
    }
    if has_short_extension(text) {
        return false;
    }
    let mut bytes = text.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphabetic())
        && bytes.all(|b| is_word_byte(b) || b == b'.' || b == b'-')
}

fn format_skill_name(value: Option<&str>) -> String {
    let text = trimmed(value).trim_start_matches('/');
    if text.is_empty() {
        String::new()
    } else {
        format!("/{text}")
    }
}

/// `coerceString`: a trimmed non-empty string, a finite number as text, or
/// the first of `path`, `text`, `uri`, `value` on an object.
fn coerce_string(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => nonempty(Some(js::trim(text))).map(str::to_string),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                Some(int.to_string())
            } else if let Some(int) = number.as_u64() {
                Some(int.to_string())
            } else {
                number
                    .as_f64()
                    .filter(|n| n.is_finite())
                    .map(js::number_to_string)
            }
        }
        Value::Object(rec) => coerce_string(rec.get("path"))
            .or_else(|| coerce_string(rec.get("text")))
            .or_else(|| coerce_string(rec.get("uri")))
            .or_else(|| coerce_string(rec.get("value"))),
        _ => None,
    }
}

/// `numberField`: a finite number, or a string of digits.
fn number_field(rec: &Record, key: &str) -> Option<f64> {
    match rec.get(key)? {
        Value::Number(number) => number.as_f64().filter(|n| n.is_finite()),
        Value::String(text) => {
            let text = js::trim(text);
            if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            text.parse::<f64>().ok()
        }
        _ => None,
    }
}

#[cfg(test)]
pub(crate) mod tests;
