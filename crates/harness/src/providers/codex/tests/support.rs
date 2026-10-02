//! Test helpers that mirror the Vitest matchers the TypeScript tests used.

use serde::Serialize;
use serde_json::Value;

/// JSON for any serializable value.
pub fn js<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).expect("serializable")
}

/// `expect(actual).toMatchObject(expected)`: every key in `expected` is in
/// `actual` with a matching value. Arrays match element by element.
pub fn matches_object(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => expected.iter().all(|(key, value)| {
            actual
                .get(key)
                .is_some_and(|found| matches_object(found, value))
        }),
        (Value::Array(actual), Value::Array(expected)) => {
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected)
                    .all(|(found, value)| matches_object(found, value))
        }
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        _ => actual == expected,
    }
}

#[track_caller]
pub fn assert_match(actual: &Value, expected: &Value) {
    assert!(
        matches_object(actual, expected),
        "expected\n{}\nto match\n{}",
        serde_json::to_string_pretty(actual).unwrap(),
        serde_json::to_string_pretty(expected).unwrap()
    );
}

/// `expect(value).not.toHaveProperty(key)`.
#[track_caller]
pub fn assert_no_key(value: &Value, key: &str) {
    assert!(
        value.get(key).is_none(),
        "expected no {key} in {}",
        serde_json::to_string(value).unwrap()
    );
}
