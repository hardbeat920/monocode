//! CSS filter blur for account identities. Each paint stage needs its own layer:
//! GPUI batches a layer's quads before its glyphs regardless of insertion order.
use crate::theme::*;
use gpui::{
    AnyElement, App, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, Window, div, prelude::*,
};
pub(super) fn masked_text(text: String, masked: bool) -> AnyElement {
    let child = div().truncate().child(text).into_any_element();
    if masked {
        Masked { child }.into_any_element()
    } else {
        child
    }
}
struct Masked {
    child: AnyElement,
}
impl IntoElement for Masked {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Masked {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
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
        (self.child.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.prepaint_at(bounds.origin, window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        window.paint_layer(bounds, |window| self.child.paint(window, cx));
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let quad = gpui::fill(bounds, content(0.02))
            .border_color(gpui::backdrop_blur_marker(5. * window.scale_factor()));
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let quad = gpui::fill(bounds, pane_fill());
        window.paint_layer(bounds, |window| window.paint_quad(quad));
    }
}
