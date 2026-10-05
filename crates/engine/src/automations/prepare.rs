//! Port of src/features/quick-composer/model/prepareQuickComposer.ts:
//! prepare the hidden panel window after the main window has painted and
//! the app is idle, never on startup's critical path. If focus leaves
//! first, wait until the workspace has it again.
//!
//! The TypeScript drove this with `requestAnimationFrame`,
//! `requestIdleCallback`, and window focus events. The engine has no
//! window, so this is the state machine; the app feeds it frames, idle
//! time, focus, and visibility, and runs the preparation when `idle` says
//! so.

/// Where the wait stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Waiting for this many more painted frames.
    Frames(u8),
    /// Waiting for idle time.
    Idle,
}

/// `prepareQuickComposerWhenIdle`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrepareWhenIdle {
    disposed: bool,
    prepared: bool,
    in_flight: bool,
    stage: Option<Stage>,
}

impl PrepareWhenIdle {
    /// A new waiter. Call `schedule` with the window's state to start.
    pub fn new() -> Self {
        Self::default()
    }

    fn blocked(&self, foreground: bool) -> bool {
        self.disposed || self.prepared || self.in_flight || !foreground
    }

    /// `schedule`: on start, on focus, and when visibility changes.
    /// `foreground` is `!document.hidden && document.hasFocus()`. Effects
    /// may run before paint, so the wait is two frames, then idle time.
    pub fn schedule(&mut self, foreground: bool) {
        self.cancel();
        if self.blocked(foreground) {
            return;
        }
        self.stage = Some(Stage::Frames(2));
    }

    /// `cancel`: on blur.
    pub fn cancel(&mut self) {
        self.stage = None;
    }

    /// A frame was painted.
    pub fn frame(&mut self) {
        self.stage = match self.stage {
            Some(Stage::Frames(2)) => Some(Stage::Frames(1)),
            Some(Stage::Frames(_)) => Some(Stage::Idle),
            other => other,
        };
    }

    /// The app wants another frame before going on.
    pub fn wants_frame(&self) -> bool {
        matches!(self.stage, Some(Stage::Frames(_)))
    }

    /// The app should report idle time.
    pub fn wants_idle(&self) -> bool {
        self.stage == Some(Stage::Idle)
    }

    /// Idle time came. `true` means prepare the panel now and report the
    /// answer to `finished`.
    pub fn idle(&mut self, foreground: bool) -> bool {
        if self.stage != Some(Stage::Idle) {
            return false;
        }
        self.stage = None;
        if self.blocked(foreground) {
            return false;
        }
        self.in_flight = true;
        true
    }

    /// `quick_composer_prepare` answered. `Ok(false)` means the native
    /// focus check deferred it; an error retries on the next focus too.
    pub fn finished(&mut self, result: Result<bool, String>) {
        if let Ok(ready) = result {
            self.prepared = ready;
        }
        self.in_flight = false;
    }

    /// The panel is ready.
    pub fn is_prepared(&self) -> bool {
        self.prepared
    }

    /// The cleanup: stop waiting for good.
    pub fn dispose(&mut self) {
        self.disposed = true;
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window in the TypeScript tests: focus and visibility.
    struct Window {
        focused: bool,
        hidden: bool,
        prepares: usize,
    }

    impl Window {
        fn foreground(&self) -> bool {
            !self.hidden && self.focused
        }

        fn paint(&self, waiter: &mut PrepareWhenIdle) {
            if waiter.wants_frame() {
                waiter.frame();
            }
        }

        /// Run idle work; the preparation answers `ready`.
        fn run_idle(&mut self, waiter: &mut PrepareWhenIdle, ready: bool) {
            if waiter.idle(self.foreground()) {
                self.prepares += 1;
                waiter.finished(Ok(ready));
            }
        }
    }

    fn window() -> Window {
        Window {
            focused: true,
            hidden: false,
            prepares: 0,
        }
    }

    #[test]
    fn waits_for_a_paint_and_idle_time_then_prepares_only_once() {
        let mut window = window();
        let mut waiter = PrepareWhenIdle::new();
        waiter.schedule(window.foreground());
        assert_eq!(window.prepares, 0);
        window.paint(&mut waiter);
        assert!(!waiter.wants_idle());
        window.paint(&mut waiter);
        assert_eq!(window.prepares, 0);
        window.run_idle(&mut waiter, true);
        assert_eq!(window.prepares, 1);
        waiter.schedule(window.foreground());
        window.paint(&mut waiter);
        window.paint(&mut waiter);
        window.run_idle(&mut waiter, true);
        assert_eq!(window.prepares, 1);
    }

    #[test]
    fn cancels_pending_work_when_focus_leaves_and_resumes_after_focus_returns() {
        let mut window = window();
        let mut waiter = PrepareWhenIdle::new();
        waiter.schedule(window.foreground());
        window.paint(&mut waiter);
        window.paint(&mut waiter);
        window.focused = false;
        waiter.cancel();
        window.run_idle(&mut waiter, true);
        assert_eq!(window.prepares, 0);
        window.focused = true;
        waiter.schedule(window.foreground());
        window.paint(&mut waiter);
        window.paint(&mut waiter);
        window.run_idle(&mut waiter, true);
        assert_eq!(window.prepares, 1);
    }

    #[test]
    fn waits_for_a_hidden_workspace_and_retries_when_the_native_focus_check_defers_preparation() {
        let mut window = window();
        window.hidden = true;
        let mut waiter = PrepareWhenIdle::new();
        waiter.schedule(window.foreground());
        assert!(!waiter.wants_frame());
        window.hidden = false;
        waiter.schedule(window.foreground());
        window.paint(&mut waiter);
        window.paint(&mut waiter);
        window.run_idle(&mut waiter, false);
        waiter.schedule(window.foreground());
        window.paint(&mut waiter);
        window.paint(&mut waiter);
        window.run_idle(&mut waiter, true);
        assert_eq!(window.prepares, 2);
    }

    #[test]
    fn cancels_work_on_cleanup_including_a_remount() {
        let mut window = window();
        let mut first = PrepareWhenIdle::new();
        first.schedule(window.foreground());
        window.paint(&mut first);
        first.dispose();
        let mut waiter = PrepareWhenIdle::new();
        waiter.schedule(window.foreground());
        window.paint(&mut waiter);
        window.paint(&mut waiter);
        window.run_idle(&mut first, true);
        window.run_idle(&mut waiter, true);
        assert_eq!(window.prepares, 1);
        waiter.dispose();
        waiter.schedule(window.foreground());
        assert!(!waiter.wants_frame());
    }

    #[test]
    fn never_prepares_before_the_main_window_painted() {
        let mut window = window();
        let mut waiter = PrepareWhenIdle::new();
        waiter.schedule(window.foreground());
        window.run_idle(&mut waiter, true);
        assert_eq!(window.prepares, 0);
        window.paint(&mut waiter);
        window.paint(&mut waiter);
        window.run_idle(&mut waiter, true);
        assert_eq!(window.prepares, 1);
    }
}
