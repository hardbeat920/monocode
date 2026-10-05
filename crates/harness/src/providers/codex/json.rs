//! The `asRecord` and `stringField` helpers from
//! src/integrations/harness/providers/codex/codexProtocol.ts, which every
//! Codex module uses.

use serde_json::{Map, Value};

use monocode_core::js;

/// A JSON object, the `Record<string, unknown>` of the TypeScript.
pub type Record = Map<String, Value>;

/// `asRecord`: the value when it is a JSON object.
pub fn as_record(value: Option<&Value>) -> Option<&Record> {
    match value {
        Some(Value::Object(map)) => Some(map),
        _ => None,
    }
}

/// `stringField`: a string that is not blank, untrimmed.
pub fn string_field<'a>(rec: Option<&'a Record>, key: &str) -> Option<&'a str> {
    match rec?.get(key) {
        Some(Value::String(text)) if !js::trim(text).is_empty() => Some(text),
        _ => None,
    }
}
