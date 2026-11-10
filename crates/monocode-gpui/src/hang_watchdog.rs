//! Sample the process while a UI stall is happening, before a restart erases
//! the blocked stacks. A dedicated thread works even if GPUI's workers stall.
use gpui::App;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

const PERIOD: Duration = Duration::from_secs(1);
const STALL: Duration = Duration::from_secs(5);
const COOLDOWN: Duration = Duration::from_secs(60);
const MAX_REPORTS: usize = 5;

pub(crate) fn start(directory: PathBuf, cx: &mut App) {
    if cx.is_test() {
        return;
    }
    let epoch = Instant::now();
    let heartbeat = Arc::new(AtomicU64::new(0));
    let beat = heartbeat.clone();
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(PERIOD).await;
            cx.update(|_| beat.store(epoch.elapsed().as_secs(), Ordering::Relaxed));
        }
    })
    .detach();
    let (stop, stopped) = mpsc::channel();
    cx.on_app_quit(move |_| {
        let _ = stop.send(());
        async {}
    })
    .detach();
    if let Err(error) = std::thread::Builder::new()
        .name("ui-watchdog".into())
        .spawn(move || {
            let mut watch = Watch::default();
            while let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(PERIOD) {
                let now = epoch.elapsed();
                let beat = Duration::from_secs(heartbeat.load(Ordering::Relaxed));
                if watch.poll(now, beat) {
                    capture(&directory);
                }
            }
        })
    {
        eprintln!("Could not start UI watchdog: {error}");
    }
}

#[derive(Default)]
struct Watch {
    polled: Duration,
    captured: Option<Duration>,
    heartbeat: Option<Duration>,
    /// One report per stall; a resumed heartbeat arms the watchdog again.
    reported: bool,
}
impl Watch {
    fn poll(&mut self, now: Duration, heartbeat: Duration) -> bool {
        let paused = now.saturating_sub(self.polled) > PERIOD * 3;
        self.polled = now;
        if self.heartbeat != Some(heartbeat) {
            self.heartbeat = Some(heartbeat);
            self.reported = false;
        }
        if paused {
            // After system sleep, give the main thread a chance to resume.
            self.reported = true;
        }
        if !self.reported
            && now.saturating_sub(heartbeat) >= STALL
            && self
                .captured
                .is_none_or(|at| now.saturating_sub(at) >= COOLDOWN)
        {
            self.captured = Some(now);
            self.reported = true;
            return true;
        }
        false
    }
}

fn capture(directory: &Path) {
    let result = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(directory)?;
        let mut reports = std::fs::read_dir(directory)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("ui-hang-"))
                    && path.extension().is_some_and(|ext| ext == "txt")
            })
            .collect::<Vec<_>>();
        reports.sort();
        let keep = reports.len().saturating_sub(MAX_REPORTS - 1);
        for path in reports.into_iter().take(keep) {
            let _ = std::fs::remove_file(path);
        }
        let path = directory.join(format!(
            "ui-hang-{}-{}.txt",
            chrono::Local::now().format("%Y%m%d-%H%M%S"),
            std::process::id()
        ));
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        let status = std::process::Command::new("/usr/bin/sample")
            .args([
                std::process::id().to_string(),
                "2".into(),
                "10".into(),
                "-file".into(),
            ])
            .arg(&path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        if !status.success() {
            let _ = std::fs::remove_file(&path);
            return Err(std::io::Error::other(format!(
                "sample exited with {status}"
            )));
        }
        eprintln!(
            "UI stalled for at least {}s; saved {}",
            STALL.as_secs(),
            path.display()
        );
        Ok(())
    })();
    if let Err(error) = result {
        eprintln!("Could not sample UI stall: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seconds(value: u64) -> Duration {
        Duration::from_secs(value)
    }

    #[test]
    fn healthy_heartbeats_do_not_capture_and_a_stall_is_sampled_once() {
        let mut watch = Watch::default();
        for time in 1..10 {
            assert!(!watch.poll(seconds(time), seconds(time)));
        }
        for time in 10..14 {
            assert!(!watch.poll(seconds(time), seconds(9)));
        }
        assert!(watch.poll(seconds(14), seconds(9)));
        for time in 15..100 {
            assert!(!watch.poll(seconds(time), seconds(9)));
        }
        assert!(!watch.poll(seconds(100), seconds(100)));
        for time in 101..105 {
            assert!(!watch.poll(seconds(time), seconds(100)));
        }
        assert!(watch.poll(seconds(105), seconds(100)));
    }

    #[test]
    fn sleep_and_repeated_short_stalls_do_not_flood_reports() {
        let mut watch = Watch::default();
        assert!(!watch.poll(seconds(1), seconds(1)));
        assert!(!watch.poll(seconds(600), seconds(1)));
        assert!(!watch.poll(seconds(601), seconds(601)));
        for time in 602..606 {
            assert!(!watch.poll(seconds(time), seconds(601)));
        }
        assert!(watch.poll(seconds(606), seconds(601)));
        assert!(!watch.poll(seconds(607), seconds(607)));
        for time in 608..666 {
            assert!(!watch.poll(seconds(time), seconds(607)));
        }
        assert!(watch.poll(seconds(666), seconds(607)));
    }

    #[test]
    fn captures_private_stack_reports_and_keeps_only_five() {
        let directory = std::env::temp_dir().join(format!(
            "monocode-watchdog-test-{}",
            monocode_core::mono::new_id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        for i in 0..MAX_REPORTS {
            std::fs::write(
                directory.join(format!("ui-hang-20000101-00000{i}.txt")),
                "old report",
            )
            .unwrap();
        }
        let other = directory.join("unrelated.txt");
        std::fs::write(&other, "keep").unwrap();
        capture(&directory);
        let reports = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("ui-hang-")
            })
            .collect::<Vec<_>>();
        assert_eq!(reports.len(), MAX_REPORTS);
        let report = reports
            .iter()
            .find(|path| {
                !path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("ui-hang-2000")
            })
            .expect("watchdog did not save a stack sample");
        let sample = std::fs::read_to_string(report).unwrap();
        assert!(
            sample.contains("Call graph:"),
            "missing stack sample: {sample}"
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(report).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(std::fs::read_to_string(&other).unwrap(), "keep");
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
