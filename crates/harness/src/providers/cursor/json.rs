//! Small helpers for reading Cursor's JSON the way the TypeScript did.

use std::sync::LazyLock;

use serde_json::{Map, Value};

/// `Record<string, unknown>`.
pub type Rec = Map<String, Value>;

/// The `{}` that `rec ?? {}` falls back to.
pub static EMPTY: LazyLock<Value> = LazyLock::new(|| Value::Object(Map::new()));

/// `rec[key]`, with JSON `null` read as missing, as `??` does.
pub fn nn<'a>(rec: &'a Rec, key: &str) -> Option<&'a Value> {
    rec.get(key).filter(|value| !value.is_null())
}

/// The first present value among `keys`, as a chain of `??`.
pub fn first_nn<'a>(rec: &'a Rec, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| nn(rec, key))
}

/// `asRecord`.
pub fn as_record(value: Option<&Value>) -> Option<&Rec> {
    match value {
        Some(Value::Object(rec)) => Some(rec),
        _ => None,
    }
}

/// An object value, or `None`. Keeps the `&Value` so it can be passed to the
/// reducer's `unknown`-taking helpers without a copy.
pub fn object(value: Option<&Value>) -> Option<&Value> {
    value.filter(|value| value.is_object())
}

/// The present values of a list, for helpers that took `...values: unknown[]`.
/// A missing or `null` value added nothing there, so it is dropped here.
pub fn present<'a>(values: &[Option<&'a Value>]) -> Vec<&'a Value> {
    values
        .iter()
        .filter_map(|value| value.filter(|value| !value.is_null()))
        .collect()
}

/// JavaScript truthiness of a parsed JSON value.
pub fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// Start `future` the way calling an `async` function does in JavaScript:
/// run it now up to its first await, then hand the rest to `spawner`. A
/// handler's synchronous prefix (emitting events, registering a resolver)
/// so happens before the next stdout line is read, as in the TypeScript.
pub fn run_now(
    spawner: &crate::core::task::SharedSpawner,
    future: futures::future::BoxFuture<'static, ()>,
) {
    let mut future = future;
    let waker = futures::task::noop_waker();
    let mut cx = std::task::Context::from_waker(&waker);
    if future.as_mut().poll(&mut cx).is_pending() {
        // The executor polls a new task at least once, which replaces the
        // no-op waker registered above.
        spawner.spawn(future);
    }
}
