//! Port of `syncNativeGlass` and `opaqueWindowBackground` in
//! src/features/settings/model/appearance.ts.
//!
//! `has-native-glass` follows the window, not the platform: Linux can turn
//! glass off in dark mode too. Whichever side moves second has to wait for
//! the other, or one of them shows through the gap. Entering glass settles
//! the window first; leaving it fades the page first. A call that a newer
//! one has overtaken is dropped rather than left to settle last.
//!
//! `NativeGlass` holds the page side (`has_native_glass`, which the app
//! shell maps onto the theme's glass surfaces) and drives the window side
//! through [`GlassWindow`].

use std::rc::Rc;
use std::time::Duration;

use gpui::{App, Context, Task};
use monocode_core::Platform;
use monocode_core::appearance::ColorScheme;
use monocode_ui::color::{Rgb, hsl_to_rgb};

/// `set_window_glass_enabled`: the native window call.
pub trait GlassWindow {
    fn set_window_glass_enabled(
        &self,
        enabled: bool,
        background: Rgb,
        cx: &mut App,
    ) -> Task<Result<(), String>>;
}

/// `opaqueWindowBackground`: the page color the window sits behind while
/// glass is off, from the theme hue, saturation, and background lightness.
pub fn opaque_window_background(hue: f64, saturation: f64, lightness: f64) -> Rgb {
    hsl_to_rgb(hue, saturation, lightness)
}

/// Whether glass is on for `scheme`: dark mode, and on Linux only with main
/// pane glass.
pub fn glass_enabled(scheme: ColorScheme, platform: Platform, body_glass: bool) -> bool {
    scheme == ColorScheme::Dark && (!platform.is_linux() || body_glass)
}

pub struct NativeGlass {
    window: Rc<dyn GlassWindow>,
    /// `--motion-feedback-duration`: how long the page takes to turn opaque.
    fade: Duration,
    /// `has-native-glass` on the root element.
    has_native_glass: bool,
    /// `glassSyncGeneration`.
    generation: u64,
    /// `glassFadeTimer`. Dropping it cancels the deferred window call.
    fade_timer: Option<Task<()>>,
    /// The window call that is settling before the page turns translucent.
    settling: Option<Task<()>>,
}

impl NativeGlass {
    pub fn new(window: Rc<dyn GlassWindow>, fade: Duration) -> Self {
        Self {
            window,
            fade,
            has_native_glass: false,
            generation: 0,
            fade_timer: None,
            settling: None,
        }
    }

    pub fn has_native_glass(&self) -> bool {
        self.has_native_glass
    }

    /// `syncNativeGlass(scheme)`. `background` is the opaque fill the window
    /// uses once glass is off.
    pub fn sync(
        &mut self,
        scheme: ColorScheme,
        platform: Platform,
        body_glass: bool,
        background: Rgb,
        cx: &mut Context<Self>,
    ) {
        let enabled = glass_enabled(scheme, platform, body_glass);
        self.generation += 1;
        let generation = self.generation;
        self.fade_timer = None;

        if enabled {
            let call = self.window.set_window_glass_enabled(true, background, cx);
            self.settling = Some(cx.spawn(async move |this, cx| {
                // `.catch(() => {})`, then `.finally`: a failed call still flips the page.
                let _ = call.await;
                this.update(cx, |this, cx| {
                    if this.generation == generation {
                        this.has_native_glass = true;
                        cx.notify();
                    }
                })
                .ok();
            }));
            return;
        }

        self.has_native_glass = false;
        cx.notify();
        let fade = self.fade;
        self.fade_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(fade).await;
            let Ok(call) = this.update(cx, |this, cx| {
                this.fade_timer = None;
                this.window.set_window_glass_enabled(false, background, cx)
            }) else {
                return;
            };
            let _ = call.await;
        }));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use gpui::{AppContext as _, Entity, TestAppContext};

    use super::*;

    /// Records each window call, which finishes at once.
    #[derive(Default)]
    struct FakeWindow {
        calls: RefCell<Vec<(bool, Rgb)>>,
        fail: RefCell<bool>,
    }

    impl GlassWindow for FakeWindow {
        fn set_window_glass_enabled(
            &self,
            enabled: bool,
            background: Rgb,
            _cx: &mut App,
        ) -> Task<Result<(), String>> {
            self.calls.borrow_mut().push((enabled, background));
            if *self.fail.borrow() {
                Task::ready(Err("no window".to_string()))
            } else {
                Task::ready(Ok(()))
            }
        }
    }

    /// A window whose calls finish only when the test settles them.
    struct HeldWindow {
        calls: RefCell<Vec<bool>>,
        settle: async_channel::Receiver<()>,
    }

    impl GlassWindow for HeldWindow {
        fn set_window_glass_enabled(
            &self,
            enabled: bool,
            _background: Rgb,
            cx: &mut App,
        ) -> Task<Result<(), String>> {
            self.calls.borrow_mut().push(enabled);
            let settle = self.settle.clone();
            cx.background_spawn(async move {
                let _ = settle.recv().await;
                Ok(())
            })
        }
    }

    const FADE: Duration = Duration::from_millis(120);
    const DARK_BG: Rgb = Rgb {
        r: 23,
        g: 23,
        b: 23,
    };

    fn glass(cx: &mut TestAppContext, window: Rc<dyn GlassWindow>) -> Entity<NativeGlass> {
        cx.new(|_| NativeGlass::new(window, FADE))
    }

    fn sync(
        glass: &Entity<NativeGlass>,
        scheme: ColorScheme,
        platform: Platform,
        body_glass: bool,
        cx: &mut TestAppContext,
    ) {
        glass.update(cx, |glass, cx| {
            glass.sync(scheme, platform, body_glass, DARK_BG, cx)
        });
    }

    fn has_glass(glass: &Entity<NativeGlass>, cx: &mut TestAppContext) -> bool {
        glass.read_with(cx, |glass, _| glass.has_native_glass())
    }

    #[gpui::test]
    fn turns_glass_on_and_marks_the_page_translucent_in_dark_mode(cx: &mut TestAppContext) {
        let window = Rc::new(FakeWindow::default());
        let glass = glass(cx, window.clone());
        sync(&glass, ColorScheme::Dark, Platform::Mac, true, cx);
        cx.run_until_parked();
        assert!(has_glass(&glass, cx));
        assert!(window.calls.borrow()[0].0);
    }

    #[gpui::test]
    fn turns_glass_off_and_paints_the_page_opaque_in_light_mode(cx: &mut TestAppContext) {
        let window = Rc::new(FakeWindow::default());
        let glass = glass(cx, window.clone());
        sync(&glass, ColorScheme::Light, Platform::Mac, true, cx);
        assert!(!has_glass(&glass, cx));
        cx.executor().advance_clock(FADE);
        cx.run_until_parked();
        assert_eq!(window.calls.borrow().as_slice(), &[(false, DARK_BG)]);
    }

    #[test]
    fn fills_an_opaque_window_with_the_themed_colour_not_a_fixed_light_one() {
        assert_eq!(
            opaque_window_background(240.0, 0.0, 20.0),
            Rgb {
                r: 51,
                g: 51,
                b: 51
            }
        );
        assert_eq!(opaque_window_background(240.0, 0.0, 9.0), DARK_BG);
    }

    #[gpui::test]
    fn keeps_the_dark_theme_opaque_on_linux_when_main_pane_glass_is_off(cx: &mut TestAppContext) {
        let window = Rc::new(FakeWindow::default());
        let glass = glass(cx, window.clone());
        sync(&glass, ColorScheme::Dark, Platform::Linux, false, cx);
        assert!(!has_glass(&glass, cx));
        cx.executor().advance_clock(FADE);
        cx.run_until_parked();
        assert_eq!(window.calls.borrow().as_slice(), &[(false, DARK_BG)]);
    }

    #[gpui::test]
    fn enables_glass_on_linux_once_main_pane_glass_is_on(cx: &mut TestAppContext) {
        let window = Rc::new(FakeWindow::default());
        let glass = glass(cx, window.clone());
        sync(&glass, ColorScheme::Dark, Platform::Linux, true, cx);
        cx.run_until_parked();
        assert!(has_glass(&glass, cx));
        assert!(window.calls.borrow()[0].0);
    }

    #[gpui::test]
    fn waits_for_the_window_to_settle_before_the_page_changes(cx: &mut TestAppContext) {
        let (settle, wait) = async_channel::unbounded();
        let window = Rc::new(HeldWindow {
            calls: RefCell::new(Vec::new()),
            settle: wait,
        });
        let glass = glass(cx, window);
        sync(&glass, ColorScheme::Dark, Platform::Mac, true, cx);
        cx.run_until_parked();
        assert!(!has_glass(&glass, cx));
        settle.try_send(()).unwrap();
        cx.run_until_parked();
        assert!(has_glass(&glass, cx));
    }

    #[gpui::test]
    fn fades_the_page_opaque_before_the_window_stops_being_transparent(cx: &mut TestAppContext) {
        let window = Rc::new(FakeWindow::default());
        let glass = glass(cx, window.clone());
        sync(&glass, ColorScheme::Dark, Platform::Linux, true, cx);
        cx.run_until_parked();
        assert!(has_glass(&glass, cx));

        window.calls.borrow_mut().clear();
        sync(&glass, ColorScheme::Dark, Platform::Linux, false, cx);
        assert!(!has_glass(&glass, cx));
        assert!(window.calls.borrow().is_empty());
        cx.executor().advance_clock(FADE);
        cx.run_until_parked();
        assert_eq!(window.calls.borrow().as_slice(), &[(false, DARK_BG)]);
    }

    #[gpui::test]
    fn drops_a_fade_still_owed_to_glass_that_is_back_on(cx: &mut TestAppContext) {
        let window = Rc::new(FakeWindow::default());
        let glass = glass(cx, window.clone());
        sync(&glass, ColorScheme::Dark, Platform::Linux, true, cx);
        cx.run_until_parked();

        window.calls.borrow_mut().clear();
        sync(&glass, ColorScheme::Dark, Platform::Linux, false, cx);
        sync(&glass, ColorScheme::Dark, Platform::Linux, true, cx);
        cx.run_until_parked();
        assert!(has_glass(&glass, cx));
        cx.executor().advance_clock(Duration::from_millis(250));
        cx.run_until_parked();
        assert_eq!(window.calls.borrow().len(), 1);
        assert!(window.calls.borrow()[0].0);
    }

    #[gpui::test]
    fn ignores_an_enable_that_a_newer_disable_has_overtaken(cx: &mut TestAppContext) {
        let (settle, wait) = async_channel::unbounded();
        let window = Rc::new(HeldWindow {
            calls: RefCell::new(Vec::new()),
            settle: wait,
        });
        let glass = glass(cx, window.clone());
        sync(&glass, ColorScheme::Dark, Platform::Linux, true, cx);
        sync(&glass, ColorScheme::Dark, Platform::Linux, false, cx);
        assert!(!has_glass(&glass, cx));

        settle.try_send(()).unwrap();
        settle.try_send(()).unwrap();
        cx.executor().advance_clock(Duration::from_millis(250));
        cx.run_until_parked();
        assert!(!has_glass(&glass, cx));
        assert_eq!(window.calls.borrow().last(), Some(&false));
    }

    #[gpui::test]
    fn still_flips_the_page_when_the_window_call_fails(cx: &mut TestAppContext) {
        let window = Rc::new(FakeWindow::default());
        *window.fail.borrow_mut() = true;
        let glass = glass(cx, window.clone());
        sync(&glass, ColorScheme::Light, Platform::Mac, true, cx);
        cx.run_until_parked();
        assert!(!has_glass(&glass, cx));
        sync(&glass, ColorScheme::Dark, Platform::Mac, true, cx);
        cx.run_until_parked();
        assert!(has_glass(&glass, cx));
    }
}
