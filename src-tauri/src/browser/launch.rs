//! The MonoCode browser (`BrowserMode::Managed`): an installed Chromium
//! browser (Chrome, Edge, Brave, Vivaldi, Opera or Arc) started by MonoCode with a profile of its own, so agents get a browser
//! without the user's remote-debugging setup or consent dialog.
//!
//! The browser runs with `--remote-debugging-port=0` on its own profile
//! folder (Chrome 136+ refuses that switch only for the default profile) and
//! writes `DevToolsActivePort` there; the link then connects exactly as it
//! does to the user's browser. Logins made in the kept profile stay on disk;
//! the private MonoCode browser (`BrowserMode::Private`) gets a fresh profile
//! per start, deleted once it has exited.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::discovery::{self, ActivePort, Candidate, Platform, ACTIVE_PORT_FILE};
use super::protocol::{BrowserChoice, BrowserKind};

/// How long a freshly started browser may take to open its debugging port.
pub const START_TIMEOUT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(100);
/// Channel id reported for the MonoCode browser in `BrowserInfo`.
pub const CHANNEL: &str = "monocode";

/// An installed browser MonoCode can start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Executable {
    pub kind: BrowserKind,
    pub path: PathBuf,
}

/// The profile folder of the MonoCode browser.
pub fn profile_dir(app_data: &Path) -> PathBuf {
    app_data.join("browser-profile")
}

/// A fresh profile folder for one start of the private MonoCode browser,
/// next to the kept profile.
pub fn private_dir(profile: &Path) -> PathBuf {
    private_root(profile).join(uuid::Uuid::new_v4().simple().to_string())
}

fn private_root(profile: &Path) -> PathBuf {
    profile.parent().unwrap_or(profile).join("browser-private")
}

/// Written into the profile at start: the process id of the browser MonoCode
/// started on it, so the next run can stop one that a crash left running.
const PID_FILE: &str = "monocode-browser.pid";

/// Startup: private profiles a crash left behind, and the browsers still
/// running on them (a headless one never exits by itself).
pub fn clear_private(profile: &Path) {
    let root = private_root(profile);
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            stop_orphan(&entry.path());
        }
    }
    let _ = std::fs::remove_dir_all(root);
}

/// Stop the browser an earlier run started on `profile`, when the recorded
/// process still runs on exactly that profile: a reused process id never
/// belongs to another program or to the user's own browser.
#[cfg(unix)]
fn stop_orphan(profile: &Path) {
    let Some(pid) = std::fs::read_to_string(profile.join(PID_FILE))
        .ok()
        .and_then(|text| text.trim().parse::<libc::pid_t>().ok())
        .filter(|pid| *pid > 1)
    else {
        return;
    };
    if !runs_on_profile(pid, profile) {
        return;
    }
    let alive = || unsafe { libc::kill(pid, 0) } == 0;
    unsafe { libc::kill(pid, libc::SIGTERM) };
    let deadline = Instant::now() + ORPHAN_WAIT;
    while alive() && Instant::now() < deadline {
        std::thread::sleep(POLL);
    }
    if alive() {
        unsafe { libc::kill(pid, libc::SIGKILL) };
        let deadline = Instant::now() + ORPHAN_WAIT;
        while alive() && Instant::now() < deadline {
            std::thread::sleep(POLL);
        }
    }
}

/// Windows needs nothing here: [`start`] puts the browser in the app's
/// kill-on-close job, so the system ends it when the app's process ends.
#[cfg(not(unix))]
fn stop_orphan(_profile: &Path) {}

/// How long a browser from an earlier run gets to exit after each signal.
#[cfg(unix)]
const ORPHAN_WAIT: Duration = Duration::from_secs(3);

/// Whether process `pid` was started with `--user-data-dir=<profile>`.
#[cfg(unix)]
fn runs_on_profile(pid: libc::pid_t, profile: &Path) -> bool {
    let Ok(output) = Command::new("ps")
        .args(["-ww", "-o", "args=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return false;
    };
    output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .contains(&format!("--user-data-dir={}", profile.display()))
}

/// The discovery candidate for the MonoCode profile, so a browser that is
/// still running from earlier is found the same way as the user's.
pub fn candidate(profile: &Path, kind: BrowserKind) -> Candidate {
    Candidate {
        channel: CHANNEL.into(),
        kind,
        user_data_dir: profile.to_path_buf(),
    }
}

/// The live endpoint of a MonoCode browser that already runs, if any.
pub fn running(profile: &Path) -> Option<ActivePort> {
    let active = discovery::read_active_port(profile)?;
    discovery::port_accepts(active.port, discovery::PROBE_TIMEOUT).then_some(active)
}

/// Executables to try for `choice`, in order. `Auto` and a custom profile
/// folder look for Chrome, then Edge, then the other Chromium browsers; a
/// channel looks for that browser.
fn wanted(choice: &BrowserChoice) -> Vec<BrowserKind> {
    match choice {
        BrowserChoice::Channel { channel } => vec![discovery::channel_kind(channel)],
        BrowserChoice::Auto | BrowserChoice::UserDataDir { .. } => AUTO.to_vec(),
    }
}

const AUTO: [BrowserKind; 6] = [
    BrowserKind::Chrome,
    BrowserKind::Edge,
    BrowserKind::Brave,
    BrowserKind::Vivaldi,
    BrowserKind::Opera,
    BrowserKind::Arc,
];

/// Well-known install locations per platform. `env` reads environment
/// variables (a seam for tests).
pub fn known_paths(
    kind: BrowserKind,
    os: Platform,
    env: impl Fn(&str) -> Option<PathBuf>,
) -> Vec<PathBuf> {
    match os {
        Platform::Windows => {
            let relatives: &[&[&str]] = match kind {
                BrowserKind::Edge => &[&["Microsoft", "Edge", "Application", "msedge.exe"]],
                BrowserKind::Chrome | BrowserKind::Chromium => {
                    &[&["Google", "Chrome", "Application", "chrome.exe"]]
                }
                BrowserKind::Brave => {
                    &[&["BraveSoftware", "Brave-Browser", "Application", "brave.exe"]]
                }
                BrowserKind::Vivaldi => &[&["Vivaldi", "Application", "vivaldi.exe"]],
                // The per-user install lives under Local\Programs; opera.exe
                // is a launcher that passes the switches on.
                BrowserKind::Opera => {
                    &[&["Programs", "Opera", "opera.exe"], &["Opera", "opera.exe"]]
                }
                // Arc for Windows is a store package with no stable path.
                BrowserKind::Arc => &[],
            };
            ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
                .iter()
                .filter_map(|var| env(var))
                .flat_map(|base| {
                    relatives.iter().map(move |relative| {
                        relative
                            .iter()
                            .fold(base.clone(), |path, part| path.join(part))
                    })
                })
                .collect()
        }
        Platform::MacOs => {
            let app = match kind {
                BrowserKind::Edge => "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
                BrowserKind::Chrome | BrowserKind::Chromium => {
                    "Google Chrome.app/Contents/MacOS/Google Chrome"
                }
                BrowserKind::Brave => "Brave Browser.app/Contents/MacOS/Brave Browser",
                BrowserKind::Vivaldi => "Vivaldi.app/Contents/MacOS/Vivaldi",
                BrowserKind::Opera => "Opera.app/Contents/MacOS/Opera",
                BrowserKind::Arc => "Arc.app/Contents/MacOS/Arc",
            };
            let mut paths = vec![Path::new("/Applications").join(app)];
            if let Some(home) = env("HOME") {
                paths.push(home.join("Applications").join(app));
            }
            paths
        }
        Platform::Linux => {
            let names: &[&str] = match kind {
                BrowserKind::Edge => &["microsoft-edge", "microsoft-edge-stable"],
                BrowserKind::Chrome => &["google-chrome", "google-chrome-stable"],
                BrowserKind::Chromium => &["chromium", "chromium-browser"],
                BrowserKind::Brave => &["brave-browser", "brave-browser-stable", "brave"],
                BrowserKind::Vivaldi => &["vivaldi", "vivaldi-stable"],
                BrowserKind::Opera => &["opera"],
                BrowserKind::Arc => &[],
            };
            let dirs: Vec<PathBuf> = env("PATH")
                .map(|path| std::env::split_paths(&path).collect())
                .unwrap_or_default();
            names
                .iter()
                .flat_map(|name| dirs.iter().map(move |dir| dir.join(name)))
                .collect()
        }
    }
}

/// The first installed browser for `choice`.
pub fn find_executable(choice: &BrowserChoice) -> Option<Executable> {
    let os = Platform::current();
    let mut kinds = wanted(choice);
    // Linux distributions ship Chromium itself; it comes right after Chrome
    // and Edge.
    if os == Platform::Linux {
        if let Some(at) = kinds
            .iter()
            .rposition(|kind| matches!(kind, BrowserKind::Chrome | BrowserKind::Edge))
        {
            kinds.insert(at + 1, BrowserKind::Chromium);
        }
    }
    kinds.into_iter().find_map(|kind| {
        known_paths(kind, os, |var| std::env::var_os(var).map(PathBuf::from))
            .into_iter()
            .find(|path| path.is_file())
            .map(|path| Executable { kind, path })
    })
}

/// Command-line switches for the MonoCode browser.
pub fn arguments(profile: &Path, show_window: bool) -> Vec<String> {
    let mut args = vec![
        format!("--user-data-dir={}", profile.display()),
        "--remote-debugging-port=0".into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
    ];
    if !show_window {
        args.push("--headless=new".into());
    }
    args.push("about:blank".into());
    args
}

/// Start the browser. A leftover `DevToolsActivePort` from an earlier run
/// goes first, so [`wait_for_endpoint`] only sees the new one.
pub fn start(exe: &Executable, profile: &Path, show_window: bool) -> std::io::Result<Child> {
    std::fs::create_dir_all(profile)?;
    let _ = std::fs::remove_file(profile.join(ACTIVE_PORT_FILE));
    let mut command = Command::new(&exe.path);
    command
        .args(arguments(profile, show_window))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = command.spawn()?;
    // Windows: the browser ends with the app even after a crash. Best
    // effort; without the job, OFF and app exit still stop it.
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let _ = crate::windows::assign_child(child.as_raw_handle());
    }
    let _ = std::fs::write(profile.join(PID_FILE), child.id().to_string());
    Ok(child)
}

/// The started browser process, as [`wait_for_endpoint`] polls it.
pub enum Started {
    Running,
    Exited(String),
    /// No longer this start's to wait for (OFF, a mode change).
    Gone,
}

/// After the started process exited: how long another process on the same
/// profile may take to come up. Chrome hands a second start of a running
/// profile to the first process and exits at once.
const HANDOVER_GRACE: Duration = Duration::from_secs(2);

/// Wait until the started browser answers on its debugging port, or it
/// exits (plus [`HANDOVER_GRACE`]), or is gone, or `timeout` passes.
pub fn wait_for_endpoint(
    profile: &Path,
    timeout: Duration,
    mut started: impl FnMut() -> Started,
) -> Result<ActivePort, String> {
    let deadline = Instant::now() + timeout;
    let mut exited: Option<(String, Instant)> = None;
    loop {
        if let Some(active) = running(profile) {
            return Ok(active);
        }
        if exited.is_none() {
            match started() {
                Started::Running => {}
                Started::Exited(status) => {
                    exited = Some((status, (Instant::now() + HANDOVER_GRACE).min(deadline)));
                }
                Started::Gone => return Err("The browser start was cancelled.".into()),
            }
        }
        if let Some((status, until)) = &exited {
            if Instant::now() >= *until {
                return Err(format!("The browser exited ({status})."));
            }
        }
        if Instant::now() >= deadline {
            return Err("The browser did not open its debugging port in time.".into());
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths_cover_both_program_files_and_the_user_install() {
        let env = |var: &str| match var {
            "ProgramFiles" => Some(PathBuf::from(r"C:\Program Files")),
            "ProgramFiles(x86)" => Some(PathBuf::from(r"C:\Program Files (x86)")),
            "LOCALAPPDATA" => Some(PathBuf::from(r"C:\Users\o\AppData\Local")),
            _ => None,
        };
        let chrome = known_paths(BrowserKind::Chrome, Platform::Windows, env);
        assert_eq!(chrome.len(), 3);
        assert_eq!(
            chrome[0],
            PathBuf::from(r"C:\Program Files")
                .join("Google")
                .join("Chrome")
                .join("Application")
                .join("chrome.exe")
        );
        assert!(chrome[2].starts_with(r"C:\Users\o\AppData\Local"));
        let edge = known_paths(BrowserKind::Edge, Platform::Windows, env);
        assert!(edge[1].ends_with(
            PathBuf::from("Microsoft")
                .join("Edge")
                .join("Application")
                .join("msedge.exe")
        ));
    }

    #[test]
    fn mac_and_linux_paths() {
        let env = |var: &str| match var {
            "HOME" => Some(PathBuf::from("/Users/o")),
            "PATH" => Some(PathBuf::from("/usr/bin")),
            _ => None,
        };
        let mac = known_paths(BrowserKind::Edge, Platform::MacOs, env);
        assert_eq!(
            mac[0],
            PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge")
        );
        assert_eq!(mac.len(), 2);
        let linux = known_paths(BrowserKind::Chrome, Platform::Linux, env);
        assert_eq!(linux[0], PathBuf::from("/usr/bin/google-chrome"));
        assert_eq!(
            known_paths(BrowserKind::Arc, Platform::MacOs, env)[0],
            PathBuf::from("/Applications/Arc.app/Contents/MacOS/Arc")
        );
        assert_eq!(
            known_paths(BrowserKind::Brave, Platform::Linux, env)[0],
            PathBuf::from("/usr/bin/brave-browser")
        );
        assert!(known_paths(BrowserKind::Arc, Platform::Linux, env).is_empty());
    }

    #[test]
    fn windows_paths_of_the_other_chromium_browsers() {
        let env = |var: &str| match var {
            "ProgramFiles" => Some(PathBuf::from(r"C:\Program Files")),
            "LOCALAPPDATA" => Some(PathBuf::from(r"C:\Users\o\AppData\Local")),
            _ => None,
        };
        let opera = known_paths(BrowserKind::Opera, Platform::Windows, env);
        assert!(opera.contains(
            &PathBuf::from(r"C:\Users\o\AppData\Local")
                .join("Programs")
                .join("Opera")
                .join("opera.exe")
        ));
        let brave = known_paths(BrowserKind::Brave, Platform::Windows, env);
        assert!(brave[0].ends_with(PathBuf::from("Application").join("brave.exe")));
        let vivaldi = known_paths(BrowserKind::Vivaldi, Platform::Windows, env);
        assert!(vivaldi[1].starts_with(r"C:\Users\o\AppData\Local"));
        assert!(known_paths(BrowserKind::Arc, Platform::Windows, env).is_empty());
    }

    #[test]
    fn choice_decides_which_browser_starts() {
        assert_eq!(
            wanted(&BrowserChoice::Auto)[..2],
            [BrowserKind::Chrome, BrowserKind::Edge]
        );
        assert_eq!(wanted(&BrowserChoice::Auto).len(), 6);
        assert_eq!(
            wanted(&BrowserChoice::Channel {
                channel: "brave".into()
            }),
            vec![BrowserKind::Brave]
        );
        assert_eq!(
            wanted(&BrowserChoice::Channel {
                channel: "msedge".into()
            }),
            vec![BrowserKind::Edge]
        );
    }

    #[test]
    fn a_browser_that_exits_at_once_is_reported_promptly() {
        let profile = std::env::temp_dir().join(format!("monocode-exit-{}", uuid::Uuid::new_v4()));
        let started = Instant::now();
        let error = wait_for_endpoint(&profile, START_TIMEOUT, || {
            Started::Exited("exit status: 1".into())
        })
        .unwrap_err();
        assert!(error.contains("exited"), "{error}");
        assert!(started.elapsed() < HANDOVER_GRACE + Duration::from_secs(2));
        let error = wait_for_endpoint(&profile, START_TIMEOUT, || Started::Gone).unwrap_err();
        assert!(error.contains("cancelled"), "{error}");
    }

    /// A private profile a crash left behind: the browser still running on
    /// it is stopped before the folder goes; a process on another profile
    /// with a recorded id is left alone.
    #[cfg(unix)]
    #[test]
    fn startup_stops_browsers_left_on_private_profiles() {
        let app = std::env::temp_dir().join(format!("monocode-orphan-{}", uuid::Uuid::new_v4()));
        let kept = profile_dir(&app);
        let orphan_profile = private_dir(&kept);
        let other_profile = private_dir(&kept);
        std::fs::create_dir_all(&orphan_profile).unwrap();
        std::fs::create_dir_all(&other_profile).unwrap();
        // `sh -c` with a second command keeps its own argv, as Chrome does.
        let spawn = |profile: &Path| {
            Command::new("sh")
                .args([
                    "-c",
                    "sleep 30; :",
                    "browser",
                    &format!("--user-data-dir={}", profile.display()),
                ])
                .spawn()
                .unwrap()
        };
        let mut orphan = spawn(&orphan_profile);
        std::fs::write(orphan_profile.join(PID_FILE), orphan.id().to_string()).unwrap();
        let mut stranger = spawn(&app.join("elsewhere"));
        std::fs::write(other_profile.join(PID_FILE), stranger.id().to_string()).unwrap();
        // The test process is the parent: reap so the exit becomes visible.
        let reaper = std::thread::spawn(move || orphan.wait().unwrap());
        clear_private(&kept);
        assert!(!reaper.join().unwrap().success());
        assert!(
            stranger.try_wait().unwrap().is_none(),
            "an unrelated process was stopped"
        );
        assert!(!private_root(&kept).exists());
        let _ = stranger.kill();
        let _ = stranger.wait();
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn arguments_use_the_own_profile_and_a_free_port() {
        let profile = Path::new("/data/browser-profile");
        let args = arguments(profile, true);
        assert_eq!(args[0], "--user-data-dir=/data/browser-profile");
        assert!(args.contains(&"--remote-debugging-port=0".to_string()));
        assert!(!args.iter().any(|a| a.starts_with("--headless")));
        assert!(!args.iter().any(|a| a == "--enable-automation"));
        assert!(arguments(profile, false).contains(&"--headless=new".to_string()));
        assert_eq!(profile_dir(Path::new("/data")), profile);
    }
}
