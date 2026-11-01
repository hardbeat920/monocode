//! Joining and leaving the hotspot, behind a [`Wifi`] trait so the sequence
//! runs against fakes in tests.
//!
//! macOS hides the network's name from apps without Location access (on
//! macOS 15 and later `networksetup -getairportnetwork` says "not associated"
//! even while connected), so a join is confirmed by the Wi-Fi link instead:
//! the name when macOS shows it, else an iPhone hotspot's fixed subnet, else
//! a new lease and router where the old network's were.
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

/// The join gets this long for the Mac to move to the hotspot.
pub const ASSOCIATE_WITHIN: Duration = Duration::from_secs(15);
/// ...then this long for the internet to answer through it.
pub const INTERNET_WITHIN: Duration = Duration::from_secs(20);
/// After a leave, how long macOS gets to pick a network before checking it.
pub const SETTLE_AFTER_LEAVE: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_secs(1);

/// The Wi-Fi link right now, from tools that need no Location access.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Link {
    pub ip: Option<Ipv4Addr>,
    pub router: Option<Ipv4Addr>,
    /// `None` when macOS hides it, which is the usual case.
    pub ssid: Option<String>,
}
impl Link {
    /// A real lease, not nothing or a self-assigned 169.254 address.
    pub fn leased(&self) -> bool {
        self.ip
            .is_some_and(|ip| !ip.is_link_local() && !ip.is_unspecified())
    }
    /// An iPhone Personal Hotspot always hands out 172.20.10.0/28.
    pub fn iphone_hotspot(&self) -> bool {
        self.leased()
            && self.ip.is_some_and(|ip| {
                let [a, b, c, d] = ip.octets();
                (a, b, c) == (172, 20, 10) && d < 16
            })
    }
    /// Whether this link is on `ssid`, having been `before` the join.
    fn joined(&self, ssid: &str, before: &Link) -> bool {
        if let Some(name) = &self.ssid {
            return name == ssid;
        }
        if !self.leased() {
            return false;
        }
        self.iphone_hotspot()
            || !before.leased()
            || (self.ip, self.router) != (before.ip, before.router)
    }
    /// Whether this link is still the network `record` joined; `None` when
    /// it can't tell, as with no lease at all.
    fn on(&self, record: &JoinRecord) -> Option<bool> {
        if let Some(name) = &self.ssid {
            return Some(*name == record.ssid);
        }
        if !self.leased() {
            return None;
        }
        Some(self.router == record.router && self.iphone_hotspot() == record.iphone())
    }
}

/// What this app joined, saved so a later run can still leave it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinRecord {
    pub ssid: String,
    pub ip: Option<Ipv4Addr>,
    pub router: Option<Ipv4Addr>,
    /// Unix seconds.
    pub at: u64,
}
impl JoinRecord {
    fn iphone(&self) -> bool {
        Link {
            ip: self.ip,
            ..Link::default()
        }
        .iphone_hotspot()
    }
}

/// The Wi-Fi operations a join and a leave need.
pub trait Wifi {
    /// The Wi-Fi device (`en0`), `None` if this Mac has none.
    fn device(&mut self) -> Option<String>;
    /// Whether Wi-Fi is on; `None` when that can't be read.
    fn powered(&mut self, device: &str) -> Option<bool>;
    /// Asks macOS to join. `Ok` only means it didn't fail outright.
    fn associate(
        &mut self,
        device: &str,
        ssid: &str,
        password: Option<&str>,
    ) -> Result<(), JoinError>;
    fn link(&mut self, device: &str) -> Link;
    /// Wi-Fi off and on, so macOS picks its usual network again.
    fn cycle(&mut self, device: &str) -> Result<(), String>;
    /// Whether the internet answers.
    fn online(&mut self) -> bool;
}

pub trait Clock {
    fn now(&self) -> Instant;
    fn sleep(&self, d: Duration);
    /// Unix seconds, for the saved record.
    fn unix(&self) -> u64;
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d)
    }
    fn unix(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }
}

/// Why a join failed, one kind per thing the person can act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JoinError {
    NoWifi,
    WifiOff,
    /// Not in range, or not broadcasting: an idle iPhone hotspot stops
    /// advertising a minute or so after the last device leaves.
    NotFound,
    WrongPassword,
    /// macOS refused: "Require administrator authorization to change
    /// networks", or the like.
    Denied,
    /// `networksetup` ran past its time limit.
    TimedOut,
    /// `networksetup` reported nothing wrong, but the Mac never moved.
    NotAssociated,
    /// Moved, but got no address from the hotspot.
    NoLease,
    /// On the hotspot, but the internet doesn't answer through it.
    NoInternet,
    /// Was on the hotspot, then macOS went back to another network.
    Reverted,
    Failed(String),
}
impl JoinError {
    /// A short name for logs.
    pub fn key(&self) -> &'static str {
        match self {
            Self::NoWifi => "no_wifi",
            Self::WifiOff => "wifi_off",
            Self::NotFound => "not_found",
            Self::WrongPassword => "wrong_password",
            Self::Denied => "denied",
            Self::TimedOut => "timed_out",
            Self::NotAssociated => "not_associated",
            Self::NoLease => "no_lease",
            Self::NoInternet => "no_internet",
            Self::Reverted => "reverted",
            Self::Failed(_) => "failed",
        }
    }
    /// Retrying can't fix it; it needs the person.
    pub fn permanent(&self) -> bool {
        matches!(self, Self::NoWifi | Self::WrongPassword | Self::Denied)
    }
    pub fn message(&self) -> String {
        match self {
            Self::NoWifi => "This Mac has no Wi-Fi.".into(),
            Self::WifiOff => "Wi-Fi is off. Turn it on to use the hotspot.".into(),
            Self::NotFound => "The hotspot isn't in range or isn't showing. On the iPhone, open Settings › Personal Hotspot.".into(),
            Self::WrongPassword => "The hotspot refused the password. Check it in Settings.".into(),
            Self::Denied => "macOS didn't allow changing networks. An administrator may need to allow it in Wi-Fi settings.".into(),
            Self::TimedOut => "macOS took too long to join the hotspot.".into(),
            Self::NotAssociated => "macOS didn't move to the hotspot.".into(),
            Self::NoLease => "Joined the hotspot, but it gave the Mac no address.".into(),
            Self::NoInternet => "Joined the hotspot, but it has no internet. Is mobile data on?".into(),
            Self::Reverted => "Joined the hotspot, but macOS switched back to another network.".into(),
            Self::Failed(detail) => format!("Couldn't join the hotspot: {detail}"),
        }
    }
}
impl std::fmt::Display for JoinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

/// `networksetup -setairportnetwork`'s output, stdout and stderr together.
/// It exits 0 even when it fails, so the words decide. `None`: no failure.
pub fn classify(out: &str) -> Option<JoinError> {
    let text = out.trim();
    let lower = text.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));
    if text.is_empty() {
        return None;
    }
    if has(&["could not find", "not find network", "-3905"]) {
        return Some(JoinError::NotFound);
    }
    if has(&["-3930", "not permitted", "administrator", "authorization"]) {
        return Some(JoinError::Denied);
    }
    if has(&["-3900", "-3912", "-3924", "-3925", "password", "passphrase"]) {
        return Some(JoinError::WrongPassword);
    }
    if has(&["power", "turned off"]) && has(&["off"]) {
        return Some(JoinError::WifiOff);
    }
    if has(&[
        "failed",
        "error",
        "could not",
        "unable",
        "not a wi-fi",
        "invalid",
    ]) {
        let first = text.lines().next().unwrap_or(text);
        return Some(JoinError::Failed(first.chars().take(160).collect()));
    }
    None
}

/// How a join went, for the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Joined {
    pub record: JoinRecord,
    pub took: Duration,
}

/// Joins `ssid` and checks it: the Mac moved to it, then the internet
/// answers through it.
pub fn join(
    wifi: &mut impl Wifi,
    clock: &impl Clock,
    ssid: &str,
    password: Option<&str>,
) -> Result<Joined, JoinError> {
    let start = clock.now();
    let device = wifi.device().ok_or(JoinError::NoWifi)?;
    if wifi.powered(&device) == Some(false) {
        return Err(JoinError::WifiOff);
    }
    let before = wifi.link(&device);
    wifi.associate(&device, ssid, password)?;
    let moved = clock.now();
    let mut link = wifi.link(&device);
    while !link.joined(ssid, &before) {
        if clock.now().saturating_duration_since(moved) >= ASSOCIATE_WITHIN {
            let named = link.ssid.as_deref() == Some(ssid);
            let self_assigned = link.ip.is_some_and(|ip| ip.is_link_local());
            return Err(if named || self_assigned {
                JoinError::NoLease
            } else {
                JoinError::NotAssociated
            });
        }
        clock.sleep(POLL);
        link = wifi.link(&device);
    }
    let reached = clock.now();
    while !wifi.online() {
        if clock.now().saturating_duration_since(reached) >= INTERNET_WITHIN {
            return Err(JoinError::NoInternet);
        }
        clock.sleep(POLL);
    }
    // The internet may have answered through a network macOS went back to.
    let now = wifi.link(&device);
    if !now.joined(ssid, &before) {
        return Err(JoinError::Reverted);
    }
    Ok(Joined {
        record: JoinRecord {
            ssid: ssid.to_string(),
            ip: now.ip,
            router: now.router,
            at: clock.unix(),
        },
        took: clock.now().saturating_duration_since(start),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Left {
    /// Wi-Fi cycled; macOS picked another network.
    Cycled,
    /// The Mac was already elsewhere, or Wi-Fi was off: nothing touched.
    AlreadyMoved,
    /// Wi-Fi cycled, but macOS went straight back to the hotspot.
    Rejoined,
}
impl Left {
    pub fn key(self) -> &'static str {
        match self {
            Self::Cycled => "cycled",
            Self::AlreadyMoved => "already_moved",
            Self::Rejoined => "rejoined",
        }
    }
}

/// Back to the usual network. Doesn't touch Wi-Fi when it's off or the Mac
/// is no longer on the hotspot `record` joined.
pub fn leave(
    wifi: &mut impl Wifi,
    clock: &impl Clock,
    record: Option<&JoinRecord>,
) -> Result<Left, String> {
    let device = wifi.device().ok_or("This Mac has no Wi-Fi.")?;
    if wifi.powered(&device) == Some(false) {
        return Ok(Left::AlreadyMoved);
    }
    if record.is_some_and(|record| wifi.link(&device).on(record) == Some(false)) {
        return Ok(Left::AlreadyMoved);
    }
    wifi.cycle(&device)?;
    let Some(record) = record else {
        return Ok(Left::Cycled);
    };
    let start = clock.now();
    loop {
        let link = wifi.link(&device);
        if link.leased() || clock.now().saturating_duration_since(start) >= SETTLE_AFTER_LEAVE {
            return Ok(if link.on(record) == Some(true) {
                Left::Rejoined
            } else {
                Left::Cycled
            });
        }
        clock.sleep(POLL);
    }
}

#[cfg(test)]
pub(crate) mod fakes {
    use super::*;
    use std::cell::Cell;

    /// Time moves only when something sleeps.
    pub struct FakeClock {
        pub start: Instant,
        pub elapsed: Cell<Duration>,
    }
    impl FakeClock {
        pub fn new() -> Self {
            Self {
                start: Instant::now(),
                elapsed: Cell::new(Duration::ZERO),
            }
        }
    }
    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            self.start + self.elapsed.get()
        }
        fn sleep(&self, d: Duration) {
            self.elapsed.set(self.elapsed.get() + d);
        }
        fn unix(&self) -> u64 {
            1_700_000_000 + self.elapsed.get().as_secs()
        }
    }

    /// A Wi-Fi interface scripted by the test: each `link()` call takes the
    /// next link from `links` and keeps the last; `online` likewise.
    #[derive(Default)]
    pub struct FakeWifi {
        pub device: Option<String>,
        pub powered: Option<bool>,
        pub associate: Option<JoinError>,
        pub links: Vec<Link>,
        pub online: Vec<bool>,
        pub calls: Vec<String>,
    }
    impl FakeWifi {
        pub fn new() -> Self {
            Self {
                device: Some("en0".into()),
                powered: Some(true),
                ..Self::default()
            }
        }
    }
    fn next<T: Clone + Default>(list: &mut Vec<T>) -> T {
        if list.len() > 1 {
            list.remove(0)
        } else {
            list.first().cloned().unwrap_or_default()
        }
    }
    impl Wifi for FakeWifi {
        fn device(&mut self) -> Option<String> {
            self.device.clone()
        }
        fn powered(&mut self, _: &str) -> Option<bool> {
            self.powered
        }
        fn associate(&mut self, _: &str, ssid: &str, _: Option<&str>) -> Result<(), JoinError> {
            self.calls.push(format!("associate {ssid}"));
            self.associate.clone().map_or(Ok(()), Err)
        }
        fn link(&mut self, _: &str) -> Link {
            next(&mut self.links)
        }
        fn cycle(&mut self, _: &str) -> Result<(), String> {
            self.calls.push("cycle".into());
            Ok(())
        }
        fn online(&mut self) -> bool {
            next(&mut self.online)
        }
    }

    pub fn link(ip: &str, router: &str) -> Link {
        Link {
            ip: ip.parse().ok(),
            router: router.parse().ok(),
            ssid: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fakes::*;
    use super::*;

    fn home() -> Link {
        link("192.168.1.20", "192.168.1.1")
    }
    fn phone() -> Link {
        link("172.20.10.3", "172.20.10.1")
    }

    #[test]
    fn classifies_networksetup_output() {
        assert_eq!(classify(""), None);
        assert_eq!(classify("\n"), None);
        assert_eq!(
            classify("Could not find network My iPhone."),
            Some(JoinError::NotFound)
        );
        assert_eq!(
            classify("Failed to join network My iPhone.\nError: -3900  The operation couldn't be completed."),
            Some(JoinError::WrongPassword)
        );
        assert_eq!(
            classify("Failed to join network X.\nError: -3930 Operation not permitted"),
            Some(JoinError::Denied)
        );
        assert_eq!(
            classify("Wi-Fi power is currently turned off."),
            Some(JoinError::WifiOff)
        );
        assert_eq!(
            classify(
                "en7 is not a Wi-Fi interface.\n** Error: Error obtaining wireless information."
            ),
            Some(JoinError::Failed("en7 is not a Wi-Fi interface.".into()))
        );
        assert!(JoinError::WrongPassword.permanent());
        assert!(!JoinError::NotFound.permanent());
    }

    #[test]
    fn join_fails_when_the_mac_never_moved_even_if_internet_answers() {
        // The old code called this a success: the internet answered within
        // the settle window, through the network the Mac never left.
        let mut wifi = FakeWifi::new();
        wifi.links = vec![home()];
        wifi.online = vec![true];
        let clock = FakeClock::new();
        assert_eq!(
            join(&mut wifi, &clock, "My iPhone", None),
            Err(JoinError::NotAssociated)
        );
        assert!(clock.elapsed.get() >= ASSOCIATE_WITHIN);
    }

    #[test]
    fn join_succeeds_on_an_iphone_subnet() {
        let mut wifi = FakeWifi::new();
        // Before, two polls with nothing (DHCP), then the hotspot's lease.
        wifi.links = vec![home(), Link::default(), Link::default(), phone()];
        wifi.online = vec![false, true];
        let clock = FakeClock::new();
        let joined = join(&mut wifi, &clock, "My iPhone", Some("pw")).unwrap();
        assert_eq!(joined.record.ssid, "My iPhone");
        assert_eq!(joined.record.router, "172.20.10.1".parse().ok());
        assert_eq!(wifi.calls, ["associate My iPhone"]);
        assert_eq!(joined.took, Duration::from_secs(3));
    }

    #[test]
    fn join_trusts_a_visible_ssid() {
        let mut wifi = FakeWifi::new();
        let named = |name: &str| Link {
            ssid: Some(name.into()),
            ..home()
        };
        wifi.links = vec![named("Home"), named("Home")];
        wifi.online = vec![true];
        let clock = FakeClock::new();
        // Same router as before, but macOS says it's still on Home.
        assert_eq!(
            join(&mut wifi, &clock, "Pixel", None),
            Err(JoinError::NotAssociated)
        );
        // An Android hotspot on some other subnet, named.
        let mut wifi = FakeWifi::new();
        wifi.links = vec![
            named("Home"),
            Link {
                ssid: Some("Pixel".into()),
                ..link("10.42.0.7", "10.42.0.1")
            },
        ];
        wifi.online = vec![true];
        assert!(join(&mut wifi, &clock, "Pixel", None).is_ok());
    }

    #[test]
    fn join_reports_no_lease() {
        let mut wifi = FakeWifi::new();
        wifi.links = vec![home(), link("169.254.3.4", "")];
        let clock = FakeClock::new();
        assert_eq!(
            join(&mut wifi, &clock, "My iPhone", None),
            Err(JoinError::NoLease)
        );
    }

    #[test]
    fn join_reports_no_internet_and_macos_switching_back() {
        let mut wifi = FakeWifi::new();
        wifi.links = vec![home(), phone()];
        wifi.online = vec![false];
        let clock = FakeClock::new();
        assert_eq!(
            join(&mut wifi, &clock, "My iPhone", None),
            Err(JoinError::NoInternet)
        );
        assert!(clock.elapsed.get() >= INTERNET_WITHIN);

        let mut wifi = FakeWifi::new();
        wifi.links = vec![home(), phone(), home()];
        wifi.online = vec![true];
        assert_eq!(
            join(&mut wifi, &FakeClock::new(), "My iPhone", None),
            Err(JoinError::Reverted)
        );
    }

    #[test]
    fn join_stops_when_wifi_is_off_or_missing_or_networksetup_fails() {
        let clock = FakeClock::new();
        let mut wifi = FakeWifi::new();
        wifi.powered = Some(false);
        assert_eq!(join(&mut wifi, &clock, "X", None), Err(JoinError::WifiOff));
        assert!(wifi.calls.is_empty(), "never asks macOS to join");
        let mut wifi = FakeWifi::new();
        wifi.device = None;
        assert_eq!(join(&mut wifi, &clock, "X", None), Err(JoinError::NoWifi));
        let mut wifi = FakeWifi::new();
        wifi.associate = Some(JoinError::NotFound);
        assert_eq!(join(&mut wifi, &clock, "X", None), Err(JoinError::NotFound));
    }

    fn record() -> JoinRecord {
        JoinRecord {
            ssid: "My iPhone".into(),
            ip: "172.20.10.3".parse().ok(),
            router: "172.20.10.1".parse().ok(),
            at: 0,
        }
    }

    #[test]
    fn leave_skips_wifi_when_already_moved_or_off() {
        let clock = FakeClock::new();
        let mut wifi = FakeWifi::new();
        wifi.links = vec![home()];
        assert_eq!(
            leave(&mut wifi, &clock, Some(&record())),
            Ok(Left::AlreadyMoved)
        );
        assert!(wifi.calls.is_empty());
        let mut wifi = FakeWifi::new();
        wifi.powered = Some(false);
        assert_eq!(
            leave(&mut wifi, &clock, Some(&record())),
            Ok(Left::AlreadyMoved)
        );
        assert!(wifi.calls.is_empty());
        // Still on the hotspot: cycle, and macOS picks home.
        let mut wifi = FakeWifi::new();
        wifi.links = vec![phone(), Link::default(), home()];
        assert_eq!(leave(&mut wifi, &clock, Some(&record())), Ok(Left::Cycled));
        assert_eq!(wifi.calls, ["cycle"]);
        // No lease: can't tell, so cycle to let macOS pick.
        let mut wifi = FakeWifi::new();
        assert_eq!(leave(&mut wifi, &clock, Some(&record())), Ok(Left::Cycled));
        assert_eq!(wifi.calls, ["cycle"]);
    }

    #[test]
    fn leave_reports_macos_rejoining_the_hotspot() {
        let mut wifi = FakeWifi::new();
        wifi.links = vec![phone(), Link::default(), phone()];
        assert_eq!(
            leave(&mut wifi, &FakeClock::new(), Some(&record())),
            Ok(Left::Rejoined)
        );
    }
}
