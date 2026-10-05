//! JavaScript semantics the ported code relies on that `monocode_core::js`
//! does not cover: `Math.min` and `Math.max` with `NaN`, `Number(value)` on
//! arbitrary JSON, `parseInt`, `decodeURIComponent`, and the field checks
//! the snapshot sanitizer makes.

use serde_json::{Map, Value};

/// `Math.min(a, b)`: `NaN` wins, where `f64::min` would drop it.
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.min(b)
    }
}

/// `Math.max(a, b)`: `NaN` wins, where `f64::max` would drop it.
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// `Number(value)` for a JSON value, with `None` standing for `undefined`.
pub fn number(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => monocode_core::js::parse_number(s).unwrap_or(f64::NAN),
        // `Number([])` is 0 and `Number([x])` is `Number(String(x))`.
        Some(Value::Array(items)) => match items.as_slice() {
            [] => 0.0,
            [Value::Array(_) | Value::Object(_)] => f64::NAN,
            [Value::Null] => 0.0,
            [Value::Bool(_)] => f64::NAN,
            [single] => number(Some(single)),
            _ => f64::NAN,
        },
        Some(Value::Object(_)) => f64::NAN,
    }
}

/// `Number.parseInt(value, 10)`: leading whitespace, an optional sign, then
/// as many decimal digits as there are. `None` stands for `NaN`.
pub fn parse_int(value: &str) -> Option<f64> {
    let text = value.trim_start_matches(monocode_core::js::is_space);
    let (negative, rest) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let parsed: f64 = digits.parse().ok()?;
    Some(if negative { -parsed } else { parsed })
}

/// `decodeURIComponent`. Returns `None` where JavaScript throws a URIError.
pub fn decode_uri_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            out.push(bytes[index]);
            index += 1;
            continue;
        }
        // A run of escapes must decode to whole UTF-8 characters.
        let mut run = Vec::new();
        while index < bytes.len() && bytes[index] == b'%' {
            let hex = bytes.get(index + 1..index + 3)?;
            let hex = std::str::from_utf8(hex).ok()?;
            run.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        }
        out.extend_from_slice(std::str::from_utf8(&run).ok()?.as_bytes());
    }
    String::from_utf8(out).ok()
}

/// `typeof value[key] === "string"`.
pub fn str_field<'a>(value: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// `typeof value[key] === "string" && value[key]`: a non-empty string.
pub fn nonempty_str<'a>(value: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    str_field(value, key).filter(|s| !s.is_empty())
}

/// `typeof value[key] === "string" && value[key].trim()`: the trimmed string
/// when it is not blank.
pub fn trimmed_str<'a>(value: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    str_field(value, key)
        .map(monocode_core::js::trim)
        .filter(|s| !s.is_empty())
}

/// `value[key] === true`.
pub fn is_true(value: &Map<String, Value>, key: &str) -> bool {
    value.get(key) == Some(&Value::Bool(true))
}

/// `value[key] != null`: present and not `null`.
pub fn is_present(value: &Map<String, Value>, key: &str) -> bool {
    !matches!(value.get(key), None | Some(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn min_and_max_keep_nan() {
        assert!(min(f64::NAN, 1.0).is_nan());
        assert!(max(1.0, f64::NAN).is_nan());
        assert_eq!(min(1.0, 2.0), 1.0);
        assert_eq!(max(1.0, 2.0), 2.0);
    }

    #[test]
    fn number_follows_js_conversion() {
        assert!(number(None).is_nan());
        assert_eq!(number(Some(&json!(null))), 0.0);
        assert_eq!(number(Some(&json!(true))), 1.0);
        assert_eq!(number(Some(&json!(" 12 "))), 12.0);
        assert!(number(Some(&json!("abc"))).is_nan());
        assert_eq!(number(Some(&json!([]))), 0.0);
        assert_eq!(number(Some(&json!(["7"]))), 7.0);
        assert!(number(Some(&json!({}))).is_nan());
    }

    #[test]
    fn parse_int_reads_a_decimal_prefix() {
        assert_eq!(parse_int("4"), Some(4.0));
        assert_eq!(parse_int(" 3abc"), Some(3.0));
        assert_eq!(parse_int("-2"), Some(-2.0));
        assert_eq!(parse_int(""), None);
        assert_eq!(parse_int("x1"), None);
    }

    #[test]
    fn decodes_uri_components() {
        assert_eq!(
            decode_uri_component("/a%20b/%C3%BC").as_deref(),
            Some("/a b/ü")
        );
        assert_eq!(decode_uri_component("100%").as_deref(), None);
        assert_eq!(decode_uri_component("%E9%41").as_deref(), None);
        assert_eq!(decode_uri_component("plain").as_deref(), Some("plain"));
    }
}
