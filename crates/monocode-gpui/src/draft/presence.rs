//! Keeps a draft's page on screen only while its card is in a frame, including
//! a frame that replays the thread from GPUI's cache.
use super::web;
use gpui::{
    App, Context, ElementId, GlobalElementId, InspectorElementId, LayoutId, Pixels, Style, Window,
    WindowId, prelude::*,
};
use std::{
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

/// One window's claim on a card: whichever paint drew the card last.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct Claim {
    pub(super) window: WindowId,
    serial: u64,
}

/// Paints the draft. `key` names the draft, so its card keeps its place in
/// GPUI's element state across frames.
pub(super) fn surface<T: 'static>(
    host: &web::Host,
    key: &str,
    placement: web::Placement,
    window: &mut Window,
    cx: &mut Context<T>,
) -> gpui::AnyElement {
    gpui::div()
        .id(ElementId::Name(format!("draft-page-{key}").into()))
        .relative()
        .size_full()
        .child(web::surface(host, placement, window, cx))
        .child(Presence(host.clone()))
        .into_any_element()
}

/// Holds the card's place in the frame. The thread around the card may be
/// replayed from GPUI's cache rather than painted, which carries this
/// element's state into the new frame without running any of its code; a
/// frame that no longer holds the card drops that state, and with it
/// `Shown`, which takes the page off screen. So the page leaves when its
/// card does: scrolled away, its conversation in the background, or a
/// surface such as Settings drawn in the thread's place.
struct Presence(web::Host);

/// The card is in a frame of this window. See `Presence`.
struct Shown {
    host: web::Host,
    claim: Claim,
}

impl Shown {
    fn claim(host: &web::Host, window: WindowId) -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let claim = Claim {
            window,
            serial: SERIAL.fetch_add(1, Ordering::Relaxed),
        };
        host.claim.set(Some(claim));
        host.set_shown(Some(window));
        Self {
            host: host.clone(),
            claim,
        }
    }

    /// Still the latest claim on `host`, from `window`.
    fn holds(&self, host: &web::Host, window: WindowId) -> bool {
        Rc::ptr_eq(&self.host.claim, &host.claim)
            && self.claim.window == window
            && host.claim.get() == Some(self.claim)
    }
}

impl Drop for Shown {
    fn drop(&mut self) {
        // A newer paint, in this window or another, holds the card now.
        if self.host.claim.get() != Some(self.claim) {
            return;
        }
        self.host.claim.set(None);
        self.host.set_shown(None);
        self.host.off_screen();
    }
}

impl IntoElement for Presence {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for Presence {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some("draft-presence".into())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = Style {
            position: gpui::Position::Absolute,
            ..Style::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: gpui::Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: gpui::Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        let Some(id) = id else {
            return;
        };
        let now = window.window_handle().window_id();
        let host = &self.0;
        window.with_element_state::<Shown, _>(id, |shown, _| match shown {
            Some(shown) if shown.holds(host, now) => ((), shown),
            older => {
                // Claim first, so the older claim's drop leaves the page up.
                let fresh = Shown::claim(host, now);
                drop(older);
                ((), fresh)
            }
        });
    }
}
