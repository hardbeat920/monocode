//! Port of the platform switches in src/platform/tauri/platform.ts.
//!
//! The TypeScript read `navigator.platform` at load time. Here the platform is
//! a value, so settings tables and shortcut labels can be computed for any
//! platform in tests.

/// Which desktop the app runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Platform {
    Mac,
    Windows,
    Linux,
}

impl Platform {
    /// The platform this binary was built for.
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Platform::Mac
        } else if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Linux
        }
    }

    pub const fn is_mac(self) -> bool {
        matches!(self, Platform::Mac)
    }

    pub const fn is_windows(self) -> bool {
        matches!(self, Platform::Windows)
    }

    pub const fn is_linux(self) -> bool {
        matches!(self, Platform::Linux)
    }

    /// `HAS_NATIVE_GLASS`: native blur on macOS and Windows, transparency-only
    /// glass on Linux.
    pub const fn has_native_glass(self) -> bool {
        true
    }

    /// `MOD`: the primary shortcut modifier as it appears in key labels.
    pub const fn mod_label(self) -> &'static str {
        if self.is_mac() { "⌘" } else { "Ctrl+" }
    }

    /// `ALT`.
    pub const fn alt_label(self) -> &'static str {
        if self.is_mac() { "⌥" } else { "Alt+" }
    }

    /// `SHIFT`.
    pub const fn shift_label(self) -> &'static str {
        if self.is_mac() { "⇧" } else { "Shift+" }
    }

    /// `CTRL` in settings.ts: the Control key, which is not the primary
    /// modifier on macOS.
    pub const fn ctrl_label(self) -> &'static str {
        if self.is_mac() { "⌃" } else { "Ctrl+" }
    }
}

impl Default for Platform {
    fn default() -> Self {
        Platform::current()
    }
}
