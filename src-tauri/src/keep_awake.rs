use std::collections::HashMap;
use std::sync::Mutex;

use tauri::WebviewWindow;

const REASON: &str = "MonoCode agent is working";

/// Invoke `set_keep_awake` with `{ enabled, display }` from each workspace
/// window when its own agent activity changes. Repeated values are safe. A
/// window's `false` only releases that window's claim. Display preference is
/// stored per window and OR-aggregated onto the shared native request.
#[tauri::command]
pub fn set_keep_awake(
    window: WebviewWindow,
    enabled: bool,
    display: bool,
    state: tauri::State<'_, KeepAwakeState>,
) -> Result<(), String> {
    state.set(window.label(), enabled, display)
}

trait RequestBackend {
    type Handle;

    fn acquire(&self, display: bool) -> Result<Self::Handle, String>;
    fn release(&self, handle: Self::Handle) -> Result<(), String>;
}

struct Claims<B: RequestBackend> {
    backend: B,
    windows: HashMap<String, bool>,
    request: Option<B::Handle>,
    display: bool,
}

impl<B: RequestBackend> Claims<B> {
    fn new(backend: B) -> Self {
        Self {
            backend,
            windows: HashMap::new(),
            request: None,
            display: false,
        }
    }

    fn desired_display(&self) -> bool {
        self.windows.values().any(|&display| display)
    }

    fn in_sync(&self) -> bool {
        if self.windows.is_empty() {
            self.request.is_none()
        } else {
            self.request.is_some() && self.display == self.desired_display()
        }
    }

    fn restore_window(&mut self, window: &str, previous: Option<bool>) {
        match previous {
            Some(display) => {
                self.windows.insert(window.to_owned(), display);
            }
            None => {
                self.windows.remove(window);
            }
        }
    }

    fn sync_request(&mut self) -> Result<(), String> {
        if self.windows.is_empty() {
            self.display = false;
            if let Some(request) = self.request.take() {
                self.backend.release(request)?;
            }
            return Ok(());
        }
        let display = self.desired_display();
        if self.request.is_some() && self.display == display {
            return Ok(());
        }
        // Hold the current request until its replacement exists so a failed
        // reacquire cannot leave remaining windows unprotected.
        let next = self.backend.acquire(display)?;
        if let Some(previous) = self.request.replace(next) {
            self.display = display;
            self.backend.release(previous)?;
        } else {
            self.display = display;
        }
        Ok(())
    }

    fn sync_or_restore(&mut self, window: &str, previous: Option<bool>) -> Result<(), String> {
        match self.sync_request() {
            Ok(()) => Ok(()),
            Err(error) => {
                if !self.in_sync() {
                    self.restore_window(window, previous);
                }
                Err(error)
            }
        }
    }

    fn set(&mut self, window: &str, enabled: bool, display: bool) -> Result<(), String> {
        if enabled {
            if self.windows.get(window) == Some(&display) && self.request.is_some() {
                if !self.in_sync() {
                    return self.sync_request();
                }
                return Ok(());
            }
            let previous = self.windows.insert(window.to_owned(), display);
            self.sync_or_restore(window, previous)
        } else if let Some(previous) = self.windows.remove(window) {
            self.sync_or_restore(window, Some(previous))
        } else if !self.in_sync() {
            self.sync_request()
        } else {
            Ok(())
        }
    }

    fn window_closed(&mut self, window: &str) -> Result<(), String> {
        let removed = self.windows.remove(window).is_some();
        if removed || !self.in_sync() {
            self.sync_request()
        } else {
            Ok(())
        }
    }
    fn release_all(&mut self) -> Result<(), String> {
        self.windows.clear();
        self.display = false;
        if let Some(request) = self.request.take() {
            self.backend.release(request)?;
        }
        Ok(())
    }
}

#[cfg(windows)]
mod windows_backend {
    use std::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Power::{
        PowerClearRequest, PowerCreateRequest, PowerRequestDisplayRequired,
        PowerRequestSystemRequired, PowerSetRequest,
    };
    use windows_sys::Win32::System::Threading::{
        POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
    };

    use super::{RequestBackend, REASON};

    pub struct WindowsPowerBackend;

    pub(super) struct WindowsHandle {
        handle: OwnedHandle,
        display: bool,
    }

    impl RequestBackend for WindowsPowerBackend {
        type Handle = WindowsHandle;

        fn acquire(&self, display: bool) -> Result<WindowsHandle, String> {
            let mut reason: Vec<u16> = format!("{REASON}\0").encode_utf16().collect();
            let context = REASON_CONTEXT {
                Version: 0,
                Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
                Reason: REASON_CONTEXT_0 {
                    SimpleReasonString: reason.as_mut_ptr(),
                },
            };
            // SAFETY: `reason` stays alive for the PowerCreateRequest call.
            let raw = unsafe { PowerCreateRequest(&context) };
            if raw == INVALID_HANDLE_VALUE {
                return Err(format!(
                    "PowerCreateRequest: {}",
                    io::Error::last_os_error()
                ));
            }
            // OwnedHandle closes the request even if PowerSetRequest fails.
            // SAFETY: PowerCreateRequest returned a valid handle.
            let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
            // SAFETY: handle is an open power request created above.
            if unsafe { PowerSetRequest(handle.as_raw_handle(), PowerRequestSystemRequired) } == 0 {
                return Err(format!("PowerSetRequest: {}", io::Error::last_os_error()));
            }
            if display
                && unsafe { PowerSetRequest(handle.as_raw_handle(), PowerRequestDisplayRequired) }
                    == 0
            {
                return Err(format!(
                    "PowerSetRequest display: {}",
                    io::Error::last_os_error()
                ));
            }
            Ok(WindowsHandle { handle, display })
        }

        fn release(&self, handle: WindowsHandle) -> Result<(), String> {
            let mut error = None;
            if handle.display {
                // SAFETY: handle is the power request acquired by this backend.
                if unsafe {
                    PowerClearRequest(handle.handle.as_raw_handle(), PowerRequestDisplayRequired)
                } == 0
                {
                    error = Some(io::Error::last_os_error());
                }
            }
            // SAFETY: handle is the power request acquired by this backend.
            if unsafe {
                PowerClearRequest(handle.handle.as_raw_handle(), PowerRequestSystemRequired)
            } == 0
            {
                error = Some(io::Error::last_os_error());
            }
            // Closing the handle removes the request even if clearing failed.
            drop(handle.handle);
            match error {
                Some(error) => Err(format!("PowerClearRequest: {error}")),
                None => Ok(()),
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod macos_backend {
    use std::ffi::{c_char, c_void, CString};
    use std::ptr;

    use super::{RequestBackend, REASON};

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const K_IOPM_ASSERTION_LEVEL_ON: u32 = 255;
    const K_IO_RETURN_SUCCESS: i32 = 0;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            c_str: *const c_char,
            encoding: u32,
        ) -> *const c_void;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithName(
            assertion_type: *const c_void,
            assertion_level: u32,
            assertion_name: *const c_void,
            assertion_id: *mut u32,
        ) -> i32;
        fn IOPMAssertionRelease(assertion_id: u32) -> i32;
    }

    struct CfString(*const c_void);

    impl CfString {
        fn from_str(value: &str) -> Result<Self, String> {
            let c_str =
                CString::new(value).map_err(|_| "keep-awake reason contained NUL".to_string())?;
            // SAFETY: c_str is a valid C string for the duration of this call.
            let raw = unsafe {
                CFStringCreateWithCString(ptr::null(), c_str.as_ptr(), K_CF_STRING_ENCODING_UTF8)
            };
            if raw.is_null() {
                Err("CFStringCreateWithCString failed".into())
            } else {
                Ok(Self(raw))
            }
        }
    }

    impl Drop for CfString {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: 0 is a CFString created by CFStringCreateWithCString.
                unsafe { CFRelease(self.0) };
            }
        }
    }

    pub struct MacosIdleBackend;

    pub(super) struct MacosHandle {
        system: u32,
        display: Option<u32>,
    }

    fn create_assertion(kind: &str) -> Result<u32, String> {
        let assertion_type = CfString::from_str(kind)?;
        let assertion_name = CfString::from_str(REASON)?;
        let mut assertion_id = 0u32;
        // SAFETY: both CFStrings are live, non-null, and assertion_id is writable.
        let status = unsafe {
            IOPMAssertionCreateWithName(
                assertion_type.0,
                K_IOPM_ASSERTION_LEVEL_ON,
                assertion_name.0,
                &mut assertion_id,
            )
        };
        if status != K_IO_RETURN_SUCCESS {
            return Err(format!("IOPMAssertionCreateWithName: {status}"));
        }
        Ok(assertion_id)
    }

    impl RequestBackend for MacosIdleBackend {
        type Handle = MacosHandle;

        fn acquire(&self, display: bool) -> Result<MacosHandle, String> {
            let system = create_assertion("PreventUserIdleSystemSleep")?;
            let display = if display {
                match create_assertion("PreventUserIdleDisplaySleep") {
                    Ok(id) => Some(id),
                    Err(error) => {
                        // SAFETY: system is an assertion created above.
                        let _ = unsafe { IOPMAssertionRelease(system) };
                        return Err(error);
                    }
                }
            } else {
                None
            };
            Ok(MacosHandle { system, display })
        }

        fn release(&self, handle: MacosHandle) -> Result<(), String> {
            let mut error = None;
            if let Some(display) = handle.display {
                // SAFETY: display is an assertion id created by IOPMAssertionCreateWithName.
                let status = unsafe { IOPMAssertionRelease(display) };
                if status != K_IO_RETURN_SUCCESS {
                    error = Some(format!("IOPMAssertionRelease display: {status}"));
                }
            }
            // SAFETY: system is an assertion id created by IOPMAssertionCreateWithName.
            let status = unsafe { IOPMAssertionRelease(handle.system) };
            if status != K_IO_RETURN_SUCCESS {
                error = Some(format!("IOPMAssertionRelease: {status}"));
            }
            match error {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod linux_backend {
    use zbus::blocking::Connection;
    use zbus::zvariant::OwnedFd;

    use super::{RequestBackend, REASON};

    pub struct LinuxIdleBackend;

    struct ScreensaverInhibit {
        conn: Connection,
        cookie: u32,
    }

    impl Drop for ScreensaverInhibit {
        fn drop(&mut self) {
            let _ = self.conn.call_method(
                Some("org.freedesktop.ScreenSaver"),
                "/org/freedesktop/ScreenSaver",
                Some("org.freedesktop.ScreenSaver"),
                "UnInhibit",
                &(self.cookie,),
            );
        }
    }

    /// Desktop-session suspend inhibitor (GNOME `org.gnome.SessionManager`).
    ///
    /// GNOME can suspend the session after inactivity even when logind's `idle`
    /// inhibitor is held. Inhibit suspend instead of session idleness so GNOME
    /// can still lock the screen automatically. Non-GNOME desktops do not have
    /// this D-Bus service.
    struct SessionInhibit {
        conn: Connection,
        cookie: u32,
    }

    impl Drop for SessionInhibit {
        fn drop(&mut self) {
            let _ = self.conn.call_method(
                Some("org.gnome.SessionManager"),
                "/org/gnome/SessionManager",
                Some("org.gnome.SessionManager"),
                "Uninhibit",
                &(self.cookie,),
            );
        }
    }

    pub(super) struct LinuxHandle {
        idle_fd: OwnedFd,
        session_suspend: Option<SessionInhibit>,
        display: Option<ScreensaverInhibit>,
    }

    fn inhibit_idle() -> Result<OwnedFd, String> {
        let conn = Connection::system().map_err(|error| error.to_string())?;
        let reply = conn
            .call_method(
                Some("org.freedesktop.login1"),
                "/org/freedesktop/login1",
                Some("org.freedesktop.login1.Manager"),
                "Inhibit",
                &("idle", "MonoCode", REASON, "block"),
            )
            .map_err(|error| error.to_string())?;
        reply
            .body()
            .deserialize()
            .map_err(|error| error.to_string())
    }

    /// Best-effort GNOME session suspend inhibitor (flag `4`).
    ///
    /// Returns `None` on non-GNOME desktops where the service is absent.
    fn inhibit_session_suspend() -> Option<SessionInhibit> {
        let conn = Connection::session().ok()?;
        let reply = conn
            .call_method(
                Some("org.gnome.SessionManager"),
                "/org/gnome/SessionManager",
                Some("org.gnome.SessionManager"),
                "Inhibit",
                // (app_id, toplevel_xid, reason, flags): flag 4 = suspend
                &("MonoCode", 0u32, REASON, 4u32),
            )
            .ok()?;
        let cookie: u32 = reply.body().deserialize().ok()?;
        Some(SessionInhibit { conn, cookie })
    }

    fn inhibit_screensaver() -> Result<ScreensaverInhibit, String> {
        let conn = Connection::session().map_err(|error| error.to_string())?;
        let reply = conn
            .call_method(
                Some("org.freedesktop.ScreenSaver"),
                "/org/freedesktop/ScreenSaver",
                Some("org.freedesktop.ScreenSaver"),
                "Inhibit",
                &("MonoCode", REASON),
            )
            .map_err(|error| error.to_string())?;
        let cookie = reply
            .body()
            .deserialize()
            .map_err(|error| error.to_string())?;
        Ok(ScreensaverInhibit { conn, cookie })
    }

    impl RequestBackend for LinuxIdleBackend {
        type Handle = LinuxHandle;

        fn acquire(&self, display: bool) -> Result<LinuxHandle, String> {
            let idle_fd = inhibit_idle()?;
            let session_suspend = inhibit_session_suspend();
            if !display {
                return Ok(LinuxHandle {
                    idle_fd,
                    session_suspend,
                    display: None,
                });
            }
            match inhibit_screensaver() {
                Ok(screensaver) => Ok(LinuxHandle {
                    idle_fd,
                    session_suspend,
                    display: Some(screensaver),
                }),
                Err(error) => {
                    drop(session_suspend);
                    drop(idle_fd);
                    Err(error)
                }
            }
        }

        fn release(&self, handle: LinuxHandle) -> Result<(), String> {
            let LinuxHandle {
                idle_fd,
                session_suspend,
                display,
            } = handle;
            drop(display);
            drop(session_suspend);
            drop(idle_fd);
            Ok(())
        }
    }
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
mod platform {
    use super::*;

    #[cfg(windows)]
    type Backend = super::windows_backend::WindowsPowerBackend;
    #[cfg(target_os = "macos")]
    type Backend = super::macos_backend::MacosIdleBackend;
    #[cfg(target_os = "linux")]
    type Backend = super::linux_backend::LinuxIdleBackend;

    #[cfg(windows)]
    fn new_backend() -> Backend {
        super::windows_backend::WindowsPowerBackend
    }
    #[cfg(target_os = "macos")]
    fn new_backend() -> Backend {
        super::macos_backend::MacosIdleBackend
    }
    #[cfg(target_os = "linux")]
    fn new_backend() -> Backend {
        super::linux_backend::LinuxIdleBackend
    }

    pub struct KeepAwakeState(Mutex<Claims<Backend>>);

    impl KeepAwakeState {
        pub fn new() -> Self {
            Self(Mutex::new(Claims::new(new_backend())))
        }

        pub fn set(&self, window: &str, enabled: bool, display: bool) -> Result<(), String> {
            match self.0.lock() {
                Ok(mut claims) => claims.set(window, enabled, display),
                Err(poisoned) => {
                    let _ = poisoned.into_inner().release_all();
                    Err("keep-awake state was poisoned".into())
                }
            }
        }

        pub fn window_closed(&self, window: &str) {
            let result = match self.0.lock() {
                Ok(mut claims) => claims.window_closed(window),
                Err(poisoned) => {
                    let _ = poisoned.into_inner().release_all();
                    Err("keep-awake state was poisoned".into())
                }
            };
            if let Err(error) = result {
                eprintln!("keep-awake window cleanup: {error}");
            }
        }

        pub fn shutdown(&self) {
            let result = match self.0.lock() {
                Ok(mut claims) => claims.release_all(),
                Err(poisoned) => poisoned.into_inner().release_all(),
            };
            if let Err(error) = result {
                eprintln!("keep-awake shutdown cleanup: {error}");
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod platform {
    pub struct KeepAwakeState;

    impl KeepAwakeState {
        pub fn new() -> Self {
            Self
        }

        pub fn set(&self, _window: &str, _enabled: bool, _display: bool) -> Result<(), String> {
            Ok(())
        }

        pub fn window_closed(&self, _window: &str) {}

        pub fn shutdown(&self) {}
    }
}

pub use platform::KeepAwakeState;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Default)]
    struct MockBackend {
        acquired: AtomicUsize,
        released: AtomicUsize,
        fail_acquire: AtomicBool,
        fail_release: AtomicBool,
    }

    impl RequestBackend for MockBackend {
        type Handle = ();

        fn acquire(&self, _display: bool) -> Result<(), String> {
            if self.fail_acquire.load(Ordering::SeqCst) {
                return Err("acquire failed".into());
            }
            self.acquired.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        fn release(&self, _handle: ()) -> Result<(), String> {
            self.released.fetch_add(1, Ordering::SeqCst);
            if self.fail_release.load(Ordering::SeqCst) {
                return Err("release failed".into());
            }
            Ok(())
        }
    }

    #[test]
    fn claims_are_idempotent_and_owned_by_window() {
        let mut claims = Claims::new(MockBackend::default());
        claims.set("first", true, false).unwrap();
        claims.set("first", true, false).unwrap();
        claims.set("second", true, false).unwrap();
        assert_eq!(claims.backend.acquired.load(Ordering::SeqCst), 1);

        claims.set("first", false, false).unwrap();
        claims.set("first", false, false).unwrap();
        assert_eq!(claims.backend.released.load(Ordering::SeqCst), 0);
        assert!(claims.windows.contains_key("second"));

        claims.set("second", false, false).unwrap();
        assert_eq!(claims.backend.released.load(Ordering::SeqCst), 1);
        assert!(claims.request.is_none());
    }

    #[test]
    fn failed_acquire_does_not_claim_window() {
        let mut claims = Claims::new(MockBackend::default());
        claims.backend.fail_acquire.store(true, Ordering::SeqCst);
        assert!(claims.set("first", true, false).is_err());
        assert!(claims.windows.is_empty());
        assert!(claims.request.is_none());
        claims.backend.fail_acquire.store(false, Ordering::SeqCst);
        claims.set("first", true, false).unwrap();
        assert_eq!(claims.backend.acquired.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failed_release_and_shutdown_drop_the_claim() {
        let mut claims = Claims::new(MockBackend::default());
        claims.set("first", true, false).unwrap();
        claims.backend.fail_release.store(true, Ordering::SeqCst);
        assert!(claims.set("first", false, false).is_err());
        assert!(claims.windows.is_empty());
        assert!(claims.request.is_none());

        claims.backend.fail_release.store(false, Ordering::SeqCst);
        claims.set("first", true, false).unwrap();
        claims.set("second", true, false).unwrap();
        claims.release_all().unwrap();
        assert!(claims.windows.is_empty());
        assert!(claims.request.is_none());
        assert_eq!(claims.backend.released.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn changing_display_reacquires_the_request() {
        let mut claims = Claims::new(MockBackend::default());
        claims.set("first", true, false).unwrap();
        assert_eq!(claims.backend.acquired.load(Ordering::SeqCst), 1);
        claims.set("first", true, true).unwrap();
        assert_eq!(claims.backend.released.load(Ordering::SeqCst), 1);
        assert_eq!(claims.backend.acquired.load(Ordering::SeqCst), 2);
        assert!(claims.display);
    }

    #[test]
    fn mixed_window_display_preferences_keep_the_stronger_claim() {
        let mut claims = Claims::new(MockBackend::default());
        claims.set("first", true, true).unwrap();
        claims.set("second", true, false).unwrap();
        assert_eq!(claims.backend.acquired.load(Ordering::SeqCst), 1);
        assert_eq!(claims.backend.released.load(Ordering::SeqCst), 0);
        assert!(claims.display);

        claims.set("second", false, false).unwrap();
        assert_eq!(claims.backend.acquired.load(Ordering::SeqCst), 1);
        assert_eq!(claims.backend.released.load(Ordering::SeqCst), 0);
        assert!(claims.display);

        claims.set("second", true, false).unwrap();
        claims.set("first", false, false).unwrap();
        assert_eq!(claims.backend.released.load(Ordering::SeqCst), 1);
        assert_eq!(claims.backend.acquired.load(Ordering::SeqCst), 2);
        assert!(!claims.display);

        claims.set("first", true, true).unwrap();
        assert_eq!(claims.backend.released.load(Ordering::SeqCst), 2);
        assert_eq!(claims.backend.acquired.load(Ordering::SeqCst), 3);
        assert!(claims.display);
    }

    #[test]
    fn failed_display_change_can_retry_without_losing_other_window() {
        let mut claims = Claims::new(MockBackend::default());
        claims.set("first", true, true).unwrap();
        claims.set("second", true, false).unwrap();

        claims.backend.fail_acquire.store(true, Ordering::SeqCst);
        assert!(claims.set("first", false, false).is_err());
        assert!(claims.windows.contains_key("second"));
        assert!(claims.request.is_some());

        claims.backend.fail_acquire.store(false, Ordering::SeqCst);
        claims.set("first", false, false).unwrap();
        assert!(claims.request.is_some());
        assert!(claims.windows.contains_key("second"));
        assert!(!claims.windows.contains_key("first"));
        assert!(!claims.display);
    }

    #[test]
    fn failed_sync_on_window_closed_does_not_restore_closed_window_and_reconciles() {
        let mut claims = Claims::new(MockBackend::default());
        claims.set("first", true, true).unwrap();
        claims.set("second", true, false).unwrap();
        assert!(claims.display);

        claims.backend.fail_acquire.store(true, Ordering::SeqCst);
        assert!(claims.window_closed("first").is_err());
        assert!(!claims.windows.contains_key("first"));
        assert!(claims.windows.contains_key("second"));
        assert!(!claims.in_sync());
        assert!(claims.display);

        claims.backend.fail_acquire.store(false, Ordering::SeqCst);
        claims.set("second", true, false).unwrap();
        assert!(claims.in_sync());
        assert!(!claims.display);
        assert!(claims.windows.contains_key("second"));
        assert!(!claims.windows.contains_key("first"));
    }
}
