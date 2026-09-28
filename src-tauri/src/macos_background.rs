//! Suppress Wry's synchronous application activation only while constructing a
//! hidden window. Do not change Dock policy or reactivate another application.
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{msg_send, sel, MainThreadMarker};
use objc2_app_kit::NSApplication;
use std::cell::Cell;
use std::ffi::CString;

thread_local! { static SUPPRESS: Cell<usize> = const { Cell::new(0) }; }

fn installed_class(object: &AnyObject) -> Option<&'static AnyClass> {
    let mut class = Some(object.class());
    while let Some(current) = class {
        if current.name().to_bytes().starts_with(b"MonoCodeQuiet_") {
            return Some(current);
        }
        class = current.superclass();
    }
    None
}

fn original_class(object: &AnyObject) -> &'static AnyClass {
    installed_class(object)
        .and_then(AnyClass::superclass)
        .expect("original application class missing")
}

extern "C" fn activate(object: &AnyObject, _: Sel) {
    if SUPPRESS.with(Cell::get) == 0 {
        unsafe {
            let _: () = msg_send![super(object, original_class(object)), activate];
        }
    }
}
extern "C" fn activate_others(object: &AnyObject, _: Sel, flag: Bool) {
    if SUPPRESS.with(Cell::get) == 0 {
        unsafe {
            let _: () =
                msg_send![super(object, original_class(object)), activateIgnoringOtherApps: flag];
        }
    }
}

fn quiet_class(original: &'static AnyClass) -> Result<&'static AnyClass, String> {
    let name = CString::new(format!(
        "MonoCodeQuiet_{}",
        original.name().to_string_lossy()
    ))
    .map_err(|error| error.to_string())?;
    if let Some(class) = AnyClass::get(&name) {
        return Ok(class);
    }
    let mut builder = ClassBuilder::new(&name, original)
        .ok_or("Could not prepare background window activation guard.")?;
    // No ivars: inherited object layout is unchanged. All other application
    // behavior, including Tao's delegates, continues to use its original class.
    unsafe {
        if original.instance_method(sel!(activate)).is_some() {
            builder.add_method(sel!(activate), activate as extern "C" fn(_, _));
        }
        builder.add_method(
            sel!(activateIgnoringOtherApps:),
            activate_others as extern "C" fn(_, _, _),
        );
    }
    Ok(builder.register())
}

struct RestoreSuppression;
impl Drop for RestoreSuppression {
    fn drop(&mut self) {
        SUPPRESS.with(|depth| depth.set(depth.get() - 1));
    }
}

fn guarded<T>(object: &AnyObject, run: impl FnOnce() -> T) -> Result<T, String> {
    let original = object.class();
    if installed_class(object).is_none() {
        let quiet = quiet_class(original)?;
        if quiet.instance_size() != original.instance_size() {
            return Err("Background application class layout mismatch.".into());
        }
        // A layout-identical subclass preserves all superclass behavior. The
        // override forwards normally except inside this synchronous scope.
        unsafe {
            AnyObject::set_class(object, quiet);
        }
    }
    SUPPRESS.with(|depth| depth.set(depth.get() + 1));
    let _restore = RestoreSuppression;
    Ok(run())
}

/// Call only on the main thread and keep the closure synchronous. The original
/// activation behavior is restored before any later user interaction or reveal.
pub fn without_activation<T>(run: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let mtm =
        MainThreadMarker::new().ok_or("Background windows must be created on the main thread.")?;
    let app = NSApplication::sharedApplication(mtm);
    guarded(app.as_ref(), run)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::{msg_send, ClassType};
    use objc2_foundation::NSObject;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static ACTIVATIONS: AtomicUsize = AtomicUsize::new(0);
    extern "C" fn activated(_: &AnyObject, _: Sel) {
        ACTIVATIONS.fetch_add(1, Ordering::SeqCst);
    }
    extern "C" fn activated_others(_: &AnyObject, _: Sel, _: Bool) {
        ACTIVATIONS.fetch_add(1, Ordering::SeqCst);
    }
    #[test]
    fn suppresses_both_activation_paths_and_restores_after_error_and_panic() {
        // Test against a private Objective-C object, never the user's app or clipboard.
        let mut builder =
            ClassBuilder::new(c"MonoCodeActivationGuardTest", NSObject::class()).unwrap();
        unsafe {
            builder.add_method(sel!(activate), activated as extern "C" fn(_, _));
            builder.add_method(
                sel!(activateIgnoringOtherApps:),
                activated_others as extern "C" fn(_, _, _),
            );
        }
        let original = builder.register();
        let object: objc2::rc::Retained<AnyObject> = unsafe { msg_send![original, new] };
        let failure: Result<(), &str> = guarded(&object, || {
            unsafe {
                let _: () = msg_send![&*object, activate];
                let _: () = msg_send![&*object, activateIgnoringOtherApps: Bool::YES];
            }
            Err("build failed")
        })
        .unwrap();
        assert!(failure.is_err());
        assert_eq!(ACTIVATIONS.load(Ordering::SeqCst), 0);
        assert_eq!(original_class(&object), original);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = guarded(&object, || panic!("builder panic"));
        }));
        assert_eq!(original_class(&object), original);
        unsafe {
            let _: () = msg_send![&*object, activate];
        }
        unsafe {
            let _: () = msg_send![&*object, activateIgnoringOtherApps: Bool::YES];
        }
        assert_eq!(ACTIVATIONS.load(Ordering::SeqCst), 2);
    }
}
