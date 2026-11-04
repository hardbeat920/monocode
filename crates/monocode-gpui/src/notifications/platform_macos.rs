use std::{
    cell::RefCell,
    collections::VecDeque,
    ptr::NonNull,
    sync::{
        Mutex,
        mpsc::{self, Receiver},
    },
};

use block2::RcBlock;
use objc2::{
    AnyThread, MainThreadMarker, define_class,
    rc::Retained,
    runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSArray, NSBundle, NSError, NSSet, NSString};
use objc2_user_notifications::*;

use super::{Notification, Permission, Target, resolved};

const CATEGORY: &str = "monocode.session";
const SHOW: &str = "monocode.session.show";
static CLICKS: Mutex<VecDeque<Target>> = Mutex::new(VecDeque::new());

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MonoCodeNativeNotificationDelegate"]
    struct Delegate;
    unsafe impl NSObjectProtocol for Delegate {}
    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn present(
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
        fn clicked(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &block2::DynBlock<dyn Fn()>,
        ) {
            let action = response.actionIdentifier();
            if (action.to_string() == SHOW
                || action.isEqualToString(unsafe { UNNotificationDefaultActionIdentifier }))
                && let Some(target) =
                    Target::from_tag(&response.notification().request().identifier().to_string())
                && let Ok(mut queue) = CLICKS.lock()
            {
                queue.push_back(target);
            }
            completion.call(());
        }
    }
);

thread_local! {
    // The center retains its delegate weakly. Keep ours alive for the process.
    static DELEGATE: RefCell<Option<Retained<Delegate>>> = const { RefCell::new(None) };
}

fn center() -> Option<Retained<UNUserNotificationCenter>> {
    // Unbundled cargo-run executables make this API raise an ObjC exception.
    NSBundle::mainBundle().bundleIdentifier()?;
    Some(UNUserNotificationCenter::currentNotificationCenter())
}

pub fn initialize() -> bool {
    if MainThreadMarker::new().is_none() {
        return false;
    }
    let Some(center) = center() else {
        return false;
    };
    DELEGATE.with(|slot| {
        if slot.borrow().is_some() {
            return;
        }
        let delegate: Retained<Delegate> = unsafe { objc2::msg_send![Delegate::alloc(), init] };
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        *slot.borrow_mut() = Some(delegate);
        let show = UNNotificationAction::actionWithIdentifier_title_options(
            &NSString::from_str(SHOW),
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
    });
    true
}

/// The badge has its own switch: alerts can be off while badges are allowed.
fn map_badge_settings(settings: &UNNotificationSettings) -> Permission {
    match settings.badgeSetting() {
        UNNotificationSetting::Enabled | UNNotificationSetting::NotSupported => Permission::Granted,
        _ => Permission::Denied,
    }
}

fn map_settings(settings: &UNNotificationSettings) -> Permission {
    match settings.authorizationStatus() {
        UNAuthorizationStatus::NotDetermined => Permission::Prompt,
        UNAuthorizationStatus::Denied => Permission::Denied,
        _ if settings.alertSetting() == UNNotificationSetting::Disabled
            || settings.alertStyle() == UNAlertStyle::None =>
        {
            Permission::Denied
        }
        _ => Permission::Granted,
    }
}

pub fn permission() -> Receiver<Permission> {
    let Some(center) = center() else {
        return resolved(Permission::Unsupported);
    };
    let (sender, receiver) = mpsc::channel();
    let block = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
        let _ = sender.send(map_settings(unsafe { settings.as_ref() }));
    });
    center.getNotificationSettingsWithCompletionHandler(&block);
    receiver
}

/// Only call from an explicit opt-in in Settings. Sending never requests permission.
pub fn request_permission() -> Receiver<Permission> {
    let Some(center) = center() else {
        return resolved(Permission::Unsupported);
    };
    let (sender, receiver) = mpsc::channel();
    let block = RcBlock::new(move |_granted: Bool, _error: *mut NSError| {
        let Some(center) = self::center() else {
            let _ = sender.send(Permission::Unsupported);
            return;
        };
        let sender = sender.clone();
        let query = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
            let _ = sender.send(map_settings(unsafe { settings.as_ref() }));
        });
        center.getNotificationSettingsWithCompletionHandler(&query);
    });
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions::Alert
            | UNAuthorizationOptions::Sound
            | UNAuthorizationOptions::Badge,
        &block,
    );
    receiver
}

/// Result arrives only when the OS accepts/rejects dispatch. The host should
/// wait at most five seconds in a background task and then play its fallback.
pub fn show(notification: Notification) -> Receiver<Result<(), String>> {
    let Some(center) = center() else {
        return resolved(Err("Notifications require an app bundle".into()));
    };
    let (sender, receiver) = mpsc::channel();
    let block = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
        if map_settings(unsafe { settings.as_ref() }) != Permission::Granted {
            let _ = sender.send(Err("Notifications are not authorized".into()));
            return;
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str("MonoCode"));
        content.setSubtitle(&NSString::from_str(&notification.subtitle));
        content.setBody(&NSString::from_str(&notification.body));
        content.setCategoryIdentifier(&NSString::from_str(CATEGORY));
        if notification.sound {
            content.setSound(Some(&UNNotificationSound::defaultSound()));
        }
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&notification.tag),
            &content,
            None,
        );
        let sender = sender.clone();
        let completion = RcBlock::new(move |error: *mut NSError| {
            let result = unsafe { error.as_ref() }
                .map(|error| Err(error.localizedDescription().to_string()))
                .unwrap_or(Ok(()));
            let _ = sender.send(result);
        });
        if let Some(center) = self::center() {
            center.addNotificationRequest_withCompletionHandler(&request, Some(&completion));
        }
    });
    center.getNotificationSettingsWithCompletionHandler(&block);
    receiver
}

pub fn drain_clicks() -> Vec<Target> {
    CLICKS
        .lock()
        .map(|mut queue| queue.drain(..).collect())
        .unwrap_or_default()
}

/// Paint the Dock tile. Call on the main thread; a count of zero clears it.
pub fn set_dock_badge(count: usize) {
    let Some(main) = MainThreadMarker::new() else {
        return;
    };
    let tile = NSApplication::sharedApplication(main).dockTile();
    tile.setShowsApplicationBadge(count > 0);
    let label = (count > 0).then(|| NSString::from_str(&count.to_string()));
    tile.setBadgeLabel(label.as_deref());
    tile.display();
}

/// Since macOS 12, `NSDockTile` badge updates are ignored until the app holds
/// the badge authorization. Only re-request once the user already answered the
/// prompt: the one-time dialog stays reserved for the Notifications toggle.
/// Returns the settings after the request, so the caller can repaint the badge
/// it drew while the grant was still missing.
pub fn request_badge_authorization() -> Option<Receiver<Permission>> {
    MainThreadMarker::new()?;
    let center = center()?;
    let (sender, receiver) = mpsc::channel();
    let handler = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
        if unsafe { settings.as_ref() }.authorizationStatus()
            == UNAuthorizationStatus::NotDetermined
        {
            let _ = sender.send(Permission::Prompt);
            return;
        }
        let sender = sender.clone();
        let done = RcBlock::new(move |_granted: Bool, _error: *mut NSError| {
            let Some(center) = self::center() else {
                return;
            };
            let sender = sender.clone();
            let query = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
                let _ = sender.send(map_badge_settings(unsafe { settings.as_ref() }));
            });
            center.getNotificationSettingsWithCompletionHandler(&query);
        });
        if let Some(center) = self::center() {
            center.requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Badge,
                &done,
            );
        }
    });
    center.getNotificationSettingsWithCompletionHandler(&handler);
    Some(receiver)
}

pub fn dismiss(tag: &str) {
    if let Some(center) = center() {
        let identifiers = NSArray::from_retained_slice(&[NSString::from_str(tag)]);
        center.removePendingNotificationRequestsWithIdentifiers(&identifiers);
        center.removeDeliveredNotificationsWithIdentifiers(&identifiers);
    }
}

pub fn open_settings() -> Result<(), String> {
    let identifier = NSBundle::mainBundle()
        .bundleIdentifier()
        .ok_or("Notifications require an app bundle")?;
    std::process::Command::new("open")
        .arg(format!(
            "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id={identifier}"
        ))
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}
