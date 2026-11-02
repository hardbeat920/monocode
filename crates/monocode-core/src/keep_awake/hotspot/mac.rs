//! The real [`Wifi`]: `networksetup` and `ipconfig`, which need no Location
//! access. Every command has a time limit, and joins and leaves take one
//! lock, so the settings test and an automatic join never interleave.
use super::cmd::{self, Ran};
use super::join::{self, JoinError, JoinRecord, Joined, Left, Link, SystemClock, Wifi};
use super::{parse_current_network, parse_saved_networks, parse_wifi_device, parse_wifi_power};
use std::sync::Mutex;
use std::time::Duration;

const NETWORKSETUP: &str = "/usr/sbin/networksetup";
const IPCONFIG: &str = "/usr/sbin/ipconfig";
const KEYCHAIN: &str = "com.monocode.hotspot";
/// Reading state is quick; a stuck one is a sick Wi-Fi stack.
const READ_LIMIT: Duration = Duration::from_secs(5);
/// Scanning and associating can take a while.
const JOIN_LIMIT: Duration = Duration::from_secs(45);

static NETWORK: Mutex<()> = Mutex::new(());

fn read(program: &str, args: &[&str]) -> Option<String> {
    cmd::run(program, args, READ_LIMIT).ok()
}

#[derive(Default)]
pub struct MacWifi {
    device: Option<String>,
}
impl Wifi for MacWifi {
    fn device(&mut self) -> Option<String> {
        if self.device.is_none() {
            self.device = wifi_device();
        }
        self.device.clone()
    }
    fn powered(&mut self, device: &str) -> Option<bool> {
        parse_wifi_power(&read(NETWORKSETUP, &["-getairportpower", device])?)
    }
    fn associate(
        &mut self,
        device: &str,
        ssid: &str,
        password: Option<&str>,
    ) -> Result<(), JoinError> {
        let mut args = vec!["-setairportnetwork", device, ssid];
        if let Some(password) = password {
            args.push(password);
        }
        let (ok, out) = match cmd::run(NETWORKSETUP, &args, JOIN_LIMIT) {
            Ran::Exited { ok, out } => (ok, out),
            Ran::TimedOut => return Err(JoinError::TimedOut),
            Ran::NotRun => return Err(JoinError::Failed("couldn't run networksetup.".into())),
        };
        // Never let the password reach a message or the log.
        let out = match password {
            Some(p) if !p.is_empty() => out.replace(p, "•••"),
            _ => out,
        };
        match join::classify(&out) {
            Some(error) => Err(error),
            None if !ok => Err(JoinError::Failed(
                "networksetup exited with an error.".into(),
            )),
            None => Ok(()),
        }
    }
    fn link(&mut self, device: &str) -> Link {
        let addr = |args: &[&str]| read(IPCONFIG, args).and_then(|s| s.trim().parse().ok());
        Link {
            ip: addr(&["getifaddr", device]),
            router: addr(&["getoption", device, "router"]),
            ssid: read(NETWORKSETUP, &["-getairportnetwork", device])
                .and_then(|s| parse_current_network(&s)),
        }
    }
    fn cycle(&mut self, device: &str) -> Result<(), String> {
        let power =
            |on: &str| cmd::run(NETWORKSETUP, &["-setairportpower", device, on], READ_LIMIT);
        power("off").ok().ok_or("Couldn't turn Wi-Fi off.")?;
        std::thread::sleep(Duration::from_secs(1));
        if power("on").ok().is_none() {
            // Wi-Fi left off is worse than any blip: one more try.
            std::thread::sleep(Duration::from_secs(1));
            power("on").ok().ok_or("Couldn't turn Wi-Fi back on.")?;
        }
        Ok(())
    }
    fn online(&mut self) -> bool {
        super::online(Duration::from_secs(3))
    }
}

pub fn wifi_device() -> Option<String> {
    parse_wifi_device(&read(NETWORKSETUP, &["-listallhardwareports"])?)
}

/// Networks this Mac remembers, most preferred first.
pub fn saved_networks() -> Vec<String> {
    wifi_device()
        .and_then(|device| read(NETWORKSETUP, &["-listpreferredwirelessnetworks", &device]))
        .map(|list| parse_saved_networks(&list))
        .unwrap_or_default()
}

/// Joins `ssid` and checks the Mac moved and the internet answers. A saved
/// password, if any, goes to `networksetup`; without one macOS uses what it
/// remembers.
pub fn join(ssid: &str) -> Result<Joined, JoinError> {
    let _one = NETWORK.lock().unwrap_or_else(|e| e.into_inner());
    let password = password(ssid);
    join::join(
        &mut MacWifi::default(),
        &SystemClock,
        ssid,
        password.as_deref(),
    )
}

/// Back to the usual network, unless the Mac already left `record`'s.
pub fn leave(record: Option<&JoinRecord>) -> Result<Left, String> {
    let _one = NETWORK.lock().unwrap_or_else(|e| e.into_inner());
    join::leave(&mut MacWifi::default(), &SystemClock, record)
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
