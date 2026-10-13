//! The agent-facing tool set (spec 12.3): names, descriptions, input schemas
//! and the mapping from a tool call to a protocol action.
//!
//! Schemas are written by hand from the parameter records in `protocol.rs`;
//! the tests prove that every schema accepts exactly the fields its record
//! accepts and requires exactly the ones it requires. Descriptions come from
//! `docs/browser-instructions.md`, the same text the CLI instructions use.

use serde_json::{json, Map, Value};

use crate::browser::protocol::ActionKind;

/// Shared agent instructions; see the file header for its layout.
pub const INSTRUCTIONS: &str = include_str!("../../../../docs/browser-instructions.md");

/// The body of `## name` in [`INSTRUCTIONS`], trimmed.
pub fn section(name: &str) -> Option<&'static str> {
    // A Windows checkout with core.autocrlf may embed the file with CRLF
    // line ends; normalise once so every lookup sees `\n`.
    static NORMALISED: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    section_in(
        NORMALISED.get_or_init(|| INSTRUCTIONS.replace("\r\n", "\n")),
        name,
    )
}

fn section_in<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let heading = format!("\n## {name}\n");
    let start = text.find(&heading)? + heading.len();
    let rest = &text[start..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    Some(rest[..end].trim())
}

/// What MCP clients show and may use to decide on confirmations.
#[derive(Clone, Copy, Debug)]
pub struct Hints {
    pub read_only: bool,
    pub destructive: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Tool {
    pub name: &'static str,
    pub title: &'static str,
    pub hints: Hints,
}

const OBSERVE: Hints = Hints {
    read_only: true,
    destructive: false,
};
const ACT: Hints = Hints {
    read_only: false,
    destructive: false,
};
/// Can submit forms, delete, pay: anything the page allows.
const COMMIT: Hints = Hints {
    read_only: false,
    destructive: true,
};

pub const TOOLS: [Tool; 15] = [
    Tool {
        name: "browser_tabs",
        title: "Browser tabs",
        hints: ACT,
    },
    Tool {
        name: "browser_navigate",
        title: "Open URL",
        hints: ACT,
    },
    Tool {
        name: "browser_snapshot",
        title: "Page snapshot",
        hints: OBSERVE,
    },
    Tool {
        name: "browser_click",
        title: "Click",
        hints: COMMIT,
    },
    Tool {
        name: "browser_hover",
        title: "Hover",
        hints: ACT,
    },
    Tool {
        name: "browser_type",
        title: "Type text",
        hints: COMMIT,
    },
    Tool {
        name: "browser_press",
        title: "Press key",
        hints: COMMIT,
    },
    Tool {
        name: "browser_select",
        title: "Select option",
        hints: ACT,
    },
    Tool {
        name: "browser_scroll",
        title: "Scroll",
        hints: ACT,
    },
    Tool {
        name: "browser_upload",
        title: "Upload files",
        hints: COMMIT,
    },
    Tool {
        name: "browser_dialog",
        title: "Answer dialog",
        hints: COMMIT,
    },
    Tool {
        name: "browser_wait",
        title: "Wait",
        hints: OBSERVE,
    },
    Tool {
        name: "browser_back",
        title: "Back or forward",
        hints: ACT,
    },
    Tool {
        name: "browser_screenshot",
        title: "Screenshot",
        hints: OBSERVE,
    },
    Tool {
        name: "browser_read",
        title: "Read page text",
        hints: OBSERVE,
    },
];

pub fn find(name: &str) -> Option<Tool> {
    TOOLS.into_iter().find(|tool| tool.name == name)
}

// --- schema building blocks -------------------------------------------------

fn tab() -> Value {
    json!({
        "type": "string",
        "pattern": "^t_[0-9a-f]{8}$",
        "description": "Your tab id (t_ + 8 hex) from browser_tabs."
    })
}

fn element(what: &str) -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "description": format!("{what} ref from the latest browser_snapshot, e.g. e12.")
    })
}

fn timeout() -> Value {
    json!({
        "type": "integer",
        "minimum": 100,
        "description": "Time limit in milliseconds; capped per action."
    })
}

fn request_id() -> Value {
    json!({
        "type": "string",
        "minLength": 1,
        "maxLength": 128,
        "pattern": "^[A-Za-z0-9._:-]+$",
        "description": "Only to retry a call whose outcome was unknown: pass the request_id it reported. Omit otherwise."
    })
}

fn one_of(values: &[&str], description: &str) -> Value {
    json!({"type": "string", "enum": values, "description": description})
}

fn object(properties: Vec<(&str, Value)>, required: &[&str]) -> Value {
    let mut map = Map::new();
    for (name, schema) in properties {
        map.insert(name.to_string(), schema);
    }
    map.insert("request_id".into(), request_id());
    json!({
        "type": "object",
        "properties": map,
        "required": required,
        "additionalProperties": false
    })
}

/// JSON Schema for a tool's arguments.
pub fn input_schema(name: &str) -> Option<Value> {
    Some(match name {
        "browser_tabs" => object(
            vec![
                (
                    "action",
                    one_of(
                        &["list", "open", "close", "focus"],
                        "list your tabs, open a new one, close one, or show one to the user.",
                    ),
                ),
                (
                    "url",
                    json!({"type": "string", "description": "open: http(s) URL to load; about:blank when omitted."}),
                ),
                (
                    "tab",
                    json!({"type": "string", "pattern": "^t_[0-9a-f]{8}$", "description": "close and focus: your tab id."}),
                ),
                (
                    "user_visible",
                    json!({"type": "boolean", "description": "focus: must be true; switches the user's visible tab."}),
                ),
            ],
            &["action"],
        ),
        "browser_navigate" => object(
            vec![
                ("tab", tab()),
                (
                    "url",
                    json!({"type": "string", "minLength": 1, "description": "http(s) URL or about:blank."}),
                ),
                (
                    "wait_until",
                    one_of(
                        &["load", "domcontentloaded"],
                        "Page event to wait for; default load.",
                    ),
                ),
                ("timeout_ms", timeout()),
            ],
            &["tab", "url"],
        ),
        "browser_snapshot" => object(
            vec![
                ("tab", tab()),
                ("scope", element("Only the subtree of this")),
                (
                    "mode",
                    one_of(
                        &["interactive", "full"],
                        "interactive (default): elements you can act on; full: also static text.",
                    ),
                ),
                (
                    "max_nodes",
                    json!({"type": "integer", "minimum": 1, "description": "Upper bound on nodes returned."}),
                ),
                ("timeout_ms", timeout()),
            ],
            &["tab"],
        ),
        "browser_click" => object(
            vec![
                ("tab", tab()),
                ("ref", element("Element")),
                (
                    "button",
                    one_of(&["left", "right", "middle"], "Mouse button; default left."),
                ),
                (
                    "count",
                    json!({"type": "integer", "enum": [1, 2], "description": "2 for a double click."}),
                ),
                (
                    "modifiers",
                    json!({
                        "type": "array",
                        "items": {"type": "string", "enum": ["Alt", "Control", "Meta", "Shift"]},
                        "uniqueItems": true,
                        "description": "Keys held during the click."
                    }),
                ),
                ("timeout_ms", timeout()),
            ],
            &["tab", "ref"],
        ),
        "browser_hover" => object(
            vec![
                ("tab", tab()),
                ("ref", element("Element")),
                ("timeout_ms", timeout()),
            ],
            &["tab", "ref"],
        ),
        "browser_type" => object(
            vec![
                ("tab", tab()),
                ("ref", element("Editable element")),
                (
                    "text",
                    json!({"type": "string", "description": "Text to insert."}),
                ),
                (
                    "clear",
                    json!({"type": "boolean", "description": "Replace the current content."}),
                ),
                (
                    "submit",
                    json!({"type": "boolean", "description": "Press Enter after typing."}),
                ),
                ("timeout_ms", timeout()),
            ],
            &["tab", "ref", "text"],
        ),
        "browser_press" => object(
            vec![
                ("tab", tab()),
                (
                    "key",
                    json!({"type": "string", "minLength": 1, "description": "Key or Modifier+Key, e.g. Enter, Escape, Control+A."}),
                ),
                ("ref", element("Element to focus first; the")),
                ("timeout_ms", timeout()),
            ],
            &["tab", "key"],
        ),
        "browser_select" => object(
            vec![
                ("tab", tab()),
                ("ref", element("<select>")),
                (
                    "values",
                    json!({"type": "array", "items": {"type": "string"}, "minItems": 1, "description": "Option values or visible labels."}),
                ),
                ("timeout_ms", timeout()),
            ],
            &["tab", "ref", "values"],
        ),
        "browser_scroll" => object(
            vec![
                ("tab", tab()),
                (
                    "ref",
                    element("Element to scroll into view (or to scroll over, with direction);"),
                ),
                (
                    "direction",
                    one_of(
                        &["up", "down", "left", "right"],
                        "Scroll by amount in this direction.",
                    ),
                ),
                (
                    "amount",
                    json!({"type": "integer", "minimum": 1, "description": "Pixels; default 600."}),
                ),
                ("timeout_ms", timeout()),
            ],
            &["tab"],
        ),
        "browser_upload" => object(
            vec![
                ("tab", tab()),
                ("ref", element("File input")),
                (
                    "paths",
                    json!({"type": "array", "items": {"type": "string"}, "minItems": 1, "description": "Absolute paths, or relative to the session's working directory."}),
                ),
                ("timeout_ms", timeout()),
            ],
            &["tab", "ref", "paths"],
        ),
        "browser_dialog" => object(
            vec![
                ("tab", tab()),
                (
                    "action",
                    one_of(&["accept", "dismiss"], "How to answer the waiting dialog."),
                ),
                (
                    "text",
                    json!({"type": "string", "description": "Answer for a prompt dialog."}),
                ),
                ("timeout_ms", timeout()),
            ],
            &["tab", "action"],
        ),
        "browser_wait" => object(
            vec![
                ("tab", tab()),
                (
                    "for",
                    one_of(
                        &["time", "text", "ref", "url", "load"],
                        "Condition to wait for.",
                    ),
                ),
                (
                    "value",
                    json!({"type": ["string", "integer"], "description": "time: milliseconds; text: text to find; ref: element ref; url: prefix or glob. Not used for load."}),
                ),
                ("timeout_ms", timeout()),
            ],
            &["tab", "for"],
        ),
        "browser_back" => object(
            vec![
                ("tab", tab()),
                ("direction", one_of(&["back", "forward"], "Default back.")),
                ("timeout_ms", timeout()),
            ],
            &["tab"],
        ),
        "browser_screenshot" => object(
            vec![
                ("tab", tab()),
                ("ref", element("Only this element; the")),
                (
                    "full_page",
                    json!({"type": "boolean", "description": "Capture beyond the viewport."}),
                ),
                ("format", one_of(&["png", "jpeg"], "Default png.")),
                (
                    "quality",
                    json!({"type": "integer", "minimum": 0, "maximum": 100, "description": "jpeg only."}),
                ),
                ("timeout_ms", timeout()),
            ],
            &["tab"],
        ),
        "browser_read" => object(
            vec![
                ("tab", tab()),
                ("ref", element("Only this element; the")),
                ("format", one_of(&["text", "markdown"], "Default text.")),
                (
                    "offset",
                    json!({"type": "integer", "minimum": 0, "description": "Continue a long page from this character."}),
                ),
                ("timeout_ms", timeout()),
            ],
            &["tab"],
        ),
        _ => return None,
    })
}

/// `tools/list` entry for one tool.
pub fn definition(tool: Tool) -> Value {
    json!({
        "name": tool.name,
        "title": tool.title,
        "description": section(tool.name).unwrap_or_default(),
        "inputSchema": input_schema(tool.name).unwrap_or_else(|| json!({"type": "object"})),
        "annotations": {
            "title": tool.title,
            "readOnlyHint": tool.hints.read_only,
            "destructiveHint": tool.hints.destructive,
            "idempotentHint": false,
            "openWorldHint": true
        }
    })
}

/// One protocol request derived from a tool call.
#[derive(Debug, PartialEq)]
pub struct Mapped {
    pub kind: ActionKind,
    pub input: Value,
    /// The agent's own id for a retry; `None` means "make a new one".
    pub request_id: Option<String>,
}

/// Translate tool arguments into a protocol action. Shape errors in fields
/// the tool itself consumes (`action`, `direction`, `request_id`) are reported
/// here; everything else is validated by the host against `protocol.rs`.
pub fn map_call(name: &str, arguments: Option<&Value>) -> Result<Mapped, String> {
    let mut args = match arguments {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(map)) => map.clone(),
        Some(_) => return Err("Arguments must be a JSON object.".into()),
    };
    let request_id =
        match args.remove("request_id") {
            None | Some(Value::Null) => None,
            Some(Value::String(id)) if super::client::valid_request_id(&id) => Some(id),
            Some(_) => return Err(
                "request_id must be 1-128 letters, digits or . _ : - (omit it unless retrying)."
                    .into(),
            ),
        };
    let kind = match name {
        "browser_tabs" => {
            let action = take_str(&mut args, "action")?;
            let (kind, allowed): (ActionKind, &[&str]) = match action.as_deref() {
                Some("list") => (ActionKind::TabsList, &[]),
                Some("open") => (ActionKind::TabsOpen, &["url"]),
                Some("close") => (ActionKind::TabsClose, &["tab"]),
                Some("focus") => (ActionKind::TabsFocus, &["tab", "user_visible"]),
                _ => return Err("action must be one of list, open, close, focus.".into()),
            };
            if let Some(extra) = args.keys().find(|key| !allowed.contains(&key.as_str())) {
                return Err(format!(
                    "`{extra}` is not used with action \"{}\".",
                    kind.name().trim_start_matches("tabs.")
                ));
            }
            kind
        }
        "browser_back" => match take_str(&mut args, "direction")?.as_deref() {
            None | Some("back") => ActionKind::Back,
            Some("forward") => ActionKind::Forward,
            Some(_) => return Err("direction must be back or forward.".into()),
        },
        _ => {
            let action = name
                .strip_prefix("browser_")
                .filter(|_| find(name).is_some())
                .ok_or_else(|| format!("Unknown tool `{name}`."))?;
            ActionKind::parse(action).ok_or_else(|| format!("Unknown tool `{name}`."))?
        }
    };
    Ok(Mapped {
        kind,
        input: Value::Object(args),
        request_id,
    })
}

fn take_str(args: &mut Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match args.remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(format!("{key} must be a string.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::protocol::*;
    use serde::de::DeserializeOwned;
    use std::collections::BTreeSet;

    /// The parameter record each tool's input must satisfy.
    fn check(kind: ActionKind, input: &Value) -> Result<Value, String> {
        fn round<T: DeserializeOwned + serde::Serialize>(input: &Value) -> Result<Value, String> {
            let parsed: T = serde_json::from_value(input.clone()).map_err(|e| e.to_string())?;
            Ok(serde_json::to_value(parsed).unwrap())
        }
        use ActionKind::*;
        match kind {
            Snapshot => round::<SnapshotParams>(input),
            Navigate => round::<NavigateParams>(input),
            Click => round::<ClickParams>(input),
            Hover => round::<HoverParams>(input),
            Type => round::<TypeParams>(input),
            Press => round::<PressParams>(input),
            Select => round::<SelectParams>(input),
            Scroll => round::<ScrollParams>(input),
            Upload => round::<UploadParams>(input),
            Dialog => round::<DialogParams>(input),
            Wait => round::<WaitParams>(input),
            Back | Forward => round::<HistoryParams>(input),
            TabsList => round::<TabsListParams>(input),
            TabsOpen => round::<TabsOpenParams>(input),
            TabsClose => round::<TabsCloseParams>(input),
            TabsFocus => round::<TabsFocusParams>(input),
            Screenshot => round::<ScreenshotParams>(input),
            Read => round::<ReadParams>(input),
        }
    }

    /// A value the schema accepts, for every property type the tools use.
    fn sample(schema: &Value) -> Value {
        if let Some(values) = schema["enum"].as_array() {
            return values[0].clone();
        }
        if schema["pattern"] == "^t_[0-9a-f]{8}$" {
            return json!("t_0a1b2c3d");
        }
        match &schema["type"] {
            Value::String(kind) => match kind.as_str() {
                "string" => json!("e1"),
                "integer" => json!(schema["minimum"].as_u64().unwrap_or(1).max(1)),
                "boolean" => json!(true),
                "array" => json!([sample(&schema["items"])]),
                other => panic!("no sample for {other}"),
            },
            Value::Array(_) => json!("x"),
            other => panic!("no sample for {other}"),
        }
    }

    fn properties(name: &str) -> (Map<String, Value>, BTreeSet<String>) {
        let schema = input_schema(name).unwrap();
        let properties = schema["properties"].as_object().unwrap().clone();
        let required = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        (properties, required)
    }

    #[test]
    fn sections_are_found_with_either_line_ending() {
        let crlf = INSTRUCTIONS.replace("\r\n", "\n").replace('\n', "\r\n");
        assert!(
            section_in(&crlf, "overview").is_none(),
            "raw CRLF needs normalising"
        );
        let normalised = crlf.replace("\r\n", "\n");
        for name in ["overview", "cli", "browser_click"] {
            assert_eq!(section_in(&normalised, name), section(name), "{name}");
            assert!(!section(name).unwrap().contains('\r'));
        }
    }

    #[test]
    fn every_tool_has_a_description_and_a_strict_schema() {
        assert!(section("overview").unwrap().contains("ref_stale"));
        assert!(section("cli").unwrap().contains("{exe} browser"));
        for tool in TOOLS {
            let definition = definition(tool);
            let description = definition["description"].as_str().unwrap();
            assert!(description.len() > 40, "{} has no description", tool.name);
            assert!(
                !description.contains("\n## "),
                "{} leaks a section",
                tool.name
            );
            assert_eq!(definition["inputSchema"]["additionalProperties"], false);
            assert_eq!(definition["inputSchema"]["type"], "object");
            assert!(definition["inputSchema"]["properties"]["request_id"].is_object());
        }
        assert!(input_schema("browser_evaluate").is_none());
        // No tool reaches page JavaScript or the raw protocol (decision D7).
        for tool in TOOLS {
            for banned in ["evaluate", "cdp", "script", "javascript"] {
                assert!(!tool.name.contains(banned));
            }
        }
    }

    /// Each tool's schema accepts exactly the record's fields and requires
    /// exactly the record's required fields.
    #[test]
    fn schemas_match_the_protocol_records() {
        for tool in TOOLS.iter().filter(|t| t.name != "browser_tabs") {
            let (props, required) = properties(tool.name);
            let full: Map<String, Value> = props
                .iter()
                .filter(|(key, _)| key.as_str() != "request_id")
                .map(|(key, schema)| (key.clone(), sample(schema)))
                .collect();
            let mapped = map_call(tool.name, Some(&Value::Object(full.clone()))).unwrap();
            let forwarded: BTreeSet<String> =
                mapped.input.as_object().unwrap().keys().cloned().collect();
            // Every schema property is a field of the record...
            let round =
                check(mapped.kind, &mapped.input).unwrap_or_else(|e| panic!("{}: {e}", tool.name));
            // ...and every record field is a schema property (serialisation
            // writes all fields, `null` for absent options).
            let fields: BTreeSet<String> = round.as_object().unwrap().keys().cloned().collect();
            assert_eq!(forwarded, fields, "{} fields differ", tool.name);

            for key in full.keys() {
                let mut without = full.clone();
                without.remove(key);
                let mapped = map_call(tool.name, Some(&Value::Object(without))).unwrap();
                let accepted = check(mapped.kind, &mapped.input).is_ok();
                assert_eq!(
                    accepted,
                    !required.contains(key),
                    "{}: `{key}` required-ness differs",
                    tool.name
                );
            }
        }
    }

    #[test]
    fn tabs_tool_maps_each_action_to_its_record() {
        let cases = [
            (json!({"action": "list"}), ActionKind::TabsList),
            (json!({"action": "open"}), ActionKind::TabsOpen),
            (
                json!({"action": "open", "url": "https://example.com"}),
                ActionKind::TabsOpen,
            ),
            (
                json!({"action": "close", "tab": "t_0a1b2c3d"}),
                ActionKind::TabsClose,
            ),
            (
                json!({"action": "focus", "tab": "t_0a1b2c3d", "user_visible": true}),
                ActionKind::TabsFocus,
            ),
        ];
        for (args, kind) in cases {
            let mapped = map_call("browser_tabs", Some(&args)).unwrap();
            assert_eq!(mapped.kind, kind);
            check(kind, &mapped.input).unwrap();
            assert!(mapped.input.get("action").is_none());
        }
        assert!(map_call(
            "browser_tabs",
            Some(&json!({"action": "list", "tab": "t_0a1b2c3d"}))
        )
        .unwrap_err()
        .contains("not used"));
        assert!(map_call("browser_tabs", Some(&json!({}))).is_err());
        assert!(map_call("browser_tabs", Some(&json!({"action": "evaluate"}))).is_err());
        let (props, required) = properties("browser_tabs");
        assert_eq!(required, BTreeSet::from(["action".to_string()]));
        assert_eq!(props.len(), 5);
    }

    #[test]
    fn back_tool_picks_the_direction_and_request_ids_are_lifted_out() {
        let back = map_call("browser_back", Some(&json!({"tab": "t_0a1b2c3d"}))).unwrap();
        assert_eq!(back.kind, ActionKind::Back);
        let forward = map_call(
            "browser_back",
            Some(&json!({"tab": "t_0a1b2c3d", "direction": "forward", "request_id": "r-1"})),
        )
        .unwrap();
        assert_eq!(forward.kind, ActionKind::Forward);
        assert_eq!(forward.request_id.as_deref(), Some("r-1"));
        assert_eq!(forward.input, json!({"tab": "t_0a1b2c3d"}));
        assert!(map_call("browser_back", Some(&json!({"direction": "up"}))).is_err());
        assert!(map_call("browser_click", Some(&json!({"request_id": "bad id"}))).is_err());
        assert!(map_call("browser_click", Some(&json!([1]))).is_err());
        assert!(map_call("browser_evaluate", None).is_err());
        assert_eq!(
            map_call("browser_snapshot", None).unwrap().kind,
            ActionKind::Snapshot
        );
    }

    #[test]
    fn every_action_is_reachable_through_some_tool() {
        let mut reached = BTreeSet::new();
        for tool in TOOLS {
            match tool.name {
                "browser_tabs" => {
                    for action in ["list", "open", "close", "focus"] {
                        let mapped =
                            map_call(tool.name, Some(&json!({ "action": action }))).unwrap();
                        reached.insert(mapped.kind.name());
                    }
                }
                "browser_back" => {
                    for direction in ["back", "forward"] {
                        let args = json!({ "direction": direction });
                        reached.insert(map_call(tool.name, Some(&args)).unwrap().kind.name());
                    }
                }
                name => {
                    reached.insert(map_call(name, None).unwrap().kind.name());
                }
            }
        }
        let all: BTreeSet<&str> = ActionKind::ALL.iter().map(|kind| kind.name()).collect();
        assert_eq!(reached, all);
    }
}
