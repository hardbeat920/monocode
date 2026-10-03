//! Shared skill library commands and provider-account export preparation.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;

use monocode_skills::{ExportState, ExportTarget, ReconcileReport, SkillEntry, SkillManager};
use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Debug)]
struct LibraryRoots {
    data: PathBuf,
    home: PathBuf,
    preview: bool,
}

/// Keep account discovery, exports, and profile removal in one lifecycle operation.
pub(crate) struct AccountLifecycleGuard {
    roots: LibraryRoots,
    _file: File,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    generation: u64,
    entries: Vec<Entry>,
    targets: Vec<Target>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    id: String,
    name: String,
    description: String,
    digest: String,
    revision: u64,
    shared: bool,
    source_path: String,
    preview_path: String,
    origins: Vec<String>,
    statuses: Vec<Status>,
    warnings: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    target_key: String,
    providers: Vec<String>,
    path: String,
    state: ExportState,
    detail: String,
}

#[derive(Serialize)]
struct Target {
    key: String,
    root: String,
    providers: Vec<String>,
}

fn absolute(path: &Path, label: &str) -> Result<PathBuf, String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(format!("{label} must be an absolute path without '..'"));
    }
    crate::harness::resolve_provider_home(path, path)
}

fn resolve_roots(
    data: &Path,
    home: &Path,
    preview_data: Option<&Path>,
    preview_home: Option<&Path>,
) -> Result<LibraryRoots, String> {
    if preview_data.is_some() != preview_home.is_some() {
        return Err("Shared skills preview requires both data and home directory overrides".into());
    }
    let data = absolute(data, "App data directory")?;
    if let Some(preview_data) = preview_data {
        let preview_data = absolute(preview_data, "Shared skills preview data directory")?;
        if preview_data != data {
            return Err("Shared skills preview data must match this app's data directory".into());
        }
    }
    let home = absolute(preview_home.unwrap_or(home), "Shared skills home directory")?;
    Ok(LibraryRoots {
        data,
        home,
        preview: preview_home.is_some() || preview_data.is_some(),
    })
}

fn library_roots(app: &AppHandle) -> Result<LibraryRoots, String> {
    let data = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let home = crate::dirs_home().ok_or("Cannot resolve the home directory")?;
    #[cfg(debug_assertions)]
    let (preview_data, preview_home) = (
        std::env::var_os("MONOCODE_SHARED_SKILLS_PREVIEW_DATA").map(PathBuf::from),
        std::env::var_os("MONOCODE_SHARED_SKILLS_PREVIEW_HOME").map(PathBuf::from),
    );
    #[cfg(not(debug_assertions))]
    let (preview_data, preview_home): (Option<PathBuf>, Option<PathBuf>) = (None, None);
    resolve_roots(
        &data,
        Path::new(&home),
        preview_data.as_deref(),
        preview_home.as_deref(),
    )
}

fn open_manager(roots: &LibraryRoots) -> Result<SkillManager, String> {
    SkillManager::open(&roots.data, &roots.home).map_err(|error| error.to_string())
}

fn lock_roots(roots: LibraryRoots) -> Result<AccountLifecycleGuard, String> {
    std::fs::create_dir_all(&roots.data).map_err(|error| error.to_string())?;
    let path = roots.data.join("shared-skills-accounts.lock");
    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("Shared skills account lock must be a regular file".into());
        }
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| format!("Cannot open shared skills account lock: {error}"))?;
    file.lock()
        .map_err(|error| format!("Cannot lock shared skills account lifecycle: {error}"))?;
    Ok(AccountLifecycleGuard { roots, _file: file })
}

pub(crate) fn lock_account_lifecycle(app: &AppHandle) -> Result<AccountLifecycleGuard, String> {
    lock_roots(library_roots(app)?)
}

fn target(provider: &str, root: PathBuf) -> ExportTarget {
    ExportTarget::new(
        format!("config:{provider}:{}", root.to_string_lossy()),
        root.join("skills"),
        vec![provider.into()],
    )
}

fn permitted_root(roots: &LibraryRoots, root: &Path) -> bool {
    !roots.preview || root.starts_with(&roots.data) || root.starts_with(&roots.home)
}

fn account_targets(roots: &LibraryRoots) -> Result<Vec<ExportTarget>, String> {
    let mut targets = Vec::new();
    for provider in ["claude", "codex"] {
        let directory = roots.data.join("provider-accounts").join(provider);
        let accounts = match std::fs::read_dir(&directory) {
            Ok(accounts) => accounts,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "Cannot read provider accounts {}: {error}",
                    directory.display()
                ))
            }
        };
        for account in accounts {
            let account = account.map_err(|error| error.to_string())?;
            let name = account.file_name();
            let Some(name) = name.to_str() else { continue };
            if name == "default"
                || name.is_empty()
                || name.len() > 80
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            {
                continue;
            }
            let path = crate::harness::resolve_provider_home(&account.path(), &roots.data)?;
            if path.is_dir() && permitted_root(roots, &path) {
                targets.push(target(provider, path));
            }
        }
    }
    targets.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(targets)
}

fn entry(entry: SkillEntry) -> Entry {
    let preview = entry.applied_path.join("SKILL.md");
    Entry {
        id: entry.id,
        name: entry.name,
        description: entry.description,
        digest: entry.digest,
        revision: entry.revision,
        shared: entry.shared,
        source_path: crate::fs::path_to_js(&entry.source_path),
        preview_path: crate::fs::path_to_js(&preview),
        origins: entry
            .origins
            .iter()
            .map(|path| crate::fs::path_to_js(path))
            .collect(),
        statuses: entry
            .statuses
            .into_iter()
            .map(|status| Status {
                target_key: status.target_key,
                providers: status.providers,
                path: crate::fs::path_to_js(&status.path),
                state: status.state,
                detail: status.detail,
            })
            .collect(),
        warnings: entry.warnings,
    }
}

fn snapshot(manager: &SkillManager, report: Option<ReconcileReport>) -> Result<Snapshot, String> {
    let mut snapshot = manager.snapshot().map_err(|error| error.to_string())?;
    if let Some(report) = report.filter(|report| report.generation == snapshot.generation) {
        // Reconciliation retains IO failure details that a later filesystem inspection cannot infer.
        for status in report.statuses {
            if let Some(entry) = snapshot
                .entries
                .iter_mut()
                .find(|entry| entry.id == status.skill_id)
            {
                if let Some(existing) = entry.statuses.iter_mut().find(|existing| {
                    existing.target_key == status.export.target_key
                        && existing.path == status.export.path
                }) {
                    if existing.state == ExportState::Pending
                        && status.export.state == ExportState::Pending
                    {
                        existing.detail = status.export.detail;
                    }
                } else if status.export.state == ExportState::Pending {
                    entry.statuses.push(status.export);
                }
            }
        }
    }
    Ok(Snapshot {
        generation: snapshot.generation,
        entries: snapshot.entries.into_iter().map(entry).collect(),
        targets: snapshot
            .targets
            .into_iter()
            .map(|target| Target {
                key: target.key,
                root: crate::fs::path_to_js(&target.root),
                providers: target.providers,
            })
            .collect(),
    })
}

async fn run(
    app: AppHandle,
    operation: impl FnOnce(&SkillManager, &LibraryRoots) -> Result<Option<ReconcileReport>, String>
        + Send
        + 'static,
) -> Result<Snapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lifecycle = lock_account_lifecycle(&app)?;
        let manager = open_manager(&lifecycle.roots)?;
        let report = operation(&manager, &lifecycle.roots)?;
        snapshot(&manager, report)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn shared_skills_snapshot(app: AppHandle) -> Result<Snapshot, String> {
    run(app, |_, _| Ok(None)).await
}

#[tauri::command]
pub async fn shared_skills_import(app: AppHandle, path: String) -> Result<Snapshot, String> {
    run(app, move |manager, roots| {
        let targets = account_targets(roots)?;
        manager
            .import(crate::fs::expand_home(&path))
            .map_err(|error| error.to_string())?;
        manager
            .reconcile(&targets)
            .map(Some)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn shared_skills_apply(app: AppHandle, id: String) -> Result<Snapshot, String> {
    run(app, move |manager, roots| {
        let targets = account_targets(roots)?;
        manager.apply(&id).map_err(|error| error.to_string())?;
        manager
            .reconcile(&targets)
            .map(Some)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn shared_skills_set_shared(
    app: AppHandle,
    id: String,
    shared: bool,
) -> Result<Snapshot, String> {
    run(app, move |manager, roots| {
        let targets = account_targets(roots)?;
        manager
            .set_shared(&id, shared)
            .map_err(|error| error.to_string())?;
        manager
            .reconcile(&targets)
            .map(Some)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn shared_skills_repair(app: AppHandle) -> Result<Snapshot, String> {
    run(app, |manager, roots| {
        manager
            .reconcile(&account_targets(roots)?)
            .map(Some)
            .map_err(|error| error.to_string())
    })
    .await
}

/// Called only by the blocking process supervisor, after account environment setup.
pub(crate) fn prepare_child(
    lifecycle: &AccountLifecycleGuard,
    command: &Command,
    cwd: &Path,
    provider: Option<&str>,
) -> Option<String> {
    preparation_warning(prepare_for_roots(&lifecycle.roots, command, cwd, provider))
}

fn preparation_warning(result: Result<Vec<String>, String>) -> Option<String> {
    match result {
        Ok(warnings) if warnings.is_empty() => None,
        Ok(warnings) => Some(warnings.join("; ")),
        Err(error) => Some(error),
    }
}

fn prepare_for_roots(
    roots: &LibraryRoots,
    command: &Command,
    cwd: &Path,
    provider: Option<&str>,
) -> Result<Vec<String>, String> {
    let mut targets = Vec::new();
    if let Some(provider) = provider.filter(|provider| matches!(*provider, "claude" | "codex")) {
        let key = if provider == "claude" {
            "CLAUDE_CONFIG_DIR"
        } else {
            "CODEX_HOME"
        };
        if let Some(root) = crate::harness::child_env_path(command, key, cwd)? {
            if permitted_root(roots, &root) {
                targets.push(target(provider, root));
            }
        }
    }
    let report = open_manager(roots)?
        .reconcile(&targets)
        .map_err(|error| error.to_string())?;
    Ok(report
        .statuses
        .into_iter()
        .filter(|status| status.export.state == ExportState::Pending)
        .map(|status| format!("{}: {}", status.export.path.display(), status.export.detail))
        .collect())
}

/// Retire metadata before any profile credentials or files are removed.
pub(crate) fn retire_account(lifecycle: &AccountLifecycleGuard, root: &Path) -> Result<(), String> {
    open_manager(&lifecycle.roots)?
        .retire_targets_under(root)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("monocode-shared-skills-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(std::fs::canonicalize(path).unwrap())
        }

        fn roots(&self, preview: bool) -> LibraryRoots {
            LibraryRoots {
                data: self.0.join("data"),
                home: self.0.join("home"),
                preview,
            }
        }

        fn import(&self, roots: &LibraryRoots) -> SkillManager {
            let source = self.0.join("review");
            std::fs::create_dir(&source).unwrap();
            std::fs::write(
                source.join("SKILL.md"),
                "---\nname: review\ndescription: Review files\n---\nOriginal instructions\n",
            )
            .unwrap();
            let manager = open_manager(roots).unwrap();
            manager.import(source).unwrap();
            manager
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn preview_roots_require_absolute_paths_and_matching_app_data() {
        let fixture = Fixture::new();
        let data = fixture.0.join("data");
        let home = fixture.0.join("home");
        let preview_home = fixture.0.join("preview-home");
        let roots = resolve_roots(&data, &home, Some(&data), Some(&preview_home)).unwrap();
        assert_eq!(roots.data, data);
        assert_eq!(roots.home, preview_home);
        assert!(roots.preview);
        assert!(!roots.data.exists());
        assert!(!roots.home.exists());
        assert!(resolve_roots(&data, &home, Some(&data), None).is_err());
        assert!(resolve_roots(&data, &home, None, Some(&preview_home)).is_err());
        assert!(resolve_roots(&data, &home, Some(&fixture.0.join("other")), None).is_err());
        assert!(
            resolve_roots(&data, &home, Some(&data), Some(Path::new("relative-home"))).is_err()
        );
        assert!(resolve_roots(
            &data,
            &home,
            Some(Path::new("relative-data")),
            Some(&preview_home)
        )
        .is_err());
        assert!(!home.exists());
    }

    #[test]
    fn snapshot_uses_camel_case_and_preview_keeps_the_applied_revision() {
        let fixture = Fixture::new();
        let roots = fixture.roots(false);
        let manager = fixture.import(&roots);
        let state = snapshot(&manager, None).unwrap();
        let editable = Path::new(&state.entries[0].source_path).join("SKILL.md");
        std::fs::write(editable, "Unapplied edit").unwrap();
        let json = serde_json::to_value(snapshot(&manager, None).unwrap()).unwrap();
        let entry = &json["entries"][0];
        assert!(entry.get("sourcePath").is_some());
        assert!(entry.get("previewPath").is_some());
        assert!(entry.get("source_path").is_none());
        let preview = entry["previewPath"].as_str().unwrap();
        assert!(std::fs::read_to_string(preview)
            .unwrap()
            .contains("Original instructions"));
        assert!(entry["statuses"][0].get("targetKey").is_some());
        assert_eq!(entry["statuses"][0]["state"], "exported");
        assert!(json["targets"].as_array().unwrap().iter().all(|target| {
            !target["providers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|provider| provider == "droid")
        }));
    }

    #[test]
    fn mutation_snapshot_retains_export_failure_details_without_overwriting_a_newer_generation() {
        let fixture = Fixture::new();
        let roots = fixture.roots(false);
        let manager = fixture.import(&roots);
        let blocked = roots.home.join(".claude/skills");
        std::fs::remove_dir_all(&blocked).unwrap();
        std::fs::write(&blocked, "Preserve this file").unwrap();
        let report = manager.reconcile(&[]).unwrap();
        let state = snapshot(&manager, Some(report.clone())).unwrap();
        let failed = state.entries[0]
            .statuses
            .iter()
            .find(|status| status.path == crate::fs::path_to_js(&blocked.join("review")))
            .unwrap();
        assert_eq!(failed.state, ExportState::Pending);
        assert!(failed
            .detail
            .starts_with("Cannot prepare export directory:"));
        assert_eq!(
            std::fs::read_to_string(&blocked).unwrap(),
            "Preserve this file"
        );
        let id = &state.entries[0].id;
        manager.set_shared(id, false).unwrap();
        let newer = snapshot(&manager, Some(report)).unwrap();
        assert!(newer.generation > state.generation);
        assert!(!newer.entries[0].shared);
        assert!(newer.entries[0].statuses.iter().any(|status| {
            status.providers.iter().any(|provider| provider == "codex")
                && status.state == ExportState::Disabled
        }));
    }

    #[test]
    fn report_does_not_hide_a_later_external_edit_without_a_library_generation_change() {
        let fixture = Fixture::new();
        let roots = fixture.roots(false);
        let manager = fixture.import(&roots);
        let report = manager.reconcile(&[]).unwrap();
        let exported = roots.home.join(".claude/skills/review");
        std::fs::write(exported.join("SKILL.md"), "Independent edit").unwrap();
        let state = snapshot(&manager, Some(report.clone())).unwrap();
        assert_eq!(state.generation, report.generation);
        let status = state.entries[0]
            .statuses
            .iter()
            .find(|status| status.path == crate::fs::path_to_js(&exported))
            .unwrap();
        assert_eq!(status.state, ExportState::Conflict);
        assert_eq!(
            std::fs::read_to_string(exported.join("SKILL.md")).unwrap(),
            "Independent edit"
        );
    }

    #[test]
    fn lifecycle_lock_prevents_captured_targets_from_recreating_a_deleted_account() {
        use std::sync::mpsc;
        use std::time::Duration;

        let fixture = Fixture::new();
        let roots = fixture.roots(false);
        let manager = fixture.import(&roots);
        let profile = roots.data.join("provider-accounts/codex/work");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(profile.join("auth.json"), "test profile artifact").unwrap();

        let mutation_guard = lock_roots(fixture.roots(false)).unwrap();
        let captured = account_targets(&mutation_guard.roots).unwrap();
        assert_eq!(captured.len(), 1);
        let deletion_roots = fixture.roots(false);
        let deletion_profile = profile.clone();
        let (attempt_tx, attempt_rx) = mpsc::channel();
        let (locked_tx, locked_rx) = mpsc::channel();
        let removal = std::thread::spawn(move || {
            attempt_tx.send(()).unwrap();
            let guard = lock_roots(deletion_roots).unwrap();
            locked_tx.send(()).unwrap();
            retire_account(&guard, &deletion_profile).unwrap();
            std::fs::remove_dir_all(deletion_profile).unwrap();
        });
        attempt_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            locked_rx.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        manager.reconcile(&captured).unwrap();
        assert!(profile.join("skills/review/SKILL.md").exists());
        drop(mutation_guard);
        locked_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        removal.join().unwrap();
        assert!(!profile.exists());

        let later_guard = lock_roots(fixture.roots(false)).unwrap();
        let targets = account_targets(&later_guard.roots).unwrap();
        assert!(targets.is_empty());
        manager.reconcile(&targets).unwrap();
        assert!(!profile.exists());
    }

    #[cfg(unix)]
    #[test]
    fn corrupt_registry_warns_while_the_provider_child_can_still_start() {
        let fixture = Fixture::new();
        let roots = fixture.roots(false);
        fixture.import(&roots);
        let registry = roots.data.join("skills/registry.json");
        std::fs::write(&registry, "Invalid registry").unwrap();
        let mut child = Command::new("sh");
        child
            .env_remove("CODEX_HOME")
            .args(["-c", "printf 'provider started'"]);
        let warning =
            preparation_warning(prepare_for_roots(&roots, &child, &fixture.0, Some("codex")));
        assert!(warning.is_some());
        let output = child.output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"provider started");
        assert_eq!(
            std::fs::read_to_string(registry).unwrap(),
            "Invalid registry"
        );
    }

    #[cfg(unix)]
    #[test]
    fn launch_preparation_exports_to_the_selected_profile_and_preserves_credentials_and_conflicts()
    {
        let fixture = Fixture::new();
        let roots = fixture.roots(false);
        let manager = fixture.import(&roots);
        let profile = roots.data.join("provider-accounts/codex/work");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(profile.join("auth.json"), "test profile artifact").unwrap();
        let mut child = Command::new("sh");
        child
            .env("CODEX_HOME", &profile)
            .args(["-c", "cat \"$CODEX_HOME/skills/review/SKILL.md\""]);
        prepare_for_roots(&roots, &child, &fixture.0, Some("codex")).unwrap();
        let output = child.output().unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout)
            .unwrap()
            .contains("Original instructions"));
        let exported = profile.join("skills/review/SKILL.md");
        std::fs::write(&exported, "Independent edit").unwrap();
        prepare_for_roots(&roots, &child, &fixture.0, Some("codex")).unwrap();
        assert_eq!(
            std::fs::read_to_string(&exported).unwrap(),
            "Independent edit"
        );
        assert_eq!(
            std::fs::read_to_string(profile.join("auth.json")).unwrap(),
            "test profile artifact"
        );
        assert!(manager.snapshot().unwrap().entries[0]
            .statuses
            .iter()
            .any(|status| {
                status.path == profile.join("skills/review")
                    && status.state == ExportState::Conflict
            }));
    }

    #[cfg(unix)]
    #[test]
    fn preview_rejects_profiles_and_child_roots_that_resolve_outside_its_directories() {
        let fixture = Fixture::new();
        let roots = fixture.roots(true);
        fixture.import(&roots);
        let external = fixture.0.join("external");
        std::fs::create_dir(&external).unwrap();
        let profiles = roots.data.join("provider-accounts/codex");
        std::fs::create_dir_all(&profiles).unwrap();
        std::os::unix::fs::symlink(&external, profiles.join("work")).unwrap();
        assert!(account_targets(&roots).unwrap().is_empty());
        let mut child = Command::new("codex");
        child.env("CODEX_HOME", profiles.join("work"));
        prepare_for_roots(&roots, &child, &fixture.0, Some("codex")).unwrap();
        assert!(!external.join("skills").exists());
    }
}
