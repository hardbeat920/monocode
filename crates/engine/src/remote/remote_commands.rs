//! Port of src/features/connections/model/remoteCommands.ts and the routing
//! half of src/platform/tauri/fs.ts (`invokeWorkspace`).
//!
//! A file or Git command whose path arguments are `remote://` paths runs on
//! the machine that owns them through `workspace.run`, and paths are
//! translated both ways, so callers never see host paths. Other packages
//! call `RemoteClient::invoke_workspace` (or `remote::invoke_workspace`)
//! before running a command locally.

use base64::Engine as _;
use serde_json::{Map, Value};

use super::remote_projects::{parse_remote_path, remote_path};

/// `REMOTE_PATH_PREFIX`.
pub const REMOTE_PATH_PREFIX: &str = "remote://";

/// `HOST_COMMANDS`: file commands a connected machine answers exactly as
/// this computer does (see host/workspace-commands.ts).
pub const HOST_COMMANDS: [&str; 42] = [
    "list_dir",
    "list_project_files",
    "read_text_file",
    "read_binary_file",
    "read_file_preview",
    "write_text_file",
    "stat_files",
    "create_path",
    "rename_path",
    "delete_path",
    "copy_path",
    "move_path",
    "git_diff_index",
    "git_diff_files",
    "git_diff_stats",
    "git_file_diff",
    "git_stage_contents",
    "git_stage_file",
    "git_unstage_file",
    "git_discard_file",
    "git_discard_all",
    "git_stage_all",
    "git_unstage_all",
    "git_commit",
    "git_head_message",
    "git_push",
    "git_pull",
    "git_sync",
    "git_pr_status",
    "git_pr_create",
    "git_history",
    "git_commit_files",
    "git_commit_file_diff",
    "git_staged_context",
    "git_range_context",
    "git_branches",
    "git_checkout",
    "git_create_branch",
    "git_stash",
    "git_worktrees",
    "search_project",
    "list_skills",
];

/// Arguments that hold paths; everything else is passed through untouched.
pub const PATH_ARGS: [&str; 6] = ["path", "cwd", "parent", "from", "destParent", "paths"];
/// Commands whose string result is a path.
const PATH_RESULTS: [&str; 4] = ["create_path", "rename_path", "copy_path", "move_path"];
/// Commands whose result entries carry a `path`.
const ENTRY_RESULTS: [&str; 4] = [
    "list_dir",
    "list_project_files",
    "stat_files",
    "list_skills",
];

pub const UNAVAILABLE: &str = "This isn’t available for projects on another machine yet.";
pub const OUTDATED: &str =
    "Update MonoCode Host in Connections settings to use this project’s files.";
pub const NOT_CONNECTED: &str = "This project’s machine isn’t connected on this computer.";
pub const CROSS_MACHINE: &str = "Files can only be copied or moved within one machine.";
/// `invokeWorkspace` with no remote runner installed.
pub const NO_RUNNER: &str = "Connect this project’s machine to open its files.";

/// `isRemotePath` in fs.ts: a `remote://` string, or an array holding one.
pub fn is_remote_path(value: &Value) -> bool {
    match value {
        Value::String(path) => path.starts_with(REMOTE_PATH_PREFIX),
        Value::Array(values) => values.iter().any(is_remote_path),
        _ => false,
    }
}

/// `invokeWorkspace`'s test: the command's paths live on a connected
/// machine, either in a path argument or in `options.cwd`.
pub fn is_remote_workspace_call(args: &Map<String, Value>) -> bool {
    let remote_options = args
        .get("options")
        .and_then(Value::as_object)
        .and_then(|options| options.get("cwd"))
        .is_some_and(is_remote_path);
    PATH_ARGS
        .iter()
        .any(|key| args.get(*key).is_some_and(is_remote_path))
        || remote_options
}

/// A command's arguments with host paths, and the machine they name.
#[derive(Debug, Clone, PartialEq)]
pub struct HostCall {
    pub environment_id: String,
    pub args: Map<String, Value>,
}

/// The `toHost` pass of `runRemoteCommand`: every path argument becomes a
/// host path on one machine.
pub fn host_call(command: &str, args: &Map<String, Value>) -> Result<HostCall, String> {
    if !HOST_COMMANDS.contains(&command) {
        return Err(UNAVAILABLE.into());
    }
    let mut environment_id: Option<String> = None;
    fn to_host(value: &Value, environment_id: &mut Option<String>) -> Result<Value, String> {
        match value {
            Value::Array(values) => values
                .iter()
                .map(|value| to_host(value, environment_id))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            Value::String(path) => {
                let parsed = parse_remote_path(path)
                    .filter(|parsed| {
                        environment_id
                            .as_deref()
                            .is_none_or(|known| known == parsed.environment_id)
                    })
                    .ok_or_else(|| CROSS_MACHINE.to_string())?;
                let host = Value::String(parsed.host_path);
                *environment_id = Some(parsed.environment_id);
                Ok(host)
            }
            other => Ok(other.clone()),
        }
    }
    let mut host_args = Map::new();
    for (key, value) in args {
        let value = if PATH_ARGS.contains(&key.as_str()) {
            to_host(value, &mut environment_id)?
        } else if key == "options"
            && let Value::Object(options) = value
        {
            let mut options = options.clone();
            if let Some(cwd) = options.get("cwd").cloned() {
                options.insert("cwd".into(), to_host(&cwd, &mut environment_id)?);
            }
            Value::Object(options)
        } else {
            value.clone()
        };
        host_args.insert(key.clone(), value);
    }
    let environment_id = environment_id
        .filter(|id| !id.is_empty())
        .ok_or_else(|| UNAVAILABLE.to_string())?;
    Ok(HostCall {
        environment_id,
        args: host_args,
    })
}

/// The `/Unsupported (host method|remote operation)/i` test: the host is
/// older than `workspace.run`.
pub fn is_unsupported(reason: &str) -> bool {
    let lower = reason.to_lowercase();
    lower.contains("unsupported host method") || lower.contains("unsupported remote operation")
}

/// `String(hostArgs.cwd).replace(/[\\/]+$/, "")`.
fn cwd_root(args: &Map<String, Value>) -> String {
    let cwd = match args.get("cwd") {
        Some(Value::String(cwd)) => cwd.clone(),
        Some(Value::Null) => "null".into(),
        Some(other) => other.to_string(),
        None => "undefined".into(),
    };
    cwd.trim_end_matches(['\\', '/']).to_string()
}

/// Replace `entry.path` with `map(entry.path)` when it is a string.
fn map_path(entry: &mut Value, key: &str, map: &dyn Fn(&str) -> String) {
    if let Some(path) = entry.get(key).and_then(Value::as_str) {
        let next = map(path);
        entry[key] = Value::String(next);
    }
}

fn map_each(list: Option<&mut Value>, map: &dyn Fn(&str) -> String) {
    if let Some(Value::Array(entries)) = list {
        for entry in entries {
            map_path(entry, "path", map);
        }
    }
}

/// The result half of `runRemoteCommand`: host paths in the answer become
/// `remote://` paths again.
pub fn remote_result(command: &str, call: &HostCall, result: Value) -> Value {
    let environment_id = call.environment_id.as_str();
    let from_host = |path: &str| remote_path(environment_id, path);
    let under_root =
        |path: &str| remote_path(environment_id, &format!("{}/{path}", cwd_root(&call.args)));
    let mut result = result;
    if PATH_RESULTS.contains(&command)
        && let Value::String(path) = &result
    {
        return Value::String(from_host(path));
    }
    if ENTRY_RESULTS.contains(&command) && result.is_array() {
        map_each(Some(&mut result), &from_host);
        return result;
    }
    match command {
        "git_diff_index" | "git_diff_files" if result.is_object() => {
            map_each(result.get_mut("files"), &under_root);
        }
        "git_file_diff" | "git_commit_file_diff" if result.is_object() => {
            map_path(&mut result, "path", &under_root);
        }
        "git_commit_files" if result.is_array() => {
            map_each(Some(&mut result), &under_root);
        }
        "search_project" if result.is_object() => {
            map_each(result.get_mut("matches"), &from_host);
        }
        "git_worktrees" if result.is_object() => {
            map_path(&mut result, "defaultRoot", &from_host);
            map_each(result.get_mut("worktrees"), &from_host);
        }
        _ => {}
    }
    result
}

/// `readBinaryFile`'s remote branch: a connected machine sends the bytes as
/// base64 inside its JSON reply.
pub fn decode_remote_binary(value: &Value) -> Result<Vec<u8>, String> {
    let data = value.as_str().ok_or("Invalid binary file response")?;
    base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|error| error.to_string())
}

/// The machine part of a `remote://` path, `""` for a local one (the
/// grouping key `statFiles` uses).
pub fn path_machine(path: &str) -> &str {
    path.strip_prefix(REMOTE_PATH_PREFIX)
        .map(|rest| rest.split('/').next().unwrap_or(""))
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn local_calls_stay_local_even_when_content_mentions_a_remote_path() {
        assert!(!is_remote_workspace_call(&args(json!({
            "path": "/home/me/note.txt",
            "content": "remote://env/home/me/repo",
        }))));
        assert!(is_remote_workspace_call(&args(
            json!({ "paths": ["/a", "remote://env/b"] })
        )));
        assert!(is_remote_workspace_call(&args(json!({
            "options": { "cwd": "remote://env/repo", "query": "x" }
        }))));
    }

    #[test]
    fn options_without_a_cwd_keep_their_shape() {
        let call = host_call(
            "search_project",
            &args(json!({ "path": "remote://env/repo", "options": { "query": "x" } })),
        )
        .unwrap();
        assert_eq!(call.args["options"], json!({ "query": "x" }));
    }

    #[test]
    fn binary_reads_decode_base64() {
        assert_eq!(
            decode_remote_binary(&json!("AAEC/w==")).unwrap(),
            vec![0, 1, 2, 255]
        );
        assert_eq!(path_machine("remote://env/home/me"), "env");
        assert_eq!(path_machine("/home/me"), "");
    }
}
