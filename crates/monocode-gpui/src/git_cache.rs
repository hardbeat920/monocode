//! Last successful checkout metadata shared by conversations and Git pickers.
use monocode_core::{
    files::path_key,
    git::{self, Snapshot},
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Mutex, OnceLock},
};

const LIMIT: usize = 24;
#[derive(Default)]
struct Memory {
    clock: u64,
    entries: HashMap<String, (u64, Snapshot)>,
}
impl Memory {
    fn get(&mut self, cwd: &str) -> Option<Snapshot> {
        let entry = self.entries.get_mut(&path_key(cwd))?;
        self.clock += 1;
        entry.0 = self.clock;
        Some(entry.1.clone())
    }
    fn put(&mut self, cwd: &str, state: Snapshot) {
        self.clock += 1;
        self.entries.insert(path_key(cwd), (self.clock, state));
        while self.entries.len() > LIMIT {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(key, _)| key.clone())
                .unwrap();
            self.entries.remove(&oldest);
        }
    }
    fn remember(&mut self, cwd: &str, state: &Snapshot) {
        // The sidebar belongs to the project root even when the active chat
        // runs in a linked worktree. Its HEAD must remain the main checkout's.
        if let Some(main) = state.worktrees.iter().find(|tree| tree.main)
            && path_key(&main.path) != path_key(cwd)
        {
            let mut root = state.clone();
            root.current = main
                .branch
                .clone()
                .or_else(|| Some(main.head.chars().take(7).collect()));
            root.detached = main.branch.is_none();
            root.linked = false;
            self.put(&main.path, root);
        }
        self.put(cwd, state.clone());
    }
}
fn memory() -> &'static Mutex<Memory> {
    static MEMORY: OnceLock<Mutex<Memory>> = OnceLock::new();
    MEMORY.get_or_init(Default::default)
}
pub(crate) fn get(cwd: &str) -> Option<Snapshot> {
    memory().lock().ok()?.get(cwd)
}
pub(crate) fn remember(cwd: &str, state: &Snapshot) {
    if let Ok(mut memory) = memory().lock() {
        memory.remember(cwd, state);
    }
}
pub(crate) fn load(cwd: &Path) -> Result<Snapshot, String> {
    let state = git::snapshot(cwd)?;
    // Failure leaves the last successful list intact.
    remember(&cwd.to_string_lossy(), &state);
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::git::Worktree;
    #[test]
    fn linked_checkout_warms_project_lists_without_replacing_its_branch() {
        let mut memory = Memory::default();
        memory.remember(
            "/repo-feature",
            &Snapshot {
                current: Some("feature".into()),
                linked: true,
                worktrees: vec![
                    Worktree {
                        path: "/repo".into(),
                        branch: Some("main".into()),
                        main: true,
                        ..Default::default()
                    },
                    Worktree {
                        path: "/repo-feature".into(),
                        branch: Some("feature".into()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        );
        assert_eq!(
            memory.get("/repo/").unwrap().current.as_deref(),
            Some("main")
        );
        assert!(!memory.get("/repo").unwrap().linked);
        assert_eq!(
            memory.get("/repo-feature").unwrap().current.as_deref(),
            Some("feature")
        );
    }
    #[test]
    fn recent_projects_survive_with_a_bounded_cache() {
        let mut memory = Memory::default();
        for i in 0..LIMIT {
            memory.put(&format!("/repo-{i}"), Snapshot::default());
        }
        memory.get("/repo-0").unwrap();
        memory.put("/new", Snapshot::default());
        assert_eq!(memory.entries.len(), LIMIT);
        assert!(memory.get("/repo-0").is_some());
        assert!(memory.get("/repo-1").is_none());
    }
}
