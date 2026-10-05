//! System-wide shortcuts. Port of the shortcut half of
//! src-tauri/src/quick_composer.rs (`parse_shortcut`, the plugin handler,
//! and `quick_composer_set_enabled`) over the `global-hotkey` crate, which
//! the Tauri plugin wrapped. Shortcut strings keep the Tauri format, such as
//! `Command+Shift+Space` or `Control+Option+KeyK`.
//!
//! On macOS, create [`GlobalHotkeys::new`] on the main thread. Presses arrive
//! inside a Carbon event callback on the main thread, so a callback must not
//! block or re-enter the UI. Send the press into a channel and handle it on
//! the app's executor. A panic in a callback is caught and logged, because
//! unwinding into the C frame would abort the process.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex, Once};

use global_hotkey::hotkey::Modifiers;
pub use global_hotkey::hotkey::{Code, HotKey};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

/// `DEFAULT_SHORTCUT`.
pub const DEFAULT_SHORTCUT: &str = "Command+Shift+Space";

/// Runs on every press of a registered shortcut.
pub type HotkeyCallback = Arc<dyn Fn() + Send + Sync + 'static>;

/// `parse_shortcut`: a shortcut needs Command or Control, so a plain
/// letter or Shift chord can never be claimed system-wide.
pub fn parse_shortcut(value: &str) -> Result<HotKey, String> {
    let shortcut = HotKey::from_str(value).map_err(|err| err.to_string())?;
    if !shortcut
        .mods
        .intersects(Modifiers::SUPER | Modifiers::CONTROL)
    {
        return Err("Use Command or Control with another key.".into());
    }
    Ok(shortcut)
}

/// What [`GlobalHotkeys`] asks of the operating system. [`SystemRegistrar`]
/// is the real one; tests pass a fake.
pub trait HotkeyRegistrar {
    fn register(&self, hotkey: HotKey) -> Result<(), String>;
    fn unregister(&self, hotkey: HotKey) -> Result<(), String>;
}

/// The OS registrar: `global_hotkey::GlobalHotKeyManager`.
pub struct SystemRegistrar(GlobalHotKeyManager);

impl SystemRegistrar {
    /// On macOS, call on the main thread.
    pub fn new() -> Result<Self, String> {
        GlobalHotKeyManager::new()
            .map(Self)
            .map_err(|err| err.to_string())
    }
}

impl HotkeyRegistrar for SystemRegistrar {
    fn register(&self, hotkey: HotKey) -> Result<(), String> {
        self.0.register(hotkey).map_err(|err| err.to_string())
    }

    fn unregister(&self, hotkey: HotKey) -> Result<(), String> {
        self.0.unregister(hotkey).map_err(|err| err.to_string())
    }
}

/// Callbacks by hotkey id. `global-hotkey` takes one process-wide handler,
/// so every [`GlobalHotkeys`] routes through this table.
fn routes() -> &'static Mutex<HashMap<u32, HotkeyCallback>> {
    static ROUTES: std::sync::OnceLock<Mutex<HashMap<u32, HotkeyCallback>>> =
        std::sync::OnceLock::new();
    ROUTES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn install_dispatch() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        GlobalHotKeyEvent::set_event_handler(Some(dispatch));
    });
}

/// Route one OS event to its callback. Only presses count, as in the Tauri
/// handler; releases are ignored. Public so tests can feed events without
/// pressing keys.
pub fn dispatch(event: GlobalHotKeyEvent) {
    if event.state != HotKeyState::Pressed {
        return;
    }
    let callback = routes()
        .lock()
        .ok()
        .and_then(|routes| routes.get(&event.id).cloned());
    if let Some(callback) = callback
        && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| callback())).is_err()
    {
        eprintln!("monocode: global shortcut handler panicked");
    }
}

/// Registered shortcuts and their callbacks.
pub struct GlobalHotkeys<R: HotkeyRegistrar = SystemRegistrar> {
    registrar: R,
    active: Mutex<HashMap<u32, HotKey>>,
}

impl GlobalHotkeys<SystemRegistrar> {
    /// The OS hotkeys. On macOS, call on the main thread.
    pub fn new() -> Result<Self, String> {
        Ok(Self::with_registrar(SystemRegistrar::new()?))
    }
}

impl<R: HotkeyRegistrar> GlobalHotkeys<R> {
    pub fn with_registrar(registrar: R) -> Self {
        install_dispatch();
        Self {
            registrar,
            active: Mutex::new(HashMap::new()),
        }
    }

    /// Claim `shortcut` system-wide and call `on_press` on each press.
    /// Registering a shortcut this instance already holds replaces its
    /// callback.
    pub fn register(&self, shortcut: &str, on_press: HotkeyCallback) -> Result<HotKey, String> {
        let hotkey = parse_shortcut(shortcut)?;
        self.register_hotkey(hotkey, on_press)?;
        Ok(hotkey)
    }

    fn register_hotkey(&self, hotkey: HotKey, on_press: HotkeyCallback) -> Result<(), String> {
        let mut active = self.active.lock().map_err(|err| err.to_string())?;
        if let std::collections::hash_map::Entry::Vacant(slot) = active.entry(hotkey.id()) {
            self.registrar
                .register(hotkey)
                .map_err(|err| format!("Could not claim {hotkey}: {err}"))?;
            slot.insert(hotkey);
        }
        routes()
            .lock()
            .map_err(|err| err.to_string())?
            .insert(hotkey.id(), on_press);
        Ok(())
    }

    /// Release `shortcut`. Releasing one this instance does not hold is a
    /// no-op.
    pub fn unregister(&self, shortcut: &str) -> Result<(), String> {
        self.unregister_hotkey(parse_shortcut(shortcut)?)
    }

    fn unregister_hotkey(&self, hotkey: HotKey) -> Result<(), String> {
        let mut active = self.active.lock().map_err(|err| err.to_string())?;
        if active.remove(&hotkey.id()).is_none() {
            return Ok(());
        }
        if let Err(err) = self.registrar.unregister(hotkey) {
            active.insert(hotkey.id(), hotkey);
            return Err(format!("Could not release {hotkey}: {err}"));
        }
        if let Ok(mut routes) = routes().lock() {
            routes.remove(&hotkey.id());
        }
        Ok(())
    }

    /// Move a callback from `from` to `to`. The replacement is claimed
    /// first. If the OS rejects it, or `from` cannot be released, `from`
    /// stays active, so a failed edit cannot strand the shortcut.
    pub fn change(&self, from: &str, to: &str, on_press: HotkeyCallback) -> Result<HotKey, String> {
        let next = parse_shortcut(to)?;
        self.change_hotkey(parse_shortcut(from)?, next, on_press)?;
        Ok(next)
    }

    fn change_hotkey(
        &self,
        previous: HotKey,
        next: HotKey,
        on_press: HotkeyCallback,
    ) -> Result<(), String> {
        if previous == next {
            return self.register_hotkey(next, on_press);
        }
        let held_next = self.is_registered_hotkey(next);
        self.register_hotkey(next, on_press)?;
        if let Err(err) = self.unregister_hotkey(previous) {
            if !held_next {
                let _ = self.unregister_hotkey(next);
            }
            return Err(err);
        }
        Ok(())
    }

    /// This instance holds `shortcut`.
    pub fn is_registered(&self, shortcut: &str) -> bool {
        parse_shortcut(shortcut).is_ok_and(|hotkey| self.is_registered_hotkey(hotkey))
    }

    fn is_registered_hotkey(&self, hotkey: HotKey) -> bool {
        self.active
            .lock()
            .is_ok_and(|active| active.contains_key(&hotkey.id()))
    }

    /// Release every shortcut this instance holds.
    pub fn unregister_all(&self) {
        let held: Vec<HotKey> = self
            .active
            .lock()
            .map(|active| active.values().copied().collect())
            .unwrap_or_default();
        for hotkey in held {
            let _ = self.unregister_hotkey(hotkey);
        }
    }
}

impl<R: HotkeyRegistrar> Drop for GlobalHotkeys<R> {
    fn drop(&mut self) {
        self.unregister_all();
    }
}

/// What [`ShortcutSlot::set_enabled`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotChange {
    /// Nothing changed.
    Unchanged,
    /// The shortcut was claimed or moved.
    Claimed(HotKey),
    /// The shortcut was released. The Tauri app also hid the panel.
    Released,
}

/// The quick composer's one shortcut: `quick_composer_set_enabled`. Every
/// workspace window reports the setting on boot, so this is idempotent.
#[derive(Debug, Default)]
pub struct ShortcutSlot {
    current: Option<HotKey>,
}

impl ShortcutSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// The claimed shortcut.
    pub fn current(&self) -> Option<HotKey> {
        self.current
    }

    /// Claim `shortcut` (the default when `None`) when `enabled`, else
    /// release whatever is claimed. A rejected replacement keeps the old
    /// shortcut active.
    pub fn set_enabled<R: HotkeyRegistrar>(
        &mut self,
        hotkeys: &GlobalHotkeys<R>,
        enabled: bool,
        shortcut: Option<&str>,
        on_press: HotkeyCallback,
    ) -> Result<SlotChange, String> {
        let next = parse_shortcut(shortcut.unwrap_or(DEFAULT_SHORTCUT))?;
        if enabled {
            if self.current == Some(next) {
                return Ok(SlotChange::Unchanged);
            }
            match self.current {
                Some(previous) => hotkeys.change_hotkey(previous, next, on_press)?,
                None => hotkeys.register_hotkey(next, on_press)?,
            }
            self.current = Some(next);
            return Ok(SlotChange::Claimed(next));
        }
        let Some(previous) = self.current else {
            return Ok(SlotChange::Unchanged);
        };
        hotkeys.unregister_hotkey(previous)?;
        self.current = None;
        Ok(SlotChange::Released)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[derive(Default)]
    struct FakeRegistrar {
        held: RefCell<HashSet<u32>>,
        calls: RefCell<Vec<String>>,
        reject: RefCell<HashSet<u32>>,
        stuck: RefCell<HashSet<u32>>,
    }

    impl HotkeyRegistrar for FakeRegistrar {
        fn register(&self, hotkey: HotKey) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("+{hotkey}"));
            if self.reject.borrow().contains(&hotkey.id()) {
                return Err("taken".into());
            }
            self.held.borrow_mut().insert(hotkey.id());
            Ok(())
        }

        fn unregister(&self, hotkey: HotKey) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("-{hotkey}"));
            if self.stuck.borrow().contains(&hotkey.id()) {
                return Err("busy".into());
            }
            self.held.borrow_mut().remove(&hotkey.id());
            Ok(())
        }
    }

    fn noop() -> HotkeyCallback {
        Arc::new(|| {})
    }

    #[test]
    fn quick_composer_shortcut_requires_a_non_shift_modifier() {
        let original = parse_shortcut(DEFAULT_SHORTCUT).unwrap();
        assert_eq!(original.key, Code::Space);
        assert_eq!(original.mods, Modifiers::SUPER | Modifiers::SHIFT);
        let custom = parse_shortcut("Control+Option+KeyK").unwrap();
        assert_eq!(custom.key, Code::KeyK);
        assert_eq!(custom.mods, Modifiers::CONTROL | Modifiers::ALT);
        assert_eq!(
            parse_shortcut("Command+KeyK").unwrap().mods,
            Modifiers::SUPER
        );
        assert_eq!(
            parse_shortcut("Control+KeyK").unwrap().mods,
            Modifiers::CONTROL
        );
        assert!(parse_shortcut("Shift+Space").is_err());
        assert!(parse_shortcut("Option+KeyK").is_err());
        assert!(parse_shortcut("Command+InvalidKey").is_err());
    }

    #[test]
    fn presses_reach_their_callback_and_releases_do_not() {
        let hotkeys = GlobalHotkeys::with_registrar(FakeRegistrar::default());
        let presses = Arc::new(AtomicUsize::new(0));
        let counter = presses.clone();
        let hotkey = hotkeys
            .register(
                "Control+Option+Shift+Command+F13",
                Arc::new(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                }),
            )
            .unwrap();
        dispatch(GlobalHotKeyEvent {
            id: hotkey.id(),
            state: HotKeyState::Pressed,
        });
        dispatch(GlobalHotKeyEvent {
            id: hotkey.id(),
            state: HotKeyState::Released,
        });
        assert_eq!(presses.load(Ordering::SeqCst), 1);
        hotkeys
            .unregister("Control+Option+Shift+Command+F13")
            .unwrap();
        dispatch(GlobalHotKeyEvent {
            id: hotkey.id(),
            state: HotKeyState::Pressed,
        });
        assert_eq!(presses.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_panicking_callback_is_contained() {
        let hotkeys = GlobalHotkeys::with_registrar(FakeRegistrar::default());
        let hotkey = hotkeys
            .register(
                "Control+Option+Shift+Command+F14",
                Arc::new(|| panic!("handler bug")),
            )
            .unwrap();
        dispatch(GlobalHotKeyEvent {
            id: hotkey.id(),
            state: HotKeyState::Pressed,
        });
    }

    #[test]
    fn changing_claims_the_replacement_before_releasing_the_old_one() {
        let hotkeys = GlobalHotkeys::with_registrar(FakeRegistrar::default());
        hotkeys
            .register("Control+Option+Shift+Command+F15", noop())
            .unwrap();
        hotkeys
            .change(
                "Control+Option+Shift+Command+F15",
                "Control+Option+Shift+Command+F16",
                noop(),
            )
            .unwrap();
        assert_eq!(
            *hotkeys.registrar.calls.borrow(),
            vec![
                "+shift+control+alt+super+F15",
                "+shift+control+alt+super+F16",
                "-shift+control+alt+super+F15",
            ]
        );
        assert!(hotkeys.is_registered("Control+Option+Shift+Command+F16"));
        assert!(!hotkeys.is_registered("Control+Option+Shift+Command+F15"));
    }

    #[test]
    fn a_rejected_replacement_keeps_the_old_shortcut() {
        let fake = FakeRegistrar::default();
        let rejected = parse_shortcut("Control+Option+Shift+Command+F18").unwrap();
        fake.reject.borrow_mut().insert(rejected.id());
        let hotkeys = GlobalHotkeys::with_registrar(fake);
        let mut slot = ShortcutSlot::new();
        slot.set_enabled(
            &hotkeys,
            true,
            Some("Control+Option+Shift+Command+F17"),
            noop(),
        )
        .unwrap();
        let err = slot
            .set_enabled(
                &hotkeys,
                true,
                Some("Control+Option+Shift+Command+F18"),
                noop(),
            )
            .unwrap_err();
        assert!(err.contains("Could not claim"));
        assert!(hotkeys.is_registered("Control+Option+Shift+Command+F17"));
        assert_eq!(
            slot.current(),
            Some(parse_shortcut("Control+Option+Shift+Command+F17").unwrap())
        );
    }

    #[test]
    fn a_stuck_old_shortcut_rolls_the_replacement_back() {
        let fake = FakeRegistrar::default();
        let stuck = parse_shortcut("Control+Option+Shift+Command+F19").unwrap();
        fake.stuck.borrow_mut().insert(stuck.id());
        let hotkeys = GlobalHotkeys::with_registrar(fake);
        hotkeys
            .register("Control+Option+Shift+Command+F19", noop())
            .unwrap();
        let err = hotkeys
            .change(
                "Control+Option+Shift+Command+F19",
                "Control+Option+Shift+Command+F20",
                noop(),
            )
            .unwrap_err();
        assert!(err.contains("Could not release"));
        assert!(hotkeys.is_registered("Control+Option+Shift+Command+F19"));
        assert!(!hotkeys.is_registered("Control+Option+Shift+Command+F20"));
        hotkeys.registrar.stuck.borrow_mut().clear();
    }

    #[test]
    fn the_slot_is_idempotent_and_releases_when_disabled() {
        let hotkeys = GlobalHotkeys::with_registrar(FakeRegistrar::default());
        let mut slot = ShortcutSlot::new();
        let shortcut = Some("Control+Option+Shift+Command+F21");
        assert!(matches!(
            slot.set_enabled(&hotkeys, true, shortcut, noop()).unwrap(),
            SlotChange::Claimed(_)
        ));
        assert_eq!(
            slot.set_enabled(&hotkeys, true, shortcut, noop()).unwrap(),
            SlotChange::Unchanged
        );
        assert_eq!(hotkeys.registrar.calls.borrow().len(), 1);
        assert_eq!(
            slot.set_enabled(&hotkeys, false, shortcut, noop()).unwrap(),
            SlotChange::Released
        );
        assert_eq!(
            slot.set_enabled(&hotkeys, false, shortcut, noop()).unwrap(),
            SlotChange::Unchanged
        );
        assert!(!hotkeys.is_registered("Control+Option+Shift+Command+F21"));
        assert!(
            slot.set_enabled(&hotkeys, true, Some("Shift+KeyK"), noop())
                .is_err()
        );
    }

    #[test]
    fn dropping_releases_every_shortcut() {
        let hotkeys = GlobalHotkeys::with_registrar(FakeRegistrar::default());
        hotkeys
            .register("Control+Option+Shift+Command+F22", noop())
            .unwrap();
        hotkeys.unregister_all();
        assert!(hotkeys.registrar.held.borrow().is_empty());
    }
}
