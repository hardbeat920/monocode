//! Desktop notifications for turns that end or stall while the window is in
//! the background.
//!
//! macOS goes through `UNUserNotificationCenter` directly: the app already
//! links it for the Dock badge, it reports the real authorization state, and
//! a delegate turns a click into a jump back to the session. Linux uses the
//! freedesktop notification bus, which has no permission model.

use serde::Serialize;
use tauri::AppHandle;

/// Emitted to every window when the user clicks a notification. Payload is
/// the session id; the window that owns that session handles it.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub const CLICK_EVENT: &str = "monocode:notification-click";

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

#[tauri::command]
pub async fn notification_permission() -> Permission {
    platform::permission().await
}

#[tauri::command]
pub async fn request_notification_permission() -> Permission {
    platform::request_permission().await
}

/// Resolves only once the platform reports the banner as scheduled: the
/// frontend skips its own turn-finished cue on success, so returning early
/// would silence a turn that never got a notification.
#[tauri::command]
pub async fn show_notification(
    app: AppHandle,
    session_id: String,
    title: String,
    subtitle: String,
    body: String,
    sound: bool,
) -> Result<(), String> {
    platform::show(&app, &session_id, &title, &subtitle, &body, sound).await
}

/// Opens the app's page in the OS notification settings, where the user can
/// re-enable alerts after declining the prompt.
#[tauri::command]
pub fn open_notification_settings(app: AppHandle) -> Result<(), String> {
    platform::open_settings(&app)
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
    use objc2::{define_class, AnyThread, DefinedClass, MainThreadMarker};
    use objc2_foundation::{NSArray, NSError, NSSet, NSString};
    use objc2_user_notifications::{
        UNAlertStyle, UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent,
        UNNotification, UNNotificationAction, UNNotificationActionOptions, UNNotificationCategory,
        UNNotificationCategoryOptions, UNNotificationPresentationOptions, UNNotificationRequest,
        UNNotificationResponse, UNNotificationSetting, UNNotificationSettings, UNNotificationSound,
        UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };
    use tauri::{AppHandle, Emitter};

    use super::{Permission, CLICK_EVENT};

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
    /// are released before any `.await` so the command future stays `Send`;
    /// only the channel crosses into the async runtime.
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
    async fn wait<T: Send + 'static>(
        rx: mpsc::Receiver<T>,
        timeout: Option<Duration>,
    ) -> Option<T> {
        tauri::async_runtime::spawn_blocking(move || match timeout {
            Some(timeout) => rx.recv_timeout(timeout).ok(),
            None => rx.recv().ok(),
        })
        .await
        .ok()
        .flatten()
    }

    pub(super) async fn permission() -> Permission {
        wait(query_permission(), None)
            .await
            .unwrap_or(Permission::Denied)
    }

    pub(super) async fn request_permission() -> Permission {
        wait(start_request(), None).await;
        permission().await
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

    pub(super) async fn show(
        _app: &AppHandle,
        session_id: &str,
        title: &str,
        subtitle: &str,
        body: &str,
        sound: bool,
    ) -> Result<(), String> {
        // Asked here rather than trusted from the frontend, whose cached
        // permission goes stale when alerts are switched off in System
        // Settings and whose badge-only case the center accepts silently.
        if wait(query_permission(), Some(DISPATCH_TIMEOUT)).await != Some(Permission::Granted) {
            return Err("notifications are not authorized".into());
        }
        wait(
            start_show(session_id, title, subtitle, body, sound),
            Some(DISPATCH_TIMEOUT),
        )
        .await
        .unwrap_or_else(|| Err("notification dispatch timed out".into()))
    }

    pub(super) fn open_settings(app: &AppHandle) -> Result<(), String> {
        let url = format!(
            "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id={}",
            app.config().identifier
        );
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    struct DelegateIvars {
        app: AppHandle,
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
                    let _ = self.ivars().app.emit(CLICK_EVENT, session_id);
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
    pub fn install_delegate(app: &AppHandle) {
        if MainThreadMarker::new().is_none() {
            return;
        }
        let delegate = Delegate::alloc().set_ivars(DelegateIvars { app: app.clone() });
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
        use objc2::{sel, ClassType};

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
            let got = tauri::async_runtime::block_on(wait(rx, None));
            assert_eq!(got, Some(Err("rejected".into())));
        }

        #[test]
        fn wait_gives_up_when_the_completion_handler_never_runs() {
            let (tx, rx) = mpsc::channel::<Result<(), String>>();
            let got = tauri::async_runtime::block_on(wait(rx, Some(Duration::from_millis(20))));
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
    use tauri::{AppHandle, Emitter};

    use super::{Permission, CLICK_EVENT};

    pub(super) async fn permission() -> Permission {
        Permission::Granted
    }

    pub(super) async fn request_permission() -> Permission {
        Permission::Granted
    }

    pub(super) async fn show(
        app: &AppHandle,
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
        let app = app.clone();
        let session_id = session_id.to_string();
        // `wait_for_action` blocks until the notification closes.
        std::thread::spawn(move || {
            handle.wait_for_action(|action| {
                if action == "default" {
                    let _ = app.emit(CLICK_EVENT, session_id.as_str());
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

    pub(super) fn open_settings(_app: &AppHandle) -> Result<(), String> {
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

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    use tauri::AppHandle;

    use super::Permission;

    pub(super) async fn permission() -> Permission {
        Permission::Unsupported
    }

    pub(super) async fn request_permission() -> Permission {
        Permission::Unsupported
    }

    pub(super) async fn show(
        _app: &AppHandle,
        _session_id: &str,
        _title: &str,
        _subtitle: &str,
        _body: &str,
        _sound: bool,
    ) -> Result<(), String> {
        Err("notifications are not supported on this platform".into())
    }

    pub(super) fn open_settings(_app: &AppHandle) -> Result<(), String> {
        Err("notifications are not supported on this platform".into())
    }
}
