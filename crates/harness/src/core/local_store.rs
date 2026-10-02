//! The `localStorage` seam for the ported feature models. The engine passes
//! an implementation over `monocode_settings::Kv`, with the same keys and
//! the same JSON values, so this crate stays free of the settings store.

use std::collections::HashMap;

use parking_lot::Mutex;

/// `localStorage.getItem` and `setItem`.
pub trait LocalStore: Send + Sync {
    fn get_item(&self, key: &str) -> Option<String>;
    /// Fails when the store cannot write, as a full `localStorage` threw.
    fn set_item(&self, key: &str, value: &str) -> Result<(), String>;
}

/// An in-memory [`LocalStore`], for tests and tools.
#[derive(Debug, Default)]
pub struct MemoryStore {
    items: Mutex<HashMap<String, String>>,
    /// When set, every write fails with this message.
    pub fail_writes: Option<String>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// A store whose writes all fail with `error`.
    pub fn failing(error: &str) -> Self {
        Self {
            items: Mutex::default(),
            fail_writes: Some(error.to_string()),
        }
    }

    pub fn clear(&self) {
        self.items.lock().clear();
    }
}

impl LocalStore for MemoryStore {
    fn get_item(&self, key: &str) -> Option<String> {
        self.items.lock().get(key).cloned()
    }

    fn set_item(&self, key: &str, value: &str) -> Result<(), String> {
        if let Some(error) = &self.fail_writes {
            return Err(error.clone());
        }
        self.items.lock().insert(key.to_string(), value.to_string());
        Ok(())
    }
}
