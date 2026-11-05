//! A native view stays where it was last placed and draws over everything
//! GPUI paints, so whatever hosts one has to notice when the paint that should
//! have placed it did not happen: the panel slid away, or Settings was drawn in
//! its place. The sentinel, put last in a window's root, steps it aside then.
use gpui::{Window, canvas, prelude::*};
use std::{cell::Cell, cell::RefCell, rc::Rc};

/// The window and epoch of the last paint that placed an overlay.
pub(crate) type Painted = Rc<Cell<(gpui::WindowId, u64)>>;

thread_local! {
    /// Bumped before each paint of a window, so an overlay can tell whether
    /// it was placed in the paint that just happened.
    static EPOCH: Cell<u64> = const { Cell::new(0) };
    static OVERLAYS: RefCell<Vec<(std::rc::Weak<dyn Overlay>, Painted)>> =
        const { RefCell::new(Vec::new()) };
}

/// A native view placed over the window's content, such as a device screen.
pub(crate) trait Overlay {
    fn step_aside(&self);
}

/// Lets the sentinel step `overlay` aside; its paint calls `mark_painted`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn track_overlay(overlay: &Rc<dyn Overlay>, painted: &Painted) {
    OVERLAYS.with(|overlays| {
        let mut overlays = overlays.borrow_mut();
        if !overlays
            .iter()
            .any(|(tracked, _)| std::ptr::addr_eq(tracked.as_ptr(), Rc::as_ptr(overlay)))
        {
            overlays.push((Rc::downgrade(overlay), painted.clone()));
        }
    });
}

/// Records that the overlay was placed in this paint of `window`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn mark_painted(painted: &Painted, window: &Window) {
    painted.set((
        Window::window_handle(window).window_id(),
        EPOCH.with(Cell::get),
    ));
}

/// Put last in a window's root, after anything that can hold an overlay.
/// Anything that floats over one has to be `deferred`, so it can see it is
/// covered.
pub(crate) fn sentinel() -> gpui::AnyElement {
    canvas(
        |_, _, _| EPOCH.with(|epoch| epoch.set(epoch.get() + 1)),
        |_, _, window, _| {
            let now = (
                Window::window_handle(window).window_id(),
                EPOCH.with(Cell::get),
            );
            OVERLAYS.with(|overlays| {
                overlays.borrow_mut().retain(|(overlay, painted)| {
                    let Some(overlay) = overlay.upgrade() else {
                        return false;
                    };
                    // Another window's overlays are its own sentinel's to hide.
                    if painted.get().0 == now.0 && painted.get() != now {
                        overlay.step_aside();
                    }
                    true
                });
            });
        },
    )
    .absolute()
    .size_0()
    .into_any_element()
}
