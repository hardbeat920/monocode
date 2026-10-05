//! The core helpers this provider calls from src/integrations/harness/core
//! (preview.ts and streamText.ts), gathered so the import paths live in one
//! place.

pub use monocode_core::reducer::preview::{
    extract_tool_preview, is_agent_tool_name, title_from_tool_input,
};
pub use monocode_core::reducer::stream_text::{
    join_stream_text, snapshot_remainder, stream_text_delta,
};

/// A string-keyed map that iterates in insertion order, like a JavaScript
/// `Map`. Setting a key that exists keeps its place. The maps this provider
/// keeps hold a handful of entries, so a vector is enough.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderedMap<V> {
    entries: Vec<(String, V)>,
}

impl<V> Default for OrderedMap<V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<V> OrderedMap<V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        self.entries
            .iter()
            .find(|(entry, _)| entry == key)
            .map(|(_, value)| value)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut V> {
        self.entries
            .iter_mut()
            .find(|(entry, _)| entry == key)
            .map(|(_, value)| value)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// `Map.set`.
    pub fn set(&mut self, key: String, value: V) {
        match self.get_mut(&key) {
            Some(slot) => *slot = value,
            None => self.entries.push((key, value)),
        }
    }

    /// `Map.delete`: true when the key was there.
    pub fn delete(&mut self, key: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|(entry, _)| entry != key);
        self.entries.len() != before
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(key, _)| key)
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, value)| value)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &V)> {
        self.entries.iter().map(|(key, value)| (key, value))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<V> FromIterator<(String, V)> for OrderedMap<V> {
    fn from_iter<I: IntoIterator<Item = (String, V)>>(iter: I) -> Self {
        let mut map = Self::new();
        for (key, value) in iter {
            map.set(key, value);
        }
        map
    }
}
