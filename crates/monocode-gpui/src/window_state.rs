use gpui::{App, Bounds, WindowBounds, px, size};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct WindowState {
    size: [f32; 2],
    maximized: bool,
    fullscreen: bool,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            size: [1180., 800.],
            maximized: false,
            fullscreen: false,
        }
    }
}

impl WindowState {
    pub fn bounds(self, cx: &App) -> WindowBounds {
        let [width, height] = self.size;
        let size = if width.is_finite() && height.is_finite() && width > 0. && height > 0. {
            size(px(width.max(820.)), px(height.max(520.)))
        } else {
            size(px(1180.), px(800.))
        };
        let bounds = Bounds::centered(None, size, cx);
        if self.fullscreen {
            WindowBounds::Fullscreen(bounds)
        } else if self.maximized {
            WindowBounds::Maximized(bounds)
        } else {
            WindowBounds::Windowed(bounds)
        }
    }

    pub fn update(&mut self, bounds: WindowBounds) -> bool {
        let mut next = *self;
        next.maximized = matches!(bounds, WindowBounds::Maximized(_));
        next.fullscreen = matches!(bounds, WindowBounds::Fullscreen(_));
        if let WindowBounds::Windowed(bounds) = bounds {
            let width = bounds.size.width.as_f32();
            let height = bounds.size.height.as_f32();
            if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. {
                return false;
            }
            next.size = [width, height];
        }
        if next == *self {
            return false;
        }
        *self = next;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, point};

    #[gpui::test]
    fn window_state_restores_size_and_mode_without_losing_normal_size(cx: &mut TestAppContext) {
        let normal = Bounds::new(point(px(10.), px(20.)), size(px(1440.), px(900.)));
        let screen = Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.)));
        let mut state = WindowState::default();
        assert!(state.update(WindowBounds::Windowed(normal)));
        assert!(!state.update(WindowBounds::Windowed(normal)));
        for mode in [
            WindowBounds::Maximized(screen),
            WindowBounds::Fullscreen(screen),
        ] {
            assert!(state.update(mode));
            let saved = serde_json::to_string(&state).unwrap();
            let restored: WindowState = serde_json::from_str(&saved).unwrap();
            let bounds = cx.update(|cx| restored.bounds(cx));
            assert_eq!(bounds.get_bounds().size, normal.size);
            assert_eq!(
                std::mem::discriminant(&bounds),
                std::mem::discriminant(&mode)
            );
        }
        assert!(state.update(WindowBounds::Windowed(normal)));
        assert!(matches!(
            cx.update(|cx| state.bounds(cx)),
            WindowBounds::Windowed(_)
        ));
    }

    #[gpui::test]
    fn window_state_defaults_and_rejects_invalid_sizes(cx: &mut TestAppContext) {
        let state: WindowState = serde_json::from_str("{}").unwrap();
        assert_eq!(
            cx.update(|cx| state.bounds(cx)).get_bounds().size,
            size(px(1180.), px(800.))
        );
        for dimensions in [
            [0., 0.],
            [-1., 700.],
            [f32::NAN, 800.],
            [900., f32::INFINITY],
        ] {
            let mut invalid = WindowState {
                size: dimensions,
                ..state
            };
            assert_eq!(
                cx.update(|cx| invalid.bounds(cx)).get_bounds().size,
                size(px(1180.), px(800.))
            );
            assert!(!invalid.update(WindowBounds::Windowed(Bounds::new(
                point(px(0.), px(0.)),
                size(px(dimensions[0]), px(dimensions[1])),
            ))));
        }
        let small = WindowState {
            size: [100., 100.],
            ..state
        };
        assert_eq!(
            cx.update(|cx| small.bounds(cx)).get_bounds().size,
            size(px(820.), px(520.))
        );
    }
}
