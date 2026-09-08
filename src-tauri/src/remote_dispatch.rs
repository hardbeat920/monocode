// Remote command dispatch for the companion link.
//
// MERGE NOTE (upstream-friendly): this file only *calls* existing commands —
// it changes none of them. When upstream adds a command, the companion keeps
// working; exposing the new command remotely is a 3-line arm plus an
// allowlist entry in remote.rs.
//
// Safety overrides (documented deviations from local behavior):
// - `session_take_in_flight` is served as a non-destructive *list*: the take
//   consumes the host's quit-restore snapshot, and a companion must never
//   steal host restore state.
// - `workspace_set_snapshot` is absorbed (Ok, no write): the companion layout
//   is ephemeral and must not clobber the desktop's restore snapshot.
// - `reveal_path` is absorbed (Ok, no-op): there is no Finder to reveal on
//   an iPad.
// - `stage/take_window_transfer` are host-window plumbing and are not in the
//   allowlist at all; the TS caller already treats rejection as "no transfer".

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};

fn required<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    let raw = args.get(name).cloned().unwrap_or(Value::Null);
    serde_json::from_value(raw).map_err(|e| format!("{name}: {e}"))
}

fn optional<T: DeserializeOwned>(args: &Value, name: &str) -> Result<Option<T>, String> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(raw) => serde_json::from_value(raw.clone()).map_err(|e| format!("{name}: {e}")),
    }
}

fn ok<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

/// Convert a binary Tauri `Response` into the `{ __bytes }` envelope the
/// companion transport decodes back to an ArrayBuffer (see protocol.ts).
fn binary_envelope(response: tauri::ipc::Response) -> Result<Value, String> {
    use tauri::ipc::IpcResponse;
    match response.body().map_err(|e| e.to_string())? {
        tauri::ipc::InvokeResponseBody::Raw(bytes) => {
            use base64::Engine;
            Ok(serde_json::json!({
                "__bytes": base64::engine::general_purpose::STANDARD.encode(bytes),
            }))
        }
        tauri::ipc::InvokeResponseBody::Json(_) => {
            Err("companion: unexpected JSON binary body".into())
        }
    }
}

pub async fn dispatch_command(
    app: &AppHandle,
    command: &str,
    args: Value,
) -> Result<Value, String> {
    if !crate::remote::is_remote_command(command) {
        return Err(format!(
            "companion: command is not available remotely ({command})"
        ));
    }
    let args_obj = if args.is_null() {
        &Value::Object(Default::default())
    } else {
        &args
    };
    match command {
        // -- host identity -------------------------------------------------
        "default_cwd" => ok(crate::default_cwd()),
        "home_dir" => ok(crate::home_dir()),
        "remote_peers" => ok(crate::remote::remote_peers(app.state())),
        "remote_status" => ok(crate::remote::remote_status(app.state())),

        // -- agent harnesses (spawned on the host; iPad never runs CLIs) ---
        "harness_resolve_cursor" => ok(crate::harness::harness_resolve_cursor()?),
        "harness_resolve_codex" => ok(crate::harness::harness_resolve_codex()?),
        "harness_resolve_opencode" => ok(crate::harness::harness_resolve_opencode()?),
        "harness_resolve_claude" => ok(crate::harness::harness_resolve_claude()?),
        "harness_resolve_pi" => ok(crate::harness::harness_resolve_pi()?),
        "harness_resolve_omp" => ok(crate::harness::harness_resolve_omp()?),
        "harness_resolve_fx" => ok(crate::harness::harness_resolve_fx()?),
        "harness_resolve_grok" => ok(crate::harness::harness_resolve_grok()?),
        "harness_free_port" => ok(crate::harness::harness_free_port()?),
        "harness_spawn" => ok(crate::harness::harness_spawn(
            app.clone(),
            app.state(),
            required(args_obj, "sessionId")?,
            required(args_obj, "command")?,
            required(args_obj, "args")?,
            required(args_obj, "cwd")?,
        )?),
        "harness_write" => ok(crate::harness::harness_write(
            app.state(),
            required(args_obj, "sessionId")?,
            required(args_obj, "line")?,
        )?),
        "harness_kill" => ok(crate::harness::harness_kill(
            app.state(),
            required(args_obj, "sessionId")?,
        )?),
        "harness_kill_all" => ok(crate::harness::harness_kill_all(app.state())?),
        "harness_http" => {
            let response = crate::harness::harness_http(
                required(args_obj, "url")?,
                required(args_obj, "method")?,
                optional(args_obj, "headers")?,
                optional(args_obj, "body")?,
                optional(args_obj, "timeoutMs")?,
            )
            .await?;
            ok(response)
        }
        "harness_sse_open" => ok(crate::harness::harness_sse_open(
            app.clone(),
            app.state(),
            required(args_obj, "sessionId")?,
            required(args_obj, "url")?,
            optional(args_obj, "headers")?,
        )?),
        "harness_sse_close" => ok(crate::harness::harness_sse_close(
            app.state(),
            required(args_obj, "sessionId")?,
        )?),
        "harness_exec" => ok(crate::harness::harness_exec(
            required(args_obj, "command")?,
            required(args_obj, "args")?,
            optional(args_obj, "cwd")?,
        )
        .await?),

        // -- terminals ------------------------------------------------------
        "pty_spawn" => ok(crate::pty::pty_spawn(
            app.clone(),
            app.state(),
            required(args_obj, "id")?,
            required(args_obj, "cwd")?,
            required(args_obj, "cols")?,
            required(args_obj, "rows")?,
        )?),
        "pty_write" => ok(crate::pty::pty_write(
            app.state(),
            required(args_obj, "id")?,
            required(args_obj, "data")?,
        )?),
        "pty_resize" => ok(crate::pty::pty_resize(
            app.state(),
            required(args_obj, "id")?,
            required(args_obj, "cols")?,
            required(args_obj, "rows")?,
        )?),
        "pty_status" => ok(crate::pty::pty_status(
            app.state(),
            required(args_obj, "id")?,
        )?),
        "pty_kill" => ok(crate::pty::pty_kill(
            app.state(),
            required(args_obj, "id")?,
        )?),
        "pty_kill_all" => ok(crate::pty::pty_kill_all(app.state())?),

        // -- sessions / workspace -------------------------------------------
        "session_upsert" => ok(crate::session_store::session_upsert(
            app.clone(),
            app.state(),
            required(args_obj, "session")?,
        )?),
        "session_list_by_project" => ok(crate::session_store::session_list_by_project(
            app.state(),
            required(args_obj, "cwd")?,
        )?),
        "session_search" => ok(crate::session_store::session_search(
            app.state(),
            required(args_obj, "options")?,
        )?),
        "session_get" => ok(crate::session_store::session_get(
            app.state(),
            required(args_obj, "sessionId")?,
        )?),
        "session_delete" => ok(crate::session_store::session_delete(
            app.clone(),
            app.state(),
            required(args_obj, "sessionId")?,
        )?),
        "session_set_archived" => ok(crate::session_store::session_set_archived(
            app.clone(),
            app.state(),
            required(args_obj, "sessionId")?,
            required(args_obj, "archived")?,
        )?),
        "session_set_pinned" => ok(crate::session_store::session_set_pinned(
            app.clone(),
            app.state(),
            required(args_obj, "sessionId")?,
            required(args_obj, "pinned")?,
        )?),
        "session_set_in_flight" => ok(crate::session_store::session_set_in_flight(
            app.state(),
            required(args_obj, "sessions")?,
        )?),
        "session_list_in_flight" => ok(crate::session_store::session_list_in_flight(app.state())?),
        // Non-destructive on purpose: the take consumes host restore state.
        "session_take_in_flight" => ok(crate::session_store::session_list_in_flight(app.state())?),
        // Absorbed on purpose: companion layout is ephemeral; see header.
        "workspace_set_snapshot" => Ok(Value::Null),
        "workspace_get_snapshot" => ok(crate::session_store::workspace_get_snapshot(app.state())?),

        // -- filesystem / git -----------------------------------------------
        "list_dir" => ok(crate::fs::list_dir(required(args_obj, "path")?)?),
        "list_project_files" => {
            ok(crate::fs::list_project_files(required(args_obj, "cwd")?).await?)
        }
        "git_diff_stats" => ok(crate::fs::git_diff_stats(required(args_obj, "cwd")?).await?),
        "git_diff_index" => ok(crate::fs::git_diff_index(required(args_obj, "cwd")?).await?),
        "git_diff_files" => ok(crate::fs::git_diff_files(required(args_obj, "cwd")?).await?),
        "git_file_diff" => ok(crate::fs::git_file_diff(
            required(args_obj, "cwd")?,
            required(args_obj, "relative")?,
        )
        .await?),
        "git_history" => ok(crate::fs::git_history(
            required(args_obj, "cwd")?,
            optional(args_obj, "limit")?,
        )
        .await?),
        "git_commit_files" => ok(crate::fs::git_commit_files(
            required(args_obj, "cwd")?,
            required(args_obj, "sha")?,
        )
        .await?),
        "git_commit_file_diff" => ok(crate::fs::git_commit_file_diff(
            required(args_obj, "cwd")?,
            required(args_obj, "sha")?,
            required(args_obj, "relative")?,
        )
        .await?),
        "git_stage_file" => ok(crate::fs::git_stage_file(
            required(args_obj, "cwd")?,
            required(args_obj, "relative")?,
        )
        .await?),
        "git_stage_contents" => ok(crate::fs::git_stage_contents(
            required(args_obj, "cwd")?,
            required(args_obj, "relative")?,
            required(args_obj, "contents")?,
        )
        .await?),
        "git_unstage_file" => ok(crate::fs::git_unstage_file(
            required(args_obj, "cwd")?,
            required(args_obj, "relative")?,
        )
        .await?),
        "git_discard_file" => ok(crate::fs::git_discard_file(
            required(args_obj, "cwd")?,
            required(args_obj, "relative")?,
        )
        .await?),
        "git_discard_all" => ok(crate::fs::git_discard_all(required(args_obj, "cwd")?).await?),
        "git_stage_all" => ok(crate::fs::git_stage_all(required(args_obj, "cwd")?).await?),
        "git_unstage_all" => ok(crate::fs::git_unstage_all(required(args_obj, "cwd")?).await?),
        "git_staged_context" => {
            ok(crate::fs::git_staged_context(required(args_obj, "cwd")?).await?)
        }
        "git_commit" => ok(crate::fs::git_commit(
            required(args_obj, "cwd")?,
            required(args_obj, "message")?,
        )
        .await?),
        "git_push" => ok(crate::fs::git_push(required(args_obj, "cwd")?).await?),
        "git_pull" => ok(crate::fs::git_pull(required(args_obj, "cwd")?).await?),
        "git_sync" => ok(crate::fs::git_sync(required(args_obj, "cwd")?).await?),
        "git_range_context" => ok(crate::fs::git_range_context(required(args_obj, "cwd")?).await?),
        "git_pr_status" => ok(crate::fs::git_pr_status(required(args_obj, "cwd")?).await?),
        "git_pr_create" => ok(crate::fs::git_pr_create(
            required(args_obj, "cwd")?,
            required(args_obj, "title")?,
            required(args_obj, "body")?,
            required(args_obj, "base")?,
            required(args_obj, "head")?,
        )
        .await?),
        "git_github_repo" => ok(crate::fs::git_github_repo(required(args_obj, "cwd")?).await?),
        "git_github_work_items" => ok(crate::fs::git_github_work_items(
            required(args_obj, "cwd")?,
            required(args_obj, "kind")?,
            required(args_obj, "assignedToMe")?,
            required(args_obj, "state")?,
            required(args_obj, "search")?,
            optional(args_obj, "limit")?,
        )
        .await?),
        "git_github_work_item_details" => ok(crate::fs::git_github_work_item_details(
            required(args_obj, "cwd")?,
            required(args_obj, "kind")?,
            required(args_obj, "number")?,
        )
        .await?),
        "git_github_work_item_thread" => ok(crate::fs::git_github_work_item_thread(
            required(args_obj, "cwd")?,
            required(args_obj, "kind")?,
            required(args_obj, "number")?,
        )
        .await?),
        "git_github_work_item_comment" => ok(crate::fs::git_github_work_item_comment(
            required(args_obj, "cwd")?,
            required(args_obj, "kind")?,
            required(args_obj, "number")?,
            required(args_obj, "body")?,
            required(args_obj, "inReplyTo")?,
        )
        .await?),
        "git_github_pr_diff" => ok(crate::fs::git_github_pr_diff(
            required(args_obj, "cwd")?,
            required(args_obj, "number")?,
        )
        .await?),
        "git_branches" => ok(crate::fs::git_branches(required(args_obj, "cwd")?).await?),
        "git_checkout" => ok(crate::fs::git_checkout(
            required(args_obj, "cwd")?,
            required(args_obj, "name")?,
            optional(args_obj, "remote")?,
        )
        .await?),
        "git_create_branch" => ok(crate::fs::git_create_branch(
            required(args_obj, "cwd")?,
            required(args_obj, "name")?,
        )
        .await?),
        "git_stash" => ok(crate::fs::git_stash(
            required(args_obj, "cwd")?,
            optional(args_obj, "message")?,
        )
        .await?),
        "create_path" => ok(crate::fs::create_path(
            required(args_obj, "parent")?,
            required(args_obj, "name")?,
            required(args_obj, "isDir")?,
        )?),
        "rename_path" => ok(crate::fs::rename_path(
            required(args_obj, "path")?,
            required(args_obj, "name")?,
        )
        .await?),
        "delete_path" => ok(crate::fs::delete_path(required(args_obj, "path")?).await?),
        "copy_path" => ok(crate::fs::copy_path(
            required(args_obj, "from")?,
            required(args_obj, "destParent")?,
        )
        .await?),
        "move_path" => ok(crate::fs::move_path(
            required(args_obj, "from")?,
            required(args_obj, "destParent")?,
        )
        .await?),
        // No-op on purpose: nothing to reveal on a companion screen.
        "reveal_path" => Ok(Value::Null),
        "clone_repo" => ok(crate::fs::clone_repo(
            required(args_obj, "url")?,
            required(args_obj, "parent")?,
        )
        .await?),
        "read_file_preview" => ok(crate::fs::read_file_preview(
            required(args_obj, "path")?,
            required(args_obj, "maxLines")?,
            optional(args_obj, "startLine")?,
        )?),
        "stat_files" => ok(crate::fs::stat_files(required(args_obj, "paths")?)?),
        "inspect_paths" => ok(crate::fs::inspect_paths(required(args_obj, "paths")?)),
        "read_file_base64" => ok(crate::fs::read_file_base64(required(args_obj, "path")?).await?),
        "read_binary_file" => {
            let response = crate::fs::read_binary_file(required(args_obj, "path")?).await?;
            binary_envelope(response)
        }
        "write_attachment" => ok(crate::fs::write_attachment(
            required(args_obj, "name")?,
            required(args_obj, "data")?,
        )
        .await?),
        "read_text_file" => ok(crate::fs::read_text_file(required(args_obj, "path")?).await?),
        "write_text_file" => ok(crate::fs::write_text_file(
            required(args_obj, "path")?,
            required(args_obj, "content")?,
        )
        .await?),

        // -- search / skills / misc host data --------------------------------
        "search_project" => {
            ok(crate::search::search_project(required(args_obj, "options")?).await?)
        }
        "list_skills" => ok(crate::skills::list_skills(required(args_obj, "cwd")?)?),
        "cursor_tool_calls" => ok(crate::cursor_store::cursor_tool_calls(
            required(args_obj, "sessionId")?,
            required(args_obj, "toolCallIds")?,
        )
        .await?),
        "fetch_claude_usage" => ok(crate::rate_limits::fetch_claude_usage().await?),
        "fetch_inbox_media" => {
            let response =
                crate::inbox_media::fetch_inbox_media(required(args_obj, "url")?).await?;
            binary_envelope(response)
        }
        "linear_status" => ok(crate::linear::linear_status(app.clone())?),
        "linear_set_token" => {
            ok(crate::linear::linear_set_token(app.clone(), required(args_obj, "token")?).await?)
        }
        "linear_list_teams" => ok(crate::linear::linear_list_teams(app.clone()).await?),
        "linear_list_issues" => ok(crate::linear::linear_list_issues(
            app.clone(),
            required(args_obj, "assignedToMe")?,
            required(args_obj, "state")?,
            required(args_obj, "teamIds")?,
            optional(args_obj, "limit")?,
        )
        .await?),
        "linear_issue_details" => {
            ok(crate::linear::linear_issue_details(app.clone(), required(args_obj, "id")?).await?)
        }
        "linear_issue_thread" => {
            ok(crate::linear::linear_issue_thread(app.clone(), required(args_obj, "id")?).await?)
        }
        "linear_issue_comment" => ok(crate::linear::linear_issue_comment(
            app.clone(),
            required(args_obj, "id")?,
            required(args_obj, "body")?,
            required(args_obj, "parentId")?,
        )
        .await?),
        "notes_list" => ok(crate::notes::notes_list(app.state())?),
        "notes_get" => ok(crate::notes::notes_get(
            app.state(),
            required(args_obj, "id")?,
        )?),
        "notes_upsert" => ok(crate::notes::notes_upsert(
            app.state(),
            required(args_obj, "note")?,
        )?),
        "notes_delete" => ok(crate::notes::notes_delete(
            app.state(),
            required(args_obj, "id")?,
        )?),
        "session_checkpoint_ensure" => ok(crate::checkpoint::session_checkpoint_ensure(
            app.state(),
            required(args_obj, "sessionId")?,
            required(args_obj, "cwd")?,
        )
        .await?),
        "session_checkpoint_prepare" => ok(crate::checkpoint::session_checkpoint_prepare(
            app.state(),
            required(args_obj, "sessionId")?,
            required(args_obj, "cwd")?,
            required(args_obj, "paths")?,
        )
        .await?),
        "session_checkpoint_capture" => ok(crate::checkpoint::session_checkpoint_capture(
            app.state(),
            required(args_obj, "sessionId")?,
            required(args_obj, "cwd")?,
            required(args_obj, "paths")?,
        )
        .await?),
        "session_checkpoint_status" => ok(crate::checkpoint::session_checkpoint_status(
            app.state(),
            required(args_obj, "sessionId")?,
            required(args_obj, "cwd")?,
        )
        .await?),
        "session_checkpoint_file_diff" => ok(crate::checkpoint::session_checkpoint_file_diff(
            app.state(),
            required(args_obj, "sessionId")?,
            required(args_obj, "cwd")?,
            required(args_obj, "relative")?,
        )
        .await?),
        "session_checkpoint_undo" => ok(crate::checkpoint::session_checkpoint_undo(
            app.state(),
            required(args_obj, "sessionId")?,
            required(args_obj, "cwd")?,
            optional(args_obj, "relative")?,
        )
        .await?),
        "session_checkpoint_keep" => ok(crate::checkpoint::session_checkpoint_keep(
            app.state(),
            required(args_obj, "sessionId")?,
            required(args_obj, "cwd")?,
            optional(args_obj, "relative")?,
        )
        .await?),
        "save_project_logo" => ok(crate::project_logo::save_project_logo(
            app.clone(),
            required(args_obj, "project")?,
            required(args_obj, "sourcePath")?,
        )
        .await?),
        "remove_project_logo" => ok(crate::project_logo::remove_project_logo(
            app.clone(),
            required(args_obj, "project")?,
        )
        .await?),

        _ => Err(format!(
            "companion: command is not available remotely ({command})"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn required_reads_typed_args() {
        let args = json!({ "path": "/tmp", "limit": 7 });
        assert_eq!(required::<String>(&args, "path").unwrap(), "/tmp");
        assert_eq!(required::<u32>(&args, "limit").unwrap(), 7);
    }

    #[test]
    fn required_rejects_missing_and_mistyped_args() {
        let args = json!({ "limit": "seven" });
        assert!(required::<String>(&args, "path").is_err());
        assert!(required::<u32>(&args, "limit").is_err());
    }

    #[test]
    fn optional_treats_missing_and_null_as_none() {
        let args = json!({ "present": "x", "nil": null });
        assert_eq!(
            optional::<String>(&args, "present").unwrap(),
            Some("x".to_string())
        );
        assert_eq!(optional::<String>(&args, "nil").unwrap(), None);
        assert_eq!(optional::<String>(&args, "absent").unwrap(), None);
        assert_eq!(optional::<u32>(&args, "absent").unwrap(), None);
    }

    #[test]
    fn optional_rejects_mistyped_args() {
        let args = json!({ "limit": [1, 2] });
        assert!(optional::<u32>(&args, "limit").is_err());
    }
}
