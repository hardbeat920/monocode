//! The native equivalent of `connections/model/remoteCommands.ts`.
//! Keep host paths behind `remote://` paths at every workspace API boundary.
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::path::Path;

const UNAVAILABLE: &str = "This isn’t available for projects on another machine yet.";
const OUTDATED: &str = "Update MonoCode Host in Connections settings to use this project’s files.";

pub fn is_remote(path: &Path) -> bool {
    path.to_str()
        .is_some_and(|path| path.starts_with("remote://"))
}

/// Run one of the same bounded workspace commands supported by the Tauri app.
/// This is synchronous, like the filesystem APIs; call from a background task.
pub fn run_remote_command(command: &str, args: Value) -> Result<Value, String> {
    run_with(command, args, |environment, params| {
        let connections = crate::remote::default_connections()?;
        let machine = connections.machine_for_environment(environment)?;
        connections.request(&machine.id, "workspace.run", params)
    })
}

fn run_with(
    command: &str,
    args: Value,
    request: impl FnOnce(&str, Value) -> Result<Value, String>,
) -> Result<Value, String> {
    let (environment, args) = host_arguments(command, args)?;
    let result =
        request(&environment, json!({"command":command,"args":args})).map_err(workspace_error)?;
    Ok(client_result(command, &environment, &args, result))
}

pub(crate) fn command<T: DeserializeOwned>(command: &str, args: Value) -> Result<T, String> {
    serde_json::from_value(run_remote_command(command, args)?)
        .map_err(|error| format!("Invalid remote workspace response: {error}"))
}

fn workspace_error(error: String) -> String {
    let lower = error.to_ascii_lowercase();
    if [
        "unsupported host method",
        "unsupported remote operation",
        "unsupported workspace command",
    ]
    .iter()
    .any(|message| lower.contains(message))
    {
        OUTDATED.into()
    } else {
        error
    }
}

fn host_arguments(command: &str, args: Value) -> Result<(String, Value), String> {
    if !matches!(
        command,
        "list_dir"
            | "list_project_files"
            | "read_text_file"
            | "read_binary_file"
            | "read_file_preview"
            | "write_text_file"
            | "stat_files"
            | "create_path"
            | "rename_path"
            | "delete_path"
            | "copy_path"
            | "move_path"
            | "git_diff_index"
            | "git_diff_files"
            | "git_diff_stats"
            | "git_file_diff"
            | "git_stage_contents"
            | "git_stage_file"
            | "git_unstage_file"
            | "git_discard_file"
            | "git_discard_all"
            | "git_stage_all"
            | "git_unstage_all"
            | "git_commit"
            | "git_locate_files"
            | "git_head_message"
            | "git_push"
            | "git_pull"
            | "git_sync"
            | "git_pr_status"
            | "git_pr_create"
            | "git_history"
            | "git_commit_files"
            | "git_commit_file_diff"
            | "git_staged_context"
            | "git_range_context"
            | "git_branches"
            | "git_checkout"
            | "git_create_branch"
            | "git_stash"
            | "git_worktrees"
            | "search_project"
    ) {
        return Err(UNAVAILABLE.into());
    }
    let Value::Object(mut args) = args else {
        return Err("Invalid workspace arguments".into());
    };
    let mut environment = None;
    for (key, value) in &mut args {
        if matches!(
            key.as_str(),
            "path" | "cwd" | "parent" | "from" | "destParent" | "paths"
        ) && !(key == "paths" && matches!(command, "git_commit" | "git_staged_context"))
        {
            to_host(value, &mut environment)?;
        } else if key == "options" {
            if let Some(options) = value.as_object_mut() {
                to_host(
                    options.get_mut("cwd").ok_or("Invalid search workspace")?,
                    &mut environment,
                )?;
            }
        }
    }
    Ok((environment.ok_or(UNAVAILABLE)?, Value::Object(args)))
}

fn to_host(value: &mut Value, environment: &mut Option<String>) -> Result<(), String> {
    if let Value::Array(values) = value {
        for value in values {
            to_host(value, environment)?;
        }
    } else if let Value::String(path) = value {
        let remote = crate::remote::parse_remote_path(path)
            .ok_or("Files can only be copied or moved within one machine.")?;
        if environment
            .as_ref()
            .is_some_and(|env| env != &remote.environment_id)
        {
            return Err("Files can only be copied or moved within one machine.".into());
        }
        *environment = Some(remote.environment_id);
        *path = remote.host_path;
    }
    Ok(())
}

fn client_result(command: &str, environment: &str, args: &Value, mut result: Value) -> Value {
    let prefix = |path: &mut Value| {
        if let Some(host) = path.as_str() {
            *path = Value::String(crate::remote::remote_path(environment, host));
        }
    };
    let root = args
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim_end_matches(['/', '\\']);
    let relative_path = |entry: &mut Value| {
        if let Some(path) = entry.get_mut("path") {
            if let Some(relative) = path.as_str() {
                *path = Value::String(crate::remote::remote_path(
                    environment,
                    &format!("{root}/{relative}"),
                ));
            }
        }
    };
    match command {
        "create_path" | "rename_path" | "copy_path" | "move_path" => prefix(&mut result),
        "list_dir" | "list_project_files" | "stat_files" => {
            if let Some(entries) = result.as_array_mut() {
                for entry in entries {
                    if let Some(path) = entry.get_mut("path") {
                        prefix(path);
                    }
                }
            }
        }
        "git_locate_files" => {
            if let Some(entries) = result.as_array_mut() {
                for entry in entries {
                    if let Some(root) = entry.get_mut("root") {
                        prefix(root);
                    }
                }
            }
        }
        "git_diff_index" | "git_diff_files" => {
            if let Some(files) = result.get_mut("files").and_then(Value::as_array_mut) {
                for file in files {
                    relative_path(file);
                }
            }
        }
        "git_file_diff" | "git_commit_file_diff" => relative_path(&mut result),
        "git_commit_files" => {
            if let Some(files) = result.as_array_mut() {
                for file in files {
                    relative_path(file);
                }
            }
        }
        "search_project" => {
            if let Some(matches) = result.get_mut("matches").and_then(Value::as_array_mut) {
                for entry in matches {
                    if let Some(path) = entry.get_mut("path") {
                        prefix(path);
                    }
                }
            }
        }
        "git_worktrees" => {
            if let Some(root) = result.get_mut("defaultRoot") {
                prefix(root);
            }
            if let Some(trees) = result.get_mut("worktrees").and_then(Value::as_array_mut) {
                for tree in trees {
                    if let Some(path) = tree.get_mut("path") {
                        prefix(path);
                    }
                }
            }
        }
        _ => {}
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_routes_paths_and_preserves_remote_failure_without_local_fallback() {
        let result = run_with("list_dir", json!({"path":"remote://env/home/code"}), |environment, params| {
            assert_eq!(environment, "env");
            assert_eq!(params, json!({"command":"list_dir","args":{"path":"/home/code"}}));
            Ok(json!([{"name":"a.txt","path":"/home/code/a.txt","isDir":false,"ignored":false}]))
        }).unwrap();
        assert_eq!(result[0]["path"], "remote://env/home/code/a.txt");
        let error = run_with(
            "read_text_file",
            json!({"path":"remote://env/home/code/a.txt"}),
            |_, _| Err("Host disconnected".into()),
        )
        .unwrap_err();
        assert_eq!(error, "Host disconnected");
        let error = run_with(
            "read_text_file",
            json!({"path":"remote://env/home/code/a.txt"}),
            |_, _| Err("Unsupported host method".into()),
        )
        .unwrap_err();
        assert_eq!(error, OUTDATED);
    }

    #[test]
    fn preserves_relative_selected_paths_and_rejects_cross_machine_copies() {
        let (_, args) = host_arguments(
            "git_commit",
            json!({"cwd":"remote://linux/home/code", "paths":["a.txt"],"message":"Done"}),
        )
        .unwrap();
        assert_eq!(args["cwd"], "/home/code");
        assert_eq!(args["paths"], json!(["a.txt"]));
        assert!(host_arguments(
            "copy_path",
            json!({"from":"remote://a/home/code/a", "destParent":"remote://b/home/code"})
        )
        .is_err());
        assert!(host_arguments(
            "copy_path",
            json!({"from":"remote://a/home/code/a", "destParent":"/tmp"})
        )
        .is_err());
        assert!(host_arguments("terminal_spawn", json!({"cwd":"remote://a/home/code"})).is_err());
    }

    #[test]
    fn maps_nested_search_and_workspace_results_without_exposing_host_paths() {
        let (env, args) = host_arguments(
            "search_project",
            json!({"options":{"cwd":"remote://windows/C:/code", "query":"needle"}}),
        )
        .unwrap();
        assert_eq!(args["options"]["cwd"], "C:/code");
        assert_eq!(
            client_result(
                "search_project",
                &env,
                &args,
                json!({"matches":[{"path":"C:/code/a.txt"}]})
            )["matches"][0]["path"],
            "remote://windows/C:/code/a.txt"
        );
        let args = json!({"cwd":"/code"});
        assert_eq!(
            client_result(
                "git_diff_index",
                "linux",
                &args,
                json!({"files":[{"path":"a.txt","relative":"a.txt"}]})
            )["files"][0]["path"],
            "remote://linux/code/a.txt"
        );
        assert_eq!(
            client_result(
                "git_worktrees",
                "linux",
                &args,
                json!({"defaultRoot":"/trees","worktrees":[{"path":"/trees/one"}]})
            )["worktrees"][0]["path"],
            "remote://linux/trees/one"
        );
    }
}
