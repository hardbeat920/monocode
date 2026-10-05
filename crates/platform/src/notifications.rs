//! Desktop notifications for turns that end or stall while the window is in
//! the background.
//!
//! macOS goes through `UNUserNotificationCenter` directly: the app already
//! links it for the Dock badge, it reports the real authorization state, and
//! a delegate turns a click into a jump back to the session. Linux uses the
//! freedesktop notification bus, which has no permission model. Windows uses
//! the WinRT toast API via `tauri-winrt-notification`, which likewise has no
//! runtime permission prompt: toasts are controlled from Windows Settings.
//!
//! Moved from src-tauri/src/notifications.rs. Every call blocks; callers run
//! them off the UI thread. `app_id` is the bundle identifier, and a
//! `ClickHandler` receives the identifier of a clicked notification.

use std::sync::Arc;

use serde::Serialize;

/// Receives the identifier (session id or reminder id) of a clicked
/// notification.
pub type ClickHandler = Arc<dyn Fn(&str) + Send + Sync>;

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn handle_click(on_click: &ClickHandler, identifier: &str) {
    on_click(identifier);
}

#[cfg(target_os = "macos")]
pub use platform::install_delegate;

/// Each platform constructs only the variants it can reach, so the lint is
/// silenced for the whole enum rather than per target.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    /// Never asked, or not yet answered.
    Prompt,
    Granted,
    /// Declined at the prompt, or alerts switched off in System Settings.
    Denied,
    /// No notification backend on this platform.
    Unsupported,
}

pub fn notification_permission(app_id: &str) -> Permission {
    platform::permission(app_id)
}

pub fn request_notification_permission(app_id: &str) -> Permission {
    platform::request_permission(app_id)
}

/// What a caller reports when the blocking task that asked for permission
/// failed, matching each platform's fallback before the move.
pub fn permission_fallback() -> Permission {
    platform::PERMISSION_FALLBACK
}

/// Returns only once the platform reports the banner as scheduled: the
/// frontend skips its own turn-finished cue on success, so returning early
/// would silence a turn that never got a notification.
pub fn show_notification(
    app_id: &str,
    on_click: &ClickHandler,
    session_id: &str,
    title: &str,
    subtitle: &str,
    body: &str,
    sound: bool,
) -> Result<(), String> {
    platform::show(app_id, on_click, session_id, title, subtitle, body, sound)
}

/// Opens the app's page in the OS notification settings, where the user can
/// re-enable alerts after declining the prompt.
pub fn open_notification_settings(app_id: &str) -> Result<(), String> {
    platform::open_settings(app_id)
}

#[cfg(target_os = "macos")]
mod platform {
    use std::cell::RefCell;
    use std::ptr::NonNull;
    use std::sync::mpsc;
    use std::time::Duration;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{AnyThread, DefinedClass, MainThreadMarker, define_class};
    use objc2_foundation::{NSArray, NSError, NSSet, NSString};
    use objc2_user_notifications::{
        UNAlertStyle, UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent,
        UNNotification, UNNotificationAction, UNNotificationActionOptions, UNNotificationCategory,
        UNNotificationCategoryOptions, UNNotificationPresentationOptions, UNNotificationRequest,
        UNNotificationResponse, UNNotificationSetting, UNNotificationSettings, UNNotificationSound,
        UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };

    use super::{ClickHandler, Permission};

    pub(super) const PERMISSION_FALLBACK: Permission = Permission::Denied;

    /// Request identifiers carry the session so a click can find it without
    /// touching `userInfo`. Each request gets a fresh suffix: reusing one
    /// replaces the previous banner, and macOS drops replacements that land
    /// while the app is frontmost.
    const ID_PREFIX: &str = "session:";

    fn request_identifier(session_id: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        format!("{ID_PREFIX}{session_id}/{nanos}")
    }

    fn session_from_identifier(identifier: &str) -> Option<&str> {
        let rest = identifier.strip_prefix(ID_PREFIX)?;
        Some(rest.split('/').next().unwrap_or(rest))
    }

    /// Category with a single "Show" button, so the banner offers the jump
    /// explicitly instead of relying on a click on the body.
    const CATEGORY: &str = "monocode.session";
    const SHOW_ACTION: &str = "monocode.session.show";

    fn options() -> UNAuthorizationOptions {
        UNAuthorizationOptions::Alert
            | UNAuthorizationOptions::Sound
            | UNAuthorizationOptions::Badge
    }

    fn map_permission(
        authorization: UNAuthorizationStatus,
        alert_setting: UNNotificationSetting,
        alert_style: UNAlertStyle,
    ) -> Permission {
        match authorization {
            UNAuthorizationStatus::NotDetermined => Permission::Prompt,
            UNAuthorizationStatus::Denied => Permission::Denied,
            // Authorization alone does not guarantee a visible alert on
            // macOS: users can leave alerts enabled but select no alert style.
            _ if alert_setting == UNNotificationSetting::Disabled
                || alert_style == UNAlertStyle::None =>
            {
                Permission::Denied
            }
            _ => Permission::Granted,
        }
    }

    fn map_settings(settings: &UNNotificationSettings) -> Permission {
        map_permission(
            settings.authorizationStatus(),
            settings.alertSetting(),
            settings.alertStyle(),
        )
    }

    /// Completion handlers run on a UN background queue. The ObjC objects
    /// are released before the caller blocks on the channel.
    fn query_permission() -> mpsc::Receiver<Permission> {
        let (tx, rx) = mpsc::channel();
        let handler = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
            let settings = unsafe { settings.as_ref() };
            let _ = tx.send(map_settings(settings));
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .getNotificationSettingsWithCompletionHandler(&handler);
        rx
    }

    fn start_request() -> mpsc::Receiver<()> {
        let (tx, rx) = mpsc::channel();
        let handler = RcBlock::new(move |_granted: Bool, _error: *mut NSError| {
            let _ = tx.send(());
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .requestAuthorizationWithOptions_completionHandler(options(), &handler);
        rx
    }

    /// A dispatch still unanswered by now has already lost to the in-app cue,
    /// so the caller gives up rather than leaving the turn silent.
    const DISPATCH_TIMEOUT: Duration = Duration::from_secs(5);

    /// `None` waits indefinitely, which is what the permission prompt needs:
    /// its handler does not run until the user answers the system dialog.
    fn wait<T: Send + 'static>(rx: mpsc::Receiver<T>, timeout: Option<Duration>) -> Option<T> {
        match timeout {
            Some(timeout) => rx.recv_timeout(timeout).ok(),
            None => rx.recv().ok(),
        }
    }

    pub(super) fn permission(_app_id: &str) -> Permission {
        wait(query_permission(), None).unwrap_or(Permission::Denied)
    }

    pub(super) fn request_permission(app_id: &str) -> Permission {
        wait(start_request(), None);
        permission(app_id)
    }

    /// Hands the request to the center and reports what its completion
    /// handler says. Authorization is settled before this runs, so an
    /// undetermined status never turns a finished turn into a system prompt.
    fn start_show(
        session_id: &str,
        title: &str,
        subtitle: &str,
        body: &str,
        sound: bool,
    ) -> mpsc::Receiver<Result<(), String>> {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setSubtitle(&NSString::from_str(subtitle));
        content.setBody(&NSString::from_str(body));
        content.setCategoryIdentifier(&NSString::from_str(CATEGORY));
        if sound {
            content.setSound(Some(&UNNotificationSound::defaultSound()));
        }
        let identifier = NSString::from_str(&request_identifier(session_id));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &identifier,
            &content,
            None,
        );
        let (tx, rx) = mpsc::channel();
        let handler = RcBlock::new(move |error: *mut NSError| {
            let result = match NonNull::new(error) {
                Some(error) => Err(format!("notification rejected: {}", unsafe {
                    error.as_ref()
                })),
                None => Ok(()),
            };
            let _ = tx.send(result);
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, Some(&handler));
        rx
    }

    pub(super) fn show(
        _app_id: &str,
        _on_click: &ClickHandler,
        session_id: &str,
        title: &str,
        subtitle: &str,
        body: &str,
        sound: bool,
    ) -> Result<(), String> {
        // Asked here rather than trusted from the frontend, whose cached
        // permission goes stale when alerts are switched off in System
        // Settings and whose badge-only case the center accepts silently.
        if wait(query_permission(), Some(DISPATCH_TIMEOUT)) != Some(Permission::Granted) {
            return Err("notifications are not authorized".into());
        }
        wait(
            start_show(session_id, title, subtitle, body, sound),
            Some(DISPATCH_TIMEOUT),
        )
        .unwrap_or_else(|| Err("notification dispatch timed out".into()))
    }

    pub(super) fn open_settings(app_id: &str) -> Result<(), String> {
        let url = format!(
            "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id={app_id}"
        );
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    struct DelegateIvars {
        on_click: ClickHandler,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "MonoCodeNotificationDelegate"]
        #[ivars = DelegateIvars]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            /// Without this macOS drops banners while the app is frontmost,
            /// and a finished background session deserves one either way.
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                completion.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List
                    | UNNotificationPresentationOptions::Sound,));
            }

            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                completion: &block2::DynBlock<dyn Fn()>,
            ) {
                let identifier = response.notification().request().identifier().to_string();
                if let Some(session_id) = session_from_identifier(&identifier) {
                    super::handle_click(&self.ivars().on_click, session_id);
                }
                completion.call(());
            }
        }
    );

    thread_local! {
        static DELEGATE: RefCell<Option<Retained<Delegate>>> = const { RefCell::new(None) };
    }

    /// Must run on the main thread once the app is ready; the center keeps a
    /// weak reference, so the delegate is retained here for the app lifetime.
    pub fn install_delegate(on_click: ClickHandler) {
        if MainThreadMarker::new().is_none() {
            return;
        }
        let delegate = Delegate::alloc().set_ivars(DelegateIvars { on_click });
        let delegate: Retained<Delegate> = unsafe { objc2::msg_send![super(delegate), init] };
        let center = UNUserNotificationCenter::currentNotificationCenter();
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        DELEGATE.with(|slot| *slot.borrow_mut() = Some(delegate));

        let show = UNNotificationAction::actionWithIdentifier_title_options(
            &NSString::from_str(SHOW_ACTION),
            &NSString::from_str("Show"),
            UNNotificationActionOptions::Foreground,
        );
        let category =
            UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
                &NSString::from_str(CATEGORY),
                &NSArray::from_retained_slice(&[show]),
                &NSArray::new(),
                UNNotificationCategoryOptions::empty(),
            );
        center.setNotificationCategories(&NSSet::from_retained_slice(&[category]));
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use objc2::runtime::AnyProtocol;
        use objc2::{ClassType, sel};

        #[test]
        fn identifier_round_trips_the_session() {
            let id = request_identifier("549ae7ac");
            assert_eq!(session_from_identifier(&id), Some("549ae7ac"));
            assert_eq!(session_from_identifier("other"), None);
        }

        #[test]
        fn permission_requires_a_visible_alert_style() {
            assert_eq!(
                map_permission(
                    UNAuthorizationStatus::Authorized,
                    UNNotificationSetting::Enabled,
                    UNAlertStyle::None,
                ),
                Permission::Denied,
            );
            assert_eq!(
                map_permission(
                    UNAuthorizationStatus::Authorized,
                    UNNotificationSetting::Enabled,
                    UNAlertStyle::Banner,
                ),
                Permission::Granted,
            );
        }

        #[test]
        fn wait_reports_what_the_completion_handler_sent() {
            let (tx, rx) = mpsc::channel();
            tx.send(Err::<(), String>("rejected".into())).unwrap();
            let got = wait(rx, None);
            assert_eq!(got, Some(Err("rejected".into())));
        }

        #[test]
        fn wait_gives_up_when_the_completion_handler_never_runs() {
            let (tx, rx) = mpsc::channel::<Result<(), String>>();
            let got = wait(rx, Some(Duration::from_millis(20)));
            assert_eq!(got, None);
            drop(tx);
        }

        #[test]
        fn delegate_registers_protocol_methods() {
            let cls = Delegate::class();
            let proto =
                AnyProtocol::get(c"UNUserNotificationCenterDelegate").expect("protocol loaded");
            assert!(cls.conforms_to(proto));
            assert!(cls.responds_to(sel!(
                userNotificationCenter:willPresentNotification:withCompletionHandler:
            )));
            assert!(cls.responds_to(sel!(
                userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:
            )));
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::{ClickHandler, Permission};

    pub(super) const PERMISSION_FALLBACK: Permission = Permission::Granted;

    pub(super) fn permission(_app_id: &str) -> Permission {
        Permission::Granted
    }

    pub(super) fn request_permission(_app_id: &str) -> Permission {
        Permission::Granted
    }

    pub(super) fn show(
        _app_id: &str,
        on_click: &ClickHandler,
        session_id: &str,
        title: &str,
        subtitle: &str,
        body: &str,
        sound: bool,
    ) -> Result<(), String> {
        let mut notification = notify_rust::Notification::new();
        notification
            .appname("MonoCode")
            .summary(&format!("{title}: {subtitle}"))
            // The body is agent output; servers render it as markup.
            .body(&escape_markup(body))
            .icon("monocode")
            // Servers only report the click when a "default" action exists.
            .action("default", "Show");
        if sound {
            notification.sound_name("message-new-instant");
        }
        let handle = notification.show().map_err(|err| err.to_string())?;
        let on_click = on_click.clone();
        let session_id = session_id.to_string();
        // `wait_for_action` blocks until the notification closes.
        std::thread::spawn(move || {
            handle.wait_for_action(|action| {
                if action == "default" {
                    super::handle_click(&on_click, &session_id);
                }
            });
        });
        Ok(())
    }

    /// The freedesktop spec parses the body as a subset of HTML.
    fn escape_markup(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for ch in text.chars() {
            match ch {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                _ => out.push(ch),
            }
        }
        out
    }

    pub(super) fn open_settings(_app_id: &str) -> Result<(), String> {
        Err("no notification settings page on this platform".into())
    }

    #[cfg(test)]
    mod tests {
        use super::escape_markup;

        #[test]
        fn escapes_markup_in_bodies() {
            assert_eq!(
                escape_markup("<b>x</b> & y"),
                "&lt;b&gt;x&lt;/b&gt; &amp; y"
            );
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{ClickHandler, Permission, handle_click};

    /// A failed permission task read as "could not ask", which is not blocked.
    pub(super) const PERMISSION_FALLBACK: Permission = Permission::Granted;

    /// Action payload for the explicit "Show" button. The body click carries
    /// no arguments, so the session id is also captured in the activation
    /// closure; the payload is a fallback for routing, not the primary path.
    /// Reminder identifiers (`reminder:<session>:<due>`) flow through the
    /// same path and are routed by `handle_click`.
    const SHOW_ACTION_PREFIX: &str = "show:";

    /// Wraps the session id in the "Show" button's activation payload.
    fn show_action(session_id: &str) -> String {
        format!("{SHOW_ACTION_PREFIX}{session_id}")
    }

    /// Reads the session id back out of a "Show" button payload.
    fn session_from_action(action: &str) -> Option<&str> {
        action.strip_prefix(SHOW_ACTION_PREFIX)
    }

    /// Collapses the WinRT setting onto the frontend's decision. Only
    /// `Enabled` is unblocked; `None` is "could not ask" and defers to the
    /// dispatch.
    fn blocked_from_setting(
        setting: Option<windows::UI::Notifications::NotificationSetting>,
    ) -> Option<bool> {
        use windows::UI::Notifications::NotificationSetting;
        match setting {
            Some(NotificationSetting::Enabled) => Some(false),
            // DisabledForApplication / ForUser / ByGroupPolicy / ByManifest.
            Some(_) => Some(true),
            None => None,
        }
    }

    /// Whether Windows blocks toasts for this AppUserModelID. `None` means the
    /// system could not be asked — an unknown ID (`tauri dev` before the
    /// shortcut exists) or a WinRT failure — and the dispatch itself decides.
    fn toasts_blocked(app_id: &str) -> Option<bool> {
        use windows::UI::Notifications::ToastNotificationManager;
        use windows::core::HSTRING;
        let notifier =
            ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id)).ok()?;
        blocked_from_setting(notifier.Setting().ok())
    }

    /// Asks the toast system, which reflects the per-app toggle, the user-wide
    /// switch and group policy. There is no prompt to show, so a request just
    /// re-reads the same state.
    pub(super) fn permission(app_id: &str) -> Permission {
        let blocked = toasts_blocked(app_id);
        if blocked == Some(true) {
            Permission::Denied
        } else {
            Permission::Granted
        }
    }

    pub(super) fn request_permission(app_id: &str) -> Permission {
        // Windows has no authorization dialog; Settings owns the decision.
        permission(app_id)
    }

    /// `Toast` is `!Send`, so it is built and dispatched on the calling
    /// thread, which the caller keeps off the UI thread.
    pub(super) fn show(
        app_id: &str,
        on_click: &ClickHandler,
        session_id: &str,
        title: &str,
        subtitle: &str,
        body: &str,
        sound: bool,
    ) -> Result<(), String> {
        show_blocking(on_click, app_id, session_id, title, subtitle, body, sound)
    }

    /// Dispatches on the blocking thread a `Toast` needs, honouring a Windows
    /// block first so the caller's in-app cue stands in when no banner shows.
    fn show_blocking(
        on_click: &ClickHandler,
        app_id: &str,
        session_id: &str,
        title: &str,
        subtitle: &str,
        body: &str,
        sound: bool,
    ) -> Result<(), String> {
        use tauri_winrt_notification::Toast;

        if toasts_blocked(app_id) == Some(true) {
            return Err("notifications are disabled in Windows settings".into());
        }

        // Installed NSIS builds resolve the bundle identifier through the
        // Start Menu shortcut's AppUserModelID. `tauri dev` has no shortcut,
        // so fall back to the PowerShell host ID (wrong branding, but visible)
        // when the real ID fails.
        match show_with_app_id(on_click, app_id, session_id, title, subtitle, body, sound) {
            Ok(()) => Ok(()),
            Err(first) if app_id != Toast::POWERSHELL_APP_ID => show_with_app_id(
                on_click,
                Toast::POWERSHELL_APP_ID,
                session_id,
                title,
                subtitle,
                body,
                sound,
            )
            .map_err(|fallback| format!("{first}; dev fallback: {fallback}")),
            Err(first) => Err(first),
        }
    }

    /// Builds and shows one toast under `app_id`, wiring the "Show" button to
    /// the shared click router.
    fn show_with_app_id(
        on_click: &ClickHandler,
        app_id: &str,
        session_id: &str,
        title: &str,
        subtitle: &str,
        body: &str,
        sound: bool,
    ) -> Result<(), String> {
        use tauri_winrt_notification::{Sound, Toast};

        let on_click = on_click.clone();
        let owned_session = session_id.to_string();
        let sound = if sound { Some(Sound::Default) } else { None };
        // The default icon comes from the AppUserModelID registration; an
        // explicit icon needs an absolute non-UNC path and is left out for v1.
        Toast::new(app_id)
            .title(title)
            .text1(subtitle)
            .text2(body)
            .sound(sound)
            .add_button("Show", &show_action(session_id))
            .on_activated(move |action| {
                let session = action
                    .as_deref()
                    .and_then(session_from_action)
                    .unwrap_or(&owned_session)
                    .to_string();
                handle_click(&on_click, session.as_str());
                Ok(())
            })
            .show()
            .map_err(|err| err.to_string())
    }

    /// Opens Settings > System > Notifications, where the per-app toggle lives.
    pub(super) fn open_settings(_app_id: &str) -> Result<(), String> {
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/C", "start", "", "ms-settings:notifications"]);
        crate::hide_window_console(&mut cmd);
        cmd.spawn().map(|_| ()).map_err(|err| err.to_string())
    }

    #[cfg(test)]
    mod tests {
        use windows::UI::Notifications::NotificationSetting;

        use super::{blocked_from_setting, session_from_action, show_action};

        #[test]
        fn show_action_round_trips_the_session() {
            let action = show_action("549ae7ac");
            assert_eq!(session_from_action(&action), Some("549ae7ac"));
            assert_eq!(session_from_action("other"), None);
        }

        #[test]
        fn show_action_preserves_reminder_identifiers() {
            let action = show_action("reminder:549ae7ac:1700000000");
            assert_eq!(
                session_from_action(&action),
                Some("reminder:549ae7ac:1700000000")
            );
        }

        #[test]
        fn only_an_enabled_setting_is_unblocked() {
            assert_eq!(
                blocked_from_setting(Some(NotificationSetting::Enabled)),
                Some(false)
            );
            assert_eq!(
                blocked_from_setting(Some(NotificationSetting::DisabledForApplication)),
                Some(true)
            );
            assert_eq!(
                blocked_from_setting(Some(NotificationSetting::DisabledForUser)),
                Some(true)
            );
            assert_eq!(
                blocked_from_setting(Some(NotificationSetting::DisabledByGroupPolicy)),
                Some(true)
            );
            assert_eq!(blocked_from_setting(None), None);
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod platform {
    use super::{ClickHandler, Permission};

    pub(super) const PERMISSION_FALLBACK: Permission = Permission::Unsupported;

    pub(super) fn permission(_app_id: &str) -> Permission {
        Permission::Unsupported
    }

    pub(super) fn request_permission(_app_id: &str) -> Permission {
        Permission::Unsupported
    }

    pub(super) fn show(
        _app_id: &str,
        _on_click: &ClickHandler,
        _session_id: &str,
        _title: &str,
        _subtitle: &str,
        _body: &str,
        _sound: bool,
    ) -> Result<(), String> {
        Err("notifications are not supported on this platform".into())
    }

    pub(super) fn open_settings(_app_id: &str) -> Result<(), String> {
        Err("notifications are not supported on this platform".into())
    }
}
