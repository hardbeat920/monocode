//! Keeps GPUI's own warnings. GPUI reports failed draws, stalled frames and
//! display link restarts through the `log` crate, and without a logger those
//! reports were dropped, so a window that stopped repainting left no trace.
//! Warnings from every crate, plus the macOS window's info lines, go to
//! `~/Library/Logs/Monocode Native/gpui.log`.
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

const MAX_BYTES: u64 = 4 * 1024 * 1024;
/// A failing draw logs every frame; keep one line per message per period.
const REPEAT: Duration = Duration::from_secs(1);

struct FileLog {
    path: PathBuf,
    state: Mutex<State>,
}

struct State {
    file: Option<File>,
    recent: HashMap<String, (Instant, u32)>,
}

pub(crate) fn init() {
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let directory = PathBuf::from(home).join("Library/Logs/Monocode Native");
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    let path = directory.join("gpui.log");
    rotate(&path);
    let logger = FileLog {
        state: Mutex::new(State {
            file: open(&path),
            recent: HashMap::new(),
        }),
        path,
    };
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

fn open(path: &PathBuf) -> Option<File> {
    OpenOptions::new().create(true).append(true).open(path).ok()
}

fn rotate(path: &PathBuf) {
    if std::fs::metadata(path).is_ok_and(|meta| meta.len() > MAX_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
}

fn wanted(metadata: &log::Metadata) -> bool {
    metadata.level() <= log::Level::Warn
        || (metadata.level() <= log::Level::Info && metadata.target().starts_with("gpui_macos"))
}

impl log::Log for FileLog {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        wanted(metadata)
    }

    fn log(&self, record: &log::Record) {
        if !wanted(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let now = Instant::now();
        let repeats = match state.recent.get_mut(&message) {
            Some((last, suppressed)) if now.duration_since(*last) < REPEAT => {
                *suppressed += 1;
                return;
            }
            Some((last, suppressed)) => {
                *last = now;
                std::mem::take(suppressed)
            }
            None => {
                if state.recent.len() > 256 {
                    state
                        .recent
                        .retain(|_, (last, _)| now.duration_since(*last) < REPEAT);
                }
                state.recent.insert(message.clone(), (now, 0));
                0
            }
        };
        if state
            .file
            .as_ref()
            .and_then(|file| file.metadata().ok())
            .is_some_and(|meta| meta.len() > MAX_BYTES)
        {
            state.file = None;
            rotate(&self.path);
            state.file = open(&self.path);
        }
        let Some(file) = state.file.as_mut() else {
            return;
        };
        let repeated = if repeats > 0 {
            format!(" (+{repeats} repeats)")
        } else {
            String::new()
        };
        let _ = writeln!(
            file,
            "{} {:<5} {}: {message}{repeated}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            record.level(),
            record.target(),
        );
    }

    fn flush(&self) {
        if let Ok(mut state) = self.state.lock()
            && let Some(file) = state.file.as_mut()
        {
            let _ = file.flush();
        }
    }
}
