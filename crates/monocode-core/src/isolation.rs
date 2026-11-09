//! APFS isolation uses the same private registry as the desktop app. All calls
//! do filesystem/Git I/O and belong on a background executor, outside DB locks.
use crate::{agent::Settings, store::Store};
pub use monocode_isolation::{publication_hook, set_publication_helper};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

// A native run and native cleanup must never use the same copy concurrently.
type Activities = HashMap<(PathBuf, String), (usize, bool)>;
static ACTIVITY: LazyLock<Mutex<Activities>> = LazyLock::new(Default::default);
static INTEGRATION: Mutex<()> = Mutex::new(());
pub struct CopyLease {
    key: (PathBuf, String),
    removing: bool,
}
impl Drop for CopyLease {
    fn drop(&mut self) {
        let Ok(mut activity) = ACTIVITY.lock() else {
            return;
        };
        let Some((runs, removing)) = activity.get_mut(&self.key) else {
            return;
        };
        if self.removing {
            *removing = false;
        } else {
            *runs = runs.saturating_sub(1);
        }
        if *runs == 0 && !*removing {
            activity.remove(&self.key);
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub path: String,
    pub project_cwd: String,
    pub session_id: String,
    pub branch: Option<String>,
    pub head: String,
    pub dirty: Option<bool>,
    pub unpushed: Option<u64>,
}
fn repository_root(cwd: &Path) -> Result<PathBuf, String> {
    let root = crate::git::git(cwd, &["rev-parse", "--show-toplevel"])?;
    Path::new(root.trim())
        .canonicalize()
        .map_err(|e| e.to_string())
}
impl Workspace {
    pub fn belongs_to(&self, cwd: &Path) -> bool {
        repository_root(cwd).is_ok_and(|root| Path::new(&self.project_cwd) == root)
    }
}

#[derive(Clone)]
pub struct Isolation {
    registry: PathBuf,
}
impl Isolation {
    pub fn at(registry: PathBuf) -> Self {
        if let Err(error) = monocode_isolation::register_publication_registry(registry.clone()) {
            eprintln!("Copy publication registry is unavailable: {error}");
        }
        Self { registry }
    }
    pub fn for_store(store: &Store) -> Self {
        Self::at(
            store
                .data_dir()
                .map(|dir| dir.join("isolation"))
                .unwrap_or_default(),
        )
    }
    fn run(&self, command: &str, args: Value) -> Result<Value, String> {
        if self.registry.as_os_str().is_empty() {
            return match command {
                "cow_list" | "cow_roots" => Ok(json!([])),
                "cow_owner" => Ok(Value::Null),
                "cow_capability" => Ok(json!({"supported": false})),
                _ => {
                    Err("Save the session database before creating copy-on-write isolation".into())
                }
            };
        }
        monocode_isolation::dispatch(&self.registry, json!({"command": command, "args": args}))
    }
    fn managed_root(&self, path: &Path) -> Result<Option<PathBuf>, String> {
        if self.registry.as_os_str().is_empty() {
            return Ok(None);
        }
        // Copy folders are siblings of their source projects, not children of
        // the registry. Keep replaced roots in discovery, then authorize them.
        let roots = monocode_isolation::registered_roots(&self.registry)?;
        let ancestors: Vec<_> = path
            .ancestors()
            .filter_map(|ancestor| {
                ancestor
                    .parent()?
                    .canonicalize()
                    .ok()
                    .zip(ancestor.file_name())
                    .map(|(parent, name)| parent.join(name))
            })
            .collect();
        #[cfg(unix)]
        let identities: Vec<_> = {
            use std::os::unix::fs::MetadataExt;
            path.ancestors()
                .filter_map(|ancestor| std::fs::metadata(ancestor).ok())
                .map(|metadata| (metadata.dev(), metadata.ino()))
                .collect()
        };
        Ok(roots
            .into_iter()
            .find(|registered| {
                let root = &registered.path;
                let normalized = root
                    .parent()
                    .and_then(|parent| parent.canonicalize().ok())
                    .zip(root.file_name())
                    .map(|(parent, name)| parent.join(name))
                    .unwrap_or_else(|| root.clone());
                #[cfg(unix)]
                if identities.contains(&registered.identity) {
                    return true;
                }
                path.starts_with(root) || ancestors.contains(&normalized)
            })
            .map(|root| root.path))
    }
    pub fn supported(&self, cwd: &Path) -> bool {
        let cwd = repository_root(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        self.run("cow_capability", json!({"cwd": cwd}))
            .is_ok_and(|value| value["supported"] == true)
    }
    pub fn list(&self, cwd: &Path) -> Result<Vec<Workspace>, String> {
        let cwd = repository_root(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        serde_json::from_value(self.run("cow_list", json!({"cwd": cwd}))?)
            .map_err(|e| e.to_string())
    }
    /// Owned metadata for pickers; does not inspect Git status in each copy.
    pub fn roots(&self, cwd: &Path) -> Result<Vec<Workspace>, String> {
        let cwd = repository_root(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        serde_json::from_value(self.run("cow_roots", json!({"cwd": cwd}))?)
            .map_err(|e| e.to_string())
    }
    pub fn owner(&self, cwd: &Path, path: &Path) -> Result<Option<Workspace>, String> {
        if !cfg!(target_os = "macos") {
            return Ok(None);
        }
        let cwd = repository_root(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        let root = self
            .managed_root(path)?
            .unwrap_or_else(|| path.to_path_buf());
        serde_json::from_value(self.run("cow_owner", json!({"cwd": cwd, "path": root}))?)
            .map_err(|e| e.to_string())
    }
    pub fn create(
        &self,
        cwd: &Path,
        owner: &str,
        settings: &mut Settings,
    ) -> Result<PathBuf, String> {
        let source = settings
            .cow_source_cwd
            .as_deref()
            .map(Path::new)
            .unwrap_or(cwd);
        let project = repository_root(cwd)?;
        let source = repository_root(source)?;
        if source != project {
            let same_copy = self
                .owner(&project, &source)?
                .is_some_and(|copy| Path::new(&copy.project_cwd) == project);
            let same_worktree = crate::git::worktrees(&project)?.iter().any(|tree| {
                !tree.missing && Path::new(&tree.path).canonicalize().ok().as_ref() == Some(&source)
            });
            if !same_copy && !same_worktree {
                return Err("Copy source is not a working copy of this project".into());
            }
        }
        let cwd = cwd.canonicalize().map_err(|e| e.to_string())?;
        let relative = cwd.strip_prefix(&project).map_err(|e| e.to_string())?;
        let value = self.run("cow_create", json!({"cwd": source, "projectCwd": project, "sessionId": owner, "base": settings.worktree_base.as_deref().unwrap_or("HEAD"), "folder": relative}))?;
        let workspace: Workspace = serde_json::from_value(value).map_err(|e| e.to_string())?;
        let execution = Path::new(&workspace.path).join(relative);
        if !execution.is_dir() {
            return Err("The selected base does not contain this project folder".into());
        }
        settings.cow_id = Some(workspace.id);
        settings.worktree_cwd = Some(execution.to_string_lossy().into_owned());
        settings.new_cow = false;
        settings.new_worktree = false;
        settings.cow_source_cwd = None;
        Ok(execution)
    }
    /// Stored ownership is authoritative, including when opening old desktop
    /// sessions whose UI ID differs from their persisted UUID.
    pub fn authorize(
        &self,
        cwd: &Path,
        owner: &str,
        settings: &mut Settings,
    ) -> Result<(), String> {
        let path = settings
            .worktree_cwd
            .as_deref()
            .map(Path::new)
            .unwrap_or(cwd);
        let root = self.managed_root(path)?;
        if settings.cow_id.is_none() && root.is_none() {
            return Ok(());
        }
        let root = root.as_deref().unwrap_or(path);
        let workspace = self.owner(root, root)?;
        match workspace {
            Some(workspace)
                if workspace.session_id == owner
                    && workspace.belongs_to(cwd)
                    && settings
                        .cow_id
                        .as_ref()
                        .is_none_or(|id| id == &workspace.id) =>
            {
                settings.cow_id = Some(workspace.id);
                self.run(
                    "cow_publication",
                    json!({"cwd":root,"cowId":settings.cow_id,"action":"initialize"}),
                )?;
                Ok(())
            }
            Some(_) => Err("This copy-on-write workspace belongs to another session".into()),
            None => Err("Copy-on-write workspace is missing or was replaced".into()),
        }
    }
    fn claim(&self, id: &str, removing: bool) -> Result<CopyLease, String> {
        let key = (
            self.registry
                .canonicalize()
                .unwrap_or_else(|_| self.registry.clone()),
            id.to_owned(),
        );
        let mut activity = ACTIVITY
            .lock()
            .map_err(|_| "Isolation activity is unavailable")?;
        let (runs, deleting) = activity.entry(key.clone()).or_default();
        if *deleting || (removing && *runs != 0) {
            return Err(
                "This copy is running or being removed; try again after it finishes".into(),
            );
        }
        if removing {
            *deleting = true;
        } else {
            *runs += 1;
        }
        Ok(CopyLease { key, removing })
    }
    pub fn lease(
        &self,
        cwd: &Path,
        owner: &str,
        settings: &Settings,
    ) -> Result<Option<CopyLease>, String> {
        let mut checked = settings.clone();
        self.authorize(cwd, owner, &mut checked)?;
        checked
            .cow_id
            .as_ref()
            .map(|id| self.claim(id, false))
            .transpose()
    }
    pub fn check_remove(&self, cwd: &Path, id: &str) -> Result<(), String> {
        let cwd = repository_root(cwd)?;
        self.run(
            "cow_check_remove",
            json!({"cwd": cwd, "cowId": id, "force": false}),
        )
        .map(|_| ())
    }
    /// Integrate only the worker's delta into its owning lead's private copy.
    /// Exclusive leases keep native providers and cleanup out of both copies.
    pub fn integrate(
        &self,
        cwd: &Path,
        worker: &str,
        settings: &Settings,
        lead: &str,
        destination: &Path,
    ) -> Result<(), String> {
        let _integration = INTEGRATION
            .lock()
            .map_err(|_| "Integration is unavailable")?;
        let mut checked = settings.clone();
        self.authorize(cwd, worker, &mut checked)?;
        let source = checked.cow_id.as_deref().ok_or("Worker copy unavailable")?;
        let target = self
            .owner(cwd, destination)?
            .ok_or("Lead copy unavailable")?;
        if target.session_id != lead || !target.belongs_to(cwd) {
            return Err("Integration destination belongs to another session".into());
        }
        let _source = self.claim(source, true)?;
        let _target = self.claim(&target.id, true)?;
        let cwd = repository_root(cwd)?;
        self.run(
            "cow_apply",
            json!({"cwd": cwd, "cowId": source, "toCwd": target.path}),
        )
        .map(|_| ())
    }
    pub fn remove(&self, cwd: &Path, id: &str) -> Result<(), String> {
        self.remove_after(cwd, id, || Ok(()))
            .and_then(|(_, removed)| removed)
    }
    /// Commit session detachment before deleting files, under the removal lease.
    /// If filesystem cleanup fails, return that durable receipt as well as the
    /// error so callers can still update the kept conversations consistently.
    pub fn remove_after<T>(
        &self,
        cwd: &Path,
        id: &str,
        detach: impl FnOnce() -> Result<T, String>,
    ) -> Result<(T, Result<(), String>), String> {
        let cwd = repository_root(cwd)?;
        let cwd = cwd.as_path();
        let copy: Option<Workspace> =
            serde_json::from_value(self.run("cow_owner", json!({"cwd": cwd, "cowId": id}))?)
                .map_err(|e| e.to_string())?;
        let copy = copy.ok_or("Copy-on-write ownership is unavailable")?;
        let _removal = self.claim(&copy.id, true)?;
        self.check_remove(cwd, id)?;
        let detached = detach()?;
        crate::provider::discard_worktree(Path::new(&copy.path));
        let removed = self
            .run(
                "cow_remove",
                json!({"cwd": cwd, "cowId": id, "force": false}),
            )
            .map(|_| ());
        Ok((detached, removed))
    }
}

impl Settings {
    /// A new conversation forks a copy rather than sharing its owner's path.
    pub fn fork_isolation(&mut self) {
        if self.cow_id.take().is_some() {
            self.cow_source_cwd = self.worktree_cwd.take();
            self.new_cow = true;
            self.new_worktree = false;
            self.worktree_base = Some("HEAD".into());
        }
    }
    pub fn select_checkout(
        &mut self,
        isolation: &Isolation,
        cwd: &Path,
        path: &str,
    ) -> Result<(), String> {
        self.clear_isolation();
        if let Some(root) = isolation.managed_root(Path::new(path))? {
            let copy = isolation
                .owner(&root, &root)?
                .ok_or("Copy-on-write ownership is unavailable")?;
            if !copy.belongs_to(cwd) {
                return Err("Copy belongs to another project".into());
            }
            self.new_cow = true;
            self.cow_source_cwd = Some(path.into());
            self.worktree_cwd = None;
            self.worktree_base = Some("HEAD".into());
        } else {
            self.worktree_cwd = (crate::files::path_key(path)
                != crate::files::path_key(&cwd.to_string_lossy()))
            .then(|| path.into());
        }
        Ok(())
    }
    pub fn pending_isolation(&self) -> bool {
        self.new_worktree || self.new_cow
    }
    pub fn clear_isolation(&mut self) {
        self.new_worktree = false;
        self.new_cow = false;
        self.cow_id = None;
        self.cow_source_cwd = None;
        self.worktree_base = None;
    }
    pub fn isolation_default(&mut self, mode: &str) {
        self.clear_isolation();
        self.new_worktree = mode == "worktree";
        self.new_cow = mode == "cow";
        if self.pending_isolation() {
            self.worktree_base = Some("HEAD".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn native_git_publishes_only_new_edits_and_discard_keeps_the_inherited_checkout() {
        use crate::{git::git, scm};
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let remote = dir.path().join("remote.git");
        std::fs::create_dir(&project).unwrap();
        git(&project, &["init", "-b", "main"]).unwrap();
        for (key, value) in [
            ("user.name", "Fixture"),
            ("user.email", "fixture@example.invalid"),
            ("commit.gpgsign", "false"),
        ] {
            git(&project, &["config", key, value]).unwrap();
        }
        std::fs::write(
            project.join("shared"),
            (0..50).map(|n| format!("line {n}\n")).collect::<String>(),
        )
        .unwrap();
        std::fs::write(project.join("inherited"), "original\n").unwrap();
        git(&project, &["add", "-A"]).unwrap();
        git(&project, &["commit", "-m", "Base"]).unwrap();
        git(
            &project,
            &[
                "clone",
                "--bare",
                project.to_str().unwrap(),
                remote.to_str().unwrap(),
            ],
        )
        .unwrap();
        git(
            &project,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        )
        .unwrap();
        std::fs::write(project.join("inherited"), "inherited private edit\n").unwrap();
        std::fs::write(project.join("old-untracked"), "inherited untracked\n").unwrap();
        let baseline = std::fs::read_to_string(project.join("shared"))
            .unwrap()
            .replace("line 3\n", "inherited line 3\n");
        std::fs::write(project.join("shared"), &baseline).unwrap();
        let store = Store::open_monocode(&dir.path().join("app/monocode.db")).unwrap();
        let isolation = Isolation::for_store(&store);
        if !isolation.supported(&project) {
            assert!(
                std::env::var_os("MONOCODE_REQUIRE_COW").is_none(),
                "APFS required"
            );
            return;
        }
        let session = store.create("codex", project.to_str().unwrap()).unwrap();
        let mut settings = Settings::default();
        settings.isolation_default("cow");
        let copy = isolation
            .create(
                &project,
                &store.persisted_session_id(session.id).unwrap(),
                &mut settings,
            )
            .unwrap();
        assert!(scm::diff_index(&copy).unwrap().files.is_empty());
        assert!(scm::staged_context(&copy).is_err());
        std::fs::write(
            copy.join("shared"),
            baseline.replace("line 30\n", "new line 30\n"),
        )
        .unwrap();
        std::fs::write(copy.join("new"), "new file\n").unwrap();
        let diff = scm::file_diff(&copy, "shared", false).unwrap();
        assert!(diff.current.contains("new line 30"));
        assert!(!diff.current.contains("inherited line 3"));
        scm::stage_contents(&copy, "shared", diff.current.as_bytes()).unwrap();
        scm::stage_file(&copy, "new").unwrap();
        let staged = scm::staged_context(&copy).unwrap();
        assert!(staged.patch.contains("new line 30") && !staged.patch.contains("inherited line 3"));
        std::fs::write(
            copy.join("shared"),
            std::fs::read_to_string(copy.join("shared"))
                .unwrap()
                .replace("line 40\n", "unstaged line 40\n"),
        )
        .unwrap();
        scm::commit(&copy, "Only new edits", false).unwrap();
        assert_eq!(
            git(&copy, &["show", "HEAD:inherited"]).unwrap().trim(),
            "original"
        );
        assert!(git(&copy, &["cat-file", "-e", "HEAD:old-untracked"]).is_err());
        let public = git(&copy, &["show", "HEAD:shared"]).unwrap();
        assert!(
            public.contains("new line 30")
                && !public.contains("inherited")
                && !public.contains("unstaged")
        );
        scm::discard_file(&copy, "shared").unwrap();
        let retained = std::fs::read_to_string(copy.join("shared")).unwrap();
        assert!(
            retained.contains("inherited line 3")
                && retained.contains("new line 30")
                && !retained.contains("unstaged")
        );
        assert!(scm::review_index(&copy, None).unwrap().files.is_empty());
        std::fs::write(copy.join("discard-new"), "discard only this new file\n").unwrap();
        scm::discard_all(&copy).unwrap();
        assert!(!copy.join("discard-new").exists());
        assert!(copy.join("old-untracked").exists());
        assert_eq!(
            std::fs::read_to_string(copy.join("inherited")).unwrap(),
            "inherited private edit\n"
        );
        std::fs::write(copy.join("new"), "amended file\n").unwrap();
        scm::stage_all(&copy).unwrap();
        scm::commit(&copy, "Only new edits amended", true).unwrap();
        scm::push(&copy).unwrap();
        let branch = git(&copy, &["branch", "--show-current"]).unwrap();
        assert_eq!(
            git(&remote, &["show", &format!("{}:inherited", branch.trim())])
                .unwrap()
                .trim(),
            "original"
        );
        let context = scm::range_context(&copy).unwrap();
        assert!(
            context.diff_patch.contains("new line 30")
                && !context.diff_patch.contains("inherited private edit")
                && !context.diff_patch.contains("inherited line 3")
        );
        assert_eq!(
            std::fs::read_to_string(project.join("inherited")).unwrap(),
            "inherited private edit\n"
        );
        isolation
            .remove(&project, settings.cow_id.as_deref().unwrap())
            .unwrap();
        assert!(!copy.exists());
    }

    #[test]
    fn sidecar_round_trip_does_not_migrate_the_desktop_schema() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("monocode.db");
        let store = Store::open_monocode(&db).unwrap();
        let session = store.create("codex", "/project").unwrap();
        let schema = || {
            rusqlite::Connection::open(&db)
                .unwrap()
                .query_row(
                    "SELECT group_concat(name, ',') FROM pragma_table_info('sessions')",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap()
        };
        let before = schema();
        let mut settings = Settings::default();
        settings.isolation_default("cow");
        settings.cow_source_cwd = Some("/donor".into());
        store.save_agent_settings(session.id, &settings).unwrap();
        drop(store);
        let store = Store::open_monocode(&db).unwrap();
        let saved = store.agent_settings(session.id).unwrap().unwrap();
        assert!(saved.new_cow);
        assert_eq!(saved.cow_source_cwd.as_deref(), Some("/donor"));
        assert_eq!(schema(), before);
    }

    #[test]
    fn desktop_owner_and_pending_copy_columns_are_read_and_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("monocode.db");
        let store = Store::open_monocode(&db).unwrap();
        let session = store.create("codex", "/project").unwrap();
        let owner = store.persisted_session_id(session.id).unwrap();
        drop(store);
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch("ALTER TABLE sessions ADD COLUMN cow_id TEXT; ALTER TABLE sessions ADD COLUMN workspace_mode TEXT; ALTER TABLE sessions ADD COLUMN worktree_base TEXT; ALTER TABLE sessions ADD COLUMN cow_source_cwd TEXT;").unwrap();
        conn.execute(
            "UPDATE sessions SET cow_id='old-copy',worktree_cwd='/copy' WHERE id=?1",
            [&owner],
        )
        .unwrap();
        let store = Store::open_monocode(&db).unwrap();
        let saved = store.agent_settings(session.id).unwrap().unwrap();
        assert_eq!(saved.cow_id.as_deref(), Some("old-copy"));
        assert_eq!(store.persisted_session_id(session.id).unwrap(), owner);
        let mut draft = Settings::default();
        draft.isolation_default("cow");
        store.save_agent_settings(session.id, &draft).unwrap();
        assert!(store.agent_settings(session.id).unwrap().unwrap().new_cow);
        let detached = store.detach_worktree("/copy", "/project").unwrap();
        assert!(
            detached.is_empty(),
            "draft no longer points at the old copy"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_copies_use_persisted_ownership_fork_privately_and_guard_cleanup() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        crate::git::git(&project, &["init", "-b", "main"]).unwrap();
        crate::git::git(
            &project,
            &["config", "user.email", "fixture@example.invalid"],
        )
        .unwrap();
        crate::git::git(&project, &["config", "user.name", "Fixture"]).unwrap();
        std::fs::write(project.join("file"), "source").unwrap();
        crate::git::git(&project, &["add", "file"]).unwrap();
        crate::git::git(&project, &["commit", "-m", "Initial"]).unwrap();
        let store = Store::open_monocode(&dir.path().join("app/monocode.db")).unwrap();
        let first = store.create("codex", project.to_str().unwrap()).unwrap();
        let owner = store.persisted_session_id(first.id).unwrap();
        let isolation = Isolation::for_store(&store);
        if !isolation.supported(&project) {
            assert!(
                std::env::var_os("MONOCODE_REQUIRE_COW").is_none(),
                "APFS required"
            );
            return;
        }
        let mut settings = Settings::default();
        settings.isolation_default("cow");
        let copy = isolation.create(&project, &owner, &mut settings).unwrap();
        store.save_agent_settings(first.id, &settings).unwrap();
        let nested = project.join("nested");
        std::fs::create_dir(&nested).unwrap();
        let mut nested_settings = Settings::default();
        nested_settings.isolation_default("cow");
        let nested_copy = isolation
            .create(&nested, "nested-owner", &mut nested_settings)
            .unwrap();
        assert_eq!(nested_copy.file_name().unwrap(), "nested");
        isolation
            .authorize(&nested, "nested-owner", &mut nested_settings)
            .unwrap();
        assert!(isolation
            .list(&nested)
            .unwrap()
            .iter()
            .any(|copy| Some(&copy.id) == nested_settings.cow_id.as_ref()));
        isolation
            .remove(&nested, nested_settings.cow_id.as_deref().unwrap())
            .unwrap();
        assert!(!nested_copy.exists());
        let unrelated = dir.path().join("unrelated");
        std::fs::create_dir(&unrelated).unwrap();
        crate::git::git(&unrelated, &["init", "-b", "main"]).unwrap();
        let mut foreign_source = Settings {
            new_cow: true,
            cow_source_cwd: Some(unrelated.to_string_lossy().into_owned()),
            ..Default::default()
        };
        assert!(isolation
            .create(&project, "foreign-source", &mut foreign_source)
            .unwrap_err()
            .contains("not a working copy"));
        let original_branch = crate::git::snapshot(&copy).unwrap().current.unwrap();
        let rename = |owner: &str, expected: &str, name: &str| {
            crate::git::naming::rename_copy(&isolation, &project, owner, &settings, expected, name)
        };
        assert!(rename("another-owner", &original_branch, "mc/unauthorized").is_err());
        assert!(rename(&owner, &original_branch, "main").is_err());
        assert_eq!(
            rename(&owner, &original_branch, "mc/descriptive-copy")
                .unwrap()
                .current
                .as_deref(),
            Some("mc/descriptive-copy")
        );
        assert!(rename(&owner, &original_branch, "mc/late-name").is_err());
        assert_eq!(
            crate::git::snapshot(&project).unwrap().current.as_deref(),
            Some("main")
        );
        assert!(
            isolation
                .authorize(&copy, "another-session", &mut Settings::default())
                .is_err(),
            "opening a copy as a project must not bypass ownership"
        );
        let active = isolation.lease(&project, &owner, &settings).unwrap();
        assert!(isolation
            .remove(&project, settings.cow_id.as_deref().unwrap())
            .is_err());
        assert!(copy.exists());
        drop(active);
        let removal = isolation
            .claim(settings.cow_id.as_deref().unwrap(), true)
            .unwrap();
        assert!(isolation.lease(&project, &owner, &settings).is_err());
        drop(removal);

        assert!(isolation
            .remove_after(
                &project,
                settings.cow_id.as_deref().unwrap(),
                || Err::<(), _>("Cannot persist detachment".into())
            )
            .is_err());
        assert!(copy.exists());
        let (receipt, removed) = isolation
            .remove_after(&project, settings.cow_id.as_deref().unwrap(), || {
                assert!(isolation.lease(&project, &owner, &settings).is_err());
                std::fs::write(copy.join("file"), "late external edit").unwrap();
                Ok("durable-detachment")
            })
            .unwrap();
        assert_eq!(receipt, "durable-detachment");
        assert!(removed.is_err());
        assert!(copy.exists());

        let alias = dir.path().join("copy-alias");
        std::os::unix::fs::symlink(&copy, &alias).unwrap();
        std::fs::create_dir(copy.join("src")).unwrap();
        for path in [&alias, &alias.join("src")] {
            assert!(
                isolation
                    .authorize(path, "another-session", &mut Settings::default())
                    .is_err(),
                "aliases must not bypass ownership"
            );
        }
        assert_ne!(owner, first.id.to_string());
        assert!(isolation
            .authorize(&project, &first.id.to_string(), &mut settings.clone())
            .is_err());
        let mut restored = store.agent_settings(first.id).unwrap().unwrap();
        isolation
            .authorize(&project, &owner, &mut restored)
            .unwrap();
        std::fs::write(copy.join("file"), "private edit").unwrap();
        assert_eq!(
            std::fs::read_to_string(project.join("file")).unwrap(),
            "source"
        );
        assert!(isolation
            .remove(&project, settings.cow_id.as_deref().unwrap())
            .is_err());
        assert!(copy.exists());
        let second = store.create("codex", project.to_str().unwrap()).unwrap();
        let mut fork = settings.clone();
        fork.fork_isolation();
        assert!(fork.worktree_cwd.is_none());
        let other = isolation
            .create(
                &project,
                &store.persisted_session_id(second.id).unwrap(),
                &mut fork,
            )
            .unwrap();
        assert_ne!(other, copy);
        assert_eq!(
            std::fs::read_to_string(other.join("file")).unwrap(),
            "private edit"
        );
        std::fs::write(other.join("file"), "second edit").unwrap();
        assert_eq!(
            std::fs::read_to_string(copy.join("file")).unwrap(),
            "private edit"
        );
        let worker = store.persisted_session_id(second.id).unwrap();
        assert!(isolation
            .integrate(&project, &worker, &fork, "wrong-lead", &copy)
            .is_err());
        let running = isolation.lease(&project, &owner, &settings).unwrap();
        assert!(isolation
            .integrate(&project, &worker, &fork, &owner, &copy)
            .is_err());
        drop(running);
        std::fs::write(copy.join("file"), "conflicting lead edit").unwrap();
        assert!(isolation
            .integrate(&project, &worker, &fork, &owner, &copy)
            .is_err());
        assert_eq!(
            std::fs::read_to_string(copy.join("file")).unwrap(),
            "conflicting lead edit"
        );
        std::fs::write(copy.join("file"), "private edit").unwrap();
        for _ in 0..2 {
            isolation
                .integrate(&project, &worker, &fork, &owner, &copy)
                .unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(copy.join("file")).unwrap(),
            "second edit"
        );
        assert_eq!(
            std::fs::read_to_string(project.join("file")).unwrap(),
            "source"
        );
        let mut selected = settings.clone();
        selected
            .select_checkout(&isolation, &project, project.to_str().unwrap())
            .unwrap();
        assert!(!selected.pending_isolation());
        assert!(selected.cow_id.is_none());
        let detached = store
            .detach_worktree(copy.to_str().unwrap(), project.to_str().unwrap())
            .unwrap();
        assert_eq!(detached.len(), 1);
        assert!(detached[0].1.worktree_removed);
        assert!(detached[0].1.cow_id.is_none());
        assert!(!detached[0].1.new_cow);
        crate::git::git(&copy, &["add", "file"]).unwrap();
        crate::git::git(&copy, &["commit", "-m", "Keep private edit"]).unwrap();
        let commit = crate::git::git(&copy, &["rev-parse", "HEAD"]).unwrap();
        isolation
            .remove(&project, settings.cow_id.as_deref().unwrap())
            .unwrap();
        assert!(!copy.exists());
        assert!(crate::git::git(&project, &["cat-file", "-e", commit.trim()]).is_ok());
    }
}
