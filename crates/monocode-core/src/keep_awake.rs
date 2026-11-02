//! Keeps the Mac awake while agents work: when to hold it, and the macOS
//! commands that do (`caffeinate`, `pmset disablesleep`, the lid sensor).
//!
//! `caffeinate` cannot stop lid-close sleep; only `pmset disablesleep`, which
//! needs root, can. A one-time sudoers rule allows exactly that command, and a
//! marker file lets the next launch undo it if MonoCode dies holding it.
use std::time::{Duration, Instant};

pub mod hotspot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// While any agent works, then for the grace period.
    Working,
    /// For a set length, and still while agents work.
    Timer,
    /// Until turned off.
    UntilOff,
}
impl Mode {
    pub fn key(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Timer => "timer",
            Self::UntilOff => "until-off",
        }
    }
    pub fn from_key(key: &str) -> Self {
        match key {
            "timer" => Self::Timer,
            "until-off" => Self::UntilOff,
            _ => Self::Working,
        }
    }
}

/// What the built-in screen does under a closed lid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LidScreen {
    Off,
    Dim,
    Leave,
}
impl LidScreen {
    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Dim => "dim",
            Self::Leave => "leave",
        }
    }
    pub fn from_key(key: &str) -> Self {
        match key {
            "dim" => Self::Dim,
            "leave" => Self::Leave,
            _ => Self::Off,
        }
    }
}

/// What happens to sound when a held lid closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LidSound {
    Leave,
    /// Muted, and unmuted again when the lid opens.
    Mute,
    /// Media paused; nothing resumes it.
    Pause,
}
impl LidSound {
    pub fn key(self) -> &'static str {
        match self {
            Self::Leave => "leave",
            Self::Mute => "mute",
            Self::Pause => "pause",
        }
    }
    pub fn from_key(key: &str) -> Self {
        match key {
            "leave" => Self::Leave,
            "pause" => Self::Pause,
            _ => Self::Mute,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Off, nothing holds; on, working agents always do.
    pub enabled: bool,
    pub grace: Duration,
    pub length: Duration,
    pub display: bool,
    /// Stay awake with the lid closed too, plugged in.
    pub lid: bool,
    /// ...and on battery.
    pub lid_on_battery: bool,
    pub lid_screen: LidScreen,
    /// Lock the screen when a held lid closes; staying awake skips the lock
    /// sleep would have done.
    pub lock: bool,
    pub lid_sound: LidSound,
    /// Low Power Mode on battery under a held, closed lid.
    pub low_power: bool,
    /// Let the Mac sleep when it runs hot under a closed lid.
    pub heat_guard: bool,
    /// On battery at or below this percent, let the Mac sleep.
    pub stop_below: Option<u8>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            grace: Duration::from_secs(5 * 60),
            length: Duration::from_secs(2 * 60 * 60),
            display: false,
            lid: false,
            lid_on_battery: false,
            lid_screen: LidScreen::Off,
            lock: true,
            lid_sound: LidSound::Mute,
            low_power: false,
            heat_guard: true,
            stop_below: Some(20),
        }
    }
}

/// The machine as last sampled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Machine {
    pub lid_closed: bool,
    pub on_ac: bool,
    pub battery: Option<u8>,
    /// macOS reports heavy thermal pressure or worse.
    pub hot: bool,
}
impl Default for Machine {
    fn default() -> Self {
        Self {
            lid_closed: false,
            on_ac: true,
            battery: None,
            hot: false,
        }
    }
}

/// A timer or "until off" started by hand. `Working` is not a session: it is
/// what holds whenever keep-awake is enabled.
#[derive(Clone, Copy, Debug)]
struct Session {
    mode: Mode,
    started: Instant,
    /// The timer's end.
    deadline: Option<Instant>,
}

/// Why the Mac is (or is not) held awake right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Keep-awake is disabled.
    Off,
    /// Enabled with nothing running yet.
    Armed,
    Working {
        agents: usize,
    },
    Grace {
        left: Duration,
        total: Duration,
    },
    Timer {
        left: Duration,
        total: Duration,
    },
    UntilOff {
        since: Duration,
    },
    /// Would hold, but the battery is at or below the cutoff.
    BatteryLow {
        percent: u8,
    },
    /// Would hold, but the Mac runs hot under a closed lid.
    Hot,
}
impl Status {
    pub fn awake(self) -> bool {
        !matches!(
            self,
            Self::Off | Self::Armed | Self::BatteryLow { .. } | Self::Hot
        )
    }
    /// Would hold if a safety cutoff weren't in the way.
    pub fn paused(self) -> bool {
        matches!(self, Self::BatteryLow { .. } | Self::Hot)
    }
}

/// What to hold on the machine for a status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hold {
    /// `caffeinate -d`: the display stays on.
    pub display: bool,
    /// `caffeinate -s`: system sleep, honored only on AC power.
    pub system: bool,
    /// `pmset disablesleep 1`: lid-close sleep too.
    pub lid: bool,
}
impl Hold {
    /// `caffeinate` flags for this hold; `-i` (idle sleep) always.
    pub fn flags(self) -> String {
        let mut flags = String::from("-i");
        if self.display {
            flags.push('d');
        }
        if self.system {
            flags.push('s');
        }
        flags
    }
}

/// Fed the current agent count on every tick rather than start and finish
/// events, so a missed change costs a tick instead of a whole run.
#[derive(Debug, Default)]
pub struct KeepAwake {
    session: Option<Session>,
    /// When it lets go after the last agent finished.
    grace_until: Option<Instant>,
    was_busy: bool,
}
impl KeepAwake {
    /// The hand-started mode, else `Working`.
    pub fn mode(&self) -> Mode {
        self.session.map_or(Mode::Working, |s| s.mode)
    }
    /// `Working` ends a timer or "until off"; the others start one.
    pub fn start(&mut self, mode: Mode, settings: &Settings, now: Instant) {
        self.session = (mode != Mode::Working).then(|| Session {
            mode,
            started: now,
            deadline: (mode == Mode::Timer).then(|| now + settings.length),
        });
    }
    /// A new length restarts a running timer from now.
    pub fn retime(&mut self, settings: &Settings, now: Instant) {
        if let Some(s) = self.session.as_mut().filter(|s| s.mode == Mode::Timer) {
            s.deadline = Some(now + settings.length);
        }
    }
    /// Follow the agents and the clock; a timer that runs out falls back to
    /// `Working`.
    pub fn tick(&mut self, busy: usize, settings: &Settings, now: Instant) {
        if !settings.enabled {
            *self = Self::default();
            return;
        }
        if busy > 0 {
            self.grace_until = None;
        } else if self.was_busy {
            self.grace_until = Some(now + settings.grace);
        }
        self.was_busy = busy > 0;
        if self.grace_until.is_some_and(|until| now >= until) {
            self.grace_until = None;
        }
        if self
            .session
            .and_then(|s| s.deadline)
            .is_some_and(|d| now >= d)
        {
            self.session = None;
        }
    }
    pub fn status(
        &self,
        busy: usize,
        settings: &Settings,
        machine: Machine,
        now: Instant,
    ) -> Status {
        if !settings.enabled {
            return Status::Off;
        }
        let status = match self.session {
            Some(Session {
                mode: Mode::UntilOff,
                started,
                ..
            }) => Status::UntilOff {
                since: now.saturating_duration_since(started),
            },
            Some(Session {
                deadline: Some(d), ..
            }) if now < d => Status::Timer {
                left: d - now,
                total: settings.length,
            },
            _ if busy > 0 => Status::Working { agents: busy },
            _ => match self.grace_until {
                Some(until) if now < until => Status::Grace {
                    left: until - now,
                    total: settings.grace,
                },
                _ => Status::Armed,
            },
        };
        match (machine.on_ac, machine.battery, settings.stop_below) {
            (false, Some(percent), Some(floor)) if status.awake() && percent <= floor => {
                Status::BatteryLow { percent }
            }
            _ if status.awake() && settings.heat_guard && machine.hot && machine.lid_closed => {
                Status::Hot
            }
            _ => status,
        }
    }
}

/// The hold a status needs, `None` when the Mac may sleep.
pub fn hold(
    status: Status,
    settings: &Settings,
    machine: Machine,
    lid_ready: bool,
) -> Option<Hold> {
    if !status.awake() {
        return None;
    }
    let lid = settings.lid && (machine.on_ac || settings.lid_on_battery) && lid_ready;
    // Under a closed lid the screen goes dark unless asked to stay lit.
    let dark = machine.lid_closed && settings.lid_screen != LidScreen::Leave;
    Some(Hold {
        display: settings.display && !dark,
        system: machine.on_ac,
        lid,
    })
}

/// Low Power Mode is wanted: on battery, under a lid this hold keeps awake.
pub fn low_power(hold: Option<Hold>, settings: &Settings, machine: Machine) -> bool {
    settings.low_power && !machine.on_ac && machine.lid_closed && hold.is_some_and(|h| h.lid)
}

/// What to do as a held lid closes or opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LidActions {
    pub lock: bool,
    pub mute: bool,
    pub pause: bool,
    /// Unmute what closing muted.
    pub unmute: bool,
}
pub fn lid_actions(settings: &Settings, closed: bool, held: bool, muted_by_us: bool) -> LidActions {
    if !closed {
        return LidActions {
            unmute: muted_by_us,
            ..LidActions::default()
        };
    }
    if !held {
        return LidActions::default();
    }
    LidActions {
        lock: settings.lock,
        mute: settings.lid_sound == LidSound::Mute,
        pause: settings.lid_sound == LidSound::Pause,
        unmute: false,
    }
}

/// A duration as the footer counts it: `4:05`, `1:02:09`.
pub fn clock(d: Duration) -> String {
    let s = d.as_secs();
    let (h, m, s) = (s / 3600, s % 3600 / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// A length as people say it: `5m`, `2h`, `2h 30m`.
pub fn length(d: Duration) -> String {
    let m = d.as_secs() / 60;
    match (m / 60, m % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// `AppleClamshellState` from `ioreg -r -k AppleClamshellState -d 1`.
pub fn parse_clamshell(ioreg: &str) -> Option<bool> {
    ioreg.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim().trim_matches('"') == "AppleClamshellState").then(|| value.trim() == "Yes")
    })
}

/// Power source and charge from `pmset -g batt`.
pub fn parse_battery(pmset: &str) -> (bool, Option<u8>) {
    let on_ac = !pmset.contains("'Battery Power'");
    let percent = pmset.lines().find_map(|line| {
        let before = line.split('%').next()?;
        if before.len() == line.len() {
            return None;
        }
        before
            .rsplit(|c: char| !c.is_ascii_digit())
            .next()?
            .parse()
            .ok()
    });
    (on_ac, percent)
}

/// The sudoers line allowing exactly the `pmset` calls keep-awake makes:
/// lid-close sleep, and the battery's power mode (`powermode`, or
/// `lowpowermode` on older macOS).
pub fn sudoers_rule(user: &str) -> Option<String> {
    let valid = !user.is_empty()
        && user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    let commands = ["-a disablesleep 0", "-a disablesleep 1"]
        .into_iter()
        .map(String::from)
        .chain((0..=2).map(|v| format!("-b powermode {v}")))
        .chain((0..=1).map(|v| format!("-b lowpowermode {v}")))
        .map(|args| format!("{PMSET} {args}"))
        .collect::<Vec<_>>()
        .join(", ");
    valid.then(|| format!("{user} ALL = (root) NOPASSWD: {commands}"))
}

/// The battery's power-mode setting from `pmset -g custom`, as its key and
/// value: `("powermode", 0)`. 1 is Low Power Mode.
pub fn parse_battery_power_mode(custom: &str) -> Option<(&'static str, u8)> {
    let battery = custom.split("Battery Power:").nth(1)?;
    let battery = battery.split("AC Power:").next()?;
    battery.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        let key = match words.next()? {
            "powermode" => "powermode",
            "lowpowermode" => "lowpowermode",
            _ => return None,
        };
        Some((key, words.next()?.parse().ok()?))
    })
}

const PMSET: &str = "/usr/bin/pmset";
pub const SUDOERS_FILE: &str = "/etc/sudoers.d/monocode-keep-awake";

#[cfg(target_os = "macos")]
pub mod mac {
    use super::*;
    use std::{
        path::Path,
        process::{Child, Command, Stdio},
    };

    fn output(program: &str, args: &[&str]) -> Option<String> {
        let out = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// The lid, power and heat, sampled now.
    pub fn machine() -> Machine {
        let lid_closed = output(
            "/usr/sbin/ioreg",
            &["-r", "-k", "AppleClamshellState", "-d", "1"],
        )
        .and_then(|s| parse_clamshell(&s))
        .unwrap_or(false);
        let (on_ac, battery) = output(PMSET, &["-g", "batt"])
            .map(|s| parse_battery(&s))
            .unwrap_or((true, None));
        Machine {
            lid_closed,
            on_ac,
            battery,
            hot: thermal_pressure().is_some_and(|level| level >= 2),
        }
    }

    /// The kernel's thermal pressure: 0 nominal, 1 moderate, 2 heavy,
    /// 3 trapping, 4 sleeping. The same source as `NSProcessInfo.thermalState`.
    pub fn thermal_pressure() -> Option<u64> {
        use std::sync::OnceLock;
        unsafe extern "C" {
            fn notify_register_check(name: *const libc::c_char, token: *mut i32) -> u32;
            fn notify_get_state(token: i32, state: *mut u64) -> u32;
        }
        static TOKEN: OnceLock<Option<i32>> = OnceLock::new();
        let token = (*TOKEN.get_or_init(|| {
            let mut token = 0;
            let name = c"com.apple.system.thermalpressurelevel";
            (unsafe { notify_register_check(name.as_ptr(), &mut token) } == 0).then_some(token)
        }))?;
        let mut state = 0;
        (unsafe { notify_get_state(token, &mut state) } == 0).then_some(state)
    }

    /// Hold the assertions until the child is killed or `pid` exits, so a
    /// crash never leaves the Mac stuck awake.
    pub fn caffeinate(hold: Hold, pid: u32) -> std::io::Result<Child> {
        Command::new("/usr/bin/caffeinate")
            .args([hold.flags(), "-w".into(), pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
    }

    /// Turns the display off now; the system stays awake.
    pub fn display_sleep_now() {
        let _ = output(PMSET, &["displaysleepnow"]);
    }

    /// Whether sudo runs `pmset args` without a password. `-k` ignores cached
    /// credentials, which would pass now and fail later.
    fn sudo_allows(args: &[&str]) -> bool {
        Command::new("/usr/bin/sudo")
            .args(["-n", "-k", "-l", PMSET])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
    /// Whether a sudoers rule lets `pmset disablesleep` run without a password.
    pub fn lid_rule_ready() -> bool {
        sudo_allows(&["-a", "disablesleep", "1"])
    }
    /// Whether the rule also covers the battery's power mode. Rules from
    /// before Low Power Mode existed don't.
    pub fn power_rule_ready() -> bool {
        let key = battery_power_mode().map_or("powermode", |(key, _)| key);
        sudo_allows(&["-b", key, "1"])
    }
    fn battery_power_mode() -> Option<(&'static str, u8)> {
        parse_battery_power_mode(&output(PMSET, &["-g", "custom"])?)
    }
    fn sudo_pmset(args: &[&str]) -> bool {
        Command::new("/usr/bin/sudo")
            .args(["-n", PMSET])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    /// Low Power Mode on battery, or back to what it was. The marker holds the
    /// previous setting, for [`restore_after_crash`]; already on, it is left
    /// alone and turning off leaves it on too.
    pub fn set_low_power(on: bool, marker: &Path) -> Result<(), String> {
        if !on {
            let Ok(saved) = std::fs::read_to_string(marker) else {
                return Ok(());
            };
            let mut words = saved.split_whitespace();
            let (Some(key @ ("powermode" | "lowpowermode")), Some(value @ ("0" | "2"))) =
                (words.next(), words.next())
            else {
                let _ = std::fs::remove_file(marker);
                return Ok(());
            };
            if !sudo_pmset(&["-b", key, value]) {
                return Err("macOS refused to change Low Power Mode.".into());
            }
            let _ = std::fs::remove_file(marker);
            return Ok(());
        }
        let (key, value) = battery_power_mode().ok_or("This Mac has no Low Power Mode.")?;
        if value == 1 {
            return Ok(());
        }
        let _ = std::fs::write(marker, format!("{key} {value}"));
        if !sudo_pmset(&["-b", key, "1"]) {
            let _ = std::fs::remove_file(marker);
            return Err("macOS refused to change Low Power Mode.".into());
        }
        Ok(())
    }

    /// Locks the screen now, as Control-Command-Q does. `login.framework` is
    /// private; if it goes away this does nothing.
    pub fn lock_screen() -> bool {
        unsafe {
            let lib = libc::dlopen(
                c"/System/Library/PrivateFrameworks/login.framework/Versions/A/login".as_ptr(),
                libc::RTLD_LAZY,
            );
            if lib.is_null() {
                return false;
            }
            let sym = libc::dlsym(lib, c"SACLockScreenImmediate".as_ptr());
            if sym.is_null() {
                return false;
            }
            let lock = std::mem::transmute::<*mut libc::c_void, unsafe extern "C" fn() -> i32>(sym);
            lock() == 0
        }
    }

    /// Whether the output is muted, from AppleScript's volume settings.
    pub fn muted() -> Option<bool> {
        let out = output(
            "/usr/bin/osascript",
            &["-e", "output muted of (get volume settings)"],
        )?;
        match out.trim() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }
    pub fn set_muted(muted: bool) -> bool {
        output(
            "/usr/bin/osascript",
            &["-e", &format!("set volume output muted {muted}")],
        )
        .is_some()
    }

    /// Pauses whatever plays media, like the keyboard's pause key. MediaRemote
    /// is private; if it goes away this does nothing.
    pub fn pause_media() -> bool {
        const PAUSE: u32 = 1;
        unsafe {
            let lib = libc::dlopen(
                c"/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote".as_ptr(),
                libc::RTLD_LAZY,
            );
            if lib.is_null() {
                return false;
            }
            let sym = libc::dlsym(lib, c"MRMediaRemoteSendCommand".as_ptr());
            if sym.is_null() {
                return false;
            }
            let send = std::mem::transmute::<
                *mut libc::c_void,
                unsafe extern "C" fn(u32, *const libc::c_void) -> bool,
            >(sym);
            send(PAUSE, std::ptr::null())
        }
    }

    /// Asks for an administrator password once and installs the rule,
    /// checked by `visudo` before it goes in place.
    pub fn install_lid_rule() -> Result<(), String> {
        let user = output("/usr/bin/id", &["-un"]).unwrap_or_default();
        let rule = sudoers_rule(user.trim()).ok_or("Couldn't read your user name.")?;
        let script = format!(
            "set -e; f=$(/usr/bin/mktemp); printf '%s\\n' '{rule}' > \"$f\"; \
             /usr/sbin/visudo -cf \"$f\" >/dev/null; \
             /usr/bin/install -m 0440 -o root -g wheel \"$f\" {SUDOERS_FILE}; /bin/rm -f \"$f\""
        );
        let apple = format!(
            "do shell script \"{}\" with prompt \"MonoCode wants to keep your Mac awake with the lid closed.\" with administrator privileges",
            script.replace('\\', "\\\\").replace('"', "\\\"")
        );
        let out = Command::new("/usr/bin/osascript")
            .args(["-e", &apple])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            let error = String::from_utf8_lossy(&out.stderr);
            return Err(if error.contains("-128") {
                "Cancelled.".into()
            } else {
                "Couldn't add the rule.".into()
            });
        }
        if lid_rule_ready() {
            Ok(())
        } else {
            Err("The rule was added but sudo doesn't accept it.".into())
        }
    }

    /// Lid-close sleep off (or back on). The marker records that MonoCode
    /// changed it, for [`restore_after_crash`].
    pub fn set_lid_sleep_disabled(disabled: bool, marker: &Path) -> Result<(), String> {
        if disabled {
            let _ = std::fs::write(marker, b"1");
        }
        let ok = Command::new("/usr/bin/sudo")
            .args([
                "-n",
                PMSET,
                "-a",
                "disablesleep",
                if disabled { "1" } else { "0" },
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            if disabled {
                let _ = std::fs::remove_file(marker);
            }
            return Err("macOS refused to change lid-close sleep.".into());
        }
        if !disabled {
            let _ = std::fs::remove_file(marker);
        }
        Ok(())
    }

    /// A previous run died while holding lid-close sleep off, or Low Power
    /// Mode on: undo both.
    pub fn restore_after_crash(lid_marker: &Path, power_marker: &Path) {
        if lid_marker.exists() {
            let _ = set_lid_sleep_disabled(false, lid_marker);
        }
        if power_marker.exists() {
            let _ = set_low_power(false, power_marker);
        }
    }

    pub use brightness::{brightness, set_brightness};
    /// DisplayServices is private but the only way to set the built-in
    /// panel's brightness; every call degrades to a no-op if it goes away.
    mod brightness {
        use std::ffi::{c_void, CStr};
        use std::sync::OnceLock;

        type Get = unsafe extern "C" fn(u32, *mut f32) -> i32;
        type Set = unsafe extern "C" fn(u32, f32) -> i32;

        #[link(name = "CoreGraphics", kind = "framework")]
        unsafe extern "C" {
            fn CGGetOnlineDisplayList(max: u32, displays: *mut u32, count: *mut u32) -> i32;
            fn CGDisplayIsBuiltin(display: u32) -> u32;
        }

        fn symbols() -> Option<&'static (Get, Set)> {
            static SYMBOLS: OnceLock<Option<(Get, Set)>> = OnceLock::new();
            SYMBOLS
                .get_or_init(|| unsafe {
                    let path = c"/System/Library/PrivateFrameworks/DisplayServices.framework/DisplayServices";
                    let lib = libc::dlopen(path.as_ptr(), libc::RTLD_LAZY);
                    if lib.is_null() {
                        return None;
                    }
                    let sym = |name: &CStr| -> *mut c_void { libc::dlsym(lib, name.as_ptr()) };
                    let get = sym(c"DisplayServicesGetBrightness");
                    let set = sym(c"DisplayServicesSetBrightness");
                    (!get.is_null() && !set.is_null()).then(|| {
                        (
                            std::mem::transmute::<*mut c_void, Get>(get),
                            std::mem::transmute::<*mut c_void, Set>(set),
                        )
                    })
                })
                .as_ref()
        }

        fn builtin() -> Option<u32> {
            let mut displays = [0u32; 16];
            let mut count = 0u32;
            unsafe {
                if CGGetOnlineDisplayList(16, displays.as_mut_ptr(), &mut count) != 0 {
                    return None;
                }
                displays[..count as usize]
                    .iter()
                    .copied()
                    .find(|d| CGDisplayIsBuiltin(*d) != 0)
            }
        }

        /// The built-in panel's brightness, 0 to 1.
        pub fn brightness() -> Option<f32> {
            let (get, _) = symbols()?;
            let display = builtin()?;
            let mut value = 0f32;
            (unsafe { get(display, &mut value) } == 0).then_some(value)
        }

        pub fn set_brightness(value: f32) -> bool {
            let Some((_, set)) = symbols() else {
                return false;
            };
            builtin().is_some_and(|d| unsafe { set(d, value.clamp(0., 1.)) } == 0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    fn settings() -> Settings {
        Settings::default()
    }

    #[test]
    fn working_agents_hold_it_and_it_lets_go_after_the_grace() {
        let s = settings();
        let t0 = Instant::now();
        let mut k = KeepAwake::default();
        let m = Machine::default();
        k.tick(0, &s, t0);
        assert_eq!(k.status(0, &s, m, t0), Status::Armed);
        k.tick(1, &s, t0);
        assert_eq!(k.status(1, &s, m, t0), Status::Working { agents: 1 });
        k.tick(0, &s, t0 + MIN);
        assert_eq!(
            k.status(0, &s, m, t0 + MIN),
            Status::Grace {
                left: 5 * MIN,
                total: 5 * MIN
            }
        );
        // A follow-up inside the grace cancels it.
        k.tick(1, &s, t0 + 3 * MIN);
        k.tick(0, &s, t0 + 4 * MIN);
        k.tick(0, &s, t0 + 8 * MIN);
        assert!(k.status(0, &s, m, t0 + 8 * MIN).awake());
        k.tick(0, &s, t0 + 9 * MIN);
        assert_eq!(k.status(0, &s, m, t0 + 9 * MIN), Status::Armed);
    }

    #[test]
    fn agents_that_overlap_or_were_missed_still_hold_it() {
        let s = settings();
        let t0 = Instant::now();
        let mut k = KeepAwake::default();
        let m = Machine::default();
        // Already running when keep-awake first hears of it.
        k.tick(2, &s, t0);
        k.tick(1, &s, t0 + MIN);
        k.tick(1, &s, t0 + 60 * MIN);
        assert_eq!(
            k.status(1, &s, m, t0 + 60 * MIN),
            Status::Working { agents: 1 }
        );
    }

    #[test]
    fn disabled_never_holds_and_forgets_its_session() {
        let on = settings();
        let off = Settings {
            enabled: false,
            ..settings()
        };
        let t0 = Instant::now();
        let mut k = KeepAwake::default();
        k.start(Mode::UntilOff, &on, t0);
        k.tick(3, &off, t0);
        assert_eq!(k.status(3, &off, Machine::default(), t0), Status::Off);
        assert_eq!(k.mode(), Mode::Working);
        k.tick(3, &on, t0);
        assert_eq!(
            k.status(3, &on, Machine::default(), t0),
            Status::Working { agents: 3 }
        );
    }

    #[test]
    fn a_timer_falls_back_to_working() {
        let t0 = Instant::now();
        let s = Settings {
            length: 30 * MIN,
            ..settings()
        };
        let m = Machine::default();
        let mut k = KeepAwake::default();
        k.start(Mode::Timer, &s, t0);
        k.tick(0, &s, t0 + 10 * MIN);
        assert_eq!(
            k.status(0, &s, m, t0 + 10 * MIN),
            Status::Timer {
                left: 20 * MIN,
                total: 30 * MIN
            }
        );
        k.tick(1, &s, t0 + 31 * MIN);
        assert_eq!(k.mode(), Mode::Working);
        assert_eq!(
            k.status(1, &s, m, t0 + 31 * MIN),
            Status::Working { agents: 1 }
        );
        k.start(Mode::Timer, &s, t0);
        k.start(Mode::Working, &s, t0);
        assert_eq!(k.status(0, &s, m, t0), Status::Armed);
    }

    #[test]
    fn low_battery_lets_the_mac_sleep_but_keeps_the_session() {
        let s = settings();
        let t0 = Instant::now();
        let mut k = KeepAwake::default();
        k.start(Mode::UntilOff, &s, t0);
        let low = Machine {
            on_ac: false,
            battery: Some(20),
            ..Machine::default()
        };
        assert_eq!(k.status(0, &s, low, t0), Status::BatteryLow { percent: 20 });
        let charging = Machine { on_ac: true, ..low };
        assert!(k.status(0, &s, charging, t0).awake());
        let never = Settings {
            stop_below: None,
            ..s
        };
        assert!(k.status(0, &never, low, t0).awake());
    }

    #[test]
    fn the_hold_follows_power_and_the_lid() {
        let s = Settings {
            display: true,
            lid: true,
            ..settings()
        };
        let on = Status::UntilOff {
            since: Duration::ZERO,
        };
        let open = Machine::default();
        assert_eq!(
            hold(on, &s, open, true),
            Some(Hold {
                display: true,
                system: true,
                lid: true
            })
        );
        assert_eq!(hold(on, &s, open, true).unwrap().flags(), "-ids");
        // No rule yet, or on battery: no lid hold, and -s means nothing on battery.
        assert!(!hold(on, &s, open, false).unwrap().lid);
        let battery = Machine {
            on_ac: false,
            ..open
        };
        assert_eq!(
            hold(on, &s, battery, true),
            Some(Hold {
                display: true,
                system: false,
                lid: false
            })
        );
        // A closed lid drops -d so the screen can go dark.
        let closed = Machine {
            lid_closed: true,
            ..open
        };
        assert!(!hold(on, &s, closed, true).unwrap().display);
        let leave = Settings {
            lid_screen: LidScreen::Leave,
            ..s.clone()
        };
        assert!(hold(on, &leave, closed, true).unwrap().display);
        assert_eq!(hold(Status::Armed, &s, open, true), None);
    }

    #[test]
    fn heat_under_a_closed_lid_lets_it_sleep() {
        let s = settings();
        let t0 = Instant::now();
        let k = KeepAwake::default();
        let hot_open = Machine {
            hot: true,
            ..Machine::default()
        };
        // Someone is at an open Mac; heat alone doesn't stop it.
        assert!(k.status(1, &s, hot_open, t0).awake());
        let hot_closed = Machine {
            lid_closed: true,
            ..hot_open
        };
        assert_eq!(k.status(1, &s, hot_closed, t0), Status::Hot);
        assert!(Status::Hot.paused());
        let unguarded = Settings {
            heat_guard: false,
            ..s
        };
        assert!(k.status(1, &unguarded, hot_closed, t0).awake());
        // Idle stays idle rather than reporting heat.
        assert_eq!(k.status(0, &s, hot_closed, t0), Status::Armed);
    }

    #[test]
    fn the_lid_holds_on_battery_only_when_asked_and_low_power_follows() {
        let on = Status::Working { agents: 1 };
        let lid = Settings {
            lid: true,
            low_power: true,
            ..settings()
        };
        let battery_closed = Machine {
            on_ac: false,
            lid_closed: true,
            battery: Some(80),
            ..Machine::default()
        };
        let plugged = hold(on, &lid, battery_closed, true);
        assert!(!plugged.unwrap().lid);
        assert!(!low_power(plugged, &lid, battery_closed));
        let anywhere = Settings {
            lid_on_battery: true,
            ..lid.clone()
        };
        let held = hold(on, &anywhere, battery_closed, true);
        assert!(held.unwrap().lid);
        assert!(low_power(held, &anywhere, battery_closed));
        let open = Machine {
            lid_closed: false,
            ..battery_closed
        };
        assert!(!low_power(hold(on, &anywhere, open, true), &anywhere, open));
        let ac = Machine {
            on_ac: true,
            ..battery_closed
        };
        assert!(!low_power(hold(on, &anywhere, ac, true), &anywhere, ac));
    }

    #[test]
    fn a_held_lid_locks_and_quiets_and_opening_restores_sound() {
        let s = settings();
        assert_eq!(
            lid_actions(&s, true, true, false),
            LidActions {
                lock: true,
                mute: true,
                pause: false,
                unmute: false
            }
        );
        // Not held: the Mac sleeps and locks by itself.
        assert_eq!(lid_actions(&s, true, false, false), LidActions::default());
        assert!(lid_actions(&s, false, true, true).unmute);
        assert!(!lid_actions(&s, false, true, false).unmute);
        let pause = Settings {
            lid_sound: LidSound::Pause,
            lock: false,
            ..s
        };
        let actions = lid_actions(&pause, true, true, false);
        assert!(actions.pause && !actions.mute && !actions.lock);
    }

    #[test]
    fn reads_the_battery_power_mode() {
        let custom = "Battery Power:\n lidwake              1\n powermode            0\nAC Power:\n powermode            2\n";
        assert_eq!(parse_battery_power_mode(custom), Some(("powermode", 0)));
        let old = "Battery Power:\n lowpowermode         1\nAC Power:\n lowpowermode         0\n";
        assert_eq!(parse_battery_power_mode(old), Some(("lowpowermode", 1)));
        assert_eq!(parse_battery_power_mode("AC Power:\n powermode 2\n"), None);
    }

    #[test]
    fn reads_the_lid_sensor_and_battery() {
        let ioreg = "+-o IOPMrootDomain  <class IOPMrootDomain>\n    {\n      \"AppleClamshellCausesSleep\" = Yes\n      \"AppleClamshellState\" = No\n    }";
        assert_eq!(parse_clamshell(ioreg), Some(false));
        assert_eq!(
            parse_clamshell(&ioreg.replace("State\" = No", "State\" = Yes")),
            Some(true)
        );
        assert_eq!(parse_clamshell("nothing"), None);

        let ac = "Now drawing from 'AC Power'\n -InternalBattery-0 (id=123)\t85%; charging; 1:02 remaining present: true";
        assert_eq!(parse_battery(ac), (true, Some(85)));
        let batt = "Now drawing from 'Battery Power'\n -InternalBattery-0 (id=1)\t7%; discharging; 0:20 remaining present: true";
        assert_eq!(parse_battery(batt), (false, Some(7)));
        assert_eq!(parse_battery("Now drawing from 'AC Power'\n"), (true, None));
    }

    #[test]
    fn the_sudoers_rule_names_only_the_pmset_calls_it_makes() {
        assert_eq!(
            sudoers_rule("kaloyan.s").unwrap(),
            "kaloyan.s ALL = (root) NOPASSWD: /usr/bin/pmset -a disablesleep 0, /usr/bin/pmset -a disablesleep 1, \
             /usr/bin/pmset -b powermode 0, /usr/bin/pmset -b powermode 1, /usr/bin/pmset -b powermode 2, \
             /usr/bin/pmset -b lowpowermode 0, /usr/bin/pmset -b lowpowermode 1"
        );
        assert_eq!(sudoers_rule("bad user"), None);
        assert_eq!(sudoers_rule("x'; rm -rf /"), None);
        assert_eq!(sudoers_rule(""), None);
    }

    #[test]
    fn formats_times() {
        assert_eq!(clock(Duration::from_secs(245)), "4:05");
        assert_eq!(clock(Duration::from_secs(3729)), "1:02:09");
        assert_eq!(length(5 * MIN), "5m");
        assert_eq!(length(120 * MIN), "2h");
        assert_eq!(length(150 * MIN), "2h 30m");
    }
}
