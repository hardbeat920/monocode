//! The same session cards and model IDs as mobileWorkspace.ts, independent of
//! whether a conversation has been materialized on the native desktop.
use crate::pickers;
use monocode_core::{
    provider::Kind,
    store::{MobileSessionMetadata, Session},
};
use serde_json::{Value, json};
use std::collections::HashMap;

/// Read each checkout once per publication. Mobile republishes every few
/// seconds for every session's checkout, so this reads HEAD from disk rather
/// than starting Git; the last cached snapshot covers anything it can't read.
/// This runs after releasing the database lock, never on the UI thread.
pub(crate) fn checkout_branch(cwd: &str, branches: &mut HashMap<String, String>) -> String {
    let key = monocode_core::files::path_key(cwd);
    if let Some(branch) = branches.get(&key) {
        return branch.clone();
    }
    let branch = head_branch(std::path::Path::new(cwd))
        .or_else(|| crate::git_cache::get(cwd).and_then(|state| state.current))
        .unwrap_or_default();
    branches.insert(key, branch.clone());
    branch
}

/// The branch HEAD names, or a detached HEAD's short hash, found from any
/// folder inside the checkout. A linked worktree's `.git` file points at its
/// own Git directory.
fn head_branch(cwd: &std::path::Path) -> Option<String> {
    let (checkout, dot_git) = cwd
        .ancestors()
        .map(|dir| (dir, dir.join(".git")))
        .find(|(_, dot_git)| dot_git.exists())?;
    let git_dir = if dot_git.is_file() {
        let text = std::fs::read_to_string(&dot_git).ok()?;
        checkout.join(text.strip_prefix("gitdir:")?.trim())
    } else {
        dot_git
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    Some(match head.strip_prefix("ref: ") {
        Some(reference) => reference
            .strip_prefix("refs/heads/")
            .unwrap_or(reference)
            .to_owned(),
        None => head.chars().take(7).collect(),
    })
}

pub(crate) struct LiveSession {
    pub loaded: bool,
    pub metadata: MobileSessionMetadata,
    pub busy: bool,
    pub waiting: Option<&'static str>,
}

/// The phone keeps model choices across desktop versions. Keep Tauri's
/// provider-qualified keys on the wire and native CLI IDs inside the desktop.
pub(crate) fn model_id(kind: Kind, model: Option<&str>) -> String {
    let native = model.and_then(|id| monocode_core::store::native_model(id, kind.id()));
    let id = match (kind, native.as_deref()) {
        (Kind::Claude, Some("claude-opus-4-6")) => "opus-4.6",
        (Kind::Claude, Some("claude-sonnet-4-6")) => "sonnet-4.6",
        (Kind::Claude, Some("claude-opus-4-5")) => "opus-4.5",
        (Kind::Claude, Some("claude-haiku-4-5")) => "haiku-4.5",
        (Kind::Claude, Some(id)) => id.strip_prefix("claude-").unwrap_or(id),
        (_, Some(id)) => id,
        (_, None) => "default",
    };
    format!("{}:{id}", kind.id())
}

/// An implicit provider default selects its actual row in the phone's sheet,
/// just as Tauri's resolveModel does. Explicit IDs and moving aliases survive.
pub(crate) fn current_model_id(
    kind: Kind,
    model: Option<&str>,
    models: &[monocode_core::catalog::Model],
) -> String {
    let native = model.and_then(|id| monocode_core::store::native_model(id, kind.id()));
    model_id(
        kind,
        native.as_deref().or_else(|| {
            models
                .iter()
                .find(|model| model.default)
                .or_else(|| models.first())
                .map(|model| model.id.as_str())
        }),
    )
}

pub(crate) fn metadata(
    saved: &MobileSessionMetadata,
    live: Option<&LiveSession>,
) -> MobileSessionMetadata {
    let mut value = live
        .filter(|live| live.loaded)
        .map_or_else(|| saved.clone(), |live| live.metadata.clone());
    if let Some(branch) = live
        .and_then(|live| live.metadata.branch.as_ref())
        .filter(|branch| !branch.is_empty())
    {
        value.branch = Some(branch.clone());
    }
    if value.worktree_removed {
        value.branch = None;
        value.worktree_cwd = None;
    }
    value
}

pub(crate) fn session(
    summary: &Session,
    id: &str,
    project: &str,
    metadata: &MobileSessionMetadata,
    live: Option<&LiveSession>,
    fallback_branch: &str,
) -> Value {
    let kind = Kind::from_id(&summary.provider);
    let mut value = json!({
        "id": id,
        "projectId": project,
        "title": monocode_core::session_title::display_title(&summary.title, &summary.provider),
        "harness": summary.provider,
        "model": model_id(kind, metadata.model.as_deref()),
        "modelName": pickers::cached_model_name(kind, metadata.model.as_deref()).to_string(),
        "branch": if metadata.worktree_removed { "" } else { metadata.branch.as_deref().filter(|branch| !branch.is_empty()).unwrap_or(fallback_branch) },
        "runtimeMode": metadata.access.id(),
        "updatedAt": summary.updated_at,
        "busy": live.is_some_and(|live| live.busy),
        "live": live.is_some(),
    });
    if !metadata.worktree_removed
        && let Some(path) = &metadata.worktree_cwd
    {
        value["worktreePath"] = json!(path);
    }
    if let Some(waiting) = live.and_then(|live| live.waiting) {
        value["waiting"] = json!(waiting);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::{agent::AccessMode, store::Store};
    use std::path::Path;

    #[test]
    fn mobile_model_ids_match_tauri_and_accept_both_desktop_id_formats() {
        for (kind, wire, native) in [
            (Kind::Claude, "claude:opus", "opus"),
            (Kind::Claude, "claude:opus-5-5", "claude-opus-5-5"),
            (Kind::Claude, "claude:opus-4.6", "claude-opus-4-6"),
            (Kind::Codex, "codex:gpt-6.1-sol", "gpt-6.1-sol"),
            (
                Kind::OpenCode,
                "opencode:anthropic/claude-opus",
                "anthropic/claude-opus",
            ),
        ] {
            assert_eq!(model_id(kind, Some(native)), wire);
            assert_eq!(model_id(kind, Some(wire)), wire);
            assert_eq!(
                monocode_core::store::native_model(wire, kind.id()).as_deref(),
                Some(native)
            );
        }
        assert_eq!(model_id(Kind::Claude, None), "claude:default");
        assert_eq!(
            monocode_core::store::native_model("claude:default", "claude"),
            None
        );
    }

    #[test]
    fn saved_and_live_mobile_cards_keep_the_same_titles_models_branches_and_worktrees() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let mut summary = store.create("claude", "/project").unwrap();
        summary.title = "claude · Fix mobile parity".into();
        let saved = MobileSessionMetadata {
            model: Some("claude-opus-5-5".into()),
            branch: Some("feature/mobile".into()),
            worktree_cwd: Some("/project-worktrees/mobile".into()),
            access: AccessMode::Auto,
            ..Default::default()
        };
        let cold = session(
            &summary,
            "session",
            "project",
            &metadata(&saved, None),
            None,
            "main",
        );
        assert_eq!(cold["title"], "Fix mobile parity");
        assert_eq!(cold["model"], "claude:opus-5-5");
        assert_eq!(cold["modelName"], "Claude Opus 5.5");
        assert_eq!(cold["branch"], "feature/mobile");
        assert_eq!(cold["worktreePath"], "/project-worktrees/mobile");
        assert_eq!(cold["runtimeMode"], "auto");
        let mut live = LiveSession {
            loaded: false,
            metadata: Default::default(),
            busy: false,
            waiting: None,
        };
        let mut opening = session(
            &summary,
            "session",
            "project",
            &metadata(&saved, Some(&live)),
            Some(&live),
            "main",
        );
        opening["live"] = json!(false);
        assert_eq!(
            opening, cold,
            "an opening or evicted view must not replace saved fields with defaults"
        );
        live.loaded = true;
        live.metadata = saved.clone();
        let mut loaded = session(
            &summary,
            "session",
            "project",
            &metadata(&saved, Some(&live)),
            Some(&live),
            "main",
        );
        loaded["live"] = json!(false);
        assert_eq!(loaded, cold);
        live.metadata.model = Some("sonnet".into());
        live.metadata.branch = Some("new-branch".into());
        live.metadata.worktree_cwd = Some("/new-copy".into());
        live.metadata.access = AccessMode::FullAccess;
        live.busy = true;
        live.waiting = Some("approval");
        let changed = session(
            &summary,
            "session",
            "project",
            &metadata(&saved, Some(&live)),
            Some(&live),
            "main",
        );
        assert_eq!(changed["model"], "claude:sonnet");
        assert_eq!(changed["branch"], "new-branch");
        assert_eq!(changed["worktreePath"], "/new-copy");
        assert_eq!(changed["runtimeMode"], "full-access");
        assert_eq!(changed["waiting"], "approval");
        assert_eq!(changed["busy"], true);
    }

    #[test]
    fn removed_worktrees_never_leak_their_saved_path_or_fall_back_to_the_checkout_branch() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let summary = store.create("codex", "/project").unwrap();
        let saved = MobileSessionMetadata {
            branch: Some("deleted".into()),
            worktree_cwd: Some("/deleted".into()),
            worktree_removed: true,
            ..Default::default()
        };
        let value = session(
            &summary,
            "id",
            "project",
            &metadata(&saved, None),
            None,
            "main",
        );
        assert_eq!(value["branch"], "");
        assert!(value.get("worktreePath").is_none());
        let alias = MobileSessionMetadata {
            model: Some("opus".into()),
            ..Default::default()
        };
        let mut summary = summary;
        summary.provider = "claude".into();
        let value = session(&summary, "id", "project", &alias, None, "main");
        assert_eq!(
            value["modelName"], "Opus",
            "a moving alias must not invent a concrete version"
        );
        assert_eq!(value["branch"], "main");
    }
}
