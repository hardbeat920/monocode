//! Native Claude tools require a TTY. Only computer-use turns get one.
use serde_json::{json, Value};
use std::io::{BufRead, Write};

/// Small stdio MCP server. It neither controls apps nor grants access. The
/// chat session gets the handoff tool; the interactive run gets show_screenshot.
pub fn run_mcp(interactive: bool) -> i32 {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { return 1 };
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(response) = mcp_response(&request, interactive) else {
            continue;
        };
        if writeln!(stdout, "{response}")
            .and_then(|_| stdout.flush())
            .is_err()
        {
            return 1;
        }
    }
    0
}

fn mcp_response(request: &Value, interactive: bool) -> Option<Value> {
    let id = request.get("id")?;
    let result = match request["method"].as_str()? {
        "initialize" => json!({
            "protocolVersion": request["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "monocode-computer-use", "version": "1.0.0"}
        }),
        "ping" => json!({}),
        "tools/list" if interactive => json!({"tools": [{
            "name": "show_screenshot",
            "description": "Show your most recent computer-use screenshot to the user beside your final answer. Screenshots otherwise stay folded inside your tool calls. Call it right after taking a screenshot that is itself part of the answer, never for screenshots taken to find your way.",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
        }]}),
        "tools/call" if interactive => {
            if request["params"]["name"] == "show_screenshot" {
                json!({"content": [{"type": "text", "text": "MonoCode shows your latest screenshot beside your answer."}]})
            } else {
                json!({"isError": true, "content": [{"type": "text", "text": "Unknown tool."}]})
            }
        }
        "tools/list" => json!({"tools": [{
            "name": "start",
            "description": "Resume this exact conversation in MonoCode's hidden interactive Claude CLI to use the native computer-use tools. Call when a task needs desktop app control or screenshots. Pass the remaining task, then END YOUR TURN immediately. MonoCode will continue automatically and ask the user for each app's access. Never use screen, AppleScript, a shell workaround or another Claude session for computer use.",
            "inputSchema": {"type": "object", "properties": {"task": {"type": "string", "minLength": 1}}, "required": ["task"], "additionalProperties": false}
        }]}),
        "tools/call" => {
            if request["params"]["name"] != "start"
                || request["params"]["arguments"]["task"]
                    .as_str()
                    .is_none_or(|s| s.trim().is_empty())
            {
                json!({"isError": true, "content": [{"type": "text", "text": "Expected start with a nonempty task."}]})
            } else {
                json!({"content": [{"type": "text", "text": "Handoff requested. End this turn now without more tool calls. MonoCode resumes the same conversation with native computer-use tools. App access still requires the user's click in MonoCode."}]})
            }
        }
        _ => {
            return Some(
                json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32601,"message":"Method not found"}}),
            )
        }
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

#[tauri::command]
pub fn claude_cu_config() -> Result<Option<Value>, String> {
    if !cfg!(target_os = "macos") {
        return Ok(None);
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(Some(json!({"mcpServers": {"monocode_computer_use": {
        "type": "stdio", "command": exe, "args": ["computer-use-mcp"]
    }}})))
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use crate::{harness, pty};
    use serde::Serialize;
    use std::collections::HashMap;
    use std::fs::{self, File};
    use std::io::{Read, Seek, SeekFrom};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use tauri::{AppHandle, Manager, State};

    struct Run {
        directory: PathBuf,
        projects: PathBuf,
        provider_session_id: String,
        transcript: Option<PathBuf>,
        offset: u64,
        partial: Vec<u8>,
    }

    #[derive(Default)]
    pub struct ComputerUseHost {
        runs: Mutex<HashMap<String, Run>>,
    }

    impl Drop for ComputerUseHost {
        fn drop(&mut self) {
            for (_, run) in self
                .runs
                .get_mut()
                .unwrap_or_else(|e| e.into_inner())
                .drain()
            {
                let _ = fs::remove_dir_all(run.directory);
            }
        }
    }

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Poll {
        screen: String,
        lines: Vec<String>,
        stopped: bool,
        failed: bool,
        running: bool,
    }

    fn transcript(projects: &Path, sid: &str) -> Option<PathBuf> {
        fs::read_dir(projects)
            .ok()?
            .filter_map(Result::ok)
            .find_map(|entry| {
                let file = entry.path().join(format!("{sid}.jsonl"));
                file.is_file().then_some(file)
            })
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
        static kAXTrustedCheckOptionPrompt: *const std::ffi::c_void;
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFBooleanTrue: *const std::ffi::c_void;
        static kCFTypeDictionaryKeyCallBacks: u8;
        static kCFTypeDictionaryValueCallBacks: u8;
        fn CFDictionaryCreate(
            allocator: *const std::ffi::c_void,
            keys: *const *const std::ffi::c_void,
            values: *const *const std::ffi::c_void,
            count: isize,
            key_callbacks: *const u8,
            value_callbacks: *const u8,
        ) -> *const std::ffi::c_void;
        fn CFRelease(value: *const std::ffi::c_void);
    }

    /// The hidden CLI inherits MonoCode's privacy grants, so MonoCode asks
    /// macOS for them itself. macOS shows its own prompt; nothing is granted here.
    fn request_privacy_access() -> (bool, bool) {
        // SAFETY: plain CoreFoundation/ApplicationServices calls with
        // framework-owned constants; the dictionary is released after use.
        unsafe {
            let accessibility = AXIsProcessTrusted() || {
                let keys = [kAXTrustedCheckOptionPrompt];
                let values = [kCFBooleanTrue];
                let options = CFDictionaryCreate(
                    std::ptr::null(),
                    keys.as_ptr(),
                    values.as_ptr(),
                    1,
                    &kCFTypeDictionaryKeyCallBacks,
                    &kCFTypeDictionaryValueCallBacks,
                );
                let trusted = AXIsProcessTrustedWithOptions(options);
                if !options.is_null() {
                    CFRelease(options);
                }
                trusted
            };
            let screen = CGPreflightScreenCaptureAccess() || CGRequestScreenCaptureAccess();
            (accessibility, screen)
        }
    }

    const RUN_PREFIX: &str = "claude-cu-";
    /// A transcript line larger than this is dropped rather than buffered.
    const MAX_PARTIAL_LINE: usize = 64 * 1024 * 1024;

    /// Runs end by removing their folder. A crash or force quit leaves it.
    fn remove_stale_runs(cache: &Path, host: &ComputerUseHost) {
        let live: Vec<PathBuf> = host
            .runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .map(|run| run.directory.clone())
            .collect();
        let Ok(entries) = fs::read_dir(cache) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let ours = entry.file_name().to_string_lossy().starts_with(RUN_PREFIX);
            if ours && path.is_dir() && !live.contains(&path) {
                let _ = fs::remove_dir_all(path);
            }
        }
    }

    fn shell_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "'\\''"))
    }

    #[tauri::command(async)]
    #[allow(clippy::too_many_arguments)]
    pub fn claude_cu_spawn(
        app: AppHandle,
        host: State<'_, ComputerUseHost>,
        terminals: State<'_, pty::PtyHost>,
        thread_id: String,
        command: String,
        binary_path: Option<String>,
        cwd: String,
        provider_session_id: String,
        account: harness::HarnessAccount,
        args: Vec<String>,
        mut settings: Value,
    ) -> Result<String, String> {
        uuid::Uuid::parse_str(&provider_session_id).map_err(|_| "Invalid Claude session id")?;
        if !harness::is_resolved_harness_binary(&command, Some("claude"), binary_path.as_deref()) {
            return Err("Computer use requires the resolved Claude CLI".into());
        }
        // The same account and GUI environment as the ordinary harness child.
        let mut cmd = std::process::Command::new(&command);
        // PTY spawn calls setsid(); process_group(0) would make it fail.
        harness::apply_gui_env(&mut cmd);
        cmd.env("MONOCODE_HARNESS_PARENT", std::process::id().to_string());
        for key in [
            "CLAUDECODE",
            "CLAUDE_PID",
            "CLAUDE_CODE_ENTRYPOINT",
            "CLAUDE_CODE_SESSION_ID",
            "CLAUDE_CODE_CHILD_SESSION",
            "CLAUDE_CODE_SESSION_ATTENDED",
            "CLAUDE_CODE_MESSAGING_SOCKET",
            "CLAUDE_CODE_MESSAGING_TOKEN",
        ] {
            cmd.env_remove(key);
        }
        harness::apply_provider_account(&app, &mut cmd, Some(&account))?;
        crate::control::configure_child(&app, &thread_id, &mut cmd);
        let config = cmd
            .get_envs()
            .find_map(|(key, value)| {
                (key == "CLAUDE_CONFIG_DIR")
                    .then(|| value.map(PathBuf::from))
                    .flatten()
            })
            .or_else(|| std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from))
            .or_else(|| crate::dirs_home().map(|home| PathBuf::from(home).join(".claude")))
            .ok_or("Claude config directory is unavailable")?;
        let workdir = crate::fs::expand_home(&cwd);
        if !workdir.is_dir() {
            return Err("Computer-use working directory does not exist".into());
        }
        let _reservation = crate::worktree_lifecycle::reserve_spawn(&workdir)?;
        let id = format!("{RUN_PREFIX}{}", uuid::Uuid::new_v4());
        let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?;
        remove_stale_runs(&cache, &host);
        let directory = cache.join(&id);
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
        let (accessibility, screen_recording) = request_privacy_access();
        // Claude's own dialog explains missing grants; debug builds keep a record.
        #[cfg(debug_assertions)]
        let _ = fs::write(
            directory.join("tcc.txt"),
            format!("accessibility={accessibility} screen_recording={screen_recording}\n"),
        );
        #[cfg(not(debug_assertions))]
        let _ = (accessibility, screen_recording);
        let signal = shell_quote(&directory.join("stop").to_string_lossy());
        let hook = json!([{"hooks": [{"type": "command", "command": format!("cat > {signal}"), "timeout": 5}]}]);
        settings["disableAllHooks"] = json!(false);
        settings["hooks"]["Stop"] = hook.clone();
        settings["hooks"]["StopFailure"] = hook;
        cmd.args(args)
            .arg("--resume")
            .arg(&provider_session_id)
            .arg("--settings")
            .arg(settings.to_string());
        let projects = config.join("projects");
        let file = transcript(&projects, &provider_session_id);
        let offset = file
            .as_ref()
            .and_then(|p| fs::metadata(p).ok())
            .map_or(0, |m| m.len());
        host.runs.lock().unwrap_or_else(|e| e.into_inner()).insert(
            id.clone(),
            Run {
                directory,
                projects,
                provider_session_id,
                transcript: file,
                offset,
                partial: Vec::new(),
            },
        );
        if let Err(error) =
            pty::spawn_unix_command(app, terminals, id.clone(), workdir, 140, 45, cmd, true)
        {
            if let Some(run) = host
                .runs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id)
            {
                let _ = fs::remove_dir_all(run.directory);
            }
            return Err(error);
        }
        Ok(id)
    }

    // Bound each read, retain incomplete UTF-8/JSON until the newline arrives.
    fn tail(run: &mut Run) -> Result<Vec<String>, String> {
        if run.transcript.is_none() {
            run.transcript = transcript(&run.projects, &run.provider_session_id);
        }
        let Some(path) = &run.transcript else {
            return Ok(Vec::new());
        };
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        if file.metadata().map_err(|e| e.to_string())?.len() < run.offset {
            run.offset = 0;
            run.partial.clear();
        }
        file.seek(SeekFrom::Start(run.offset))
            .map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(1024 * 1024)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        run.offset += bytes.len() as u64;
        run.partial.extend(bytes);
        let Some(end) = run.partial.iter().rposition(|b| *b == b'\n') else {
            if run.partial.len() > MAX_PARTIAL_LINE {
                run.partial.clear();
            }
            return Ok(Vec::new());
        };
        let lines = run.partial[..end]
            .split(|b| *b == b'\n')
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect();
        run.partial.drain(..=end);
        Ok(lines)
    }

    #[tauri::command(async)]
    pub fn claude_cu_poll(
        host: State<'_, ComputerUseHost>,
        terminals: State<'_, pty::PtyHost>,
        id: String,
    ) -> Result<Poll, String> {
        let mut runs = host.runs.lock().unwrap_or_else(|e| e.into_inner());
        let run = runs.get_mut(&id).ok_or("Computer-use session is closed")?;
        let screen = terminals.captured_screen(&id);
        // The PTY is hidden, so debug builds leave its last screen on disk.
        #[cfg(debug_assertions)]
        if let Some(screen) = &screen {
            let _ = fs::write(run.directory.join("screen.txt"), screen);
        }
        // Each Stop is reported once, so a later turn in the same run (after
        // the user steers it) is not mistaken for one that already ended.
        let stop = run.directory.join("stop");
        let signal = fs::read(&stop)
            .ok()
            .and_then(|data| serde_json::from_slice::<Value>(&data).ok());
        if signal.is_some() {
            let _ = fs::remove_file(&stop);
        }
        // Mirroring is display-only; a transcript hiccup must not end the run.
        let lines = tail(run).unwrap_or_default();
        Ok(Poll {
            running: screen.is_some(),
            screen: screen.unwrap_or_default(),
            lines,
            stopped: signal.is_some(),
            failed: signal
                .as_ref()
                .is_some_and(|s| s["hook_event_name"] == "StopFailure"),
        })
    }

    #[tauri::command(async)]
    pub fn claude_cu_close(
        host: State<'_, ComputerUseHost>,
        terminals: State<'_, pty::PtyHost>,
        id: String,
        cancelled: bool,
    ) -> Result<(), String> {
        let run = host
            .runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        if run.is_none() {
            return Ok(());
        }
        let _ = pty::pty_write(
            terminals.clone(),
            id.clone(),
            if cancelled { "\u{3}" } else { "/exit\r" }.into(),
        );
        for _ in 0..15 {
            if terminals.captured_screen(&id).is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        pty::pty_kill(terminals, id)?;
        if let Some(run) = run {
            let _ = fs::remove_dir_all(run.directory);
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn shell_paths_are_literal() {
            assert_eq!(shell_quote("a'b $(x)"), "'a'\\''b $(x)'");
        }
        #[test]
        fn stale_run_folders_are_removed_but_live_and_foreign_ones_kept() {
            let cache =
                std::env::temp_dir().join(format!("monocode-cu-cache-{}", uuid::Uuid::new_v4()));
            let stale = cache.join(format!("{RUN_PREFIX}stale"));
            let live = cache.join(format!("{RUN_PREFIX}live"));
            let foreign = cache.join("other-cache");
            for dir in [&stale, &live, &foreign] {
                fs::create_dir_all(dir).unwrap();
            }
            let host = ComputerUseHost::default();
            host.runs.lock().unwrap().insert(
                "live".into(),
                Run {
                    directory: live.clone(),
                    projects: cache.clone(),
                    provider_session_id: String::new(),
                    transcript: None,
                    offset: 0,
                    partial: Vec::new(),
                },
            );
            remove_stale_runs(&cache, &host);
            assert!(!stale.exists());
            assert!(live.exists() && foreign.exists());
            host.runs.lock().unwrap().clear();
            fs::remove_dir_all(cache).unwrap();
        }
        #[test]
        fn transcript_tail_keeps_partial_json_and_skips_history() {
            let directory =
                std::env::temp_dir().join(format!("monocode-cu-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&directory).unwrap();
            let path = directory.join("transcript.jsonl");
            fs::write(&path, "old\n").unwrap();
            let mut run = Run {
                directory: directory.clone(),
                projects: directory.clone(),
                provider_session_id: String::new(),
                transcript: Some(path.clone()),
                offset: 4,
                partial: Vec::new(),
            };
            let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
            file.write_all(b"{\"text\":\"hel").unwrap();
            assert!(tail(&mut run).unwrap().is_empty());
            file.write_all("lo 🌍\"}\n".as_bytes()).unwrap();
            assert_eq!(tail(&mut run).unwrap(), vec!["{\"text\":\"hello 🌍\"}"]);
            assert!(tail(&mut run).unwrap().is_empty());
            fs::remove_dir_all(directory).unwrap();
        }
    }
}

#[cfg(target_os = "macos")]
pub use native::*;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mcp_requires_a_task_and_ignores_notifications() {
        assert!(mcp_response(&json!({"method":"notifications/initialized"}), false).is_none());
        let invalid = mcp_response(&json!({"id":1,"method":"tools/call","params":{"name":"start","arguments":{"task":" "}}}), false).unwrap();
        assert_eq!(invalid["result"]["isError"], true);
        let valid = mcp_response(&json!({"id":2,"method":"tools/call","params":{"name":"start","arguments":{"task":"Open Calculator"}}}), false).unwrap();
        assert!(valid["result"]["isError"].is_null());
    }
    #[test]
    fn interactive_runs_only_offer_show_screenshot() {
        let list = mcp_response(&json!({"id":1,"method":"tools/list"}), true).unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "show_screenshot");
        let start = mcp_response(&json!({"id":2,"method":"tools/call","params":{"name":"start","arguments":{"task":"x"}}}), true).unwrap();
        assert_eq!(start["result"]["isError"], true);
    }
}
