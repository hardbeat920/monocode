//! Port of src/features/settings/model/displayPrefs.ts: how usage meters and
//! account emails display.
//!
//! Both are flags stored as `"1"` and `"0"`. Each `subscribe_*` function
//! replaces the `monocode:*change` window event and the cross-window
//! `storage` event; it fires when any window changes the stored value.
//! `Kv` cannot reject a write, so the TypeScript fallback that held a value
//! in memory after a failed save has no counterpart here.

use crate::kv::{Kv, Subscription};
use crate::storage_flags::{read_flag, write_flag};

pub const SHOW_REMAINING_USAGE_KEY: &str = "monocode.showRemainingUsage";
pub const MASK_EMAILS_KEY: &str = "monocode.maskEmails";

pub const SHOW_REMAINING_USAGE_DEFAULT: bool = false;
pub const MASK_EMAILS_DEFAULT: bool = false;

/// `loadShowRemainingUsage`: usage meters fill with what is left instead of
/// what is used.
pub fn load_show_remaining_usage(kv: &Kv) -> bool {
    read_flag(kv, SHOW_REMAINING_USAGE_KEY).unwrap_or(SHOW_REMAINING_USAGE_DEFAULT)
}

/// `saveShowRemainingUsage`.
pub fn save_show_remaining_usage(kv: &Kv, value: bool) {
    write_flag(kv, SHOW_REMAINING_USAGE_KEY, value);
}

/// `subscribeShowRemainingUsage`.
pub fn subscribe_show_remaining_usage(
    kv: &Kv,
    on_store_change: impl Fn() + Send + Sync + 'static,
) -> Subscription {
    kv.subscribe_key(SHOW_REMAINING_USAGE_KEY, move |_| on_store_change())
}

/// `loadMaskEmails`: account emails stay hidden until clicked.
pub fn load_mask_emails(kv: &Kv) -> bool {
    read_flag(kv, MASK_EMAILS_KEY).unwrap_or(MASK_EMAILS_DEFAULT)
}

/// `saveMaskEmails`.
pub fn save_mask_emails(kv: &Kv, value: bool) {
    write_flag(kv, MASK_EMAILS_KEY, value);
}

/// `subscribeMaskEmails`.
pub fn subscribe_mask_emails(
    kv: &Kv,
    on_store_change: impl Fn() + Send + Sync + 'static,
) -> Subscription {
    kv.subscribe_key(MASK_EMAILS_KEY, move |_| on_store_change())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    type Subscribe = fn(&Kv, Box<dyn Fn() + Send + Sync>) -> Subscription;

    struct Pref {
        key: &'static str,
        load: fn(&Kv) -> bool,
        save: fn(&Kv, bool),
        subscribe: Subscribe,
    }

    const PREFS: [Pref; 2] = [
        Pref {
            key: SHOW_REMAINING_USAGE_KEY,
            load: load_show_remaining_usage,
            save: save_show_remaining_usage,
            subscribe: |kv, f| subscribe_show_remaining_usage(kv, f),
        },
        Pref {
            key: MASK_EMAILS_KEY,
            load: load_mask_emails,
            save: save_mask_emails,
            subscribe: |kv, f| subscribe_mask_emails(kv, f),
        },
    ];

    fn counter() -> (Arc<AtomicUsize>, Box<dyn Fn() + Send + Sync>) {
        let count = Arc::new(AtomicUsize::new(0));
        let sink = Arc::clone(&count);
        (
            count,
            Box::new(move || {
                sink.fetch_add(1, Ordering::SeqCst);
            }),
        )
    }

    #[test]
    fn defaults_off_and_notifies_this_window_when_saved() {
        for pref in PREFS {
            let kv = Kv::in_memory();
            let (count, listener) = counter();
            let _subscription = (pref.subscribe)(&kv, listener);
            assert!(!(pref.load)(&kv), "{}", pref.key);

            (pref.save)(&kv, true);

            assert!((pref.load)(&kv), "{}", pref.key);
            assert_eq!(kv.get_item(pref.key).as_deref(), Some("1"));
            assert_eq!(count.load(Ordering::SeqCst), 1, "{}", pref.key);
        }
    }

    #[test]
    fn follows_a_change_saved_in_another_window() {
        for pref in PREFS {
            let kv = Kv::in_memory();
            // Another window holds its own handle to the same store.
            let other = kv.clone();
            let (count, listener) = counter();
            let _subscription = (pref.subscribe)(&kv, listener);

            other.set_item(pref.key, "1");
            assert_eq!(count.load(Ordering::SeqCst), 1, "{}", pref.key);
            assert!((pref.load)(&kv), "{}", pref.key);

            other.set_item("unrelated", "1");
            assert_eq!(count.load(Ordering::SeqCst), 1, "{}", pref.key);

            other.remove_item(pref.key);
            assert_eq!(count.load(Ordering::SeqCst), 2, "{}", pref.key);
            assert!(!(pref.load)(&kv), "{}", pref.key);
        }
    }
}
