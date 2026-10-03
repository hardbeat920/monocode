//! Serializes a map-shaped value without one of its keys, such as a session
//! without its `blocks`. TypeScript wrote `const { blocks, ...rest } =
//! session`; this keeps a long transcript from being copied or serialized
//! just to be dropped.

use serde::ser::{self, Impossible, Serialize, SerializeMap, SerializeStruct, Serializer};

pub struct WithoutKey<'a, T: ?Sized> {
    pub value: &'a T,
    pub key: &'static str,
}

impl<T: Serialize + ?Sized> Serialize for WithoutKey<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value.serialize(Outer {
            inner: serializer,
            key: self.key,
        })
    }
}

struct Outer<S> {
    inner: S,
    key: &'static str,
}

fn not_a_map<E: ser::Error>() -> E {
    E::custom("WithoutKey only serializes maps and structs")
}

/// Whether a map key serializes to the string `name`.
fn is_key<T: Serialize + ?Sized>(key: &T, name: &str) -> bool {
    matches!(serde_json::to_value(key), Ok(serde_json::Value::String(text)) if text == name)
}

pub struct Map<M> {
    inner: M,
    key: &'static str,
    skipping: bool,
}

impl<M: SerializeMap> SerializeMap for Map<M> {
    type Ok = M::Ok;
    type Error = M::Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Self::Error> {
        self.skipping = is_key(key, self.key);
        if self.skipping {
            Ok(())
        } else {
            self.inner.serialize_key(key)
        }
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Self::Error> {
        if self.skipping {
            Ok(())
        } else {
            self.inner.serialize_value(value)
        }
    }

    fn serialize_entry<K: Serialize + ?Sized, V: Serialize + ?Sized>(
        &mut self,
        key: &K,
        value: &V,
    ) -> Result<(), Self::Error> {
        if is_key(key, self.key) {
            Ok(())
        } else {
            self.inner.serialize_entry(key, value)
        }
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end()
    }
}

pub struct Struct<S> {
    inner: S,
    key: &'static str,
}

impl<S: SerializeStruct> SerializeStruct for Struct<S> {
    type Ok = S::Ok;
    type Error = S::Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Self::Error> {
        if key == self.key {
            self.inner.skip_field(key)
        } else {
            self.inner.serialize_field(key, value)
        }
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        self.inner.end()
    }
}

macro_rules! refuse {
    ($($name:ident($($arg:ty),*);)*) => {
        $(fn $name(self, $(_: $arg),*) -> Result<Self::Ok, Self::Error> {
            Err(not_a_map())
        })*
    };
}

impl<S: Serializer> Serializer for Outer<S> {
    type Ok = S::Ok;
    type Error = S::Error;
    type SerializeSeq = Impossible<S::Ok, S::Error>;
    type SerializeTuple = Impossible<S::Ok, S::Error>;
    type SerializeTupleStruct = Impossible<S::Ok, S::Error>;
    type SerializeTupleVariant = Impossible<S::Ok, S::Error>;
    type SerializeMap = Map<S::SerializeMap>;
    type SerializeStruct = Struct<S::SerializeStruct>;
    type SerializeStructVariant = Impossible<S::Ok, S::Error>;

    refuse! {
        serialize_bool(bool);
        serialize_i8(i8);
        serialize_i16(i16);
        serialize_i32(i32);
        serialize_i64(i64);
        serialize_u8(u8);
        serialize_u16(u16);
        serialize_u32(u32);
        serialize_u64(u64);
        serialize_f32(f32);
        serialize_f64(f64);
        serialize_char(char);
        serialize_str(&str);
        serialize_bytes(&[u8]);
        serialize_none();
        serialize_unit();
        serialize_unit_struct(&'static str);
        serialize_unit_variant(&'static str, u32, &'static str);
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Self::Error> {
        Err(not_a_map())
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(not_a_map())
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(not_a_map())
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(not_a_map())
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(not_a_map())
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Ok(Map {
            inner: self
                .inner
                .serialize_map(len.map(|len| len.saturating_sub(1)))?,
            key: self.key,
            skipping: false,
        })
    }

    fn serialize_struct(
        self,
        name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(Struct {
            inner: self.inner.serialize_struct(name, len)?,
            key: self.key,
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(not_a_map())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(serde::Serialize)]
    struct Plain {
        id: &'static str,
        blocks: Vec<u8>,
    }

    #[derive(serde::Serialize)]
    struct Flattened {
        id: &'static str,
        blocks: Vec<u8>,
        #[serde(flatten)]
        extra: serde_json::Map<String, serde_json::Value>,
    }

    #[test]
    fn drops_one_key_from_structs_and_flattened_maps() {
        let plain = Plain {
            id: "a",
            blocks: vec![1],
        };
        assert_eq!(
            serde_json::to_value(WithoutKey {
                value: &plain,
                key: "blocks"
            })
            .unwrap(),
            json!({ "id": "a" })
        );
        let mut extra = serde_json::Map::new();
        extra.insert("busy".into(), json!(true));
        let flattened = Flattened {
            id: "b",
            blocks: vec![2],
            extra,
        };
        assert_eq!(
            serde_json::to_string(&WithoutKey {
                value: &flattened,
                key: "blocks"
            })
            .unwrap(),
            r#"{"id":"b","busy":true}"#
        );
    }
}
