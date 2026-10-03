//! The OS side of attention for this process. `NativePlatform` calls
//! `UNUserNotificationCenter`, which raises an Objective-C exception in a
//! process without an app bundle (`cargo run`, tests). Outside a bundle the
//! app keeps the Dock badge and sounds and reports notifications as
//! unsupported.

use monocode_engine::attention::notifications::{NotificationPermission, NotificationText};
use monocode_engine::attention::sound_synth::SoundName;
use monocode_engine::attention::{AttentionPlatform, NativePlatform};

/// Whether the executable runs from a macOS app bundle. Always true on
/// other platforms, whose notification APIs need no bundle.
pub fn running_in_bundle() -> bool {
    if !cfg!(target_os = "macos") {
        return true;
    }
    std::env::current_exe()
        .map(|path| path.to_string_lossy().contains(".app/Contents/MacOS/"))
        .unwrap_or(false)
}

/// `NativePlatform` without notification calls.
pub struct UnbundledPlatform {
    pub native: NativePlatform,
    /// Play cues. Tests turn them off.
    pub sounds: bool,
}

impl AttentionPlatform for UnbundledPlatform {
    fn notification_permission(&self) -> NotificationPermission {
        NotificationPermission::Unsupported
    }

    fn request_notification_permission(&self) -> NotificationPermission {
        NotificationPermission::Unsupported
    }

    fn open_notification_settings(&self) -> Result<(), String> {
        Err("Notifications need the MonoCode app bundle".into())
    }

    fn show_notification(
        &self,
        _session_id: &str,
        _text: &NotificationText,
        _sound: bool,
    ) -> Result<(), String> {
        Err("Notifications need the MonoCode app bundle".into())
    }

    fn set_dock_badge(&self, count: u32) {
        self.native.set_dock_badge(count);
    }

    fn play_sound(&self, sound: SoundName, volume: f64) {
        if self.sounds {
            self.native.play_sound(sound, volume);
        }
    }
}
