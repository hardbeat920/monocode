//! Port of src/features/sessions/ui/TranscriptPool.tsx: the transcripts kept
//! alive after their pane closes, so going back to a session reuses its laid
//! out view instead of building every turn and markdown block again.
//!
//! React moved one mounted DOM container between pane hosts. In GPUI a pane
//! simply renders the pooled entity, so the pool only tracks which pane hosts
//! each transcript and drops the least recently shown parked ones.

use std::collections::HashMap;
use std::hash::Hash;

/// `TRANSCRIPT_POOL_LIMIT`: parked transcripts kept for a quick revisit.
pub const TRANSCRIPT_POOL_LIMIT: usize = 12;

/// `TranscriptPoolEntry`.
#[derive(Debug, Clone)]
pub struct PoolEntry<T, H> {
    pub id: String,
    pub value: T,
    /// The pane showing it, `None` while parked.
    pub host: Option<H>,
    /// When a pane last showed it, so the oldest parked ones go first.
    pub shown_at: u64,
}

impl<T, H> PoolEntry<T, H> {
    /// A parked transcript is hidden (`visible: false, parked: true`).
    pub fn is_parked(&self) -> bool {
        self.host.is_none()
    }
}

/// `TranscriptPool`. `T` is the pooled view (an `Entity<TranscriptView>` in
/// the app), `H` identifies a pane.
#[derive(Debug)]
pub struct TranscriptPool<T, H> {
    entries: Vec<PoolEntry<T, H>>,
    index: HashMap<String, usize>,
    clock: u64,
    limit: usize,
}

impl<T, H: Clone + Eq + Hash> Default for TranscriptPool<T, H> {
    fn default() -> Self {
        Self::new(TRANSCRIPT_POOL_LIMIT)
    }
}

impl<T, H: Clone + Eq + Hash> TranscriptPool<T, H> {
    pub fn new(limit: usize) -> Self {
        Self {
            entries: Vec::new(),
            index: HashMap::new(),
            clock: 0,
            limit,
        }
    }

    /// `show`: put session `id` in `host`, building its view with `build` the
    /// first time. The entry keeps its place, so panes showing transcripts in
    /// a new order do not reorder (and rebuild) them.
    pub fn show(&mut self, id: &str, host: H, build: impl FnOnce() -> T) -> &T {
        self.clock += 1;
        let at = match self.index.get(id) {
            Some(&at) => at,
            None => {
                self.entries.push(PoolEntry {
                    id: id.to_string(),
                    value: build(),
                    host: None,
                    shown_at: 0,
                });
                let at = self.entries.len() - 1;
                self.index.insert(id.to_string(), at);
                at
            }
        };
        let entry = &mut self.entries[at];
        entry.host = Some(host);
        entry.shown_at = self.clock;
        &entry.value
    }

    /// `park`: `host` no longer shows `id`. Ignored when another pane has
    /// taken the session since. Returns the transcripts dropped past the limit.
    pub fn park(&mut self, id: &str, host: &H) -> Vec<PoolEntry<T, H>> {
        let Some(&at) = self.index.get(id) else {
            return Vec::new();
        };
        if self.entries[at].host.as_ref() != Some(host) {
            return Vec::new();
        }
        self.entries[at].host = None;
        self.trim()
    }

    fn trim(&mut self) -> Vec<PoolEntry<T, H>> {
        let mut parked: Vec<(u64, String)> = self
            .entries
            .iter()
            .filter(|entry| entry.host.is_none())
            .map(|entry| (entry.shown_at, entry.id.clone()))
            .collect();
        if parked.len() <= self.limit {
            return Vec::new();
        }
        parked.sort();
        let drop: Vec<String> = parked[..parked.len() - self.limit]
            .iter()
            .map(|(_, id)| id.clone())
            .collect();
        let mut dropped = Vec::new();
        let mut kept = Vec::with_capacity(self.entries.len());
        for entry in self.entries.drain(..) {
            if drop.contains(&entry.id) {
                dropped.push(entry);
            } else {
                kept.push(entry);
            }
        }
        self.entries = kept;
        self.index = self
            .entries
            .iter()
            .enumerate()
            .map(|(at, entry)| (entry.id.clone(), at))
            .collect();
        dropped
    }

    pub fn get(&self, id: &str) -> Option<&PoolEntry<T, H>> {
        self.index.get(id).map(|&at| &self.entries[at])
    }

    /// `getSnapshot`: every pooled transcript, in the order first shown.
    pub fn entries(&self) -> &[PoolEntry<T, H>] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// Builds numbered instances, so a test can see whether a view was reused.
    struct Builder {
        built: Cell<usize>,
    }

    impl Builder {
        fn new() -> Self {
            Self {
                built: Cell::new(0),
            }
        }

        fn build(&self) -> usize {
            self.built.set(self.built.get() + 1);
            self.built.get()
        }
    }

    #[test]
    fn cycles_through_ten_chats_without_rebuilding_any_transcript() {
        let mut pool: TranscriptPool<usize, &str> = TranscriptPool::default();
        let builder = Builder::new();
        let ids: Vec<String> = (0..10).map(|i| format!("chat-{i}")).collect();
        let mut instances = HashMap::new();
        let mut previous: Option<String> = None;
        for round in 0..2 {
            for id in &ids {
                if let Some(previous) = previous.take() {
                    pool.park(&previous, &"pane");
                }
                let value = *pool.show(id, "pane", || builder.build());
                if round == 0 {
                    instances.insert(id.clone(), value);
                } else {
                    assert_eq!(instances[id], value);
                }
                previous = Some(id.clone());
            }
        }
        assert_eq!(builder.built.get(), 10);
    }

    #[test]
    fn reuses_the_view_when_a_session_is_shown_again() {
        let mut pool: TranscriptPool<usize, &str> = TranscriptPool::default();
        let builder = Builder::new();
        let a = *pool.show("a", "pane", || builder.build());
        pool.park("a", &"pane");
        pool.show("b", "pane", || builder.build());
        pool.park("b", &"pane");
        assert_eq!(*pool.show("a", "pane", || builder.build()), a);
        assert_eq!(builder.built.get(), 2);
    }

    #[test]
    fn marks_a_parked_transcript_hidden_until_a_pane_shows_it_again() {
        let mut pool: TranscriptPool<usize, &str> = TranscriptPool::default();
        pool.show("a", "pane", || 1);
        pool.park("a", &"pane");
        assert!(pool.entries()[0].is_parked());
        pool.show("a", "pane", || 2);
        assert!(!pool.entries()[0].is_parked());
    }

    #[test]
    fn unmounts_the_least_recently_shown_transcripts_beyond_the_limit() {
        let mut pool: TranscriptPool<&str, &str> = TranscriptPool::new(2);
        let mut dropped = Vec::new();
        let mut previous: Option<&str> = None;
        for id in ["a", "b", "c", "d"] {
            if let Some(previous) = previous {
                dropped.extend(
                    pool.park(previous, &"pane")
                        .into_iter()
                        .map(|entry| entry.id),
                );
            }
            pool.show(id, "pane", || id);
            previous = Some(id);
        }
        dropped.extend(pool.park("d", &"pane").into_iter().map(|entry| entry.id));
        assert_eq!(dropped, ["a", "b"]);
        assert_eq!(
            pool.entries()
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["c", "d"]
        );
    }

    #[test]
    fn keeps_transcripts_in_place_when_panes_show_them_in_a_new_order() {
        let mut pool: TranscriptPool<&str, &str> = TranscriptPool::default();
        pool.show("a", "left", || "a");
        pool.show("b", "right", || "b");
        pool.show("a", "left", || "a2");
        pool.show("b", "right", || "b2");
        pool.show("a", "left", || "a3");
        assert_eq!(
            pool.entries()
                .iter()
                .map(|entry| entry.value)
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
    }

    #[test]
    fn ignores_a_park_from_a_pane_that_no_longer_hosts_the_session() {
        let mut pool: TranscriptPool<&str, &str> = TranscriptPool::default();
        pool.show("a", "stale", || "a");
        pool.show("a", "host", || "a");
        pool.park("a", &"stale");
        assert_eq!(pool.entries()[0].host, Some("host"));
    }
}
