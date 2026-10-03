//! JavaScript value semantics the TypeScript host relied on when it read
//! untyped request parameters: `String(x)`, `Number(x)`,
//! `Number.isSafeInteger(x)`, and truthiness.

use serde_json::Value;

/// `Number.MAX_SAFE_INTEGER`.
pub const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// `Number(text)` for a string.
pub fn number_from_str(text: &str) -> f64 {
    let text = monocode_core::js::trim(text);
    if text.is_empty() {
        return 0.0;
    }
    let (sign, unsigned) = match text.as_bytes()[0] {
        b'-' => (-1.0, &text[1..]),
        b'+' => (1.0, &text[1..]),
        _ => (1.0, text),
    };
    if unsigned == "Infinity" {
        return sign * f64::INFINITY;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = text.strip_prefix(prefix) {
            return u64::from_str_radix(digits, radix)
                .map(|value| value as f64)
                .unwrap_or(f64::NAN);
        }
    }
    // Rust also accepts `inf` and `NaN`, which JavaScript does not.
    if !unsigned
        .bytes()
        .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-'))
    {
        return f64::NAN;
    }
    text.parse::<f64>().unwrap_or(f64::NAN)
}

/// `Number(value)`, with `None` as `undefined`.
pub fn number(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(flag)) => f64::from(u8::from(*flag)),
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => number_from_str(text),
        Some(Value::Array(items)) => match items.as_slice() {
            [] => 0.0,
            [only] => number_from_str(&string(Some(only))),
            _ => f64::NAN,
        },
        Some(Value::Object(_)) => f64::NAN,
    }
}

/// `Number.isSafeInteger` for an already numeric value.
pub fn is_safe_integer_f64(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0 && value.abs() <= MAX_SAFE_INTEGER
}

/// `Number.isSafeInteger(value)`: only JSON numbers qualify.
pub fn safe_integer(value: Option<&Value>) -> Option<i64> {
    let number = value?.as_number()?;
    if let Some(integer) = number.as_i64() {
        return (integer.unsigned_abs() as f64 <= MAX_SAFE_INTEGER).then_some(integer);
    }
    let float = number.as_f64()?;
    is_safe_integer_f64(float).then_some(float as i64)
}

/// `String(value ?? "")`.
pub fn string(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => number_string(number),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => string(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

fn number_string(number: &serde_json::Number) -> String {
    if number.is_i64() || number.is_u64() {
        return number.to_string();
    }
    let float = number.as_f64().unwrap_or(f64::NAN);
    if float.fract() == 0.0 && float.abs() < 1e21 {
        format!("{float:.0}")
    } else {
        // TODO(port): JavaScript switches to exponent notation at different
        // magnitudes than Rust. Only unusual request IDs reach this.
        float.to_string()
    }
}

/// JavaScript truthiness.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => true,
    }
}

/// `value === expected` for a number.
pub fn is_number(value: Option<&Value>, expected: f64) -> bool {
    value.and_then(Value::as_f64) == Some(expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_and_strings_follow_javascript() {
        assert_eq!(number(Some(&json!("12"))), 12.0);
        assert_eq!(number(Some(&json!(" 0x10 "))), 16.0);
        assert_eq!(number(Some(&json!(""))), 0.0);
        assert!(number(Some(&json!("inf"))).is_nan());
        assert!(number(None).is_nan());
        assert_eq!(number(Some(&Value::Null)), 0.0);
        assert_eq!(safe_integer(Some(&json!(3.0))), Some(3));
        assert_eq!(safe_integer(Some(&json!(3.5))), None);
        assert_eq!(safe_integer(Some(&json!("3"))), None);
        assert_eq!(safe_integer(Some(&json!(9_007_199_254_740_992_u64))), None);
        assert_eq!(string(Some(&json!(12))), "12");
        assert_eq!(string(Some(&json!(true))), "true");
        assert_eq!(string(Some(&json!({}))), "[object Object]");
        assert_eq!(string(Some(&json!([1, null, "a"]))), "1,,a");
        assert_eq!(string(None), "");
    }
}
