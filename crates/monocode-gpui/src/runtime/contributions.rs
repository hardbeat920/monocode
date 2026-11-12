//! Records the commits and pull requests a turn's tool calls made, off the
//! turn's thread: resolving a commit runs Git, which the stream never waits on.
use super::{Database, locked};
use monocode_core::{contributions, tool::ToolCall};
use std::path::PathBuf;
use std::sync::{
    LazyLock,
    atomic::{AtomicU64, Ordering},
};

static BOOT: LazyLock<String> = LazyLock::new(monocode_core::mono::new_id);
static EPOCH: AtomicU64 = AtomicU64::new(0);

pub(crate) fn scan_version() -> String {
    format!("{}-{}", *BOOT, EPOCH.load(Ordering::Relaxed))
}

pub(crate) fn invalidate(store: &monocode_core::store::Store, session: i64) {
    if store
        .save_setting(&format!("{}{session}", contributions::SCANNED), "0")
        .is_err()
    {
        // A failed marker write must invalidate cached scans until restart too.
        EPOCH.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn record(store: &Database, session: i64, cwd: &str, tool: &ToolCall) {
    let found = contributions::found(tool);
    if found.is_empty() {
        return;
    }
    let store = store.clone();
    let cwd = PathBuf::from(cwd);
    std::thread::spawn(move || {
        let now = monocode_core::time::now_ms();
        let resolved: Vec<_> = found
            .iter()
            .map(|f| contributions::resolve(f, session, &cwd, now))
            .collect();
        if let Ok(store) = locked(&store) {
            for (item, resolved) in resolved {
                // shortcut: unresolved commits are retried from history, upgrade when checkout provenance is stored.
                if !resolved || store.record_contribution(&item).is_err() {
                    invalidate(&store, session);
                }
            }
        }
    });
}
