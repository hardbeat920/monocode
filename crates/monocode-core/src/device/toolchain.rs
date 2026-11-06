//! The device tools MonoCode runs with the user's Node: expo-device-hub
//! streams simulators and emulators, agent-device lets agents drive them.
//! Each is npm-installed at a pinned version, into a staging directory that is
//! moved into place only once it is complete.
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Hub,
    AgentDevice,
}

impl Tool {
    pub const ALL: [Tool; 2] = [Tool::Hub, Tool::AgentDevice];
    pub fn package(self) -> &'static str {
        match self {
            Self::Hub => "expo-device-hub",
            Self::AgentDevice => "agent-device",
        }
    }
    /// Pinned: the hub's wire formats are not a stable API, and
    /// `protocol.rs` speaks this version's.
    pub fn version(self) -> &'static str {
        match self {
            Self::Hub => "0.19.0-canary-20261023-1357114",
            Self::AgentDevice => "0.21.31",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Hub => "Device Hub",
            Self::AgentDevice => "agent-device",
        }
    }
    fn entry(self) -> &'static [&'static str] {
        match self {
            Self::Hub => &["dist", "server", "cli.mjs"],
            Self::AgentDevice => &["bin", "agent-device.mjs"],
        }
    }
    pub fn install_dir(self) -> PathBuf {
        root()
            .join("tools")
            .join(self.package())
            .join(self.version())
    }
    pub fn entry_path(self) -> PathBuf {
        let mut path = self.install_dir().join("node_modules").join(self.package());
        for part in self.entry() {
            path.push(part);
        }
        path
    }
    pub fn installed(self) -> bool {
        self.entry_path().is_file()
            && std::fs::read_to_string(self.install_dir().join(SENTINEL))
                .is_ok_and(|text| text.trim() == self.version())
    }
}

const SENTINEL: &str = ".install-complete";

/// Where the tools, the hub's state and agent-device's launcher live.
pub fn root() -> PathBuf {
    crate::store::monocode_database().with_file_name("devices")
}

/// The PATH device tools run with: what a terminal would have, plus the
/// Android SDK's tools.
pub fn search_path() -> String {
    let mut parts: Vec<PathBuf> = Vec::new();
    if let Some(sdk) = android_sdk() {
        parts.push(sdk.join("platform-tools"));
        parts.push(sdk.join("emulator"));
    }
    parts.extend(std::env::split_paths(&crate::scm::shell::gui_search_path()));
    std::env::join_paths(parts)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&search_path())
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

pub fn node() -> Option<PathBuf> {
    which("node")
}

/// The Android SDK the emulator tools come from: `ANDROID_HOME`, the SDK
/// Manager's default location, or wherever `adb` on PATH lives.
pub fn android_sdk() -> Option<PathBuf> {
    let has_tools = |root: &Path| {
        root.join("platform-tools/adb").is_file() || root.join("emulator/emulator").is_file()
    };
    for key in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(root) = std::env::var_os(key).filter(|v| !v.is_empty()) {
            return Some(PathBuf::from(root));
        }
    }
    let home = crate::accounts::home()?;
    for candidate in [home.join("Library/Android/sdk"), home.join("Android/Sdk")] {
        if has_tools(&candidate) {
            return Some(candidate);
        }
    }
    let adb = std::env::split_paths(&crate::scm::shell::gui_search_path())
        .map(|dir| dir.join("adb"))
        .find(|path| path.is_file())?;
    let resolved = std::fs::canonicalize(&adb).unwrap_or(adb);
    let root = resolved.parent()?.parent()?.to_path_buf();
    has_tools(&root).then_some(root)
}

/// What this Mac has for each platform, shown in Settings and the panel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Doctor {
    pub node: Option<(PathBuf, String)>,
    pub xcode: bool,
    pub android_sdk: Option<PathBuf>,
    pub ffmpeg: bool,
}

impl Doctor {
    /// Blocking: runs `node --version` and `xcrun --find simctl`.
    pub fn check() -> Self {
        let node = node().and_then(|path| {
            let output = Command::new(&path)
                .arg("--version")
                .env("PATH", search_path())
                .output()
                .ok()?;
            output.status.success().then(|| {
                let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                (path, version)
            })
        });
        let xcode = cfg!(target_os = "macos")
            && Command::new("/usr/bin/xcrun")
                .args(["--find", "simctl"])
                .output()
                .is_ok_and(|output| output.status.success());
        Self {
            node,
            xcode,
            android_sdk: android_sdk(),
            ffmpeg: which("ffmpeg").is_some(),
        }
    }
    /// Node 22.12 or newer, which agent-device asks for.
    pub fn node_supported(&self) -> bool {
        self.node
            .as_ref()
            .and_then(|(_, version)| parse_version(version))
            .is_some_and(|version| version >= (22, 12))
    }
}

fn parse_version(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// One install at a time: two would race for the same directory.
static INSTALLING: Mutex<()> = Mutex::new(());

/// Blocking: installs `tool` unless it already is. Runs npm from the user's
/// Node, so nothing is downloaded but the pinned package and its tree.
pub fn install(tool: Tool) -> Result<PathBuf, String> {
    let _guard = INSTALLING.lock().unwrap_or_else(|e| e.into_inner());
    if tool.installed() {
        return Ok(tool.entry_path());
    }
    let node = node().ok_or("MonoCode needs Node.js for device tools. Install it from nodejs.org or with Homebrew, then try again.")?;
    let npm = node
        .parent()
        .map(|dir| dir.join("npm"))
        .filter(|path| path.is_file())
        .or_else(|| which("npm"))
        .ok_or("npm was not found beside Node.js.")?;
    let final_dir = tool.install_dir();
    let parent = final_dir.parent().ok_or("Invalid tools directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&final_dir);
    let staging = tempfile::Builder::new()
        .prefix(".staging-")
        .tempdir_in(parent)
        .map_err(|e| e.to_string())?;
    let mut command = Command::new(&npm);
    command
        .args(["install", "--prefix"])
        .arg(staging.path())
        .args([
            "--no-fund",
            "--no-audit",
            "--no-update-notifier",
            "--loglevel=error",
        ])
        .arg(format!("{}@{}", tool.package(), tool.version()))
        .env("PATH", search_path())
        .current_dir(staging.path());
    crate::process::configure_child(&mut command);
    let output = command
        .output()
        .map_err(|e| format!("Could not run npm: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.lines().rev().take(6).collect::<Vec<_>>();
        let detail = detail.into_iter().rev().collect::<Vec<_>>().join("\n");
        return Err(format!(
            "npm could not install {}.\n{detail}",
            tool.package()
        ));
    }
    let mut staged = staging.path().join("node_modules").join(tool.package());
    for part in tool.entry() {
        staged.push(part);
    }
    if !staged.is_file() {
        return Err(format!(
            "{} installed without its entry point.",
            tool.package()
        ));
    }
    std::fs::write(
        staging.path().join(SENTINEL),
        format!("{}\n", tool.version()),
    )
    .map_err(|e| e.to_string())?;
    let staging = staging.keep();
    if let Err(error) = std::fs::rename(&staging, &final_dir) {
        let _ = std::fs::remove_dir_all(&staging);
        if !tool.installed() {
            return Err(format!(
                "Could not move {} into place: {error}",
                tool.package()
            ));
        }
    }
    remove_other_versions(tool);
    Ok(tool.entry_path())
}

/// Older pinned versions are dead weight once this one is in place.
fn remove_other_versions(tool: Tool) {
    let Some(parent) = tool.install_dir().parent().map(Path::to_path_buf) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name != tool.version() && !name.to_string_lossy().starts_with(".staging-") {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Removes every installed tool. The hub must be stopped first.
pub fn remove_all() -> Result<(), String> {
    let _guard = INSTALLING.lock().unwrap_or_else(|e| e.into_inner());
    match std::fs::remove_dir_all(root().join("tools")) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

/// A launcher for agent-device that agents run by its full path, so a login
/// shell resetting PATH cannot lose it. It runs the pinned CLI with the
/// user's Node.
pub fn agent_launcher() -> Result<PathBuf, String> {
    if !Tool::AgentDevice.installed() {
        return Err(
            "agent-device is not installed. Turn on agent access in Settings › Devices.".into(),
        );
    }
    let node = node().ok_or("Node.js was not found.")?;
    let dir = root().join("bin");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let quote = |path: &Path| format!("'{}'", path.to_string_lossy().replace('\'', "'\"'\"'"));
    let script = format!(
        "#!/bin/sh\nPATH={}\nexport PATH\nexport AGENT_DEVICE_NO_UPDATE_NOTIFIER=1\nexec {} {} \"$@\"\n",
        quote(Path::new(&search_path())),
        quote(&node),
        quote(&Tool::AgentDevice.entry_path()),
    );
    let path = dir.join("agent-device");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(script.as_str()) {
        let temporary = dir.join(".agent-device.tmp");
        std::fs::write(&temporary, &script).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
        std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_versions_compare_by_major_and_minor() {
        assert_eq!(parse_version("v22.11.0"), Some((22, 11)));
        let doctor = |version: &str| Doctor {
            node: Some(("node".into(), version.into())),
            ..Default::default()
        };
        assert!(!doctor("v22.11.0").node_supported());
        assert!(doctor("v22.12.0").node_supported());
        assert!(doctor("v24.1.0").node_supported());
        assert!(!Doctor::default().node_supported());
    }

    #[test]
    fn tools_live_under_their_pinned_version() {
        let entry = Tool::Hub.entry_path();
        assert!(entry.ends_with("node_modules/expo-device-hub/dist/server/cli.mjs"));
        assert!(entry.to_string_lossy().contains(Tool::Hub.version()));
    }
}
