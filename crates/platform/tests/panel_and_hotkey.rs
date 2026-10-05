//! Main-thread checks for the panel helpers and the global hotkey. AppKit
//! and Carbon reject these calls off the main thread, so this test runs
//! without the libtest harness.
//!
//! The default checks keep their windows hidden. `--ignored` instead runs
//! presentation checks in an active macOS console session. Those checks show
//! and close only their own windows. Neither mode sends keyboard input.

#[cfg(target_os = "macos")]
fn main() {
    macos::run();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("panel_and_hotkey: macOS only, skipped");
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use block2::RcBlock;
    use monocode_platform::global_hotkey::GlobalHotkeys;
    use monocode_platform::macos_panel::{
        PanelLevel, PanelStyle, fit_ns_height, focus_reader, hide, is_key, is_panel, is_visible,
        make_ns_panel, present_ns, present_with_fallback, visibility_reader,
    };
    use monocode_platform::panel_geometry::{QUICK_COMPOSER_MAX_HEIGHT, QUICK_COMPOSER_WIDTH};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{MainThreadMarker, MainThreadOnly, msg_send, sel};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSPanel, NSTextField,
        NSVisualEffectMaterial, NSVisualEffectView, NSWindow, NSWindowCollectionBehavior,
        NSWindowStyleMask, NSWorkspace,
    };
    use objc2_foundation::{
        NSDate, NSOperationQueue, NSPoint, NSRect, NSRunLoop, NSSize, NSString,
    };
    use raw_window_handle::{AppKitWindowHandle, HasWindowHandle, RawWindowHandle, WindowHandle};

    type Check = (&'static str, fn(MainThreadMarker));

    pub fn run() {
        let mtm = MainThreadMarker::new().expect("the test runs on the main thread");
        // The shared application, never activated or run.
        let app = NSApplication::sharedApplication(mtm);
        if std::env::args().any(|arg| arg == "--ignored") {
            let previous_app = frontmost_pid();
            let was_active = app.isActive();
            // A standalone test has no GPUI launch callback. Use the same
            // process activation policy as GPUI, without activating another app.
            assert!(app.setActivationPolicy(NSApplicationActivationPolicy::Regular));
            app.finishLaunching();
            assert_eq!(app.isActive(), was_active);
            assert_eq!(frontmost_pid(), previous_app);
            let checks = RcBlock::new(|| {
                let result = std::panic::catch_unwind(|| {
                    let mtm = MainThreadMarker::new().unwrap();
                    assert!(NSApplication::sharedApplication(mtm).isRunning());
                    let checks: [Check; 2] = [
                        ("panel_presentation_keeps_content_and_focus", presentation),
                        ("ordinary_window_presentation_fallback", fallback),
                    ];
                    for (name, check) in checks {
                        check(mtm);
                        println!("test {name} ... ok");
                    }
                    println!("\ntest result: ok. {} passed", checks.len());
                });
                // Every check closes its own windows before this exit. No
                // synthetic input event is needed to wake and stop AppKit.
                std::process::exit(if result.is_ok() { 0 } else { 101 });
            });
            // The callback has no captures and runs only on AppKit's main queue.
            unsafe { NSOperationQueue::mainQueue().addOperationWithBlock(&checks) };
            app.run();
            panic!("the application loop ended before the presentation checks");
        }
        let was_active = app.isActive();
        let checks: [Check; 5] = [
            ("panel_takes_the_quick_composer_style", panel_style),
            ("panel_setup_is_idempotent", idempotent),
            ("plain_windows_are_refused", plain_window),
            ("fitting_keeps_the_top_edge", fitting),
            ("hotkey_registers_and_releases", hotkey),
        ];
        for (name, check) in checks {
            check(mtm);
            println!("test {name} ... ok");
        }
        assert_eq!(
            app.isActive(),
            was_active,
            "the checks must not activate the app"
        );
        println!("\ntest result: ok. {} passed", checks.len());
    }

    fn rect() -> NSRect {
        NSRect::new(
            NSPoint::new(200.0, 600.0),
            NSSize::new(QUICK_COMPOSER_WIDTH, 128.0),
        )
    }

    /// A hidden panel shaped like zui's `WindowKind::PopUp` window.
    fn hidden_panel(mtm: MainThreadMarker) -> Retained<NSPanel> {
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            rect(),
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            true,
        );
        unsafe { panel.setReleasedWhenClosed(false) };
        panel
    }

    struct TestWindow<'a>(&'a NSWindow);

    impl HasWindowHandle for TestWindow<'_> {
        fn window_handle(&self) -> Result<WindowHandle<'_>, raw_window_handle::HandleError> {
            let content = self
                .0
                .contentView()
                .ok_or(raw_window_handle::HandleError::Unavailable)?;
            let pointer = NonNull::from(&*content).cast::<c_void>();
            let handle = AppKitWindowHandle::new(pointer);
            // The borrowed window retains its content view for this handle's lifetime.
            Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::AppKit(handle)) })
        }
    }

    struct CloseWindow<'a>(&'a NSWindow);

    impl Drop for CloseWindow<'_> {
        fn drop(&mut self) {
            self.0.orderOut(None);
            self.0.close();
        }
    }

    fn wait_for(description: &str, ready: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !ready() && Instant::now() < deadline {
            NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
        }
        if !ready() {
            let app = NSApplication::sharedApplication(MainThreadMarker::new().unwrap());
            eprintln!(
                "panel state: active={} policy={:?} running={} frontmost={:?}",
                app.isActive(),
                app.activationPolicy(),
                app.isRunning(),
                frontmost_pid()
            );
            for window in app.windows() {
                eprintln!(
                    "owned window: visible={} key={} main={} panel={} first_responder={}",
                    window.isVisible(),
                    window.isKeyWindow(),
                    window.isMainWindow(),
                    is_panel(&window),
                    window.firstResponder().is_some()
                );
            }
        }
        assert!(ready(), "{description}");
    }

    fn frontmost_pid() -> Option<i32> {
        NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .map(|app| app.processIdentifier())
    }

    fn presentation(mtm: MainThreadMarker) {
        let app = NSApplication::sharedApplication(mtm);
        let was_active = app.isActive();
        let previous_app = frontmost_pid().expect("run this check in an active console session");
        let panel = hidden_panel(mtm);
        let window: &NSWindow = &panel;
        let _close = CloseWindow(window);
        make_ns_panel(window, &PanelStyle::quick_composer()).unwrap();
        let handle = TestWindow(window);
        let visible = visibility_reader(&handle).unwrap();
        let focused = focus_reader(&handle).unwrap();
        let field = NSTextField::initWithFrame(
            NSTextField::alloc(mtm),
            NSRect::new(NSPoint::new(16.0, 32.0), NSSize::new(400.0, 24.0)),
        );
        let draft = NSString::from_str("An isolated panel draft");
        field.setStringValue(&draft);
        window.contentView().unwrap().addSubview(&field);
        let frame = window.frame();

        present_with_fallback(&handle).unwrap();
        wait_for("the promoted panel must be visible and key", || {
            visible() && focused() && is_visible(&handle) && is_key(&handle)
        });
        assert!(window.makeFirstResponder(Some(&field)));
        assert!(!window.isMainWindow());
        assert!(!can_become(window, false));
        assert_eq!(app.isActive(), was_active);
        assert_eq!(frontmost_pid(), Some(previous_app));

        hide(&handle).unwrap();
        wait_for("the panel must hide and release key status", || {
            !visible() && !focused()
        });
        assert_eq!(frontmost_pid(), Some(previous_app));
        assert_eq!(app.isActive(), was_active);
        assert_eq!(field.stringValue(), draft);
        assert_eq!(window.frame(), frame);

        present_with_fallback(&handle).unwrap();
        wait_for("the same panel must become visible and key again", || {
            visible() && focused()
        });
        assert!(window.makeFirstResponder(Some(&field)));
        assert!(window.firstResponder().is_some());
        assert_eq!(field.stringValue(), draft);
        assert_eq!(window.frame(), frame);
        assert!(!window.isMainWindow());
        assert_eq!(frontmost_pid(), Some(previous_app));
        assert_eq!(app.isActive(), was_active);
        hide(&handle).unwrap();
        wait_for("the restored panel must release focus on hide", || {
            !visible() && !focused()
        });
        assert_eq!(frontmost_pid(), Some(previous_app));
        println!("panel foreground stayed with pid {previous_app}; own window closed on return");
    }

    fn fallback(mtm: MainThreadMarker) {
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(),
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
                NSBackingStoreType::Buffered,
                true,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        let _close = CloseWindow(&window);
        let handle = TestWindow(&window);
        assert!(make_ns_panel(&window, &PanelStyle::quick_composer()).is_err());
        assert!(present_ns(&window).is_err());
        assert!(!is_panel(&window));
        present_with_fallback(&handle).unwrap();
        wait_for(
            "the ordinary-window fallback must be visible and key",
            || is_visible(&handle) && is_key(&handle),
        );
        hide(&handle).unwrap();
        wait_for("the ordinary-window fallback must hide", || {
            !is_visible(&handle) && !is_key(&handle)
        });
    }

    fn can_become(window: &NSWindow, key: bool) -> bool {
        let answer: Bool = if key {
            unsafe { msg_send![window, canBecomeKeyWindow] }
        } else {
            unsafe { msg_send![window, canBecomeMainWindow] }
        };
        answer.as_bool()
    }

    fn vibrancy_views(window: &NSWindow) -> Vec<Retained<NSVisualEffectView>> {
        window
            .contentView()
            .map(|content| {
                content
                    .subviews()
                    .iter()
                    .filter_map(|view| view.downcast::<NSVisualEffectView>().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn panel_style(mtm: MainThreadMarker) {
        let panel = hidden_panel(mtm);
        let window: &NSWindow = &panel;
        make_ns_panel(window, &PanelStyle::quick_composer()).unwrap();
        assert!(is_panel(window));
        let object: &AnyObject = window.as_ref();
        assert!(
            object
                .class()
                .name()
                .to_string_lossy()
                .starts_with("MonoCodePanel_")
        );
        assert!(can_become(window, true), "the prompt needs keys");
        assert!(
            !can_become(window, false),
            "main status would pull the workspace window forward"
        );
        assert!(
            window
                .styleMask()
                .contains(NSWindowStyleMask::NonactivatingPanel)
        );
        assert_eq!(window.level(), PanelLevel::Status.raw());
        assert!(!window.hidesOnDeactivate());
        assert!(panel.isFloatingPanel());
        assert!(!panel.becomesKeyOnlyIfNeeded());
        let behavior = window.collectionBehavior();
        for flag in [
            NSWindowCollectionBehavior::CanJoinAllSpaces,
            NSWindowCollectionBehavior::FullScreenAuxiliary,
            NSWindowCollectionBehavior::Transient,
            NSWindowCollectionBehavior::IgnoresCycle,
        ] {
            assert!(behavior.contains(flag), "missing {flag:?}");
        }
        let views = vibrancy_views(window);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].material(), NSVisualEffectMaterial::Popover);
        assert!(!window.isVisible());
        assert!(!window.isKeyWindow());
        window.close();
    }

    fn idempotent(mtm: MainThreadMarker) {
        let panel = hidden_panel(mtm);
        let window: &NSWindow = &panel;
        make_ns_panel(window, &PanelStyle::quick_composer()).unwrap();
        let object: &AnyObject = window.as_ref();
        let first = object.class();
        make_ns_panel(
            window,
            &PanelStyle {
                level: PanelLevel::PopUpMenu,
                all_spaces: false,
                vibrancy_radius: Some(12.0),
            },
        )
        .unwrap();
        assert_eq!(object.class(), first, "one subclass, not a chain");
        assert_eq!(vibrancy_views(window).len(), 1);
        assert_eq!(window.level(), PanelLevel::PopUpMenu.raw());
        assert!(
            !window
                .collectionBehavior()
                .contains(NSWindowCollectionBehavior::CanJoinAllSpaces)
        );
        // The private activation setter exists on this macOS, or is skipped.
        let _: bool =
            unsafe { msg_send![window, respondsToSelector: sel!(_setPreventsActivation:)] };
        window.close();
    }

    fn plain_window(mtm: MainThreadMarker) {
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(),
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                true,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        assert!(make_ns_panel(&window, &PanelStyle::quick_composer()).is_err());
        assert!(!is_panel(&window));
        // Ordering a plain window front would activate the app.
        assert!(present_ns(&window).is_err());
        assert!(!window.isVisible());
        window.close();
    }

    fn fitting(mtm: MainThreadMarker) {
        let panel = hidden_panel(mtm);
        let window: &NSWindow = &panel;
        let before = window.frame();
        let top = before.origin.y + before.size.height;
        fit_ns_height(
            window,
            QUICK_COMPOSER_WIDTH,
            300.0,
            QUICK_COMPOSER_MAX_HEIGHT,
        );
        let after = window.frame();
        assert_eq!(after.size.height, 300.0);
        assert_eq!(after.origin.y + after.size.height, top);
        fit_ns_height(
            window,
            QUICK_COMPOSER_WIDTH,
            9000.0,
            QUICK_COMPOSER_MAX_HEIGHT,
        );
        assert_eq!(window.frame().size.height, QUICK_COMPOSER_MAX_HEIGHT);
        assert!(!window.isVisible());
        window.close();
    }

    fn hotkey(_: MainThreadMarker) {
        // An unusual chord, claimed and released at once, so the user's
        // Command+Shift+Space is never touched.
        const CHORD: &str = "Control+Option+Shift+Command+F19";
        let hotkeys = GlobalHotkeys::new().expect("hotkey manager");
        hotkeys.register(CHORD, Arc::new(|| {})).expect("claim");
        assert!(hotkeys.is_registered(CHORD));
        const NEXT: &str = "Control+Option+Shift+Command+F18";
        hotkeys
            .change(CHORD, NEXT, Arc::new(|| {}))
            .expect("change");
        assert!(hotkeys.is_registered(NEXT));
        assert!(!hotkeys.is_registered(CHORD));
        hotkeys.unregister(NEXT).expect("release");
        assert!(!hotkeys.is_registered(NEXT));
    }
}
