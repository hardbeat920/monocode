//! Recursive file-system change signals for a checkout, so Git status reads
//! again when something on disk changed instead of on a fixed timer.
//!
//! Every relevant event collapses into one pending signal; build output,
//! dependency folders and Git's object store never wake a reader.
use notify::{RecursiveMode, Watcher};
use std::path::{Component, Path, PathBuf};

/// Folders whose churn never changes what Git status shows.
const IGNORED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".next",
    ".turbo",
    ".cache",
    "__pycache__",
    ".venv",
    "DerivedData",
];
/// Parts of a Git directory that change without changing status.
const IGNORED_GIT_DIRS: &[&str] = &["objects", "logs", "lfs", "hooks"];

pub(crate) struct Watch {
    _watcher: notify::RecommendedWatcher,
}

/// Watches each existing root recursively. The receiver holds at most one
/// pending signal however many files changed. None if the platform watcher
/// can't start, so callers fall back to polling.
pub(crate) fn watch(roots: &[PathBuf]) -> Option<(Watch, async_channel::Receiver<()>)> {
    let (tx, rx) = async_channel::bounded(1);
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let relevant = match event {
            Ok(event) => !event.kind.is_access() && event.paths.iter().any(|p| relevant(p)),
            // A dropped or overflowed event stream: read again to be safe.
            Err(_) => true,
        };
        if relevant {
            let _ = tx.try_send(());
        }
    })
    .ok()?;
    let mut watching = false;
    for root in roots {
        if root.exists() && watcher.watch(root, RecursiveMode::Recursive).is_ok() {
            watching = true;
        }
    }
    watching.then_some((Watch { _watcher: watcher }, rx))
}

fn relevant(path: &Path) -> bool {
    if path
        .file_name()
        .is_some_and(|name| name == "index.lock" || name == ".DS_Store")
    {
        return false;
    }
    let mut in_git = false;
    for component in path.components() {
        let Component::Normal(name) = component else {
            continue;
        };
        let Some(name) = name.to_str() else {
            continue;
        };
        if in_git && IGNORED_GIT_DIRS.contains(&name) {
            return false;
        }
        if IGNORED_DIRS.contains(&name) {
            return false;
        }
        // A main checkout's `.git`, or a common dir such as `repo.git`.
        in_git = name == ".git" || name.ends_with(".git");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::relevant;
    use std::path::Path;

    #[test]
    fn source_index_and_refs_wake_readers_but_build_output_does_not() {
        assert!(relevant(Path::new("/repo/src/main.rs")));
        assert!(relevant(Path::new("/repo/.git/index")));
        assert!(relevant(Path::new("/repo/.git/HEAD")));
        assert!(relevant(Path::new("/repo/.git/refs/heads/main")));
        assert!(!relevant(Path::new("/repo/.git/objects/ab/cdef")));
        assert!(!relevant(Path::new("/repo/.git/index.lock")));
        assert!(!relevant(Path::new("/repo/target/debug/app")));
        assert!(!relevant(Path::new("/repo/web/node_modules/x/index.js")));
    }

    #[test]
    fn an_edit_signals_once_and_build_output_stays_quiet() {
        use std::time::Duration;
        let root = std::env::temp_dir().join(format!("monocode-fs-watch-{}", std::process::id()));
        std::fs::create_dir_all(root.join("target")).unwrap();
        // Canonical, as FSEvents reports /private/var for /var.
        let root = root.canonicalize().unwrap();
        let (_watch, changes) = super::watch(std::slice::from_ref(&root)).unwrap();
        // FSEvents starts delivering shortly after the stream opens.
        std::thread::sleep(Duration::from_millis(300));
        while changes.try_recv().is_ok() {}

        std::fs::write(root.join("target/out.o"), "build").unwrap();
        std::thread::sleep(Duration::from_millis(800));
        assert!(
            changes.try_recv().is_err(),
            "build output must not wake readers"
        );

        for n in 0..5 {
            std::fs::write(root.join("main.rs"), format!("fn main() {{ {n} }}")).unwrap();
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while changes.is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            changes.len(),
            1,
            "a burst of edits leaves one pending signal"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
