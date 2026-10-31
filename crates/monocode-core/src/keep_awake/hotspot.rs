//! Moves the Mac onto a hotspot when Wi-Fi drops while agents work, and back
//! to its usual network once they finish.
//!
//! macOS hides the current network's name from apps without Location access,
//! so this never asks which network the Mac is on. It notices a drop by
//! failing to reach the internet, and remembers that it made the switch.
use std::time::{Duration, Instant};

/// Offline this long before joining, so a blip doesn't switch networks.
pub const OFFLINE_BEFORE_JOIN: Duration = Duration::from_secs(15);
/// Between attempts to join.
pub const RETRY: Duration = Duration::from_secs(60);
/// Between reachability checks while agents work.
pub const PROBE_EVERY: Duration = Duration::from_secs(5);
/// How long a join gets to bring the internet back.
pub const JOIN_SETTLE: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Stay,
    Join,
    /// Back to the usual network.
    Leave,
}

#[derive(Debug, Default)]
pub struct Hotspot {
    offline_since: Option<Instant>,
    last_try: Option<Instant>,
    /// This moved the Mac onto the hotspot and hasn't moved it back.
    joined: bool,
}
impl Hotspot {
    pub fn on_hotspot(&self) -> bool {
        self.joined
    }
    /// Feeds the latest reachability check. `working`: agents work and a
    /// hotspot is set up.
    pub fn step(&mut self, online: bool, working: bool, now: Instant) -> Step {
        if !working {
            self.offline_since = None;
            self.last_try = None;
            return if self.joined { Step::Leave } else { Step::Stay };
        }
        if online {
            self.offline_since = None;
            return Step::Stay;
        }
        let since = *self.offline_since.get_or_insert(now);
        let due = now.saturating_duration_since(since) >= OFFLINE_BEFORE_JOIN
            && self
                .last_try
                .is_none_or(|at| now.saturating_duration_since(at) >= RETRY);
        if due {
            self.last_try = Some(now);
            Step::Join
        } else {
            Step::Stay
        }
    }
    /// Someone is back at the Mac: let it return to its usual network.
    pub fn lid_opened(&self) -> Step {
        if self.joined {
            Step::Leave
        } else {
            Step::Stay
        }
    }
    pub fn joined(&mut self) {
        self.joined = true;
        self.offline_since = None;
    }
    pub fn left(&mut self) {
        *self = Self::default();
    }
}

/// Whether the internet answers: a TCP handshake with any of a few hosts.
/// Addresses first, so a dead resolver can't make it look online or stall it.
pub fn online(timeout: Duration) -> bool {
    use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
    let fixed = ["1.1.1.1:443", "8.8.8.8:443"]
        .into_iter()
        .filter_map(|a| a.parse::<SocketAddr>().ok());
    let reach = |addr: SocketAddr| TcpStream::connect_timeout(&addr, timeout).is_ok();
    fixed.into_iter().any(reach)
        || ("api.anthropic.com", 443)
            .to_socket_addrs()
            .into_iter()
            .flatten()
            .take(1)
            .any(reach)
}

/// Whether a turn's error reads like the connection dropped, rather than the
/// provider refusing or failing on its own.
pub fn network_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    [
        "connection error",
        "connection reset",
        "connection refused",
        "connection closed",
        "network",
        "econnreset",
        "econnrefused",
        "enotfound",
        "etimedout",
        "enetunreach",
        "ehostunreach",
        "getaddrinfo",
        "fetch failed",
        "socket hang up",
        "stream disconnected",
        "dns error",
        "error sending request",
        "offline",
    ]
    .iter()
    .any(|needle| error.contains(needle))
}

/// The Wi-Fi device (`en0`) from `networksetup -listallhardwareports`.
pub fn parse_wifi_device(ports: &str) -> Option<String> {
    let mut lines = ports.lines();
    while let Some(line) = lines.next() {
        if matches!(
            line.trim(),
            "Hardware Port: Wi-Fi" | "Hardware Port: AirPort"
        ) {
            return lines
                .next()?
                .trim()
                .strip_prefix("Device: ")
                .map(|d| d.trim().to_string());
        }
    }
    None
}

/// Names from `networksetup -listpreferredwirelessnetworks`.
pub fn parse_saved_networks(list: &str) -> Vec<String> {
    list.lines()
        .filter(|line| line.starts_with('\t'))
        .map(|line| line.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

/// `networksetup -setairportnetwork` exits 0 even when it fails; it says so
/// on stdout instead.
pub fn join_failed(out: &str) -> Option<String> {
    let out = out.trim();
    let failed = ["Failed", "Could not", "Error", "not find"]
        .iter()
        .any(|word| out.contains(word));
    failed.then(|| {
        if out.contains("Could not find") || out.contains("not find") {
            "The hotspot isn't in range. Is Personal Hotspot on?".to_string()
        } else if out.contains("-3900") || out.contains("password") {
            "Couldn't join. Check the password.".to_string()
        } else {
            "Couldn't join the hotspot.".to_string()
        }
    })
}

#[cfg(target_os = "macos")]
pub mod mac {
    use super::*;
    use std::process::{Command, Stdio};

    const NETWORKSETUP: &str = "/usr/sbin/networksetup";
    const KEYCHAIN: &str = "com.monocode.hotspot";

    fn run(args: &[&str]) -> Option<String> {
        let out = Command::new(NETWORKSETUP)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    pub fn wifi_device() -> Option<String> {
        parse_wifi_device(&run(&["-listallhardwareports"])?)
    }

    /// Networks this Mac remembers, most preferred first.
    pub fn saved_networks() -> Vec<String> {
        wifi_device()
            .and_then(|device| run(&["-listpreferredwirelessnetworks", &device]))
            .map(|list| parse_saved_networks(&list))
            .unwrap_or_default()
    }

    /// Joins `ssid` and waits for the internet. A saved password, if any, is
    /// passed to `networksetup`; without one macOS uses what it remembers.
    pub fn join(ssid: &str) -> Result<(), String> {
        let device = wifi_device().ok_or("This Mac has no Wi-Fi.")?;
        let password = password(ssid);
        let mut args = vec!["-setairportnetwork", device.as_str(), ssid];
        if let Some(password) = &password {
            args.push(password);
        }
        let out = run(&args).ok_or("Couldn't run networksetup.")?;
        if let Some(error) = join_failed(&out) {
            return Err(error);
        }
        let start = Instant::now();
        while start.elapsed() < JOIN_SETTLE {
            if online(Duration::from_secs(2)) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        Err("Joined the hotspot, but it has no internet.".into())
    }

    /// Wi-Fi off and on, so macOS picks its usual network again.
    pub fn leave() -> Result<(), String> {
        let device = wifi_device().ok_or("This Mac has no Wi-Fi.")?;
        run(&["-setairportpower", &device, "off"]).ok_or("Couldn't turn Wi-Fi off.")?;
        std::thread::sleep(Duration::from_secs(1));
        run(&["-setairportpower", &device, "on"]).ok_or("Couldn't turn Wi-Fi back on.")?;
        Ok(())
    }

    fn entry(ssid: &str) -> Option<keyring::Entry> {
        keyring::Entry::new(KEYCHAIN, ssid).ok()
    }
    /// The hotspot's password from the Keychain.
    pub fn password(ssid: &str) -> Option<String> {
        entry(ssid)?.get_password().ok().filter(|p| !p.is_empty())
    }
    pub fn set_password(ssid: &str, password: &str) -> Result<(), String> {
        entry(ssid)
            .ok_or("Couldn't open the Keychain.")?
            .set_password(password)
            .map_err(|_| "Couldn't save to the Keychain.".to_string())
    }
    pub fn forget_password(ssid: &str) {
        if let Some(entry) = entry(ssid) {
            let _ = entry.delete_credential();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Duration = Duration::from_secs(1);

    #[test]
    fn joins_after_a_sustained_drop_and_retries_slowly() {
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        assert_eq!(h.step(true, true, t0), Step::Stay);
        assert_eq!(h.step(false, true, t0), Step::Stay);
        assert_eq!(h.step(false, true, t0 + 10 * S), Step::Stay);
        assert_eq!(h.step(false, true, t0 + 15 * S), Step::Join);
        // The join failed; the next try waits a minute.
        assert_eq!(h.step(false, true, t0 + 30 * S), Step::Stay);
        assert_eq!(h.step(false, true, t0 + 75 * S), Step::Join);
        h.joined();
        assert!(h.on_hotspot());
        assert_eq!(h.step(true, true, t0 + 80 * S), Step::Stay);
    }

    #[test]
    fn a_blip_does_not_switch() {
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        h.step(false, true, t0);
        h.step(true, true, t0 + 10 * S);
        assert_eq!(h.step(false, true, t0 + 20 * S), Step::Stay);
        assert_eq!(h.step(false, true, t0 + 30 * S), Step::Stay);
    }

    #[test]
    fn goes_back_when_agents_finish_or_the_lid_opens() {
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        assert_eq!(h.step(false, false, t0), Step::Stay);
        assert_eq!(h.lid_opened(), Step::Stay);
        h.joined();
        assert_eq!(h.lid_opened(), Step::Leave);
        assert_eq!(h.step(true, false, t0), Step::Leave);
        h.left();
        assert!(!h.on_hotspot());
        assert_eq!(h.step(true, false, t0), Step::Stay);
    }

    #[test]
    fn tells_a_dropped_connection_from_other_failures() {
        assert!(network_error("API Error: Connection error."));
        assert!(network_error(
            "request failed: getaddrinfo ENOTFOUND api.anthropic.com"
        ));
        assert!(network_error("stream disconnected before completion"));
        assert!(network_error("TypeError: fetch failed"));
        assert!(!network_error("API Error: 529 Overloaded"));
        assert!(!network_error("Prompt is too long"));
        assert!(!network_error("Ready"));
    }

    #[test]
    fn reads_networksetup() {
        let ports = "\nHardware Port: Ethernet\nDevice: en1\n\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: aa\n";
        assert_eq!(parse_wifi_device(ports).as_deref(), Some("en0"));
        assert_eq!(
            parse_wifi_device("Hardware Port: Ethernet\nDevice: en1"),
            None
        );
        let list = "Preferred networks on en0:\n\tHome Wi-Fi\n\tMy iPhone\n";
        assert_eq!(parse_saved_networks(list), ["Home Wi-Fi", "My iPhone"]);
        assert_eq!(join_failed(""), None);
        assert!(join_failed("Could not find network My iPhone.").is_some());
        assert_eq!(
            join_failed("Failed to join network My iPhone.\nError: -3900  The operation couldn't be completed.").unwrap(),
            "Couldn't join. Check the password."
        );
    }
}
