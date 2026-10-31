//! Composer bash mode: `!command` runs in the thread's folder, and its output
//! is shared with the agent in the next turn.
use crate::transcript::Row;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex, OnceLock,
};
use std::time::{Duration, Instant};

pub const ROLE: &str = "shell";
/// The transcript keeps this much of a run: its start and its end.
const STORED_HEAD: usize = 16 * 1024;
const STORED_TAIL: usize = 48 * 1024;
/// Each run's share of the next prompt, and all runs together.
const PROMPT_RUN_CHARS: usize = 16_000;
const PROMPT_TOTAL_CHARS: usize = 48_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    #[default]
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// One `!command`, stored as a `shell` transcript row.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellRun {
    pub id: String,
    pub command: String,
    pub cwd: String,
    #[serde(default)]
    pub output: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<i32>,
    #[serde(default)]
    pub state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}
impl ShellRun {
    pub fn new(command: &str, cwd: &str) -> Self {
        Self {
            id: format!("shell-{}", uuid::Uuid::new_v4()),
            command: command.into(),
            cwd: cwd.into(),
            ..Default::default()
        }
    }
    pub fn row(&self) -> Row {
        Row {
            role: ROLE.into(),
            text: serde_json::to_string(self).unwrap_or_default(),
            first: true,
            sent_at: Some(crate::time::now_ms()),
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }
}

/// The composer text's command when it opens with `!`, else `None`.
/// `\!` is the escape for a message that starts with a literal `!`.
pub fn command(text: &str) -> Option<&str> {
    text.strip_prefix('!')
}
/// `!command` on the first line, then an optional question for the agent.
pub fn split(text: &str) -> Option<(&str, &str)> {
    let rest = command(text)?;
    let (command, question) = rest.split_once('\n').unwrap_or((rest, ""));
    Some((command.trim(), question.trim()))
}

/// Where the shell rows the next message carries begin.
pub fn pending_start(rows: &[Row]) -> usize {
    rows.iter()
        .rposition(|row| row.role != ROLE)
        .map_or(0, |at| at + 1)
}
/// Shell rows the next message carries: the ones since the last prompt.
pub fn pending(rows: &[Row]) -> Vec<ShellRun> {
    rows[pending_start(rows)..]
        .iter()
        .filter_map(|row| ShellRun::parse(&row.text))
        .collect()
}

/// The block the agent reads before the user's message. Without one, the
/// output is the whole message.
pub fn context(runs: &[ShellRun], has_message: bool) -> Option<String> {
    if runs.is_empty() {
        return None;
    }
    let mut budget = PROMPT_TOTAL_CHARS;
    let mut blocks = Vec::new();
    for run in runs.iter().rev() {
        let output = head_and_tail(run.output.trim_end(), PROMPT_RUN_CHARS.min(budget));
        budget = budget.saturating_sub(output.chars().count());
        let status = match (run.state, run.exit) {
            (State::Cancelled, _) => " status=\"cancelled\"".to_owned(),
            (_, Some(code)) => format!(" exit=\"{code}\""),
            _ => String::new(),
        };
        blocks.push(format!(
            "<shell-command cwd=\"{}\"{status}>\n$ {}\n{output}\n</shell-command>",
            run.cwd, run.command
        ));
        if budget == 0 {
            break;
        }
    }
    blocks.reverse();
    let intro = if has_message {
        "The user ran these shell commands before this message. Their output is context for it."
    } else {
        "The user ran these shell commands and shared their output with you. Read it and respond to what it shows: explain failures, summarize results, and suggest a next step if one is useful."
    };
    Some(format!("{intro}\n{}", blocks.join("\n")))
}

/// Errors and summaries come last, so the tail gets most of the room.
fn head_and_tail(text: &str, count: usize) -> String {
    let total = text.chars().count();
    if total <= count {
        return text.into();
    }
    let head = count / 4;
    let tail = count - head;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(total - tail).collect();
    format!("{start}\n… {} characters omitted …\n{end}", total - count)
}

/// A delivered run as a line of replayed history.
pub fn history_line(run: &ShellRun) -> String {
    format!(
        "$ {}\n{}",
        run.command,
        head_and_tail(run.output.trim_end(), 2_000)
    )
}

/// A run's live output, bounded to its head and tail.
#[derive(Default)]
pub struct Output {
    head: String,
    tail: String,
    omitted: usize,
}
impl Output {
    pub fn push(&mut self, text: &str) {
        let room = STORED_HEAD.saturating_sub(self.head.len());
        let split = floor_boundary(text, room.min(text.len()));
        self.head.push_str(&text[..split]);
        self.tail.push_str(&text[split..]);
        if self.tail.len() > STORED_TAIL {
            let cut = ceil_boundary(&self.tail, self.tail.len() - STORED_TAIL);
            self.omitted += cut;
            self.tail.drain(..cut);
        }
    }
    pub fn text(&self) -> String {
        let text = if self.omitted == 0 {
            format!("{}{}", self.head, self.tail)
        } else {
            format!(
                "{}\n… {} bytes omitted …\n{}",
                self.head, self.omitted, self.tail
            )
        };
        clean(&text)
    }
}
fn floor_boundary(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}
fn ceil_boundary(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at += 1;
    }
    at
}

/// Drops terminal escapes and keeps what a `\r` progress line ends on.
fn clean(text: &str) -> String {
    static ANSI: OnceLock<regex::Regex> = OnceLock::new();
    let ansi = ANSI.get_or_init(|| {
        regex::Regex::new(
            r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]",
        )
        .unwrap()
    });
    let text = ansi.replace_all(text, "");
    text.split('\n')
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            line.rsplit('\r').next().unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn running() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static RUNNING: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    RUNNING.get_or_init(Default::default)
}
/// Stops the run with this id. A run that hasn't started yet stops as soon
/// as it does.
pub fn cancel(id: &str) {
    running()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(id.to_owned())
        .or_default()
        .store(true, Ordering::Relaxed);
}

/// Runs `run.command` in a login shell, streaming bounded output through
/// `progress`. Blocks until the command exits or is cancelled.
pub fn execute(run: &mut ShellRun, mut progress: impl FnMut(&ShellRun)) {
    let cancel = running()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(run.id.clone())
        .or_default()
        .clone();
    let started = Instant::now();
    let result = if cancel.load(Ordering::Relaxed) {
        Ok(None)
    } else {
        spawn_and_wait(run, &cancel, &mut progress)
    };
    running()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&run.id);
    run.duration_ms = Some(started.elapsed().as_millis() as u64);
    match result {
        Ok(exit) if cancel.load(Ordering::Relaxed) => {
            run.exit = exit;
            run.state = State::Cancelled;
        }
        Ok(exit) => {
            run.exit = exit;
            run.state = if exit == Some(0) {
                State::Completed
            } else {
                State::Failed
            };
        }
        Err(error) => {
            if !run.output.is_empty() && !run.output.ends_with('\n') {
                run.output.push('\n');
            }
            run.output.push_str(&error);
            run.state = State::Failed;
        }
    }
    progress(run);
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut shell = Command::new("cmd");
        shell.args(["/C", command]);
        shell
    }
    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|shell| !shell.is_empty())
            .unwrap_or_else(|| {
                if cfg!(target_os = "macos") {
                    "/bin/zsh".into()
                } else {
                    "/bin/sh".into()
                }
            });
        let mut shell = Command::new(shell);
        shell.args(["-l", "-c", command]);
        shell
    }
}

/// Chunks in flight between the pipe readers and the run loop. A command
/// that writes faster than the loop keeps up blocks on its pipe instead of
/// queueing its output in memory.
const CHANNEL_CHUNKS: usize = 64;
const CHUNK: usize = 8192;

/// One thread per stream, each sending what it reads. Readers stop once the
/// receiver is dropped.
fn read_streams(
    streams: impl IntoIterator<Item = Box<dyn Read + Send>>,
) -> (mpsc::Receiver<Vec<u8>>, Vec<std::thread::JoinHandle<()>>) {
    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(CHANNEL_CHUNKS);
    let readers = streams
        .into_iter()
        .map(|mut stream| {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut buffer = [0u8; CHUNK];
                while let Ok(read) = stream.read(&mut buffer) {
                    if read == 0 || tx.send(buffer[..read].to_vec()).is_err() {
                        break;
                    }
                }
            })
        })
        .collect();
    (rx, readers)
}

fn spawn_and_wait(
    run: &mut ShellRun,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(&ShellRun),
) -> Result<Option<i32>, String> {
    let cwd = crate::scm::shell::expand_home(Path::new(&run.cwd));
    if !cwd.is_dir() {
        return Err(format!("{} is not a folder", run.cwd));
    }
    let mut command = shell_command(&run.command);
    crate::scm::shell::apply_gui_env(&mut command);
    command
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .env("PAGER", "cat")
        .env("GIT_PAGER", "cat");
    crate::process::isolate(&mut command);
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let _registration = crate::process::register(child.id());
    let (rx, readers) = read_streams(
        [
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
        .flatten(),
    );
    let mut output = Output::default();
    let mut pending = Vec::new();
    let mut last_paint = Instant::now();
    let mut dirty = false;
    let mut exit = None;
    let mut exited_at: Option<Instant> = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(bytes) => {
                pending.extend_from_slice(&bytes);
                let valid = match std::str::from_utf8(&pending) {
                    Ok(_) => pending.len(),
                    Err(error) if error.error_len().is_none() => error.valid_up_to(),
                    Err(_) => pending.len(),
                };
                output.push(&String::from_utf8_lossy(&pending[..valid]));
                pending.drain(..valid);
                dirty = true;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Ok(Some(status)) = child.try_wait() {
                    exit = Some(status.code());
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        if cancel.load(Ordering::Relaxed) {
            crate::process::terminate(&mut child);
            break;
        }
        if exit.is_none() {
            if let Ok(Some(status)) = child.try_wait() {
                exit = Some(status.code());
                exited_at = Some(Instant::now());
            }
        }
        // `cmd &` keeps the pipes open after the shell exits; it goes on
        // without the transcript.
        if exited_at.is_some_and(|at| at.elapsed() >= Duration::from_millis(500)) {
            break;
        }
        if dirty && last_paint.elapsed() >= Duration::from_millis(100) {
            run.output = output.text();
            progress(run);
            last_paint = Instant::now();
            dirty = false;
        }
    }
    if !pending.is_empty() {
        output.push(&String::from_utf8_lossy(&pending));
    }
    run.output = output.text();
    let exit = match exit {
        Some(exit) => exit,
        None => child.wait().map_err(|e| e.to_string())?.code(),
    };
    for reader in readers {
        if reader.is_finished() {
            let _ = reader.join();
        }
    }
    Ok(exit)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn run(command: &str) -> ShellRun {
        let dir = std::env::temp_dir();
        let mut run = ShellRun::new(command, dir.to_str().unwrap());
        execute(&mut run, |_| {});
        run
    }

    #[test]
    fn records_output_and_exit_status() {
        let ok = run("echo hello; echo oops >&2");
        assert_eq!(ok.state, State::Completed);
        assert_eq!(ok.exit, Some(0));
        assert!(ok.output.contains("hello") && ok.output.contains("oops"));
        let failed = run("exit 3");
        assert_eq!(failed.state, State::Failed);
        assert_eq!(failed.exit, Some(3));
    }

    #[test]
    fn a_missing_folder_fails_without_spawning() {
        let mut run = ShellRun::new("echo hi", "/definitely/not/here");
        execute(&mut run, |_| {});
        assert_eq!(run.state, State::Failed);
        assert!(run.output.contains("not a folder"));
    }

    #[test]
    fn cancel_kills_the_process_group() {
        let dir = std::env::temp_dir();
        let mut shell = ShellRun::new("sleep 30 & sleep 30", dir.to_str().unwrap());
        let id = shell.id.clone();
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            cancel(&id);
        });
        let started = Instant::now();
        execute(&mut shell, |_| {});
        canceller.join().unwrap();
        assert_eq!(shell.state, State::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn cancel_works_after_output_pipes_close() {
        let dir = std::env::temp_dir();
        let mut shell = ShellRun::new("exec >/dev/null 2>&1; sleep 30", dir.to_str().unwrap());
        let id = shell.id.clone();
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            cancel(&id);
        });
        let started = Instant::now();
        execute(&mut shell, |_| {});
        canceller.join().unwrap();
        assert_eq!(shell.state, State::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    /// Counts what a never-ending stream has handed out.
    struct Endless(Arc<std::sync::atomic::AtomicUsize>);
    impl Read for Endless {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            buffer.fill(b'y');
            self.0.fetch_add(buffer.len(), Ordering::Relaxed);
            Ok(buffer.len())
        }
    }

    #[test]
    fn readers_wait_for_a_slow_consumer() {
        let read = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (rx, readers) = read_streams([Box::new(Endless(read.clone())) as Box<dyn Read + Send>]);
        std::thread::sleep(Duration::from_millis(300));
        let bound = (CHANNEL_CHUNKS + 2) * CHUNK;
        assert!(read.load(Ordering::Relaxed) <= bound);
        drop(rx);
        for reader in readers {
            reader.join().unwrap();
        }
    }

    #[test]
    fn sustained_output_stays_bounded_and_cancels() {
        let dir = std::env::temp_dir();
        let mut shell = ShellRun::new("yes", dir.to_str().unwrap());
        let id = shell.id.clone();
        let started = Instant::now();
        let mut largest = 0;
        let mut cancelled_at = None;
        execute(&mut shell, |run| {
            largest = largest.max(run.output.len());
            // Stop once the output has overflowed its bound a few times over,
            // or give up so a stuck run still ends.
            let omitted = run.output.contains("bytes omitted");
            if cancelled_at.is_none() && (omitted || started.elapsed() > Duration::from_secs(10)) {
                cancelled_at = Some(Instant::now());
                cancel(&id);
            }
        });
        let cancelled_at = cancelled_at.unwrap();
        assert_eq!(shell.state, State::Cancelled);
        assert!(shell.output.contains("bytes omitted"));
        assert!(cancelled_at.elapsed() < Duration::from_secs(5));
        assert!(shell.output.starts_with("y\ny\n"));
        assert!(largest < STORED_HEAD + STORED_TAIL + 64);
    }

    #[test]
    fn output_keeps_head_and_tail_and_drops_escapes() {
        let mut output = Output::default();
        output.push("\x1b[31mred\x1b[0m\n");
        output.push("10%\r50%\r100%\n");
        assert_eq!(output.text(), "red\n100%\n");
        let mut big = Output::default();
        big.push(&"a".repeat(STORED_HEAD));
        big.push(&"b".repeat(STORED_TAIL * 2));
        big.push("end");
        let text = big.text();
        assert!(text.starts_with('a') && text.ends_with("end"));
        assert!(text.contains("bytes omitted"));
        assert!(text.len() < STORED_HEAD + STORED_TAIL + 64);
    }

    #[test]
    fn pending_runs_are_the_shell_rows_after_the_last_prompt() {
        let user = Row {
            role: "user".into(),
            text: "hi".into(),
            first: true,
            sent_at: None,
        };
        let mut old = ShellRun::new("ls", "/tmp");
        old.output = "old".into();
        let mut new = ShellRun::new("git status", "/tmp");
        new.output = "clean".into();
        new.exit = Some(0);
        let rows = vec![old.row(), user.clone(), new.row()];
        let pending = pending(&rows);
        assert_eq!(pending, vec![new.clone()]);
        let context = context(&pending, true).unwrap();
        assert!(context.contains("$ git status\nclean"));
        assert!(context.contains("exit=\"0\""));
        assert!(super::pending(&[old.row(), user]).is_empty());
    }

    #[test]
    fn bang_prefix_is_a_command() {
        assert_eq!(command("!ls -la"), Some("ls -la"));
        assert_eq!(command("\\!important"), None);
        assert_eq!(command(" !ls"), None);
        assert_eq!(
            split("!cargo test\nwhy does it fail?"),
            Some(("cargo test", "why does it fail?"))
        );
        assert_eq!(split("! ls -la "), Some(("ls -la", "")));
    }
}
