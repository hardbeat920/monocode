use super::protocol::ToolCall;
use super::transport::{Cancel, NativeError, Result};
use serde_json::{json, Value};
#[cfg(windows)]
use std::io::Read;
use std::path::{Component, Path, PathBuf};
#[cfg(windows)]
use std::time::{Duration, Instant};

const MAX_FILE: u64 = 1024 * 1024;
const MAX_OUTPUT: usize = 64 * 1024;

#[derive(Debug, PartialEq)]
pub enum Permission {
    Allow,
    Ask,
    Deny,
}

pub fn permission(name: &str, mode: &str, plan: bool) -> Permission {
    match name {
        "read_file" | "list_files" | "search_files" => Permission::Allow,
        "edit_file" | "write_file"
            if !plan && (mode == "auto-accept-edits" || mode == "full-access") =>
        {
            Permission::Allow
        }
        "powershell" if !plan && mode == "full-access" => Permission::Allow,
        "edit_file" | "write_file" | "powershell" if !plan => Permission::Ask,
        _ => Permission::Deny,
    }
}

pub fn definitions(plan: bool) -> Value {
    let schema = |properties: Value, required: Value| json!({"type":"OBJECT","properties":properties,"required":required});
    let string = json!({"type":"STRING"});
    let mut definitions = vec![
        json!({"name":"read_file","description":"Read a UTF-8 file inside the workspace (maximum 1 MiB).","parameters":schema(json!({"path":string}),json!(["path"]))}),
        json!({"name":"list_files","description":"List a directory inside the workspace. Paths are relative to the workspace.","parameters":schema(json!({"path":string}),json!(["path"]))}),
        json!({"name":"search_files","description":"Search UTF-8 workspace files for literal text, bounded to 5000 files and 20 directory levels.","parameters":schema(json!({"path":string,"query":string}),json!(["path","query"]))}),
    ];
    if !plan {
        definitions.extend([
            json!({"name":"write_file","description":"Create or replace a workspace file with UTF-8 content. Requires edit permission.","parameters":schema(json!({"path":string,"content":string}),json!(["path","content"]))}),
            json!({"name":"edit_file","description":"Replace one exact occurrence of oldText in a workspace file. Requires edit permission.","parameters":schema(json!({"path":string,"oldText":string,"newText":string}),json!(["path","oldText","newText"]))}),
            json!({"name":"powershell","description":"Execute PowerShell in the workspace. Requires command permission; the approved command can access resources outside the workspace. Execution is limited to 120 seconds.","parameters":schema(json!({"command":string}),json!(["command"]))}),
        ]);
    }
    json!([{"functionDeclarations":definitions}])
}

fn argument<'a>(args: &'a Value, name: &str) -> Result<&'a str> {
    args[name]
        .as_str()
        .ok_or_else(|| NativeError::new("tool", "A required tool argument is missing."))
}

pub fn workspace_path(root: &Path, supplied: &str, create: bool) -> Result<PathBuf> {
    let deny = || {
        NativeError::new(
            "tool",
            "File tools can only access paths inside this workspace.",
        )
    };
    let path = Path::new(supplied);
    if supplied.contains('\0') || path.components().any(|component| matches!(component, Component::Normal(name) if name.to_string_lossy().contains(':'))) { return Err(deny()); }
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let resolved = match joined.canonicalize() {
        Ok(path) => path,
        Err(_) if create && !joined.exists() => {
            let parent = joined
                .parent()
                .ok_or_else(deny)?
                .canonicalize()
                .map_err(|_| deny())?;
            parent.join(joined.file_name().ok_or_else(deny)?)
        }
        Err(_) => {
            return Err(NativeError::new(
                "tool",
                "Workspace path does not exist or cannot be read.",
            ))
        }
    };
    let root = root.canonicalize().map_err(|_| deny())?;
    if !resolved.starts_with(&root) {
        return Err(deny());
    }
    // Refuse dangling symlinks as creation targets too.
    if create && std::fs::symlink_metadata(&joined).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(deny());
    }
    Ok(resolved)
}

fn text_file(path: &Path) -> Result<String> {
    if !path.is_file()
        || std::fs::metadata(path)
            .map_err(|_| NativeError::new("tool", "Could not read file."))?
            .len()
            > MAX_FILE
    {
        return Err(NativeError::new(
            "tool",
            "File is not a regular file or exceeds 1 MiB.",
        ));
    }
    std::fs::read_to_string(path)
        .map_err(|_| NativeError::new("tool", "Could not read file as UTF-8."))
}

fn truncated(mut text: String) -> String {
    if text.len() > MAX_OUTPUT {
        let mut end = MAX_OUTPUT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n[Output truncated]");
    }
    text
}

pub fn file_tool(root: &Path, call: &ToolCall, cancel: &Cancel) -> Result<Value> {
    if cancel.is_cancelled() {
        return Err(NativeError::cancelled());
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|_| NativeError::new("tool", "Workspace is unavailable."))?;
    let root = canonical_root.as_path();
    let create = call.name == "write_file";
    let path = workspace_path(root, argument(&call.args, "path")?, create)?;
    let output = match call.name.as_str() {
        "read_file" => truncated(text_file(&path)?),
        "list_files" => {
            let mut entries = std::fs::read_dir(path)
                .map_err(|_| NativeError::new("tool", "Could not list workspace directory."))?
                .take(1000)
                .filter_map(|entry| entry.ok())
                .map(|entry| {
                    let suffix = if entry.file_type().is_ok_and(|t| t.is_dir()) {
                        "/"
                    } else {
                        ""
                    };
                    format!("{}{suffix}", entry.file_name().to_string_lossy())
                })
                .collect::<Vec<_>>();
            entries.sort();
            truncated(entries.join("\n"))
        }
        "search_files" => {
            let query = argument(&call.args, "query")?;
            if query.is_empty() {
                return Err(NativeError::new("tool", "Search query must not be empty."));
            }
            let mut output = String::new();
            let mut files = 0;
            search(root, &path, query, 0, &mut files, &mut output, cancel)?;
            truncated(output)
        }
        "write_file" => {
            let content = argument(&call.args, "content")?;
            if content.len() > MAX_FILE as usize {
                return Err(NativeError::new("tool", "File content exceeds 1 MiB."));
            }
            std::fs::write(&path, content)
                .map_err(|_| NativeError::new("tool", "Could not write workspace file."))?;
            "File written.".into()
        }
        "edit_file" => {
            let old = argument(&call.args, "oldText")?;
            let new = argument(&call.args, "newText")?;
            let source = text_file(&path)?;
            if old.is_empty() || source.matches(old).count() != 1 {
                return Err(NativeError::new(
                    "tool",
                    "oldText must match exactly one occurrence.",
                ));
            }
            let content = source.replacen(old, new, 1);
            if content.len() > MAX_FILE as usize {
                return Err(NativeError::new("tool", "Edited file exceeds 1 MiB."));
            }
            std::fs::write(&path, content)
                .map_err(|_| NativeError::new("tool", "Could not edit workspace file."))?;
            "File edited.".into()
        }
        _ => return Err(NativeError::new("tool", "Unknown Antigravity file tool.")),
    };
    Ok(json!({"output":output}))
}

fn search(
    root: &Path,
    path: &Path,
    query: &str,
    depth: usize,
    files: &mut usize,
    output: &mut String,
    cancel: &Cancel,
) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(NativeError::cancelled());
    }
    if depth > 20 || *files >= 5000 || output.len() >= MAX_OUTPUT {
        return Ok(());
    }
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Ok(());
    }
    let resolved = path
        .canonicalize()
        .map_err(|_| NativeError::new("tool", "Could not inspect workspace path."))?;
    if !resolved.starts_with(root) {
        return Ok(());
    }
    if resolved.is_dir() {
        let entries = std::fs::read_dir(resolved)
            .map_err(|_| NativeError::new("tool", "Could not search workspace directory."))?;
        for entry in entries.take(5000).flatten() {
            if [".git", "node_modules", "target"]
                .contains(&entry.file_name().to_string_lossy().as_ref())
            {
                continue;
            }
            search(root, &entry.path(), query, depth + 1, files, output, cancel)?;
            if *files >= 5000 || output.len() >= MAX_OUTPUT {
                break;
            }
        }
    } else {
        *files += 1;
        if let Ok(text) = text_file(&resolved) {
            for (index, line) in text.lines().enumerate() {
                if line.contains(query) {
                    output.push_str(&format!(
                        "{}:{}:{}\n",
                        resolved.strip_prefix(root).unwrap_or(&resolved).display(),
                        index + 1,
                        line
                    ));
                    if output.len() >= MAX_OUTPUT {
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}

pub async fn execute(root: PathBuf, call: ToolCall, cancel: Cancel) -> Result<Value> {
    tokio::task::spawn_blocking(move || {
        if call.name == "powershell" {
            powershell(&root, argument(&call.args, "command")?, &cancel)
        } else {
            file_tool(&root, &call, &cancel)
        }
    })
    .await
    .map_err(|_| NativeError::new("tool", "Antigravity tool execution failed."))?
}

#[cfg(windows)]
fn invocation_job() -> Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::JobObjects::{
        CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    unsafe {
        let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if handle.is_null() {
            return Err(NativeError::new(
                "tool",
                "Could not create a PowerShell invocation job.",
            ));
        }
        let job = OwnedHandle::from_raw_handle(handle);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            std::mem::size_of_val(&limits) as u32,
        ) == 0
        {
            return Err(NativeError::new(
                "tool",
                "Could not configure a PowerShell invocation job.",
            ));
        }
        Ok(job)
    }
}

#[cfg(windows)]
fn powershell(root: &Path, command: &str, cancel: &Cancel) -> Result<Value> {
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};
    if command.len() > 32 * 1024 {
        return Err(NativeError::new("tool", "PowerShell command is too large."));
    }
    if cancel.is_cancelled() {
        return Err(NativeError::cancelled());
    }
    let system = std::env::var_os("SystemRoot")
        .ok_or_else(|| NativeError::new("tool", "Windows PowerShell could not be found."))?;
    let mut cmd =
        Command::new(PathBuf::from(system).join("System32/WindowsPowerShell/v1.0/powershell.exe"));
    cmd.args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        command,
    ])
    .current_dir(root)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    crate::harness::apply_gui_env(&mut cmd);
    let job = invocation_job()?;
    let mut child = crate::windows::spawn_managed_in_job(&mut cmd, Some(&job))
        .map_err(|_| NativeError::new("tool", "Could not start Windows PowerShell."))?;
    let output = Arc::new(Mutex::new(Vec::new()));
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    for mut stream in [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let output = output.clone();
        let done = done_tx.clone();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(n) = stream.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                let mut output = output.lock().unwrap_or_else(|p| p.into_inner());
                let remaining = MAX_OUTPUT.saturating_sub(output.len());
                output.extend_from_slice(&chunk[..n.min(remaining)]);
            }
            let _ = done.send(());
        });
    }
    drop(done_tx);
    let start = Instant::now();
    let result = loop {
        if cancel.is_cancelled() || start.elapsed() >= Duration::from_secs(120) {
            break Err(if cancel.is_cancelled() {
                NativeError::cancelled()
            } else {
                NativeError::new("timeout", "PowerShell tool timed out after 120 seconds.")
            });
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                break Ok(status.code());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                break Err(NativeError::new("tool", "PowerShell process failed."));
            }
        }
    };
    // Closing the nested job kills descendants even after the leader exits.
    // Pipe collection is bounded too; never join a potentially blocked reader.
    drop(job);
    let _ = child.kill();
    let _ = done_rx.recv_timeout(Duration::from_millis(500));
    let _ = done_rx.recv_timeout(Duration::from_millis(500));
    let code = result?;
    let output = output.lock().unwrap_or_else(|p| p.into_inner());
    Ok(json!({"output":String::from_utf8_lossy(&output),"exitCode":code}))
}

#[cfg(not(windows))]
fn powershell(_: &Path, _: &str, _: &Cancel) -> Result<Value> {
    Err(NativeError::new(
        "platform",
        "Native Antigravity tools require Windows.",
    ))
}
