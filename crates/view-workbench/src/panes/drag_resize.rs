//! Port of src/shared/hooks/useDragResize.ts: drag a pane's width.
//!
//! The hook wrote the live width onto the DOM node so React re-renders could
//! not fight the cursor, and committed it on release. [`DragResize`] keeps
//! the same two widths: [`DragResize::live_width`] while dragging and
//! [`DragResize::width`] once committed.

/// Which side of the pane the handle sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResizeDirection {
    /// The handle is on the left edge: dragging left widens the pane.
    Left,
    #[default]
    Right,
}

/// `clampTo`: rounds, then clamps.
fn clamp_to(value: f32, min: f32, max: f32) -> f32 {
    value.round().max(min).min(max)
}

#[derive(Debug, Clone, PartialEq)]
pub struct DragResize {
    pub min: f32,
    pub default_width: f32,
    pub direction: ResizeDirection,
    width: f32,
    live: f32,
    drag: Option<(f32, f32)>,
}

impl DragResize {
    /// `useDragResize({ min, max, defaultWidth, initial })`. `max` is the
    /// current maximum; the hook read it through a callback.
    pub fn new(
        min: f32,
        max: f32,
        default_width: f32,
        initial: f32,
        direction: ResizeDirection,
    ) -> Self {
        let width = clamp_to(initial, min, max);
        Self {
            min,
            default_width,
            direction,
            width,
            live: width,
            drag: None,
        }
    }

    /// The committed width (`width`).
    pub fn width(&self) -> f32 {
        self.width
    }

    /// What the pane shows right now (`widthRef`).
    pub fn live_width(&self) -> f32 {
        self.live
    }

    /// `dragging`.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// `onPointerDown` on the handle at window x.
    pub fn press(&mut self, x: f32) {
        self.drag = Some((x, self.live));
    }

    /// A pointer move at window x. Returns the live width.
    pub fn drag_to(&mut self, x: f32, max: f32) -> f32 {
        if let Some((start_x, start_w)) = self.drag {
            let sign = if self.direction == ResizeDirection::Left {
                -1.0
            } else {
                1.0
            };
            self.live = clamp_to(start_w + (x - start_x) * sign, self.min, max);
        }
        self.live
    }

    /// Pointer up or cancel: commits the live width. Returns the width for
    /// `onCommit`, or `None` when no drag was running.
    pub fn release(&mut self, max: f32) -> Option<f32> {
        self.drag.take()?;
        Some(self.commit(self.live, max))
    }

    /// `onDoubleClick`: back to the default width.
    pub fn double_click(&mut self, max: f32) -> f32 {
        self.commit(self.default_width, max)
    }

    fn commit(&mut self, next: f32, max: f32) -> f32 {
        let value = clamp_to(next, self.min, max);
        self.live = value;
        self.width = value;
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_the_initial_width_and_commits_on_release() {
        let mut resize = DragResize::new(200.0, 600.0, 320.0, 900.0, ResizeDirection::Right);
        assert_eq!(resize.width(), 600.0);
        resize.press(100.0);
        assert!(resize.dragging());
        assert_eq!(resize.drag_to(50.4, 600.0), 550.0);
        assert_eq!(resize.width(), 600.0);
        assert_eq!(resize.release(600.0), Some(550.0));
        assert_eq!(resize.width(), 550.0);
        assert_eq!(resize.release(600.0), None);
    }

    #[test]
    fn a_left_handle_grows_toward_the_left_and_double_click_resets() {
        let mut resize = DragResize::new(200.0, 600.0, 320.0, 300.0, ResizeDirection::Left);
        resize.press(500.0);
        assert_eq!(resize.drag_to(450.0, 600.0), 350.0);
        assert_eq!(resize.drag_to(900.0, 600.0), 200.0);
        resize.release(600.0);
        assert_eq!(resize.double_click(600.0), 320.0);
        assert_eq!(resize.live_width(), 320.0);
    }
}
