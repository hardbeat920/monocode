//! Records the commits and pull requests a turn's tool calls made, off the
//! turn's thread: resolving a commit runs Git, which the stream never waits on.
use super::{Database, locked};
use monocode_core::{contributions, tool::ToolCall};
use std::path::PathBuf;

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
            .map(|f| (contributions::resolve(f, session, &cwd, now), f.amend))
            .collect();
        if let Ok(store) = locked(&store) {
            for (item, amend) in resolved {
                let _ = store.record_contribution(&item, amend);
            }
        }
    });
}
