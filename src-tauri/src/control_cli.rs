//! The desktop executable also provides a small, JSON-only control client.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde_json::{json, Value};

const USAGE: &str = r#"MonoCode local control — supervise this orchestration run from the lead agent.

Usage: {exe} control ACTION [--json JSON | --input FILE|-] [--request-id ID]

Actions, with the JSON object each one takes:
  list      {}
            The run, every task with its status and latest result, and the
            harness/model IDs you may assign.
  delegate  {"title":"Short title","harness":"<id from list>",
             "model":"<id from list>","prompt":"Self-contained instructions",
             "files":["src/feature"],"dependsOn":["<taskId>"]}
            Queue a worker and return its taskId. "model" is optional and
            defaults to the first model list allows for that harness.
            "files" is the write scope: project-relative paths, where a
            directory covers its descendants and ["."] reserves the whole
            checkout. "dependsOn" holds taskIds that must be reviewed first.
  get       {"taskId":"..."}
            One task, including its latest result.
  wait      {"timeoutSeconds":20}
            Block until a task changes state, or until the timeout (0-25).
            Returns at once when paused, stopped, or nothing is running or queued.
  respond   {"taskId":"...","requestId":7,"decision":"allow"|"deny"}
            Answer an approval an agent is blocked on. Agents never prompt the
            user; list, get and wait report the prompt as that task's
            "needsInput", and it stays stopped until you decide.
  answer    {"taskId":"...","requestId":9,"answers":{"<questionId>":["<optionId>"]}}
            Answer a question an agent asked, or pass "skip":true instead of
            "answers". The question and its options come from needsInput.
  steer     {"taskId":"...","text":"..."}
            Redirect an agent that is still running, without discarding the
            work it has already done. Use this the moment you see it going
            the wrong way; message only lands once it has stopped.
  message   {"taskId":"...","text":"..."}
            Send a stopped worker another turn within its existing scope; it
            keeps its session, checkout and history.
  retry     {"taskId":"...","text":"...","files":["src/feature"]}
            Retry a stopped worker with corrected project-relative write
            scopes. Use this only when the additional files are required.
  cancel    {"taskId":"..."}
            Cancel a task, whether it is running or still queued.
  review    {"taskId":"..."}
            Accept a completed task's result.
  finish    {}
            End the run, once every task is accepted or cancelled.

Usual loop: list -> delegate ... -> wait or get -> steer an agent that drifts,
unblock one with respond or answer -> inspect the changes yourself -> message
for corrections -> review each task -> finish.

When paused, list, get and wait still return the reason and recovery steps.
Do not keep polling or retry mutations. Explain the pause and ask the user to
click Resume in MonoCode. Resume continues interrupted workers in their
retained checkouts. A policy-blocked worker remains stopped until message,
retry or cancel explicitly resolves it.

Output is one JSON line: {"ok":true,"result":...} or {"ok":false,"error":"..."}.
The exit code is 0 only when "ok" is true.

Input must be a JSON object; unknown fields are rejected rather than ignored.
--json takes it inline, --input FILE reads a file, --input - reads stdin.

Every call carries a request ID, and the run applies each ID at most once. A
failed response reports the ID it used whenever the outcome is unknown — a
timeout, say. Retry that exact call with --request-id ID; retrying a delegate
under a fresh ID instead would queue a second worker.

Tasks run inside the MonoCode app, not in this process. Exiting this CLI, or a
failure here, never cancels a task that was already accepted.

MonoCode sets MONOCODE_CONTROL_ENDPOINT and MONOCODE_CONTROL_TOKEN for the lead
agent's process only. They are already in your environment; never print them.
"#;

const ACTIONS: [&str; 12] = [
    "list", "delegate", "get", "steer", "message", "retry", "cancel", "wait", "review", "finish",
    "respond", "answer",
];
const APP_ACTIONS: [&str; 10] = [
    "models.list",
    "sessions.list",
    "sessions.read",
    "sessions.send",
    "sessions.draft",
    "sessions.start",
    "folders.list",
    "folders.move",
    "notes.list",
    "notes.read",
];
const APP_USAGE: &str = r#"MonoCode app access — use in a thread enabled by /mono.

Usage: {exe} app ACTION [--json JSON | --input FILE|-] [--request-id ID]

Actions:
  models.list    {}  Available providers, models, settings and permission modes.
  sessions.list  {}  Project sessions with IDs, busy status and hasDraft.
  sessions.read  {"sessionId":"...","before":"<turnId>","limit":3,"maxChars":1200}
                  Read up to 3 recent user/assistant exchanges. Tools and
                  reasoning are omitted. Omit before for the newest page;
                  pass nextBefore from a result for older exchanges. maxChars
                  caps each message (200-6000, default 1200).
  sessions.send  {"sessionId":"...","prompt":"..."}
                  Submit a follow-up to an idle session in this project.
                  A busy session is rejected. Reuse --request-id on retries.
  sessions.draft {"sessionId":"...","prompt":"..."}
                  Save an unsent draft in an idle project session. Existing
                  drafts are preserved; send or remove one in MonoCode first.
                  Reuse --request-id on retries.
  sessions.start {"prompt":"...","harness":"codex","model":"codex:...",
                  "effort":"high","reveal":false,
                  "workspaceMode":"current","draft":false}
                  Create a tab with the prompt. Set draft:true to save it
                  unsent; no agent turn runs. Otherwise the turn is submitted.
                  Returns after creation/acceptance, not agent completion;
                  use its ID with folders.move immediately. Optional model,
                  effort, modelSettings, permission mode and workspace choice
                  use composer values. Omit runtimeMode to inherit this
                  session's permission mode; set it to override. Run
                  models.list for allowed IDs. cwd is your project; no attachments.
  folders.list   {}  Folders in your current project.
  folders.move   {"sessionId":"...","folderId":"..."}
                  Or use "newFolderName":"Research" to create a folder.
  notes.list     {"limit":30,"offset":0}  Titles and short previews only.
  notes.read     {"id":"..."}  Full body of one note.

The output is one JSON line: {"ok":true,"result":...} or {"ok":false,"error":"..."}.
Use --input - to pass JSON on stdin. Never print MonoCode credentials.
Keep the same --request-id when retrying a call after an uncertain result.
"#;

/// Quote for the shell the lead agent actually runs commands in, and only when
/// the path needs it. The path is absolute, so a leading slash means a POSIX
/// shell — where a backslash escapes rather than separates, and so is never
/// safe bare.
fn quoted(value: &str) -> String {
    if !value.starts_with('/') {
        return if value.contains([' ', '\t', '"']) {
            format!("\"{}\"", value.replace('"', ""))
        } else {
            value.into()
        };
    }
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._/-:".contains(c))
    {
        value.into()
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

pub fn help() -> String {
    let exe = std::env::current_exe()
        .map(|path| quoted(&path.to_string_lossy()))
        .unwrap_or_else(|_| "monocode".into());
    USAGE.replace("{exe}", &exe)
}

pub fn app_help() -> String {
    let exe = std::env::current_exe()
        .map(|path| quoted(&path.to_string_lossy()))
        .unwrap_or_else(|_| "monocode".into());
    APP_USAGE.replace("{exe}", &exe)
}

enum Parsed {
    Help,
    Call(String, Value, String),
}

pub fn run(args: Vec<String>) -> i32 {
    run_mode(args, false)
}

pub fn run_app(args: Vec<String>) -> i32 {
    run_mode(args, true)
}

fn run_mode(args: Vec<String>, app_mode: bool) -> i32 {
    let parsed = match parse_args_for(&args, app_mode) {
        Ok(parsed) => parsed,
        Err(error) => {
            println!("{}", json!({"ok": false, "error": error}));
            return 1;
        }
    };
    let (action, input, request_id) = match parsed {
        Parsed::Help => {
            println!("{}", if app_mode { app_help() } else { help() });
            return 0;
        }
        Parsed::Call(action, input, request_id) => (action, input, request_id),
    };
    match send(&action, &input, &request_id, app_mode) {
        Ok(mut value) => {
            if value.get("ok").and_then(Value::as_bool) == Some(true) {
                println!("{value}");
                return 0;
            }
            value = with_retry_hint(value, &request_id);
            println!("{value}");
            1
        }
        Err(Failure { error, sent }) => {
            // The call may have reached the run even though its answer was
            // lost. Hand back the request ID so a retry cannot duplicate it.
            let mut response = json!({"ok": false, "error": error});
            if sent {
                response["requestId"] = json!(request_id);
                response["retryWith"] = json!(format!("--request-id {request_id}"));
            }
            println!("{response}");
            1
        }
    }
}

fn with_retry_hint(mut value: Value, request_id: &str) -> Value {
    // A denied app call never reached the executor. Repeating the same ID
    // cannot grant access, and suggesting it sends agents in loops.
    if value.get("retryable").and_then(Value::as_bool) == Some(false) {
        return value;
    }
    if let Some(object) = value.as_object_mut() {
        object
            .entry("requestId")
            .or_insert_with(|| json!(request_id));
        object
            .entry("retryWith")
            .or_insert_with(|| json!(format!("--request-id {request_id}")));
    }
    value
}

struct Failure {
    error: String,
    /// The request was already on the wire, so the run may have applied it.
    sent: bool,
}
fn unsent(error: impl Into<String>) -> Failure {
    Failure {
        error: error.into(),
        sent: false,
    }
}
fn sent(error: impl Into<String>) -> Failure {
    Failure {
        error: error.into(),
        sent: true,
    }
}

fn send(action: &str, input: &Value, request_id: &str, app_mode: bool) -> Result<Value, Failure> {
    let endpoint_key = if app_mode {
        "MONOCODE_APP_ENDPOINT"
    } else {
        "MONOCODE_CONTROL_ENDPOINT"
    };
    let token_key = if app_mode {
        "MONOCODE_APP_TOKEN"
    } else {
        "MONOCODE_CONTROL_TOKEN"
    };
    let endpoint = std::env::var(endpoint_key).map_err(|_| {
        unsent(if app_mode {
            "No MonoCode app connection. Start this agent turn in MonoCode."
        } else {
            "No MonoCode connection. Confirm the Orchestrator proposal in MonoCode first."
        })
    })?;
    let token = std::env::var(token_key)
        .map_err(|_| unsent("No MonoCode session credential. Start the agent from MonoCode."))?;
    let address: SocketAddr = endpoint
        .parse()
        .map_err(|_| unsent("Invalid MonoCode endpoint"))?;
    if !address.ip().is_loopback() {
        return Err(unsent("MonoCode control only connects to localhost"));
    }
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))
        .map_err(|error| {
            unsent(format!(
                "Cannot connect to MonoCode at {address}: {error}. The app may have restarted, or this agent's sandbox may be blocking localhost."
            ))
        })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(40)))
        .map_err(|e| unsent(e.to_string()))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| unsent(e.to_string()))?;
    writeln!(
        stream,
        "{}",
        json!({"token":token,"action":action,"input":input,"requestId":request_id,"namespace":if app_mode { "app" } else { "control" }})
    )
    .map_err(|e| sent(e.to_string()))?;
    let mut line = String::new();
    let max_response: u64 = if app_mode { 4_000_000 } else { 2_000_000 };
    BufReader::new(stream)
        .take(max_response + 1)
        .read_line(&mut line)
        .map_err(|e| sent(format!("No reply from MonoCode: {e}")))?;
    if line.len() > max_response as usize {
        return Err(sent("MonoCode response is too large"));
    }
    serde_json::from_str(&line).map_err(|_| sent("MonoCode returned an invalid response"))
}

fn read_capped(mut source: impl Read) -> Result<String, String> {
    let mut raw = String::new();
    source
        .by_ref()
        .take(262_145)
        .read_to_string(&mut raw)
        .map_err(|e| e.to_string())?;
    if raw.len() > 262_144 {
        return Err("Input exceeds 256 KiB".into());
    }
    Ok(raw)
}

#[cfg(test)]
fn parse_args(args: &[String]) -> Result<Parsed, String> {
    parse_args_for(args, false)
}

fn parse_args_for(args: &[String], app_mode: bool) -> Result<Parsed, String> {
    let is_help = |value: &str| matches!(value, "help" | "--help" | "-h");
    let Some(action) = args.first() else {
        return Ok(Parsed::Help);
    };
    if is_help(action) {
        return Ok(Parsed::Help);
    }
    let action = action.clone();
    let allowed = if app_mode {
        APP_ACTIONS.contains(&action.as_str())
    } else {
        ACTIONS.contains(&action.as_str())
    };
    if !allowed {
        let names = if app_mode {
            APP_ACTIONS.join(", ")
        } else {
            ACTIONS.join(", ")
        };
        return Err(format!(
            "Unknown action: {action}. Use one of: {names}. Run {} --help.",
            if app_mode { "app" } else { "control" }
        ));
    }
    let mut input = None;
    let mut request_id = uuid::Uuid::new_v4().to_string();
    let mut index = 1;
    while index < args.len() {
        let flag = &args[index];
        // `control delegate --help` should explain the command, not fail.
        if is_help(flag) {
            return Ok(Parsed::Help);
        }
        if !flag.starts_with("--") {
            return Err(format!(
                "Unexpected argument: {flag}. Pass the JSON object as --json '<JSON>'."
            ));
        }
        let value = args.get(index + 1).ok_or_else(|| {
            format!("Missing value for {flag}. Run control --help for the argument list.")
        })?;
        match flag.as_str() {
            "--request-id" => request_id = value.clone(),
            "--json" | "--input" => {
                if input.is_some() {
                    return Err("Supply only one input".into());
                }
                let raw = if flag == "--json" {
                    if value.len() > 262_144 {
                        return Err("Input exceeds 256 KiB".into());
                    }
                    value.clone()
                } else if value == "-" {
                    read_capped(std::io::stdin())?
                } else {
                    read_capped(std::fs::File::open(value).map_err(|e| e.to_string())?)?
                };
                let parsed: Value = serde_json::from_str(&raw)
                    .map_err(|e| format!("Invalid JSON: {e}. Pass one JSON object, e.g. --json '{{\"taskId\":\"...\"}}'."))?;
                if !parsed.is_object() {
                    return Err("Input must be a JSON object".into());
                }
                input = Some(parsed);
            }
            _ => {
                return Err(format!(
                    "Unknown option: {flag}. Supported: --json, --input, --request-id."
                ))
            }
        }
        index += 2;
    }
    if request_id.is_empty() || request_id.len() > 128 {
        return Err("Invalid request ID".into());
    }
    if app_mode
        && !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("App request IDs may contain only letters, digits, - and _".into());
    }
    Ok(Parsed::Call(
        action,
        input.unwrap_or_else(|| json!({})),
        request_id,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }
    fn call(values: &[&str]) -> Result<(String, Value, String), String> {
        match parse_args(&args(values))? {
            Parsed::Call(action, input, id) => Ok((action, input, id)),
            Parsed::Help => Err("help".into()),
        }
    }
    #[test]
    fn validates_inputs_without_invoking_a_shell() {
        let (_, input, id) = call(&[
            "delegate",
            "--json",
            r#"{"prompt":"$(touch nope) `hello`\nnext"}"#,
            "--request-id",
            "retry-1",
        ])
        .unwrap();
        assert_eq!(id, "retry-1");
        assert_eq!(input["prompt"], "$(touch nope) `hello`\nnext");
        assert!(call(&["delegate", "--json", "[]"]).is_err());
        assert!(call(&["delegate", "--json", "{}", "--json", "{}"]).is_err());
        assert!(call(&["unknown"]).is_err());
    }
    #[test]
    fn explains_help_and_malformed_invocations() {
        assert!(matches!(parse_args(&args(&[])), Ok(Parsed::Help)));
        assert!(matches!(parse_args(&args(&["--help"])), Ok(Parsed::Help)));
        // Agents commonly probe a subcommand for its own usage text.
        assert!(matches!(
            parse_args(&args(&["delegate", "--help"])),
            Ok(Parsed::Help)
        ));
        assert!(call(&["get", r#"{"taskId":"x"}"#])
            .unwrap_err()
            .contains("--json"));
        assert!(call(&["get", "--json"]).unwrap_err().contains("--help"));
        assert!(call(&["get", "--taskId", "x"])
            .unwrap_err()
            .contains("Unknown option"));
        assert!(call(&["get", "--json", "{taskId}"])
            .unwrap_err()
            .contains("Invalid JSON"));
    }
    #[test]
    fn help_names_every_action_and_the_real_executable() {
        let text = help();
        for action in ACTIONS {
            assert!(text.contains(action), "help omits {action}");
        }
        assert!(!text.contains("{exe}"));
        assert!(text.contains("--request-id"));
    }
    #[test]
    fn quotes_the_control_path_only_when_the_shell_needs_it() {
        assert_eq!(
            quoted("/Applications/MonoCode.app/Contents/MacOS/monocode"),
            "/Applications/MonoCode.app/Contents/MacOS/monocode"
        );
        assert_eq!(quoted("/Users/a b/MonoCode"), "'/Users/a b/MonoCode'");
        assert_eq!(quoted("C:\\Tools\\monocode.exe"), "C:\\Tools\\monocode.exe");
        assert_eq!(
            quoted("C:\\Program Files\\MonoCode\\monocode.exe"),
            "\"C:\\Program Files\\MonoCode\\monocode.exe\""
        );
        // A backslash escapes in a POSIX shell, so bare would rewrite the path.
        assert_eq!(quoted("/Users/a\\b/MonoCode"), "'/Users/a\\b/MonoCode'");
        assert_eq!(quoted("/Users/it's/MonoCode"), r"'/Users/it'\''s/MonoCode'");
    }
    #[test]
    fn app_mode_exposes_only_app_actions_and_safe_request_ids() {
        assert!(matches!(
            parse_args_for(&args(&["notes.list"]), true),
            Ok(Parsed::Call(_, _, _))
        ));
        for action in ["sessions.read", "sessions.send", "sessions.draft"] {
            assert!(matches!(
                parse_args_for(&args(&[action, "--json", r#"{"sessionId":"other"}"#]), true),
                Ok(Parsed::Call(_, _, _))
            ));
            assert!(app_help().contains(action));
        }
        assert!(app_help().contains("draft:true"));
        assert!(app_help().contains("inherit this"));
        assert!(parse_args_for(&args(&["delegate"]), true).is_err());
        assert!(
            parse_args_for(&args(&["sessions.start", "--request-id", "bad/id"]), true).is_err()
        );
        assert!(app_help().contains("notes.read"));
    }

    #[test]
    fn inactive_app_turn_does_not_suggest_retries() {
        let denied = with_retry_hint(json!({"ok":false,"retryable":false}), "id-1");
        assert!(denied.get("retryWith").is_none());
        assert!(denied.get("requestId").is_none());
        let uncertain = with_retry_hint(json!({"ok":false,"error":"timeout"}), "id-1");
        assert_eq!(uncertain["retryWith"], "--request-id id-1");
    }
}
