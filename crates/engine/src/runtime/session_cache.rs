//! Port of src/features/sessions/data/sessionCache.ts: closed chats stay
//! here so clicking a session card can paint without reading disk.

use monocode_core::Session;
use serde_json::Value;

/// `SESSION_LOAD_CACHE_LIMIT`.
pub const SESSION_LOAD_CACHE_LIMIT: usize = 12;
/// `SESSION_LOAD_CACHE_MAX_BYTES`.
pub const SESSION_LOAD_CACHE_MAX_BYTES: usize = 32 * 1024 * 1024;

/// `estimateSessionCacheBytes`: a conservative retained-size estimate with
/// the TypeScript's weights: 2 bytes per UTF-16 unit, 8 per number, 4 per
/// boolean, 24 plus 8 per element for arrays, 32 plus 8 per field for
/// objects.
///
/// The TypeScript walked the live object graph. This walks the session's
/// JSON value, which skips unset optional fields the object graph counted
/// as `undefined` slots. The cache estimates each session once, on insert.
pub fn estimate_session_cache_bytes(session: &Session) -> usize {
    match serde_json::to_value(session) {
        Ok(value) => estimate_value(&value),
        Err(_) => 0,
    }
}

fn estimate_value(value: &Value) -> usize {
    let mut bytes = 0;
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::String(text) => bytes += text.encode_utf16().count() * 2,
            Value::Number(_) => bytes += 8,
            Value::Bool(_) => bytes += 4,
            Value::Null => {}
            Value::Array(items) => {
                bytes += 24 + items.len() * 8;
                pending.extend(items.iter());
            }
            Value::Object(fields) => {
                bytes += 32 + fields.len() * 8;
                pending.extend(fields.values());
            }
        }
    }
    bytes
}

/// The closed-session cache, oldest first.
#[derive(Debug, Default, Clone)]
pub struct SessionCache {
    entries: Vec<(Session, usize)>,
}

impl SessionCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take a cached session out. The cache owns closed sessions only, so a
    /// session that opens moves out instead of staying as a stale copy.
    pub fn take(&mut self, session_id: &str) -> Option<Session> {
        let index = self
            .entries
            .iter()
            .position(|(session, _)| session.id == session_id)?;
        Some(self.entries.remove(index).0)
    }

    pub fn remove(&mut self, session_id: &str) {
        self.entries.retain(|(session, _)| session.id != session_id);
    }

    pub fn contains(&self, session_id: &str) -> bool {
        self.entries
            .iter()
            .any(|(session, _)| session.id == session_id)
    }

    pub fn get(&self, session_id: &str) -> Option<&Session> {
        self.entries
            .iter()
            .map(|(session, _)| session)
            .find(|session| session.id == session_id)
    }

    /// Cached ids, oldest first.
    pub fn ids(&self) -> Vec<&str> {
        self.entries
            .iter()
            .map(|(session, _)| session.id.as_str())
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `rememberLoadedSession` with the default limits.
    pub fn remember(&mut self, session: Session) {
        remember_loaded_session(
            self,
            session,
            SESSION_LOAD_CACHE_LIMIT,
            SESSION_LOAD_CACHE_MAX_BYTES,
        );
    }
}

/// `rememberLoadedSession`: insert as newest, then evict the oldest until
/// both the count and the byte budget fit. A session larger than the whole
/// budget is not kept.
pub fn remember_loaded_session(
    cache: &mut SessionCache,
    session: Session,
    limit: usize,
    max_bytes: usize,
) {
    cache.remove(&session.id);
    let session_bytes = estimate_session_cache_bytes(&session);
    if limit == 0 || session_bytes > max_bytes {
        return;
    }
    cache.entries.push((session, session_bytes));
    let mut cache_bytes: usize = cache.entries.iter().map(|(_, bytes)| bytes).sum();
    while cache.entries.len() > limit || cache_bytes > max_bytes {
        if cache.entries.is_empty() {
            break;
        }
        let (_, evicted) = cache.entries.remove(0);
        cache_bytes -= evicted;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::HarnessId;
    use monocode_core::block::{Block, BlockRole};

    fn chat(id: &str) -> Session {
        Session::blank(id, HarnessId::Cursor, "cursor:auto", "/tmp/project")
    }

    #[test]
    fn keeps_the_newest_session_and_drops_the_oldest_past_the_limit() {
        let mut cache = SessionCache::new();
        remember_loaded_session(&mut cache, chat("a"), 2, SESSION_LOAD_CACHE_MAX_BYTES);
        remember_loaded_session(&mut cache, chat("b"), 2, SESSION_LOAD_CACHE_MAX_BYTES);
        remember_loaded_session(&mut cache, chat("c"), 2, SESSION_LOAD_CACHE_MAX_BYTES);
        assert_eq!(cache.ids(), vec!["b", "c"]);
    }

    #[test]
    fn treats_a_repeat_as_newest_so_it_is_not_evicted() {
        let mut cache = SessionCache::new();
        for id in ["a", "b", "a", "c"] {
            remember_loaded_session(&mut cache, chat(id), 2, SESSION_LOAD_CACHE_MAX_BYTES);
        }
        assert_eq!(cache.ids(), vec!["a", "c"]);
    }

    #[test]
    fn evicts_by_estimated_memory_as_well_as_entry_count() {
        let mut cache = SessionCache::new();
        let mut a = chat("a");
        let mut b = chat("b");
        a.blocks = vec![Block::new("a1", BlockRole::Assistant, "a".repeat(200))];
        b.blocks = vec![Block::new("b1", BlockRole::Assistant, "b".repeat(200))];
        let budget = estimate_session_cache_bytes(&a).max(estimate_session_cache_bytes(&b));
        remember_loaded_session(&mut cache, a, 12, budget);
        remember_loaded_session(&mut cache, b, 12, budget);
        assert_eq!(cache.ids(), vec!["b"]);
    }

    #[test]
    fn does_not_retain_one_session_larger_than_the_whole_budget() {
        let mut cache = SessionCache::new();
        let mut oversized = chat("large");
        oversized.blocks = vec![Block::new(
            "large-1",
            BlockRole::Assistant,
            "x".repeat(1_000),
        )];
        remember_loaded_session(&mut cache, oversized, 12, 100);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn take_moves_a_session_out() {
        let mut cache = SessionCache::new();
        cache.remember(chat("a"));
        assert!(cache.contains("a"));
        assert_eq!(cache.take("a").map(|s| s.id), Some("a".into()));
        assert!(cache.is_empty());
    }
}
