//! Save the main window's geometry and restore its visible display.

use std::collections::HashMap;
use std::time::Duration;

use gpui::{
    App, AppContext as _, Bounds, Context, Entity, Global, Pixels, Subscription, Task, Window,
    WindowBounds, WindowId, point, px, size,
};
use monocode_app::boot::AppServices;
use monocode_settings::Kv;
use serde::{Deserialize, Serialize};

const KEY: &str = "monocode.nativeMainWindowState";

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Mode {
    Windowed,
    Maximized,
    Fullscreen,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
struct SavedBounds {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    mode: Mode,
}

impl SavedBounds {
    fn capture(window: &Window) -> Self {
        let mut saved = Self::from_bounds(window.window_bounds());
        if saved.mode == Mode::Windowed && window.is_maximized() {
            saved.mode = Mode::Maximized;
        }
        saved
    }

    fn from_bounds(bounds: WindowBounds) -> Self {
        let mode = match bounds {
            WindowBounds::Windowed(_) => Mode::Windowed,
            WindowBounds::Maximized(_) => Mode::Maximized,
            WindowBounds::Fullscreen(_) => Mode::Fullscreen,
        };
        let bounds = bounds.get_bounds();
        Self {
            x: bounds.origin.x.into(),
            y: bounds.origin.y.into(),
            width: bounds.size.width.into(),
            height: bounds.size.height.into(),
            mode,
        }
    }

    fn restore(self, displays: &[Bounds<Pixels>]) -> Option<WindowBounds> {
        if ![self.x, self.y, self.width, self.height]
            .iter()
            .all(|n| n.is_finite())
            || self.width <= 0.
            || self.height <= 0.
        {
            return None;
        }
        let mut bounds = Bounds::new(
            point(px(self.x), px(self.y)),
            size(px(self.width.max(800.)), px(self.height.max(520.))),
        );
        if let Some(display) = displays
            .iter()
            .find(|display| display.intersects(&bounds))
            .or_else(|| displays.first())
        {
            bounds.size.width = bounds.size.width.min(display.size.width.max(px(800.)));
            bounds.size.height = bounds.size.height.min(display.size.height.max(px(520.)));
            if !display.intersects(&bounds) {
                bounds.origin = display.center() - (bounds.size / 2.).into();
            } else {
                bounds.origin.x = bounds.origin.x.clamp(
                    display.origin.x - bounds.size.width + px(80.),
                    display.right() - px(80.),
                );
                bounds.origin.y = bounds
                    .origin
                    .y
                    .clamp(display.top(), display.bottom() - px(40.));
            }
        }
        Some(match self.mode {
            Mode::Windowed => WindowBounds::Windowed(bounds),
            Mode::Maximized => WindowBounds::Maximized(bounds),
            Mode::Fullscreen => WindowBounds::Fullscreen(bounds),
        })
    }

    fn with_restore_bounds(self, previous: Self) -> Self {
        if self.mode == Mode::Windowed {
            self
        } else {
            Self {
                mode: self.mode,
                ..previous
            }
        }
    }
}

#[derive(Deserialize)]
struct LegacyBounds {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    #[serde(default)]
    prev_x: f32,
    #[serde(default)]
    prev_y: f32,
    #[serde(default)]
    maximized: bool,
    #[serde(default)]
    fullscreen: bool,
}

impl LegacyBounds {
    fn logical(self, scale: f32) -> SavedBounds {
        let scale = if scale.is_finite() && scale > 0. {
            scale
        } else {
            1.
        };
        SavedBounds {
            x: if self.maximized { self.prev_x } else { self.x } / scale,
            y: if self.maximized { self.prev_y } else { self.y } / scale,
            width: self.width / scale,
            height: self.height / scale,
            mode: if self.fullscreen {
                Mode::Fullscreen
            } else if self.maximized {
                Mode::Maximized
            } else {
                Mode::Windowed
            },
        }
    }
}

fn legacy_scale() -> f32 {
    #[cfg(target_os = "macos")]
    return monocode_platform::macos_panel::primary_scale_factor();
    #[cfg(windows)]
    return monocode_platform::windows::primary_scale_factor();
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        ["GPUI_FORCE_SCALE_FACTOR", "GDK_SCALE"]
            .iter()
            .find_map(|key| std::env::var(key).ok()?.parse::<f32>().ok())
            .unwrap_or(1.)
    }
}

pub fn restore(cx: &App) -> Option<WindowBounds> {
    let services = AppServices::try_global(cx)?;
    let saved = services
        .kv
        .get_item(KEY)
        .and_then(|raw| serde_json::from_str::<SavedBounds>(&raw).ok())
        .or_else(|| {
            let raw = std::fs::read(services.data_dir.path.join(".window-state.json")).ok()?;
            let mut states: HashMap<String, LegacyBounds> = serde_json::from_slice(&raw).ok()?;
            Some(states.remove("main")?.logical(legacy_scale()))
        })?;
    let displays: Vec<_> = cx
        .displays()
        .iter()
        .map(|display| display.visible_bounds())
        .collect();
    saved.restore(&displays)
}

#[derive(Default)]
struct Observers(HashMap<WindowId, Entity<Observer>>);
impl Global for Observers {}

struct Observer {
    kv: Kv,
    saved: SavedBounds,
    pending: Option<Task<()>>,
    _subscription: Subscription,
}

impl Observer {
    fn flush(&self) {
        if let Ok(raw) = serde_json::to_string(&self.saved) {
            self.kv.set_item(KEY, &raw);
        }
    }

    fn changed(&mut self, window: &Window, cx: &mut Context<Self>) {
        let saved = SavedBounds::capture(window).with_restore_bounds(self.saved);
        if self.saved == saved || saved.width <= 0. || saved.height <= 0. {
            return;
        }
        self.saved = saved;
        self.pending = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            this.update(cx, |this, _| this.flush()).ok();
        }));
    }
}

pub fn install(window: &mut Window, initial: Option<WindowBounds>, cx: &mut App) {
    let Some(kv) = AppServices::try_global(cx).map(|services| services.kv.clone()) else {
        return;
    };
    let observer = cx.new(|cx| {
        cx.on_release(|this: &mut Observer, _| this.flush())
            .detach();
        Observer {
            kv,
            saved: SavedBounds::capture(window).with_restore_bounds(
                initial
                    .map(SavedBounds::from_bounds)
                    .unwrap_or_else(|| SavedBounds::capture(window)),
            ),
            pending: None,
            _subscription: cx
                .observe_window_bounds(window, |this, window, cx| this.changed(window, cx)),
        }
    });
    let weak = observer.downgrade();
    cx.on_app_quit(move |cx| {
        weak.update(cx, |this, _| this.flush()).ok();
        async {}
    })
    .detach();
    cx.default_global::<Observers>()
        .0
        .insert(window.window_handle().window_id(), observer);
    cx.on_window_closed(|cx, id| {
        if cx.has_global::<Observers>() {
            cx.global_mut::<Observers>().0.remove(&id);
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Bounds<Pixels> {
        Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.)))
    }

    #[test]
    fn geometry_round_trips_every_window_mode() {
        let bounds = Bounds::new(point(px(90.), px(60.)), size(px(1000.), px(700.)));
        for mode in [
            WindowBounds::Windowed(bounds),
            WindowBounds::Maximized(bounds),
            WindowBounds::Fullscreen(bounds),
        ] {
            let saved = SavedBounds::from_bounds(mode);
            let raw = serde_json::to_string(&saved).unwrap();
            let read: SavedBounds = serde_json::from_str(&raw).unwrap();
            assert_eq!(read.restore(&[screen()]), Some(mode));
        }
    }

    #[test]
    fn removed_monitor_moves_the_saved_window_to_a_visible_display() {
        let saved = SavedBounds {
            x: 3000.,
            y: -900.,
            width: 1200.,
            height: 800.,
            mode: Mode::Windowed,
        };
        let restored = saved.restore(&[screen()]).unwrap().get_bounds();
        assert_eq!(restored.origin, point(px(360.), px(140.)));
        assert!(screen().contains(&restored.origin));
        let invalid = SavedBounds {
            width: f32::NAN,
            ..saved
        };
        assert!(invalid.restore(&[screen()]).is_none());
    }

    #[test]
    fn legacy_geometry_uses_restore_position_and_physical_scale() {
        let legacy: LegacyBounds = serde_json::from_value(serde_json::json!({
            "x": 0, "y": 0, "prev_x": 180, "prev_y": 120,
            "width": 2000, "height": 1400, "maximized": true, "fullscreen": true,
            "visible": false, "decorated": true
        }))
        .unwrap();
        let saved = legacy.logical(2.);
        assert_eq!(
            (saved.x, saved.y, saved.width, saved.height),
            (90., 60., 1000., 700.)
        );
        assert_eq!(saved.mode, Mode::Fullscreen);
    }

    #[test]
    fn maximizing_and_fullscreen_preserve_the_normal_restore_geometry() {
        let normal = SavedBounds::from_bounds(WindowBounds::Windowed(Bounds::new(
            point(px(90.), px(60.)),
            size(px(1000.), px(700.)),
        )));
        let maximized = SavedBounds {
            mode: Mode::Maximized,
            x: 0.,
            y: 0.,
            width: 1920.,
            height: 1080.,
        };
        let saved = maximized.with_restore_bounds(normal);
        assert_eq!(
            saved,
            SavedBounds {
                mode: Mode::Maximized,
                ..normal
            }
        );
        let fullscreen = SavedBounds {
            mode: Mode::Fullscreen,
            ..maximized
        };
        assert_eq!(
            fullscreen.with_restore_bounds(saved),
            SavedBounds {
                mode: Mode::Fullscreen,
                ..normal
            }
        );
        assert_eq!(normal.with_restore_bounds(saved), normal);
    }
}
