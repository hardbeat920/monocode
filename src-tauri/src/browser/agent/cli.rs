//! `monocode browser ACTION ...`: the browser tools as a CLI (spec 12.5), for
//! providers without MCP support and for manual testing. Same request, same
//! endpoint, same token as the MCP server.
//!
//! Output is the result envelope as one JSON line, except that a successful
//! snapshot prints its text and a screenshot is written to a file whose path
//! replaces the image in `data`. Exit codes: 0 done, 2 blocked, 3 failed,
//! 4 unknown, 1 for errors of the CLI itself (no credentials, bad arguments).

use std::io::Read;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::{json, Map, Value};

use super::client::{self, Credentials, MissingCredentials};
use crate::browser::protocol::{ActionKind, ActionResult, Outcome};

const USAGE: &str = r#"MonoCode browser — use your own tabs in the user's Chrome or Edge.

Usage: {exe} browser ACTION [--FIELD VALUE ...] [--json JSON | --input FILE|-]
                     [--request-id ID] [--envelope] [--out FILE]

Actions and their fields (* required). Every action except tabs.* also takes
--timeout-ms.
  tabs.list                      Your tabs.
  tabs.open      --url           Open a background tab; data.tab is its id.
  tabs.close     --tab*          Close one of your tabs.
  tabs.focus     --tab* --user-visible true
                                 Show the tab to the user; only when asked.
  navigate       --tab* --url* --wait-until load|domcontentloaded
  snapshot       --tab* --scope REF --mode interactive|full --max-nodes N
  click          --tab* --ref* --button left|right|middle --count 1|2
                 --modifiers Alt|Control|Meta|Shift (repeatable)
  hover          --tab* --ref*
  type           --tab* --ref* --text* --clear --submit
  press          --tab* --key* (Enter, Escape, Control+A) --ref
  select         --tab* --ref* --values* (repeatable)
  scroll         --tab* --ref | --direction up|down|left|right --amount PX
  upload         --tab* --ref* --paths* (repeatable)
  dialog         --tab* --action* accept|dismiss --text
  wait           --tab* --for* time|text|ref|url|load --value
  back, forward  --tab*
  screenshot     --tab* --ref --full-page --format png|jpeg --quality N
                 The image is written to --out FILE (default: a temporary
                 file) and data.path names it.
  read           --tab* --ref --format text|markdown --offset N

Fields can also come as one JSON object (--json '{"tab":"t_1a2b3c4d"}',
--input FILE, or --input - for stdin); flags given as well override it.
Boolean flags take true or false, or nothing for true.

Take a snapshot first and act on its refs (e12). After every action that
changes the page, take a new snapshot; old refs fail with ref_stale.

Output is one JSON result: outcome (done, blocked, failed, unknown), reason,
message, retryable, observed, data. A done snapshot prints only its text;
pass --envelope for the JSON. Exit code: 0 done, 2 blocked, 3 failed,
4 unknown, 1 when the CLI itself could not run the call.

blocked means nothing was sent: change the plan, never work around it.
unknown means the action may have happened: take a snapshot before retrying,
and retry only with --request-id set to the request_id it reported, so
MonoCode can recognise the retry. Dialogs are never answered for you.

MonoCode sets MONOCODE_BROWSER_ENDPOINT and MONOCODE_BROWSER_TOKEN for
sessions it starts. They are already in your environment; never print them.
"#;

/// Flags whose value is text.
const STRINGS: [&str; 13] = [
    "tab",
    "ref",
    "url",
    "text",
    "key",
    "scope",
    "mode",
    "button",
    "direction",
    "action",
    "for",
    "wait_until",
    "format",
];
/// Flags whose value is a non-negative integer.
const INTEGERS: [&str; 6] = [
    "timeout_ms",
    "max_nodes",
    "count",
    "amount",
    "quality",
    "offset",
];
/// Flags that may stand alone for `true`.
const BOOLEANS: [&str; 4] = ["clear", "submit", "full_page", "user_visible"];
/// Repeatable flags collected into an array.
const LISTS: [&str; 3] = ["values", "paths", "modifiers"];

pub fn help() -> String {
    USAGE.replace("{exe}", &exe())
}

fn exe() -> String {
    std::env::current_exe()
        .map(|path| crate::control_cli::quoted(&path.to_string_lossy()))
        .unwrap_or_else(|_| "monocode".into())
}

#[derive(Debug, PartialEq)]
enum Parsed {
    Help,
    Call(Call),
}

#[derive(Debug, PartialEq)]
struct Call {
    kind: ActionKind,
    input: Value,
    request_id: String,
    envelope: bool,
    out: Option<PathBuf>,
}

pub fn run(args: Vec<String>) -> i32 {
    let (text, code) = execute(&args, &mut std::io::stdin(), Credentials::from_env);
    println!("{text}");
    code
}

/// Parse, call and render one invocation: the text to print and the exit code.
fn execute(
    args: &[String],
    stdin: &mut dyn Read,
    credentials: impl FnOnce() -> Result<Credentials, MissingCredentials>,
) -> (String, i32) {
    let call = match parse(args, stdin) {
        Ok(Parsed::Help) => return (help(), 0),
        Ok(Parsed::Call(call)) => call,
        Err(error) => return (json!({"ok": false, "error": error}).to_string(), 1),
    };
    let credentials = match credentials() {
        Ok(credentials) => credentials,
        Err(missing) => {
            return (
                json!({"ok": false, "error": missing.message()}).to_string(),
                1,
            )
        }
    };
    let result = client::call(
        &credentials,
        call.kind.name(),
        &call.input,
        &call.request_id,
    );
    render(&call, result)
}

/// [`execute`] with explicit credentials, for the module's end-to-end tests.
#[cfg(test)]
pub(crate) fn execute_with(args: &[&str], credentials: Credentials) -> (String, i32) {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    execute(&args, &mut std::io::empty(), move || Ok(credentials))
}

fn exit_code(outcome: Outcome) -> i32 {
    match outcome {
        Outcome::Done => 0,
        Outcome::Blocked => 2,
        Outcome::Failed => 3,
        Outcome::Unknown => 4,
    }
}

/// What to print for a result, and the exit code.
fn render(call: &Call, mut result: ActionResult) -> (String, i32) {
    let code = exit_code(result.outcome);
    if result.outcome == Outcome::Done {
        match call.kind {
            ActionKind::Snapshot if !call.envelope => {
                if let Some(text) = client::data_text(result.data.as_ref()) {
                    return (text.to_string(), code);
                }
            }
            ActionKind::Screenshot => {
                if let Some((image, mime)) = client::data_image(result.data.as_ref()) {
                    let path = call
                        .out
                        .clone()
                        .unwrap_or_else(|| default_image_path(&result.request_id, &mime));
                    match save_image(image, &path) {
                        Ok(bytes) => {
                            let mut data = client::data_without(
                                result.data.as_ref(),
                                &client::IMAGE_KEYS,
                            )
                            .and_then(|data| data.as_object().cloned())
                            .unwrap_or_default();
                            data.insert("path".into(), json!(path.to_string_lossy()));
                            data.insert("mime_type".into(), json!(mime));
                            data.insert("bytes".into(), json!(bytes));
                            result.data = Some(Value::Object(data));
                        }
                        Err(error) => {
                            return (
                                json!({"ok": false, "error": format!("The screenshot was taken but could not be saved to {}: {error}", path.display())}).to_string(),
                                1,
                            )
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (
        serde_json::to_string(&result).unwrap_or_else(|_| "{}".into()),
        code,
    )
}

fn default_image_path(request_id: &str, mime: &str) -> PathBuf {
    let extension = if mime == "image/jpeg" { "jpg" } else { "png" };
    let safe: String = request_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    std::env::temp_dir().join(format!("monocode-browser-{safe}.{extension}"))
}

fn save_image(base64: &str, path: &Path) -> Result<usize, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64.trim())
        .map_err(|e| format!("invalid image data ({e})"))?;
    std::fs::write(path, &bytes).map_err(|e| e.to_string())?;
    Ok(bytes.len())
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

fn parse(args: &[String], stdin: &mut dyn Read) -> Result<Parsed, String> {
    let is_help = |value: &str| matches!(value, "help" | "--help" | "-h");
    let Some(action) = args.first() else {
        return Ok(Parsed::Help);
    };
    if is_help(action) {
        return Ok(Parsed::Help);
    }
    let kind = ActionKind::parse(action).ok_or_else(|| {
        let names: Vec<&str> = ActionKind::ALL.iter().map(|kind| kind.name()).collect();
        format!(
            "Unknown action: {action}. Use one of: {}. Run browser --help.",
            names.join(", ")
        )
    })?;
    let mut base: Option<Map<String, Value>> = None;
    let mut fields = Map::new();
    let mut request_id = None;
    let mut envelope = false;
    let mut out = None;
    let mut index = 1;
    while index < args.len() {
        let flag = &args[index];
        index += 1;
        if is_help(flag) {
            return Ok(Parsed::Help);
        }
        let Some(name) = flag.strip_prefix("--") else {
            return Err(format!(
                "Unexpected argument: {flag}. Pass fields as --name value. Run browser --help."
            ));
        };
        let name = name.replace('-', "_");
        if name == "envelope" {
            envelope = true;
            continue;
        }
        if BOOLEANS.contains(&name.as_str()) {
            let value = match args.get(index).map(String::as_str) {
                Some("true") => {
                    index += 1;
                    true
                }
                Some("false") => {
                    index += 1;
                    false
                }
                _ => true,
            };
            fields.insert(name, json!(value));
            continue;
        }
        let value = args
            .get(index)
            .ok_or_else(|| format!("Missing value for {flag}. Run browser --help."))?;
        index += 1;
        match name.as_str() {
            "request_id" => request_id = Some(value.clone()),
            "out" => out = Some(PathBuf::from(value)),
            "json" | "input" => {
                if base.is_some() {
                    return Err("Supply only one of --json and --input".into());
                }
                let raw = if name == "json" {
                    if value.len() > 262_144 {
                        return Err("Input exceeds 256 KiB".into());
                    }
                    value.clone()
                } else if value == "-" {
                    read_capped(&mut *stdin)?
                } else {
                    read_capped(std::fs::File::open(value).map_err(|e| e.to_string())?)?
                };
                let parsed: Value = serde_json::from_str(&raw).map_err(|e| {
                    format!("Invalid JSON: {e}. Pass one object, e.g. --json '{{\"tab\":\"t_1a2b3c4d\"}}'.")
                })?;
                let Value::Object(object) = parsed else {
                    return Err("Input must be a JSON object".into());
                };
                base = Some(object);
            }
            "value" => {
                fields.insert(name, json!(value));
            }
            field if STRINGS.contains(&field) => {
                fields.insert(name, json!(value));
            }
            field if INTEGERS.contains(&field) => {
                let number: u64 = value
                    .parse()
                    .map_err(|_| format!("{flag} takes a whole number, not {value}"))?;
                fields.insert(name, json!(number));
            }
            field if LISTS.contains(&field) => {
                let items: Vec<Value> = if value.trim_start().starts_with('[') {
                    serde_json::from_str(value)
                        .map_err(|_| format!("{flag} takes a value or a JSON array of strings"))?
                } else {
                    vec![json!(value)]
                };
                let list = fields.entry(name).or_insert_with(|| json!([]));
                list.as_array_mut()
                    .ok_or_else(|| format!("{flag} was given twice in different forms"))?
                    .extend(items);
            }
            _ => {
                return Err(format!(
                    "Unknown option: {flag}. Run browser --help for the fields of each action."
                ))
            }
        }
    }
    let mut input = base.unwrap_or_default();
    input.extend(fields);
    // `wait --for time --value 500` means 500 ms, not the text "500".
    if input.get("for").and_then(Value::as_str) == Some("time") {
        if let Some(ms) = input
            .get("value")
            .and_then(Value::as_str)
            .and_then(|v| v.parse::<u64>().ok())
        {
            input.insert("value".into(), json!(ms));
        }
    }
    let request_id = request_id.unwrap_or_else(client::new_request_id);
    if !client::valid_request_id(&request_id) {
        return Err("Request IDs are 1-128 letters, digits or . _ : -".into());
    }
    Ok(Parsed::Call(Call {
        kind,
        input: Value::Object(input),
        request_id,
        envelope,
        out,
    }))
}

/// Instructions injected into turns of providers without MCP (spec 12.5):
/// the shared overview plus the CLI section, with this executable's path.
pub fn instructions() -> String {
    let exe = exe();
    let overview = super::tools::section("overview").unwrap_or_default();
    let cli = super::tools::section("cli").unwrap_or_default();
    // The overview names actions generically; the CLI section shows how to
    // call them. MCP tool names would only confuse a CLI-only agent.
    format!("{cli}\n\n{overview}").replace("{exe}", &exe)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::protocol::{Reason, ScreenshotParams, WaitParams};

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| s.to_string()).collect()
    }

    fn call(values: &[&str]) -> Result<Call, String> {
        match parse(&args(values), &mut std::io::empty())? {
            Parsed::Call(call) => Ok(call),
            Parsed::Help => Err("help".into()),
        }
    }

    #[test]
    fn flags_become_typed_protocol_fields() {
        let click = call(&[
            "click",
            "--tab",
            "t_0a1b2c3d",
            "--ref",
            "e12",
            "--count",
            "2",
            "--modifiers",
            "Shift",
            "--modifiers",
            "Alt",
            "--timeout-ms",
            "500",
        ])
        .unwrap();
        assert_eq!(click.kind, ActionKind::Click);
        assert_eq!(
            click.input,
            json!({"tab": "t_0a1b2c3d", "ref": "e12", "count": 2, "modifiers": ["Shift", "Alt"], "timeout_ms": 500})
        );
        assert!(click.request_id.starts_with("req_"));

        let typed = call(&[
            "type",
            "--tab",
            "t_0a1b2c3d",
            "--ref",
            "e1",
            "--text",
            "123",
            "--clear",
            "--submit",
            "false",
        ])
        .unwrap();
        assert_eq!(typed.input["text"], "123");
        assert_eq!(typed.input["clear"], true);
        assert_eq!(typed.input["submit"], false);

        let wait = call(&[
            "wait",
            "--tab",
            "t_0a1b2c3d",
            "--for",
            "time",
            "--value",
            "500",
        ])
        .unwrap();
        let parsed: WaitParams = serde_json::from_value(wait.input.clone()).unwrap();
        assert_eq!(parsed.value, Some(json!(500)));
        let text = call(&[
            "wait",
            "--tab",
            "t_0a1b2c3d",
            "--for",
            "text",
            "--value",
            "500",
        ])
        .unwrap();
        assert_eq!(text.input["value"], "500");

        let select = call(&[
            "select",
            "--tab",
            "t_0a1b2c3d",
            "--ref",
            "e2",
            "--values",
            r#"["a","b"]"#,
        ])
        .unwrap();
        assert_eq!(select.input["values"], json!(["a", "b"]));

        let shot = call(&[
            "screenshot",
            "--tab",
            "t_0a1b2c3d",
            "--full-page",
            "--format",
            "jpeg",
        ])
        .unwrap();
        serde_json::from_value::<ScreenshotParams>(shot.input).unwrap();
    }

    #[test]
    fn json_input_is_the_base_and_flags_override_it() {
        let navigate = call(&[
            "navigate",
            "--json",
            r#"{"tab":"t_0a1b2c3d","url":"https://a.example"}"#,
            "--url",
            "https://b.example",
            "--request-id",
            "retry-1",
        ])
        .unwrap();
        assert_eq!(navigate.input["url"], "https://b.example");
        assert_eq!(navigate.input["tab"], "t_0a1b2c3d");
        assert_eq!(navigate.request_id, "retry-1");

        let mut stdin = std::io::Cursor::new(br#"{"tab":"t_0a1b2c3d"}"#.to_vec());
        let Parsed::Call(from_stdin) =
            parse(&args(&["snapshot", "--input", "-"]), &mut stdin).unwrap()
        else {
            panic!("expected a call")
        };
        assert_eq!(from_stdin.input, json!({"tab": "t_0a1b2c3d"}));
        assert_eq!(call(&["tabs.list"]).unwrap().input, json!({}));
    }

    #[test]
    fn mistakes_are_reported_without_contacting_monocode() {
        assert!(call(&["evaluate"]).unwrap_err().contains("Unknown action"));
        assert!(call(&["click", "--selector", "#x"])
            .unwrap_err()
            .contains("Unknown option"));
        assert!(call(&["click", "e12"])
            .unwrap_err()
            .contains("Unexpected argument"));
        assert!(call(&["click", "--tab"])
            .unwrap_err()
            .contains("Missing value"));
        assert!(call(&["scroll", "--amount", "-5"])
            .unwrap_err()
            .contains("whole number"));
        assert!(call(&["click", "--json", "[]"])
            .unwrap_err()
            .contains("object"));
        assert!(call(&["click", "--json", "{}", "--input", "x"]).is_err());
        assert!(call(&["click", "--request-id", "a b"]).is_err());
        assert!(matches!(
            parse(&args(&[]), &mut std::io::empty()),
            Ok(Parsed::Help)
        ));
        assert!(matches!(
            parse(&args(&["click", "--help"]), &mut std::io::empty()),
            Ok(Parsed::Help)
        ));
    }

    #[test]
    fn help_and_instructions_name_every_action_and_the_executable() {
        let text = help();
        for kind in ActionKind::ALL {
            assert!(text.contains(kind.name()), "help omits {}", kind.name());
        }
        assert!(!text.contains("{exe}"));
        let injected = instructions();
        assert!(!injected.contains("{exe}"));
        assert!(injected.contains("browser snapshot --tab"));
        assert!(injected.contains("never print it"));
    }

    #[test]
    fn exit_codes_and_output_follow_the_outcome() {
        let snapshot = call(&["snapshot", "--tab", "t_0a1b2c3d"]).unwrap();
        let done = ActionResult::done("r1", "ok").with_data(json!({"text": "- link [ref=e1]"}));
        assert_eq!(
            render(&snapshot, done.clone()),
            ("- link [ref=e1]".into(), 0)
        );
        let enveloped = Call {
            envelope: true,
            ..call(&["snapshot", "--tab", "t_0a1b2c3d"]).unwrap()
        };
        let (text, code) = render(&enveloped, done);
        assert_eq!(code, 0);
        assert!(text.contains("\"outcome\":\"done\""));

        for (reason, code) in [
            (Reason::NotGranted, 2),
            (Reason::RefStale, 3),
            (Reason::LinkDown, 4),
        ] {
            let (text, exit) = render(&snapshot, ActionResult::from_reason("r2", reason));
            assert_eq!(exit, code);
            let back: ActionResult = serde_json::from_str(&text).unwrap();
            assert_eq!(back.reason, Some(reason));
        }
    }

    #[test]
    fn screenshots_are_written_to_a_file_and_named_in_data() {
        let dir = std::env::temp_dir().join(format!("monocode-cli-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shot.png");
        let shot = call(&[
            "screenshot",
            "--tab",
            "t_0a1b2c3d",
            "--out",
            path.to_str().unwrap(),
        ])
        .unwrap();
        let png = [0x89u8, b'P', b'N', b'G'];
        let encoded = base64::engine::general_purpose::STANDARD.encode(png);
        let result = ActionResult::done("r3", "Captured.")
            .with_data(json!({"image": encoded, "mime_type": "image/png", "width": 1}));
        let (text, code) = render(&shot, result);
        assert_eq!(code, 0);
        let printed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(printed["data"]["path"], path.to_string_lossy().as_ref());
        assert_eq!(printed["data"]["bytes"], 4);
        assert_eq!(printed["data"]["width"], 1);
        assert!(printed["data"].get("image").is_none());
        assert_eq!(std::fs::read(&path).unwrap(), png);
        std::fs::remove_dir_all(dir).unwrap();

        assert!(default_image_path("req_1/../x", "image/jpeg")
            .to_string_lossy()
            .ends_with("monocode-browser-req_1x.jpg"));
    }
}
