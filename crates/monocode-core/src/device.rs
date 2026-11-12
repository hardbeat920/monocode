//! iOS Simulators and Android Emulators inside MonoCode: the device hub that
//! lists and streams them, the wire formats it speaks, the tools it runs, and
//! what agents are told when they open one.
pub mod hub;
pub mod protocol;
pub mod toolchain;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set in a provider's environment when device tools are offered to it.
pub const ENV: &str = "MONOCODE_DEVICES";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Ios,
    Android,
}

impl Platform {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ios => "iOS",
            Self::Android => "Android",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: String,
    pub platform: Platform,
    #[serde(default)]
    pub booted: bool,
    #[serde(default)]
    pub physical: bool,
    #[serde(default = "yes")]
    pub supported: bool,
}

fn yes() -> bool {
    true
}

impl Device {
    /// A phone on USB is on while it is plugged in: MonoCode can show and
    /// drive it, but not boot, shut down or delete it.
    pub fn ensure_virtual(&self, action: &str) -> Result<(), String> {
        if self.physical {
            Err(format!(
                "{} is a phone connected over USB; MonoCode can't {action} it.",
                self.name
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct DeviceList {
    pub devices: Vec<Device>,
    /// Problems listing one platform, such as a missing Android SDK.
    pub errors: Vec<String>,
}

impl DeviceList {
    pub fn from_hub(value: &Value) -> Result<Self, String> {
        let mut devices = Vec::new();
        for key in ["simulators", "emulators"] {
            for device in value[key].as_array().into_iter().flatten() {
                if let Ok(device) = serde_json::from_value::<Device>(device.clone()) {
                    devices.push(device);
                }
            }
        }
        // Booted first, then by name, so the one in use is at hand.
        devices.sort_by(|a, b| {
            b.booted
                .cmp(&a.booted)
                .then(a.platform.label().cmp(b.platform.label()))
                .then(a.name.cmp(&b.name))
        });
        let errors = value["errors"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|error| error["message"].as_str())
            // The SDK's absence is reported once, by the platform check.
            .filter(|message| !message.starts_with("[android-utils] "))
            .map(str::to_owned)
            .collect();
        Ok(Self { devices, errors })
    }
    pub fn find(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|device| device.id == id)
    }
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static AGENTS: AtomicBool = AtomicBool::new(false);

/// The app sets this from Settings › Devices. Nothing is installed or started
/// while it is off.
pub fn set_enabled(devices: bool, agents: bool) {
    let was = ENABLED.swap(devices, Ordering::Relaxed);
    AGENTS.store(devices && agents, Ordering::Relaxed);
    if was && !devices {
        hub::stop();
    }
}
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}
/// Whether agents get the device tools.
pub fn agents_enabled() -> bool {
    AGENTS.load(Ordering::Relaxed)
}

/// What `device_open` tells the agent: how to drive this device with the
/// pinned agent-device CLI.
pub fn quick_start(device: &Device, launcher: &std::path::Path, session: &str) -> Value {
    let quote = |text: &str| {
        if text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./:-".contains(c))
        {
            text.to_owned()
        } else {
            format!("'{}'", text.replace('\'', "'\"'\"'"))
        }
    };
    let executable = quote(&launcher.to_string_lossy());
    let target_args = match device.platform {
        Platform::Ios => vec![
            "--platform",
            "ios",
            "--udid",
            &device.id,
            "--session",
            session,
        ],
        Platform::Android => vec![
            "--platform",
            "android",
            "--serial",
            &device.id,
            "--session",
            session,
        ],
    };
    let target = target_args
        .iter()
        .map(|arg| quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let first_use = match device.platform {
        Platform::Ios => {
            "First use builds an XCTest runner and can take a couple of minutes; later commands are fast."
        }
        Platform::Android => "The Android snapshot helper installs itself on first use.",
    };
    let guide = [
        format!(
            "The user is watching {} ({}) in MonoCode's Device panel.",
            device.name, device.version
        ),
        format!(
            "Drive it with {executable}. Use this exact path; a login shell may reset PATH. Always pass {target}."
        ),
        "Typical loop:".into(),
        format!(
            "  {executable} open <bundle-or-package-id> {target}     # or: open <app> <deep-link-url>"
        ),
        format!(
            "  {executable} snapshot -i {target}                     # accessibility tree with @eN refs"
        ),
        format!("  {executable} click @e3 {target}"),
        format!("  {executable} fill @e5 \"text\" {target}"),
        format!("  {executable} install <app> <path-to-.app-or-.apk> {target}"),
        format!(
            "Prefer snapshot refs over coordinates. Call device_screenshot to see the screen. Run {executable} help for workflow guides and {executable} <command> --help for flags."
        ),
        "simctl, adb and xcrun stay available for builds, logs and ports.".into(),
        first_use.into(),
    ];
    json!({
        "device": device,
        "agentDevice": {"command": launcher, "targetArgs": target_args},
        "quickStart": guide.join("\n"),
    })
}

/// agent-device scopes its session to this name: one per thread and device.
/// The app actions agents reach through the MonoCode MCP server.
pub const ACTIONS: [&str; 5] = [
    "devices.list",
    "devices.open",
    "devices.screenshot",
    "devices.close",
    "devices.set",
];

/// What `devices.set` changes on an iOS Simulator: serve-sim's names for
/// its settings, and the simulated location.
pub const SETTINGS: [&str; 10] = [
    "appearance",
    "text-size",
    "color-filter",
    "liquid-glass",
    "reduce-motion",
    "increase-contrast",
    "reduce-transparency",
    "voiceover",
    "show-borders",
    "location",
];

/// Refuses input the app would not act on, before it crosses the bridge.
pub fn validate(action: &str, input: &Value) -> Result<(), String> {
    let fields: &[&str] = match action {
        "devices.list" => &[],
        "devices.open" => &["device", "platform"],
        "devices.screenshot" | "devices.close" => &["device"],
        "devices.set" => &["device", "setting", "value"],
        _ => return Err("Unknown device action".into()),
    };
    let object = input.as_object().ok_or("Input must be an object")?;
    for key in object.keys() {
        if !fields.contains(&key.as_str()) {
            return Err(format!("{action} does not take {key}"));
        }
    }
    if let Some(device) = object.get("device") {
        let device = device.as_str().ok_or("device must be a string")?;
        if device.trim().is_empty() || device.len() > 256 {
            return Err("device must be 1 to 256 characters".into());
        }
    } else if !matches!(
        action,
        "devices.list" | "devices.screenshot" | "devices.set"
    ) {
        return Err(format!("{action} needs device"));
    }
    if let Some(platform) = object.get("platform") {
        if !matches!(platform.as_str(), Some("ios" | "android")) {
            return Err("platform must be ios or android".into());
        }
    }
    if action == "devices.set" {
        let setting = object
            .get("setting")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !SETTINGS.contains(&setting) {
            return Err(format!("setting must be one of {}", SETTINGS.join(", ")));
        }
        let value = object
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if value.trim().is_empty() || value.len() > 64 {
            return Err("value must be 1 to 64 characters".into());
        }
    }
    Ok(())
}

/// The device `query` names: an id, else a name ("iPhone 17 Pro"), on
/// `platform` when given. A booted match wins over one that is off.
pub fn resolve<'a>(
    list: &'a DeviceList,
    query: &str,
    platform: Option<Platform>,
) -> Option<&'a Device> {
    let query = query.trim();
    let candidates = || {
        list.devices
            .iter()
            .filter(move |d| d.supported && platform.is_none_or(|p| d.platform == p))
    };
    candidates()
        .find(|d| d.id == query)
        .or_else(|| candidates().find(|d| d.booted && d.name.eq_ignore_ascii_case(query)))
        .or_else(|| candidates().find(|d| d.name.eq_ignore_ascii_case(query)))
}

pub fn session_name(session: i64, device: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("{session}:{device}").as_bytes());
    let hex: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("mc-{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hub_lists_become_booted_first_with_usb_phones() {
        let value = json!({
            "simulators": [
                {"id":"A","name":"iPhone 17","version":"iOS 27.0","platform":"ios","booted":false},
                {"id":"B","name":"iPad","version":"iOS 27.0","platform":"ios","booted":true}
            ],
            "emulators": [
                {"id":"emulator-5554","name":"Pixel","platform":"android","booted":true,"physical":false},
                {"id":"R5C","name":"Phone","platform":"android","booted":true,"physical":true}
            ],
            "errors": [{"message":"[android-utils] adb not found"},{"message":"other"}]
        });
        let list = DeviceList::from_hub(&value).unwrap();
        let ids: Vec<_> = list.devices.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, ["R5C", "emulator-5554", "B", "A"]);
        assert!(list.find("R5C").is_some_and(|d| d.physical));
        assert_eq!(list.errors, ["other"]);
        assert!(list.find("A").is_some_and(|d| d.supported));
    }

    #[test]
    fn devices_are_found_by_id_or_name() {
        let device = |id: &str, name: &str, platform, booted| Device {
            id: id.into(),
            name: name.into(),
            version: String::new(),
            platform,
            booted,
            physical: false,
            supported: true,
        };
        let list = DeviceList {
            devices: vec![
                device("A", "iPhone 17", Platform::Ios, false),
                device("B", "iPhone 17", Platform::Ios, true),
                device("emulator-5554", "Pixel 9", Platform::Android, true),
            ],
            errors: Vec::new(),
        };
        assert_eq!(resolve(&list, "A", None).unwrap().id, "A");
        assert_eq!(resolve(&list, "iphone 17", None).unwrap().id, "B");
        assert_eq!(
            resolve(&list, "Pixel 9", Some(Platform::Android))
                .unwrap()
                .id,
            "emulator-5554"
        );
        assert!(resolve(&list, "Pixel 9", Some(Platform::Ios)).is_none());
        assert!(validate("devices.open", &json!({})).is_err());
        assert!(validate("devices.open", &json!({"device":"A","x":1})).is_err());
        assert!(validate("devices.screenshot", &json!({})).is_ok());
        assert!(validate("devices.open", &json!({"device":"A","platform":"web"})).is_err());
    }

    #[test]
    fn device_settings_name_a_known_setting_and_a_value() {
        assert!(validate(
            "devices.set",
            &json!({"setting":"appearance","value":"dark"})
        )
        .is_ok());
        assert!(validate(
            "devices.set",
            &json!({"device":"B","setting":"location","value":"35.6,139.6"})
        )
        .is_ok());
        assert!(validate("devices.set", &json!({"setting":"volume","value":"1"})).is_err());
        assert!(validate("devices.set", &json!({"setting":"appearance"})).is_err());
        assert!(validate("devices.set", &json!({"setting":"appearance","value":" "})).is_err());
        assert!(validate(
            "devices.set",
            &json!({"setting":"appearance","value":"dark","x":1})
        )
        .is_err());
    }

    #[test]
    fn quick_start_quotes_the_launcher_and_pins_the_device() {
        let device = Device {
            id: "0E9B".into(),
            name: "iPhone".into(),
            version: "iOS 27.0".into(),
            platform: Platform::Ios,
            booted: true,
            physical: false,
            supported: true,
        };
        let value = quick_start(
            &device,
            std::path::Path::new(
                "/Users/me/Library/Application Support/MonoCode/devices/bin/agent-device",
            ),
            "mc-1",
        );
        let text = value["quickStart"].as_str().unwrap();
        assert!(text.contains("'/Users/me/Library/Application Support/MonoCode/devices/bin/agent-device' snapshot -i --platform ios --udid 0E9B --session mc-1"));
        assert_eq!(session_name(1, "A"), session_name(1, "A"));
        assert_ne!(session_name(1, "A"), session_name(2, "A"));
    }
}
