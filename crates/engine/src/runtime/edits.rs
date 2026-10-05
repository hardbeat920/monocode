//! Port of `trackSessionEdits` and `nudgeOpenEditors` from App.tsx: what an
//! agent's file edit does to checkpoints, open editors, and the git views.
//! The submit pipeline calls both for every tool event of a turn.

use std::time::Duration;

use gpui::App;
use monocode_core::HarnessEvent;
use monocode_core::block::{ToolPreview, ToolPreviewKind};

use super::checkpoint::notify_review_changed;
use super::engine::Engine;
use super::reducer::is_edit_tool;

/// How long after a completed edit the open editors are nudged again.
const RENUDGE_DELAY: Duration = Duration::from_millis(150);

struct ToolEvent<'a> {
    updated: bool,
    kind: Option<&'a str>,
    title: Option<&'a str>,
    status: Option<&'a str>,
    preview: Option<&'a ToolPreview>,
    paths: Option<&'a [String]>,
}

fn tool_event(event: &HarnessEvent) -> Option<ToolEvent<'_>> {
    match event {
        HarnessEvent::ToolStarted {
            kind,
            title,
            status,
            preview,
            paths,
            ..
        } => Some(ToolEvent {
            updated: false,
            kind: kind.as_deref(),
            title: Some(title.as_str()),
            status: status.as_deref(),
            preview: preview.as_ref(),
            paths: paths.as_deref(),
        }),
        HarnessEvent::ToolUpdated {
            kind,
            title,
            status,
            preview,
            paths,
            ..
        } => Some(ToolEvent {
            updated: true,
            kind: kind.as_deref(),
            title: title.as_deref(),
            status: status.as_deref(),
            preview: preview.as_ref(),
            paths: paths.as_deref(),
        }),
        _ => None,
    }
}

/// The event's paths plus the preview's path, without repeats.
fn edit_paths(event: &ToolEvent<'_>) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    let preview_path = event.preview.and_then(|preview| preview.path.as_ref());
    for path in event.paths.unwrap_or_default().iter().chain(preview_path) {
        if !paths.contains(path) {
            paths.push(path.clone());
        }
    }
    paths
}

fn completed(status: Option<&str>) -> bool {
    matches!(status, Some("completed" | "success"))
}

/// `trackSessionEdits`: snapshot a file before an agent edit starts, and
/// capture it once the edit completes.
pub fn track_session_edits(session_id: &str, cwd: &str, event: &HarnessEvent, cx: &mut App) {
    let Some(tool) = tool_event(event) else {
        return;
    };
    if !is_edit_tool(tool.kind, tool.title, tool.preview) {
        return;
    }
    let paths = edit_paths(&tool);
    if paths.is_empty() || cwd == "~" {
        return;
    }
    let Some(checkpoints) = Engine::try_global(cx).map(|engine| engine.checkpoints.clone()) else {
        return;
    };
    if !(tool.updated && completed(tool.status)) {
        checkpoints.prepare(session_id, cwd, paths).detach();
        return;
    }
    let capture = checkpoints.capture(session_id, cwd, paths);
    let session_id = session_id.to_string();
    cx.spawn(async move |cx| {
        let _ = capture.await;
        cx.update(|cx| notify_review_changed(Some(&session_id), cx));
    })
    .detach();
}

/// `nudgeOpenEditors`: reload open files an agent edited, and refresh git
/// and the file tree after edits and shell commands.
pub fn nudge_open_editors(event: &HarnessEvent, cwd: &str, cx: &mut App) {
    let Some(tool) = tool_event(event) else {
        return;
    };
    if !tool.updated {
        return;
    }
    let Some(hooks) = Engine::try_global(cx).map(|engine| engine.hooks.workspace.clone()) else {
        return;
    };
    let done = completed(tool.status);
    let kind = tool.kind.map(|kind| kind.trim().to_lowercase());
    let shell = kind.as_deref() == Some("execute")
        || tool
            .preview
            .is_some_and(|preview| preview.kind == ToolPreviewKind::Shell);
    let cwd = cwd.to_string();
    if shell {
        if !done {
            return;
        }
        hooks.nudge_watched_files(None, cx);
        hooks.notify_git_changed(cx);
        hooks.nudge_workspace(Some(&cwd), cx);
        let later = hooks.clone();
        let timer = cx.background_executor().timer(RENUDGE_DELAY);
        cx.spawn(async move |cx| {
            timer.await;
            cx.update(|cx| {
                later.nudge_watched_files(None, cx);
                later.nudge_workspace(Some(&cwd), cx);
            });
        })
        .detach();
        return;
    }
    if !is_edit_tool(tool.kind, tool.title, tool.preview) {
        return;
    }
    let mut resolved: Vec<String> = Vec::new();
    for path in edit_paths(&tool) {
        let path = hooks.resolve_workspace_path(&path, &cwd).unwrap_or(path);
        if !resolved.contains(&path) {
            resolved.push(path);
        }
    }
    let paths = (!resolved.is_empty()).then_some(resolved);
    if done {
        // A successful edit is authoritative. Reload it even if a startup
        // race or a coarse filesystem timestamp makes the mtime look unchanged.
        hooks.invalidate_watched_files(paths.as_deref(), cx);
    } else if let Some(paths) = paths.as_deref() {
        hooks.nudge_watched_files(Some(paths), cx);
    }
    if done {
        let later = hooks.clone();
        let timer = cx.background_executor().timer(RENUDGE_DELAY);
        let again = paths.clone();
        cx.spawn(async move |cx| {
            timer.await;
            cx.update(|cx| later.nudge_watched_files(again.as_deref(), cx));
        })
        .detach();
        hooks.notify_git_changed(cx);
        hooks.nudge_workspace(Some(&cwd), cx);
    }
}
