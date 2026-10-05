//! `JSON.parse` for the stored records this package reads, keeping the key
//! order JavaScript gives `Object.entries`. `serde_json::Map` sorts its keys
//! unless the `preserve_order` feature is on, so the records here are parsed
//! into a layout `JsRecord` instead. A stored record written back keeps the
//! order the TypeScript wrote.

use std::fmt;

use monocode_layout::tab_groups::JsRecord;
use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;

/// A parsed JSON document: an object in document order, an array, or any
/// other value.
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    Object(JsRecord<Value>),
    Array(Vec<Value>),
    Other(Value),
}

struct ParsedVisitor;

impl<'de> Visitor<'de> for ParsedVisitor {
    type Value = Parsed;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Parsed, A::Error> {
        let mut record = JsRecord::new();
        while let Some((key, value)) = access.next_entry::<String, Value>()? {
            // A repeated key keeps its first position and takes the last value.
            record.insert(key, value);
        }
        Ok(Parsed::Object(record))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Parsed, A::Error> {
        let mut items = Vec::new();
        while let Some(value) = access.next_element::<Value>()? {
            items.push(value);
        }
        Ok(Parsed::Array(items))
    }

    fn visit_unit<E>(self) -> Result<Parsed, E> {
        Ok(Parsed::Other(Value::Null))
    }

    fn visit_bool<E>(self, value: bool) -> Result<Parsed, E> {
        Ok(Parsed::Other(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Parsed, E> {
        Ok(Parsed::Other(Value::from(value)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Parsed, E> {
        Ok(Parsed::Other(Value::from(value)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Parsed, E> {
        Ok(Parsed::Other(Value::from(value)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Parsed, E> {
        Ok(Parsed::Other(Value::String(value.to_string())))
    }
}

impl<'de> Deserialize<'de> for Parsed {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(ParsedVisitor)
    }
}

/// `JSON.parse(raw)`. `None` when the text is not JSON.
pub fn parse(raw: &str) -> Option<Parsed> {
    serde_json::from_str(raw).ok()
}

/// A stored object: `parsed && typeof parsed === "object" &&
/// !Array.isArray(parsed)`. `None` for anything else.
pub fn parse_object(raw: &str) -> Option<JsRecord<Value>> {
    match parse(raw)? {
        Parsed::Object(record) => Some(record),
        _ => None,
    }
}

/// A stored object where an array also counts, keyed by index, the way
/// `parsed && typeof parsed === "object"` lets arrays through.
pub fn parse_object_or_array(raw: &str) -> Option<JsRecord<Value>> {
    match parse(raw)? {
        Parsed::Object(record) => Some(record),
        Parsed::Array(items) => {
            let mut record = JsRecord::new();
            for (index, value) in items.into_iter().enumerate() {
                record.insert(index.to_string(), value);
            }
            Some(record)
        }
        Parsed::Other(_) => None,
    }
}

/// `typeof value === "number" && Number.isFinite(value)`.
pub fn finite_number(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
}

/// A finite number as an integer-valued timestamp or count.
pub fn finite_i64(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    value
        .as_i64()
        .or_else(|| finite_number(Some(value)).map(|number| number as i64))
}

/// `JSON.stringify` of a record.
pub fn stringify<V: serde::Serialize>(record: &JsRecord<V>) -> String {
    serde_json::to_string(record).unwrap_or_else(|_| "{}".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_document_order_and_puts_index_keys_first() {
        let record = parse_object(r#"{"/b":1,"/a":2,"3":3,"1":4,"/b":5}"#).unwrap();
        let keys: Vec<&str> = record.iter().map(|(key, _)| key).collect();
        assert_eq!(keys, ["1", "3", "/b", "/a"]);
        assert_eq!(record.get("/b"), Some(&Value::from(5)));
        assert_eq!(stringify(&record), r#"{"1":4,"3":3,"/b":5,"/a":2}"#);
    }

    #[test]
    fn tells_objects_arrays_and_garbage_apart() {
        assert!(parse_object("[1]").is_none());
        assert!(parse_object("null").is_none());
        assert!(parse_object("{").is_none());
        let from_array = parse_object_or_array(r#"["x","y"]"#).unwrap();
        assert_eq!(from_array.get("1"), Some(&Value::from("y")));
        assert!(parse_object_or_array("7").is_none());
    }
}
