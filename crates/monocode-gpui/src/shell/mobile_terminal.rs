//! Shells a paired phone opens in a project. Each runs on its own PTY and keeps
//! its output, numbered by byte offset, so a phone that was backgrounded picks
//! up where it stopped; a phone-side emulator draws the bytes. The core host
//! decides who may ask; this owns the shells and checks they are the asking
//! device's own.
use base64::{Engine, engine::general_purpose::STANDARD};
use monocode_core::mobile::{MobileHost, Push};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    io::{Read, Write},
    path::Path,
    sync::{Arc, Condvar, Mutex, Weak},
    time::{Duration, Instant},
};

/// Output kept for a phone that was away, per shell.
const KEEP: usize = 256 * 1024;
/// One frame's share of the output, so a burst arrives as a few frames.
const FRAME: usize = 32 * 1024;
const MAX_SHELLS: usize = 8;
const MAX_PER_DEVICE: usize = 2;
const MAX_INPUT: usize = 64 * 1024;
/// How far ahead of the next keystroke a request may arrive.
const MAX_REORDER: u64 = 64;
/// A shell nobody is attached to is closed after this, or sooner once it ended.
const DETACHED_TTL: Duration = Duration::from_secs(30 * 60);
const ENDED_TTL: Duration = Duration::from_secs(5 * 60);
/// The pump's wait when no shell wrote, for new attaches and the TTLs.
const TICK: Duration = Duration::from_millis(30);
/// Output that follows within this of the last frame joins the next one.
const MIN_GAP: Duration = Duration::from_millis(4);

/// Rouses the pump as soon as a shell writes, so an echo goes out at once
/// rather than on the next tick.
#[derive(Default)]
struct Wake {
    rung: Mutex<bool>,
    bell: Condvar,
}
impl Wake {
    fn ring(&self) {
        if let Ok(mut rung) = self.rung.lock() {
            *rung = true;
            self.bell.notify_one();
        }
    }
    fn wait(&self, timeout: Duration) {
        let Ok(rung) = self.rung.lock() else {
            return;
        };
        if let Ok((mut rung, _)) = self.bell.wait_timeout_while(rung, timeout, |rung| !*rung) {
            *rung = false;
        }
    }
}

/// The last `KEEP` bytes a shell wrote, with the offset of the first.
#[derive(Default)]
struct Output {
    start: u64,
    bytes: Vec<u8>,
}
impl Output {
    fn end(&self) -> u64 {
        self.start + self.bytes.len() as u64
    }
    fn push(&mut self, data: &[u8]) {
        self.bytes.extend_from_slice(data);
        if self.bytes.len() > KEEP {
            let drop = self.bytes.len() - KEEP;
            self.bytes.drain(..drop);
            self.start += drop as u64;
        }
    }
    /// Up to `limit` bytes from offset `from`, which is clamped to what is kept.
    fn from(&self, from: u64, limit: usize) -> (u64, &[u8]) {
        let from = from.clamp(self.start, self.end());
        let at = (from - self.start) as usize;
        let to = self.bytes.len().min(at + limit);
        (from, &self.bytes[at..to])
    }
}

/// Keystrokes arrive as separate requests, possibly out of order and possibly
/// twice; this puts them back in order, once.
struct Inputs {
    next: u64,
    held: BTreeMap<u64, Vec<u8>>,
}
impl Inputs {
    fn accept(&mut self, seq: u64, data: Vec<u8>) -> Result<Vec<Vec<u8>>, String> {
        if seq < self.next {
            return Ok(Vec::new());
        }
        if seq >= self.next + MAX_REORDER {
            return Err("Terminal input arrived out of order".into());
        }
        self.held.insert(seq, data);
        let mut ready = Vec::new();
        while let Some(data) = self.held.remove(&self.next) {
            ready.push(data);
            self.next += 1;
        }
        Ok(ready)
    }
}

/// The phone following a shell, and how much of its output it has.
struct Attached {
    watch: String,
    sent: u64,
    /// The first frame says where input continues and whether to clear the screen.
    hello: Option<bool>,
    sent_exit: bool,
}

struct Shell {
    device: String,
    name: String,
    started: Instant,
    idle_since: Option<Instant>,
    output: Output,
    inputs: Inputs,
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    attached: Option<Attached>,
    exit: Option<i32>,
}
impl Shell {
    fn kill(&mut self) {
        let _ = self.child.kill();
    }
}

/// What Settings lists for an open shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Open {
    pub id: String,
    pub device: String,
    pub name: String,
    pub seconds: u64,
    pub ended: bool,
}

/// The shells phones have open, by shell ID.
#[derive(Clone, Default)]
pub(crate) struct Terminals {
    shells: Arc<Mutex<HashMap<String, Shell>>>,
    pump: Arc<Mutex<bool>>,
    wake: Arc<Wake>,
}

/// Where a phone's terminal opens: the project (or one of its worktrees).
pub(crate) struct Opening {
    pub watch: String,
    pub device: String,
    pub cwd: String,
    pub shell: Option<String>,
    pub after: Option<u64>,
    pub cols: u16,
    pub rows: u16,
}

impl Terminals {
    /// Starts a shell, or reattaches to the device's own. Blocks on the PTY.
    pub fn open(&self, host: &MobileHost, opening: Opening) -> Result<(), String> {
        let mut shells = self.shells.lock().map_err(|_| "Terminals unavailable")?;
        if let Some(id) = &opening.shell {
            let shell = shells
                .get_mut(id)
                .filter(|shell| shell.device == opening.device)
                .ok_or("That terminal has ended")?;
            let from = opening.after;
            shell.attach(opening.watch, from);
            drop(shells);
            self.start_pump(host);
            return Ok(());
        }
        // A phone's abandoned shells make room for a new one, oldest first.
        let own: Vec<(String, Instant, bool)> = shells
            .iter()
            .filter(|(_, shell)| shell.device == opening.device)
            .map(|(id, shell)| (id.clone(), shell.started, shell.attached.is_some()))
            .collect();
        if own.len() >= MAX_PER_DEVICE {
            let oldest = own
                .iter()
                .filter(|(_, _, attached)| !attached)
                .min_by_key(|(_, started, _)| *started)
                .ok_or("Close a terminal first")?;
            if let Some(mut gone) = shells.remove(&oldest.0) {
                gone.kill();
            }
        }
        if shells.len() >= MAX_SHELLS {
            return Err("The desktop has too many terminals open".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let mut shell = spawn(&opening.cwd, opening.device, opening.cols, opening.rows)?;
        let reader = shell.reader.take().ok_or("Terminal unavailable")?;
        shell.shell.attach(opening.watch, None);
        shells.insert(id.clone(), shell.shell);
        drop(shells);
        let shells = Arc::downgrade(&self.shells);
        let wake = self.wake.clone();
        std::thread::Builder::new()
            .name("monocode-mobile-terminal".into())
            .spawn(move || read(reader, shells, id, wake))
            .map_err(|error| error.to_string())?;
        self.start_pump(host);
        Ok(())
    }

    /// The phone stopped following; the shell stays for it to come back to.
    pub fn detach(&self, watch: &str) {
        let Ok(mut shells) = self.shells.lock() else {
            return;
        };
        for shell in shells.values_mut() {
            if shell
                .attached
                .as_ref()
                .is_some_and(|attached| attached.watch == watch)
            {
                shell.attached = None;
                shell.idle_since = Some(Instant::now());
            }
        }
    }

    pub fn input(&self, device: &str, id: &str, seq: u64, data: &str) -> Result<Value, String> {
        let data = STANDARD
            .decode(data)
            .map_err(|_| "Terminal input is not valid")?;
        if data.len() > MAX_INPUT {
            return Err("Terminal input is too large".into());
        }
        let mut shells = self.shells.lock().map_err(|_| "Terminals unavailable")?;
        let shell = own(&mut shells, device, id)?;
        if shell.exit.is_some() {
            return Err("That terminal has ended".into());
        }
        for chunk in shell.inputs.accept(seq, data)? {
            shell
                .writer
                .write_all(&chunk)
                .and_then(|()| shell.writer.flush())
                .map_err(|error| format!("Could not write to the terminal: {error}"))?;
        }
        Ok(json!({ "next": shell.inputs.next }))
    }

    pub fn resize(&self, device: &str, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let mut shells = self.shells.lock().map_err(|_| "Terminals unavailable")?;
        own(&mut shells, device, id)?
            .master
            .resize(size(cols, rows))
            .map_err(|error| format!("Could not resize the terminal: {error}"))
    }

    /// The phone closes one of its own shells.
    pub fn close(&self, device: &str, id: &str) -> Result<(), String> {
        let mut shells = self.shells.lock().map_err(|_| "Terminals unavailable")?;
        own(&mut shells, device, id)?;
        if let Some(mut shell) = shells.remove(id) {
            shell.kill();
        }
        Ok(())
    }

    /// Ends one device's shells, or every shell: terminals were turned off or
    /// a phone was revoked.
    pub fn close_for(&self, device: Option<&str>) {
        let Ok(mut shells) = self.shells.lock() else {
            return;
        };
        shells.retain(|_, shell| {
            let ends = device.is_none_or(|device| shell.device == device);
            if ends {
                shell.kill();
            }
            !ends
        });
    }

    /// Ends a shell from Settings.
    pub fn kill(&self, id: &str) {
        if let Ok(mut shells) = self.shells.lock()
            && let Some(mut shell) = shells.remove(id)
        {
            shell.kill();
        }
    }

    pub fn list(&self) -> Vec<Open> {
        let Ok(shells) = self.shells.lock() else {
            return Vec::new();
        };
        let mut open: Vec<Open> = shells
            .iter()
            .map(|(id, shell)| Open {
                id: id.clone(),
                device: shell.device.clone(),
                name: shell.name.clone(),
                seconds: shell.started.elapsed().as_secs(),
                ended: shell.exit.is_some(),
            })
            .collect();
        open.sort_by(|a, b| b.seconds.cmp(&a.seconds).then_with(|| a.id.cmp(&b.id)));
        open
    }

    /// Sends each attached phone the output it has not had, and clears out
    /// shells nobody came back to. Runs until the service is dropped.
    fn start_pump(&self, host: &MobileHost) {
        let Ok(mut started) = self.pump.lock() else {
            return;
        };
        if std::mem::replace(&mut *started, true) {
            return;
        }
        let shells = Arc::downgrade(&self.shells);
        let host = host.clone();
        let wake = self.wake.clone();
        let spawned = std::thread::Builder::new()
            .name("monocode-mobile-terminal-pump".into())
            .spawn(move || {
                let mut last = Instant::now();
                loop {
                    wake.wait(TICK);
                    if let Some(rest) = MIN_GAP.checked_sub(last.elapsed()) {
                        std::thread::sleep(rest);
                    }
                    last = Instant::now();
                    let Some(shells) = shells.upgrade() else {
                        return;
                    };
                    let Ok(mut shells) = shells.lock() else {
                        return;
                    };
                    flush(&host, &mut shells);
                    shells.retain(|_, shell| {
                        let keep = shell.idle_since.is_none_or(|since| {
                            since.elapsed()
                                < if shell.exit.is_some() {
                                    ENDED_TTL
                                } else {
                                    DETACHED_TTL
                                }
                        });
                        if !keep {
                            shell.kill();
                        }
                        keep
                    });
                }
            });
        if spawned.is_err() {
            *started = false;
        }
    }
}

fn own<'a>(
    shells: &'a mut HashMap<String, Shell>,
    device: &str,
    id: &str,
) -> Result<&'a mut Shell, String> {
    shells
        .get_mut(id)
        .filter(|shell| shell.device == device)
        .ok_or_else(|| "That terminal has ended".to_string())
}

impl Shell {
    /// Follows the shell from `after`, or from the start of what is kept. The
    /// phone clears its screen unless it carries on from where it was.
    fn attach(&mut self, watch: String, after: Option<u64>) {
        let resumes =
            after.is_some_and(|after| after >= self.output.start && after <= self.output.end());
        self.attached = Some(Attached {
            watch,
            sent: if resumes {
                after.unwrap_or_default()
            } else {
                self.output.start
            },
            hello: Some(!resumes),
            sent_exit: false,
        });
        self.idle_since = None;
    }
}

fn flush(host: &MobileHost, shells: &mut HashMap<String, Shell>) {
    for (id, shell) in shells.iter_mut() {
        let Some(attached) = shell.attached.as_mut() else {
            continue;
        };
        loop {
            let (from, chunk) = shell.output.from(attached.sent, FRAME);
            let end = from + chunk.len() as u64;
            let hello = attached.hello;
            let finished = shell
                .exit
                .filter(|_| end == shell.output.end() && !attached.sent_exit);
            if chunk.is_empty() && hello.is_none() && finished.is_none() {
                break;
            }
            let mut frame = json!({ "shellId": id, "seq": end });
            if !chunk.is_empty() {
                frame["data"] = Value::String(STANDARD.encode(chunk));
            }
            if let Some(reset) = hello {
                frame["reset"] = Value::Bool(reset);
                frame["next"] = json!(shell.inputs.next);
                frame["from"] = json!(from);
            }
            if let Some(code) = finished {
                frame["exit"] = json!(code);
            }
            match host.push(&attached.watch, frame) {
                Ok(()) => {
                    attached.sent = end;
                    attached.hello = None;
                    attached.sent_exit |= finished.is_some();
                    if chunk.len() < FRAME {
                        break;
                    }
                }
                // The phone is behind: the next tick sends from the same offset.
                Err(Push::Busy) => break,
                Err(Push::Gone) => {
                    shell.attached = None;
                    shell.idle_since = Some(Instant::now());
                    break;
                }
            }
        }
    }
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.clamp(5, 200),
        cols: cols.clamp(20, 400),
        pixel_width: 0,
        pixel_height: 0,
    }
}

struct Spawned {
    shell: Shell,
    reader: Option<Box<dyn Read + Send>>,
}

/// The login shell in `cwd`, with the environment the desktop's own terminal gives.
fn spawn(cwd: &str, device: String, cols: u16, rows: u16) -> Result<Spawned, String> {
    let pair = native_pty_system()
        .openpty(size(cols, rows))
        .map_err(|error| format!("Could not open a terminal: {error}"))?;
    let workdir = crate::terminal::working_dir(cwd);
    let (program, args) = crate::terminal::default_shell();
    let mut command = CommandBuilder::new(&program);
    command.args(&args);
    command.cwd(&workdir);
    for (key, value) in [
        ("TERM", "xterm-256color".to_owned()),
        ("COLORTERM", "truecolor".to_owned()),
        ("TERM_PROGRAM", "MonoCode".to_owned()),
        ("PATH", monocode_core::scm::terminal_search_path()),
        ("PWD", workdir.to_string_lossy().into_owned()),
    ] {
        command.env(key, value);
    }
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| format!("Could not start the shell: {error}"))?;
    drop(pair.slave);
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| error.to_string())?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| error.to_string())?;
    let name = Path::new(&workdir)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Terminal".into());
    Ok(Spawned {
        shell: Shell {
            device,
            name,
            started: Instant::now(),
            idle_since: None,
            output: Output::default(),
            inputs: Inputs {
                next: 1,
                held: BTreeMap::new(),
            },
            writer,
            master: pair.master,
            child,
            attached: None,
            exit: None,
        },
        reader: Some(reader),
    })
}

/// Collects a shell's output until it ends, then records how.
fn read(
    mut reader: Box<dyn Read + Send>,
    shells: Weak<Mutex<HashMap<String, Shell>>>,
    id: String,
    wake: Arc<Wake>,
) {
    let mut buffer = [0u8; 8192];
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        let Some(shells) = shells.upgrade() else {
            return;
        };
        let Ok(mut shells) = shells.lock() else {
            return;
        };
        // Closed from the phone or Settings.
        let Some(shell) = shells.get_mut(&id) else {
            return;
        };
        shell.output.push(&buffer[..count]);
        drop(shells);
        wake.ring();
    }
    for _ in 0..50 {
        let Some(shells) = shells.upgrade() else {
            return;
        };
        let Ok(mut shells) = shells.lock() else {
            return;
        };
        let Some(shell) = shells.get_mut(&id) else {
            return;
        };
        if let Ok(Some(status)) = shell.child.try_wait() {
            shell.exit = Some(status.exit_code() as i32);
            shell.idle_since.get_or_insert_with(Instant::now);
            return;
        }
        drop(shells);
        std::thread::sleep(Duration::from_millis(100));
    }
    if let Some(shells) = shells.upgrade()
        && let Ok(mut shells) = shells.lock()
        && let Some(shell) = shells.get_mut(&id)
    {
        shell.exit = Some(0);
        shell.idle_since.get_or_insert_with(Instant::now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_keeps_the_latest_bytes_by_offset() {
        let mut output = Output::default();
        output.push(b"hello ");
        output.push(b"world");
        assert_eq!(output.from(0, 100), (0, &b"hello world"[..]));
        assert_eq!(output.from(6, 100), (6, &b"world"[..]));
        assert_eq!(output.from(6, 3), (6, &b"wor"[..]));
        // Offsets past the end are the end: nothing new.
        assert_eq!(output.from(99, 100), (11, &b""[..]));
        output.push(&vec![b'x'; KEEP]);
        assert_eq!(output.start, 11);
        assert_eq!(output.end(), 11 + KEEP as u64);
        // A phone that was away longer than we kept starts from what is left.
        assert_eq!(output.from(0, 4).0, 11);
        assert_eq!(output.from(0, 4).1, b"xxxx");
    }

    #[test]
    fn keystrokes_apply_once_and_in_order() {
        let mut inputs = Inputs {
            next: 5,
            held: BTreeMap::new(),
        };
        assert!(inputs.accept(7, b"c".to_vec()).unwrap().is_empty());
        assert!(inputs.accept(6, b"b".to_vec()).unwrap().is_empty());
        assert_eq!(
            inputs.accept(5, b"a".to_vec()).unwrap(),
            vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]
        );
        assert_eq!(inputs.next, 8);
        // A retry of one already applied does nothing.
        assert!(inputs.accept(6, b"b".to_vec()).unwrap().is_empty());
        assert!(inputs.accept(8 + MAX_REORDER, b"z".to_vec()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_shell_runs_what_the_phone_types_and_only_for_its_own_device() {
        let dir = std::env::temp_dir();
        let terminals = Terminals::default();
        let spawned = spawn(&dir.to_string_lossy(), "phone".into(), 80, 24).unwrap();
        let mut shell = spawned.shell;
        let reader = spawned.reader.unwrap();
        shell.attached = None;
        terminals.shells.lock().unwrap().insert("s".into(), shell);
        let weak = Arc::downgrade(&terminals.shells);
        std::thread::spawn(move || read(reader, weak, "s".into(), Arc::default()));

        let typed = STANDARD.encode("echo mono$((20+22))\n");
        assert!(terminals.input("other", "s", 1, &typed).is_err());
        assert_eq!(terminals.input("phone", "s", 1, &typed).unwrap()["next"], 2);
        let deadline = Instant::now() + Duration::from_secs(10);
        let seen = loop {
            let text = {
                let shells = terminals.shells.lock().unwrap();
                String::from_utf8_lossy(&shells["s"].output.bytes).into_owned()
            };
            if text.contains("mono42") || Instant::now() > deadline {
                break text;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert!(seen.contains("mono42"), "shell output was {seen:?}");

        terminals
            .resize("phone", "s", 100, 30)
            .expect("resize the shell");
        assert!(terminals.close("other", "s").is_err());
        assert_eq!(terminals.list().len(), 1);
        terminals.close("phone", "s").unwrap();
        assert!(terminals.list().is_empty());
    }
}
