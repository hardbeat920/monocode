//! Thin adapters; desktop and remote hosts share the native isolation implementation.
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

async fn run(app: AppHandle, command: &'static str, args: Value) -> Result<Value, String> {
    let store = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("isolation");
    tauri::async_runtime::spawn_blocking(move || {
        monocode_isolation::dispatch(&store, json!({"command":command,"args":args}))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn cow_capability(app: AppHandle, cwd: String) -> Result<Value, String> {
    run(app, "cow_capability", json!({"cwd":cwd})).await
}
#[tauri::command]
pub async fn cow_create(
    app: AppHandle,
    cwd: String,
    session_id: String,
    project_cwd: Option<String>,
    base: Option<String>,
) -> Result<Value, String> {
    run(
        app,
        "cow_create",
        json!({"cwd":cwd,"sessionId":session_id,"projectCwd":project_cwd,"base":base}),
    )
    .await
}
#[tauri::command]
pub async fn cow_list(
    app: AppHandle,
    cwd: String,
    store: tauri::State<'_, crate::session_store::SessionStore>,
) -> Result<Value, String> {
    let mut listed = run(app, "cow_list", json!({"cwd":cwd})).await?;
    let conn = store.lock_conn()?;
    if let Some(rows) = listed.as_array_mut() {
        for row in rows {
            let path = row["path"]
                .as_str()
                .ok_or("Copy-on-write path is unavailable")?;
            let ids = crate::worktrees::session_ids(&conn, std::path::Path::new(path))?;
            row["sessionIds"] = json!(ids);
        }
    }
    Ok(listed)
}
#[tauri::command]
pub async fn cow_status(app: AppHandle, cwd: String, cow_id: String) -> Result<Value, String> {
    run(app, "cow_status", json!({"cwd":cwd,"cowId":cow_id})).await
}
#[tauri::command]
pub async fn cow_file_diff(
    app: AppHandle,
    cwd: String,
    cow_id: String,
    relative: String,
) -> Result<Value, String> {
    run(
        app,
        "cow_file_diff",
        json!({"cwd":cwd,"cowId":cow_id,"relative":relative}),
    )
    .await
}
fn owned_copy(
    app: &AppHandle,
    cwd: &str,
    cow_id: &str,
) -> Result<(std::path::PathBuf, String, std::path::PathBuf, (u64, u64)), String> {
    let isolation = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("isolation");
    let row = monocode_isolation::dispatch(
        &isolation,
        json!({"command":"cow_owner","args":{"cwd":cwd,"cowId":cow_id}}),
    )?;
    let path = row["path"]
        .as_str()
        .ok_or("Copy-on-write path is unavailable")?;
    let project = row["projectCwd"]
        .as_str()
        .ok_or("Copy-on-write project is unavailable")?;
    let identity = (
        row["rootIdentity"][0]
            .as_str()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or("Copy-on-write ownership identity is unavailable")?,
        row["rootIdentity"][1]
            .as_str()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or("Copy-on-write ownership identity is unavailable")?,
    );
    Ok((
        std::path::PathBuf::from(path),
        project.to_owned(),
        isolation,
        identity,
    ))
}

#[tauri::command(async)]
pub fn cow_check_remove(
    app: AppHandle,
    cwd: String,
    cow_id: String,
    force: Option<bool>,
    terminals: tauri::State<'_, crate::pty::PtyHost>,
) -> Result<(), String> {
    let (path, _, isolation, _) = owned_copy(&app, &cwd, &cow_id)?;
    if terminals.has_working_dir(&path) {
        return Err("Close the terminals using this workspace first.".into());
    }
    monocode_isolation::dispatch(
        &isolation,
        json!({"command":"cow_check_remove","args":{"cwd":cwd,"cowId":cow_id,"force":force}}),
    )?;
    Ok(())
}

#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
pub fn cow_remove(
    app: AppHandle,
    cwd: String,
    cow_id: String,
    force: Option<bool>,
    keep_sessions: Option<bool>,
    store: tauri::State<'_, crate::session_store::SessionStore>,
    terminals: tauri::State<'_, crate::pty::PtyHost>,
    agents: tauri::State<'_, crate::harness::HarnessHost>,
) -> Result<crate::worktrees::WorktreeRemoval, String> {
    let (path, project, isolation, identity) = owned_copy(&app, &cwd, &cow_id)?;
    let _reservation = crate::worktree_lifecycle::reserve_removal(&path)?;
    if terminals.has_working_dir(&path) || agents.has_working_dir(&path) {
        return Err("Close the terminals and agent processes using this workspace first.".into());
    }
    let conn = store.lock_conn()?;
    crate::worktrees::remove_registered_workspace_with_sessions(
        &conn,
        &path,
        &project,
        identity,
        keep_sessions.unwrap_or(false),
        || {
            monocode_isolation::dispatch(
                &isolation,
                json!({"command":"cow_remove","args":{"cwd":cwd,"cowId":cow_id,"force":force}}),
            )?;
            Ok(())
        },
    )
}
#[tauri::command]
pub async fn cow_apply(
    app: AppHandle,
    cwd: String,
    cow_id: String,
    to_cwd: String,
) -> Result<Value, String> {
    run(
        app,
        "cow_apply",
        json!({"cwd":cwd,"cowId":cow_id,"toCwd":to_cwd}),
    )
    .await
}
