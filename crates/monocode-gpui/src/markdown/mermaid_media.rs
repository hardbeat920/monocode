//! Measure the media at its actual width. Combining an aspect ratio and a
//! maximum height in a flex child leaves excess space in min-content layouts.
use gpui::{prelude::*, *};

pub(super) struct FittedMedia {
    child: AnyElement,
    ratio: f32,
    max_height: f32,
}

impl FittedMedia {
    pub(super) fn new(child: AnyElement, ratio: f32, max_height: f32) -> Self {
        Self {
            child: div()
                .relative()
                .size_full()
                .overflow_hidden()
                .child(child)
                .into_any_element(),
            ratio,
            max_height,
        }
    }
}

impl IntoElement for FittedMedia {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for FittedMedia {
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
        _: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.flex_shrink = 0.;
        let ratio = self.ratio;
        let max_height = self.max_height;
        let layout = window.request_measured_layout(style, move |known, available, _, _| {
            let width = known.width.unwrap_or(match available.width {
                AvailableSpace::Definite(width) => width,
                _ => px(1.),
            });
            size(width, (width / ratio).min(px(max_height)))
        });
        (layout, ())
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
        self.child
            .layout_as_root(bounds.size.map(AvailableSpace::Definite), window, cx);
        self.child.prepaint_at(bounds.origin, window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}
