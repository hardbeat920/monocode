//! Read catalog fields without expanding unknown YAML values into owned trees.

use crate::{Error, Result};
use serde::de::{self, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::collections::BTreeSet;
use std::fmt;

const MAX_FIELDS: usize = 256;

pub(crate) struct Frontmatter {
    pub name: String,
    pub description: String,
    pub extensions: Vec<String>,
    pub has_dependencies: bool,
}

pub(crate) fn parse(yaml: &str) -> Result<Frontmatter> {
    serde_yaml_ng::from_str(yaml)
        .map_err(|error| Error::InvalidBundle(format!("Invalid YAML frontmatter: {error}")))
}

impl<'de> Deserialize<'de> for Frontmatter {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct FieldsVisitor;
        impl<'de> Visitor<'de> for FieldsVisitor {
            type Value = Frontmatter;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a YAML frontmatter mapping")
            }

            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Self::Value, M::Error> {
                let mut seen = BTreeSet::new();
                let mut name = None;
                let mut description = None;
                let mut extensions = Vec::new();
                let mut has_dependencies = false;
                while let Some(BoundedString::<256>(key)) = map.next_key()? {
                    if seen.len() >= MAX_FIELDS {
                        return Err(de::Error::custom("Frontmatter exceeds 256 fields"));
                    }
                    if !seen.insert(key.clone()) {
                        return Err(de::Error::custom(format!(
                            "Duplicate frontmatter field '{key}'"
                        )));
                    }
                    match key.as_str() {
                        "name" => name = Some(map.next_value::<BoundedString<64>>()?.0),
                        "description" => {
                            description = Some(map.next_value::<BoundedString<1024>>()?.0)
                        }
                        "license" => {
                            map.next_value::<BoundedString<4096>>()?;
                        }
                        "compatibility" => {
                            map.next_value::<BoundedString<4096>>()?;
                            has_dependencies = true;
                        }
                        "metadata" => {
                            map.next_value::<MetadataMapping>()?;
                        }
                        "allowed-tools" => {
                            map.next_value::<IgnoredAny>()?;
                            has_dependencies = true;
                        }
                        _ => {
                            // The parser still validates the complete YAML document. Skipping
                            // values avoids allocating repeated scalar or collection aliases.
                            map.next_value::<IgnoredAny>()?;
                            extensions.push(key);
                        }
                    }
                }
                Ok(Frontmatter {
                    name: name.ok_or_else(|| de::Error::missing_field("name"))?,
                    description: description
                        .ok_or_else(|| de::Error::missing_field("description"))?,
                    extensions,
                    has_dependencies,
                })
            }
        }
        deserializer.deserialize_any(FieldsVisitor)
    }
}

struct BoundedString<const MAX: usize>(String);

impl<'de, const MAX: usize> Deserialize<'de> for BoundedString<MAX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct StringVisitor<const MAX: usize>;
        impl<const MAX: usize> Visitor<'_> for StringVisitor<MAX> {
            type Value = BoundedString<MAX>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                write!(formatter, "a string with at most {MAX} characters")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
                if value.chars().take(MAX + 1).count() > MAX {
                    return Err(E::custom(format!(
                        "Frontmatter string exceeds {MAX} characters"
                    )));
                }
                Ok(BoundedString(value.into()))
            }

            fn visit_string<E: de::Error>(
                self,
                value: String,
            ) -> std::result::Result<Self::Value, E> {
                if value.chars().take(MAX + 1).count() > MAX {
                    return Err(E::custom(format!(
                        "Frontmatter string exceeds {MAX} characters"
                    )));
                }
                Ok(BoundedString(value))
            }
        }
        // deserialize_any preserves YAML scalar types. An unquoted boolean is
        // rejected instead of coerced into a catalog string.
        deserializer.deserialize_any(StringVisitor::<MAX>)
    }
}

struct MetadataMapping;

impl<'de> Deserialize<'de> for MetadataMapping {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct MappingVisitor;
        impl<'de> Visitor<'de> for MappingVisitor {
            type Value = MetadataMapping;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a metadata mapping")
            }

            fn visit_map<M: MapAccess<'de>>(
                self,
                mut map: M,
            ) -> std::result::Result<Self::Value, M::Error> {
                while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
                Ok(MetadataMapping)
            }
        }
        deserializer.deserialize_any(MappingVisitor)
    }
}
