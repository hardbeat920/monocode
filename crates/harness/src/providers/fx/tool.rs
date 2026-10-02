//! Port of src/integrations/harness/providers/fx/fxTool.ts: fx tool metadata
//! recovery.
//!
//! Unlike every other ACP harness MonoCode speaks to, fx sends no `rawInput`,
//! no `locations`, and no `diff` content on `tool_call` and
//! `tool_call_update`. It sends a gerund title ("Reading"), a kind, a status,
//! and, once the call completes, a free-text result. The path, query, and
//! command the transcript needs are mined back out of that text.
//!
//! The shapes below come from `fx acp` 0.0.5 wire captures.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use monocode_core::block::{ToolPreview, ToolPreviewKind};
use monocode_core::js;

type Rec = Map<String, Value>;

/// `FxToolInfo`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FxToolInfo {
    pub title: Option<String>,
    /// Overrides fx's reported kind when it mislabels the call.
    pub kind: Option<String>,
    pub preview: Option<ToolPreview>,
    pub detail: Option<String>,
    /// True once the call is understood. The generic ACP extraction must not
    /// run afterwards: on a shell call it mistakes `command_result.cwd` for a
    /// read target and relabels `echo hi` as "Read /tmp/project".
    pub resolved: bool,
}

/// `fxToolVerb`: fx labels calls with a gerund; the transcript wants the
/// plain verb.
pub fn fx_tool_verb(title: Option<&str>) -> Option<&'static str> {
    let key = js::trim(title?).to_lowercase();
    Some(match key.as_str() {
        "listing" => "List",
        "reading" => "Read",
        "writing" => "Write",
        "editing" => "Edit",
        "searching" => "Find",
        "running" => "Run",
        "fetching" => "Fetch",
        "thinking" => "Think",
        "deleting" => "Delete",
        "moving" => "Move",
        _ => return None,
    })
}

/// `fxToolInfo`.
pub fn fx_tool_info(update: &Rec, tool: &Rec) -> FxToolInfo {
    let raw_title = string_field(update, "title").or_else(|| string_field(tool, "title"));
    let verb = fx_tool_verb(raw_title.as_deref()).map(str::to_string);
    let kind = string_field(update, "kind")
        .or_else(|| string_field(tool, "kind"))
        .unwrap_or_default()
        .to_lowercase();
    let text = result_text(update, tool);
    let verb_or_title = || verb.clone().or_else(|| raw_title.clone());

    if let Some(command) = command_result(update, tool) {
        let detail = [
            shell_output(text.as_deref()).or_else(|| text.clone()),
            command.output,
        ]
        .into_iter()
        .flatten()
        .filter(|part| !js::trim(part).is_empty())
        .collect::<Vec<_>>()
        .join("\n");
        return FxToolInfo {
            title: Some(command.command),
            detail: (!detail.is_empty()).then_some(detail),
            resolved: true,
            ..FxToolInfo::default()
        };
    }

    if kind == "execute" {
        return FxToolInfo {
            title: verb_or_title(),
            detail: shell_output(text.as_deref()).or_else(|| text.clone()),
            resolved: true,
            ..FxToolInfo::default()
        };
    }

    if let Some(search) = grep_result(text.as_deref()) {
        let mut preview = ToolPreview::new(ToolPreviewKind::Search);
        preview.query = Some(search.query);
        return FxToolInfo {
            title: verb_or_title(),
            preview: Some(preview),
            detail: search.body,
            resolved: true,
            ..FxToolInfo::default()
        };
    }

    if let Some(read) = read_result(text.as_deref()) {
        let mut preview = ToolPreview::new(ToolPreviewKind::Read);
        preview.file_name = Some(basename(&read.path));
        preview.path = Some(read.path);
        preview.start_line = read.start_line;
        return FxToolInfo {
            title: verb_or_title(),
            preview: Some(preview),
            detail: read.body,
            resolved: true,
            ..FxToolInfo::default()
        };
    }

    if let Some(path) = listing_result(text.as_deref()) {
        // fx reports a directory listing as kind "read", which the shared
        // title builder renders as "Read .". Report it as a plain call with no
        // preview: the transcript only builds a file chip for "Read" and
        // "Find" labels, so a read-shaped preview would only add the wrong label.
        return FxToolInfo {
            title: Some(format!("List {path}")),
            kind: Some("other".into()),
            detail: text,
            resolved: true,
            ..FxToolInfo::default()
        };
    }

    if let Some(wrote) = write_result(text.as_deref()) {
        let name = basename(&wrote.path);
        let mut preview = ToolPreview::new(ToolPreviewKind::Write);
        preview.path = Some(wrote.path);
        preview.file_name = Some(name.clone());
        return FxToolInfo {
            title: Some(format!("{} {name}", wrote.verb)),
            preview: Some(preview),
            detail: text,
            resolved: true,
            ..FxToolInfo::default()
        };
    }

    FxToolInfo {
        title: verb_or_title(),
        detail: text,
        resolved: false,
        ..FxToolInfo::default()
    }
}

/// `exit_code=0\n<stdout>\nhi\n</stdout>\n`
fn shell_output(text: Option<&str>) -> Option<String> {
    let text = text?;
    let joined = [section(text, "stdout"), section(text, "stderr")]
        .into_iter()
        .flatten()
        .filter(|part| !js::trim(part).is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let joined = js::trim(&joined);
    (!joined.is_empty()).then(|| joined.to_string())
}

struct CommandResult {
    command: String,
    output: Option<String>,
}

/// fx extends ACP with a `command_result` object on shell calls.
fn command_result(update: &Rec, tool: &Rec) -> Option<CommandResult> {
    let rec = [
        update.get("command_result"),
        update.get("commandResult"),
        tool.get("command_result"),
        tool.get("commandResult"),
    ]
    .into_iter()
    .find_map(|value| value.and_then(Value::as_object))?;
    let command = string_field(rec, "command")?;
    let code = rec
        .get("exit_code")
        .filter(|value| !value.is_null())
        .or_else(|| rec.get("exitCode"))
        .and_then(Value::as_f64);
    let output = code
        .filter(|code| *code != 0.0)
        .map(|code| format!("exit {}", js::number_to_string(code)));
    Some(CommandResult { command, output })
}

struct GrepResult {
    query: String,
    body: Option<String>,
}

static GREP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\[(?:grep|search|glob)\]\s+.*?\bfor\s+(.+?)\s*$").unwrap());

/// `[grep] 2 matches for export\n - app.ts:1: …`
fn grep_result(text: Option<&str>) -> Option<GrepResult> {
    let text = text?;
    let captures = GREP.captures(text)?;
    let query = js::trim(&captures[1]).to_string();
    if query.is_empty() {
        return None;
    }
    let end = captures.get(0).map(|found| found.end()).unwrap_or(0);
    let body = js::trim(&text[end..]);
    Some(GrepResult {
        query,
        body: (!body.is_empty()).then(|| body.to_string()),
    })
}

struct ReadResult {
    path: String,
    start_line: Option<i64>,
    body: Option<String>,
}

static FIRST_LINE_NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(\d+)\t").unwrap());

/// `<path>notes.txt</path>\n<content>\n1\thello\n</content>`
fn read_result(text: Option<&str>) -> Option<ReadResult> {
    let text = text?;
    let path = section(text, "path").map(|path| js::trim(&path).to_string())?;
    if path.is_empty() {
        return None;
    }
    let body = section(text, "content");
    let start_line = body
        .as_deref()
        .and_then(|body| FIRST_LINE_NUMBER.captures(body))
        .and_then(|captures| js::parse_number(&captures[1]))
        .map(|line| line as i64);
    Some(ReadResult {
        path,
        start_line,
        body: body
            .map(|body| js::trim(&body).to_string())
            .filter(|body| !body.is_empty()),
    })
}

static LISTING_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^(\S.*?):\s*$").unwrap());
static LISTING_ENTRY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*[-*]\s+\S").unwrap());

/// `.:\n- notes.txt\n`: a directory listing keyed by its header line.
fn listing_result(text: Option<&str>) -> Option<String> {
    let text = text?;
    let captures = LISTING_HEADER.captures(text)?;
    if captures.get(0).map(|found| found.start()) != Some(0) {
        return None;
    }
    if !LISTING_ENTRY.is_match(text) {
        return None;
    }
    Some(js::trim(&captures[1]).to_string())
}

struct WriteResult {
    verb: &'static str,
    path: String,
}

static WRITE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^\s*(wrote|edited|created|updated|deleted|moved|removed)\s+(\S+?)(?:\s+\(|\s*$)",
    )
    .unwrap()
});

/// `wrote out.txt (4 bytes)` and `edited app.ts (41 bytes)`.
fn write_result(text: Option<&str>) -> Option<WriteResult> {
    let captures = WRITE.captures(text?)?;
    let verb = match captures[1].to_lowercase().as_str() {
        "wrote" | "created" => "Write",
        "deleted" | "removed" => "Delete",
        "moved" => "Move",
        _ => "Edit",
    };
    Some(WriteResult {
        verb,
        path: captures[2].to_string(),
    })
}

/// fx nests result text as `content[].content.text`.
fn result_text(update: &Rec, tool: &Rec) -> Option<String> {
    let raw = update
        .get("content")
        .filter(|value| !value.is_null())
        .or_else(|| tool.get("content"));
    let text = flatten_text(raw);
    (!js::trim(&text).is_empty()).then_some(text)
}

fn flatten_text(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| flatten_text(Some(item)))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Some(Value::Object(rec)) => match rec.get("text") {
            Some(Value::String(text)) => text.clone(),
            _ => match rec.get("content").filter(|value| !value.is_null()) {
                Some(content) => flatten_text(Some(content)),
                None => String::new(),
            },
        },
        _ => String::new(),
    }
}

/// The first `<tag>...</tag>` body, without the newlines just inside the tags.
fn section(text: &str, tag: &str) -> Option<String> {
    let pattern = format!(r"<{tag}>\n?([\s\S]*?)\n?</{tag}>");
    let regex = Regex::new(&pattern).ok()?;
    regex.captures(text).map(|captures| captures[1].to_string())
}

static TRAILING_SEPARATORS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[/\\]+$").unwrap());

fn basename(path: &str) -> String {
    let stripped = TRAILING_SEPARATORS.replace(path, "");
    let trimmed = if stripped.is_empty() { path } else { &stripped };
    trimmed
        .split(['/', '\\'])
        .rfind(|part| !part.is_empty())
        .unwrap_or(trimmed)
        .to_string()
}

/// fxTool's `stringField` trims the value it returns.
fn string_field(rec: &Rec, key: &str) -> Option<String> {
    match rec.get(key) {
        Some(Value::String(value)) if !js::trim(value).is_empty() => {
            Some(js::trim(value).to_string())
        }
        _ => None,
    }
}
