//! The OS side of attention: notification permission and banners, the Dock
//! badge, and short sounds. These were Tauri commands (`show_notification`,
//! `set_dock_badge`, ...) and the cuelume package. `NativePlatform` calls
//! `monocode_platform`; tests use `testing::FakePlatform`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;

use parking_lot::Mutex;

use super::notifications::{NotificationPermission, NotificationText};
use super::sound_synth::{self, SoundName};

/// What the attention entities ask of the OS.
///
/// Methods marked blocking run on the background executor. `set_dock_badge`
/// runs on the main thread. `play_sound` returns at once.
pub trait AttentionPlatform: Send + Sync {
    /// `notification_permission`. Blocking.
    fn notification_permission(&self) -> NotificationPermission;

    /// `request_notification_permission`: shows the OS prompt when undecided.
    /// Blocking.
    fn request_notification_permission(&self) -> NotificationPermission;

    /// `open_notification_settings`. Blocking.
    fn open_notification_settings(&self) -> Result<(), String>;

    /// `show_notification`. Returns once the OS scheduled the banner.
    /// Blocking.
    fn show_notification(
        &self,
        session_id: &str,
        text: &NotificationText,
        sound: bool,
    ) -> Result<(), String>;

    /// `set_dock_badge`: this window's pending approval count.
    fn set_dock_badge(&self, count: u32);

    /// Play a cue at `volume` (0 to 1).
    fn play_sound(&self, sound: SoundName, volume: f64);
}

/// The real OS calls.
pub struct NativePlatform {
    app_id: String,
    badge_label: String,
    on_click: monocode_platform::notifications::ClickHandler,
    sounds: SystemSoundPlayer,
}

impl NativePlatform {
    /// `app_id` is the bundle identifier (`com.monocode.desktop`).
    /// `badge_label` names this window's share of the Dock badge. `on_click`
    /// receives the session id of a clicked banner, on any thread.
    pub fn new(
        app_id: impl Into<String>,
        badge_label: impl Into<String>,
        on_click: monocode_platform::notifications::ClickHandler,
    ) -> Self {
        Self {
            app_id: app_id.into(),
            badge_label: badge_label.into(),
            on_click,
            sounds: SystemSoundPlayer::new(std::env::temp_dir().join("monocode-sounds")),
        }
    }

    /// Install the macOS click delegate and request badge authorization.
    /// Call once on the main thread after launch; elsewhere it does nothing.
    pub fn install(&self) {
        #[cfg(target_os = "macos")]
        {
            monocode_platform::notifications::install_delegate(self.on_click.clone());
            monocode_platform::macos::request_badge_authorization();
        }
    }
}

impl AttentionPlatform for NativePlatform {
    fn notification_permission(&self) -> NotificationPermission {
        monocode_platform::notifications::notification_permission(&self.app_id).into()
    }

    fn request_notification_permission(&self) -> NotificationPermission {
        monocode_platform::notifications::request_notification_permission(&self.app_id).into()
    }

    fn open_notification_settings(&self) -> Result<(), String> {
        monocode_platform::notifications::open_notification_settings(&self.app_id)
    }

    fn show_notification(
        &self,
        session_id: &str,
        text: &NotificationText,
        sound: bool,
    ) -> Result<(), String> {
        monocode_platform::notifications::show_notification(
            &self.app_id,
            &self.on_click,
            session_id,
            &text.title,
            &text.subtitle,
            &text.body,
            sound,
        )
    }

    fn set_dock_badge(&self, count: u32) {
        #[cfg(target_os = "macos")]
        monocode_platform::macos::paint_window_badge(&self.badge_label, count);
        #[cfg(not(target_os = "macos"))]
        let _ = (&self.badge_label, count);
    }

    fn play_sound(&self, sound: SoundName, volume: f64) {
        self.sounds.play(sound, volume);
    }
}

/// Plays rendered cues through the player every desktop already has:
/// `afplay` on macOS, `paplay`, `pw-play`, or `aplay` on Linux, and
/// `System.Media.SoundPlayer` on Windows. Each cue is rendered once per
/// volume to a WAV file in `dir`.
///
/// This avoids an audio stack dependency (cpal or rodio, both permissive)
/// and its Linux ALSA build requirement. An in-process player can replace it
/// behind `AttentionPlatform::play_sound`.
pub struct SystemSoundPlayer {
    dir: PathBuf,
    rendered: Arc<Mutex<HashMap<(SoundName, u32), PathBuf>>>,
}

impl SystemSoundPlayer {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            rendered: Arc::default(),
        }
    }

    fn file(&self, sound: SoundName, volume: f64) -> Option<PathBuf> {
        let key = (sound, (volume.clamp(0.0, 1.0) * 1000.0).round() as u32);
        if let Some(path) = self.rendered.lock().get(&key) {
            return Some(path.clone());
        }
        std::fs::create_dir_all(&self.dir).ok()?;
        let path = self.dir.join(format!("{}-{}.wav", sound.as_str(), key.1));
        let bytes = sound_synth::wav_bytes(&sound_synth::render(sound, volume));
        let temp = path.with_extension(format!("wav.{}", std::process::id()));
        std::fs::write(&temp, bytes).ok()?;
        std::fs::rename(&temp, &path).ok()?;
        self.rendered.lock().insert(key, path.clone());
        Some(path)
    }

    /// Render if needed and play on a helper thread. Failures are silent: a
    /// cue never surfaces an error.
    pub fn play(&self, sound: SoundName, volume: f64) {
        if volume <= 0.0 {
            return;
        }
        let player = Self {
            dir: self.dir.clone(),
            rendered: self.rendered.clone(),
        };
        let _ = std::thread::Builder::new()
            .name("monocode-sound".into())
            .spawn(move || {
                if let Some(path) = player.file(sound, volume) {
                    play_file(&path);
                }
            });
    }
}

fn run(mut command: Command) -> bool {
    monocode_platform::hide_window_console(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn play_file(path: &std::path::Path) {
    if cfg!(target_os = "macos") {
        let mut command = Command::new("/usr/bin/afplay");
        command.arg(path);
        run(command);
    } else if cfg!(windows) {
        let script = format!(
            "(New-Object System.Media.SoundPlayer '{}').PlaySync()",
            path.display().to_string().replace('\'', "''")
        );
        let mut command = Command::new("powershell");
        command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        run(command);
    } else {
        for player in ["paplay", "pw-play", "aplay"] {
            let mut command = Command::new(player);
            if player == "aplay" {
                command.arg("-q");
            }
            command.arg(path);
            if run(command) {
                break;
            }
        }
    }
}
