use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::worktrees::contains_working_dir;

#[derive(Default)]
struct Lifecycle {
    reservations: Mutex<Vec<(PathBuf, bool)>>,
}

static LIFECYCLE: Lifecycle = Lifecycle {
    reservations: Mutex::new(Vec::new()),
};

/// Held from before process creation through host registration, or across the
/// entire removal. The registry mutex is only held while acquiring/releasing.
pub(crate) struct Reservation<'a> {
    lifecycle: &'a Lifecycle,
    path: PathBuf,
    removing: bool,
}

impl Lifecycle {
    fn reserve(&self, path: &Path, removing: bool) -> Result<Reservation<'_>, String> {
        let path = match path.canonicalize() {
            Ok(path) => path,
            // Git can remove a registered worktree whose folder is missing.
            Err(_) if removing => path.to_path_buf(),
            Err(error) => return Err(format!("Cannot open working directory: {error}")),
        };
        let mut reservations = self.reservations.lock().unwrap_or_else(|e| e.into_inner());
        if reservations.iter().any(|(other, exclusive)| {
            (removing || *exclusive)
                && (contains_working_dir(&path, other) || contains_working_dir(other, &path))
        }) {
            return Err(
                "This working copy is starting a process or being deleted. Try again when it finishes."
                    .into(),
            );
        }
        reservations.push((path.clone(), removing));
        Ok(Reservation {
            lifecycle: self,
            path,
            removing,
        })
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        let mut reservations = self
            .lifecycle
            .reservations
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(index) = reservations
            .iter()
            .position(|(path, removing)| *path == self.path && *removing == self.removing)
        {
            reservations.swap_remove(index);
        }
    }
}

pub(crate) fn reserve_spawn(path: &Path) -> Result<Reservation<'static>, String> {
    LIFECYCLE.reserve(path, false)
}

pub(crate) fn reserve_removal(path: &Path) -> Result<Reservation<'static>, String> {
    LIFECYCLE.reserve(path, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removal_and_in_flight_spawns_exclude_each_other_until_guards_drop() {
        let root =
            std::env::temp_dir().join(format!("monocode-lifecycle-{}", uuid::Uuid::new_v4()));
        let child = root.join("src");
        let sibling = root.with_file_name(format!(
            "{}-other",
            root.file_name().unwrap().to_string_lossy()
        ));
        std::fs::create_dir_all(&child).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        let lifecycle = Lifecycle::default();
        let first = lifecycle.reserve(&child, false).unwrap();
        let second = lifecycle.reserve(&child, false).unwrap();
        assert!(lifecycle.reserve(&root, true).is_err());
        drop(first);
        assert!(lifecycle.reserve(&root, true).is_err());
        drop(second);
        let removal = lifecycle.reserve(&root, true).unwrap();
        assert!(lifecycle.reserve(&child, false).is_err());
        assert!(lifecycle.reserve(&root.join("src/.."), false).is_err());
        assert!(lifecycle.reserve(&root, true).is_err());
        assert!(lifecycle.reserve(&sibling, false).is_ok());
        drop(removal);
        assert!(lifecycle.reserve(&child, false).is_ok());
        std::fs::remove_dir_all(&root).unwrap();
        assert!(lifecycle.reserve(&child, false).is_err());
        std::fs::remove_dir_all(&sibling).unwrap();
    }
}
