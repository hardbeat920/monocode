use gpui::{
    AnyElement, Bounds, CursorStyle, MouseButton, Pixels, ResizeEdge, Size, Window,
    WindowControlArea, div, point, prelude::*, px, size,
};

pub(super) const CONTROLS_WIDTH: f32 = 106.;

// Windows uses the Tauri app's `WindowControls` glyphs.
#[cfg(target_os = "windows")]
const MINIMIZE_ICON: &str = "minus-sign";
#[cfg(target_os = "windows")]
const MAXIMIZE_ICON: &str = "window-maximize";
#[cfg(target_os = "windows")]
const RESTORE_ICON: &str = "window-restore";
#[cfg(target_os = "windows")]
const CLOSE_ICON: &str = "cancel-01";
#[cfg(not(target_os = "windows"))]
const MINIMIZE_ICON: &str = "minus";
#[cfg(not(target_os = "windows"))]
const MAXIMIZE_ICON: &str = "square";
#[cfg(not(target_os = "windows"))]
const RESTORE_ICON: &str = "copy";
#[cfg(not(target_os = "windows"))]
const CLOSE_ICON: &str = "close";

pub(super) fn controls(
    window: &Window,
    on_close: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let maximized = window.is_maximized();
    div()
        .absolute()
        .top_0()
        .right_0()
        .w(px(CONTROLS_WIDTH))
        .h(px(crate::theme::TITLE_BAR_HEIGHT))
        .px(px(8.))
        .flex()
        .items_center()
        .occlude()
        .child(caption_button(
            crate::theme::icon_button("window-minimize", "Minimize window", MINIMIZE_ICON)
                .debug_selector(|| "window-minimize".into()),
            WindowControlArea::Min,
            |_, window, _| window.minimize_window(),
        ))
        .child(caption_button(
            crate::theme::icon_button(
                "window-maximize",
                if maximized {
                    "Restore window"
                } else {
                    "Maximize window"
                },
                if maximized {
                    RESTORE_ICON
                } else {
                    MAXIMIZE_ICON
                },
            )
            .debug_selector(|| "window-maximize".into()),
            WindowControlArea::Max,
            |_, window, _| window.zoom_window(),
        ))
        .child(caption_button(
            crate::theme::icon_button("window-close", "Close window", CLOSE_ICON)
                .debug_selector(|| "window-close".into()),
            WindowControlArea::Close,
            on_close,
        ))
        .into_any_element()
}

/// On Windows the buttons hit-test as the system caption buttons. Windows
/// then minimizes, toggles maximize and restore, shows Snap Layouts and
/// closes through `WM_CLOSE`. GPUI's `zoom_window` can only maximize there.
fn caption_button(
    button: gpui::Stateful<gpui::Div>,
    area: WindowControlArea,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<gpui::Div> {
    let button = button.w(px(30.));
    if cfg!(target_os = "windows") {
        return button.window_control_area(area);
    }
    button
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(on_click)
}

// Keep the resize border narrow around controls, with larger corner targets.
fn resize_regions(viewport: Size<Pixels>) -> [(ResizeEdge, CursorStyle, Bounds<Pixels>); 8] {
    use CursorStyle::*;
    use ResizeEdge::*;
    let w = f32::from(viewport.width);
    let h = f32::from(viewport.height);
    let edge = 6.;
    let corner = 12.;
    [
        (Top, ResizeUpDown, corner, 0., w - 2. * corner, edge),
        (
            Bottom,
            ResizeUpDown,
            corner,
            h - edge,
            w - 2. * corner,
            edge,
        ),
        (Left, ResizeLeftRight, 0., corner, edge, h - 2. * corner),
        (
            Right,
            ResizeLeftRight,
            w - edge,
            corner,
            edge,
            h - 2. * corner,
        ),
        (TopLeft, ResizeUpLeftDownRight, 0., 0., corner, corner),
        (
            TopRight,
            ResizeUpRightDownLeft,
            w - corner,
            0.,
            corner,
            corner,
        ),
        (
            BottomLeft,
            ResizeUpRightDownLeft,
            0.,
            h - corner,
            corner,
            corner,
        ),
        (
            BottomRight,
            ResizeUpLeftDownRight,
            w - corner,
            h - corner,
            corner,
            corner,
        ),
    ]
    .map(|(edge, cursor, x, y, w, h)| {
        (
            edge,
            cursor,
            Bounds::new(point(px(x), px(y)), size(px(w), px(h))),
        )
    })
}

pub(super) fn resize_handles(window: &Window) -> Vec<AnyElement> {
    if window.is_fullscreen()
        || window.is_maximized()
        || !matches!(
            window.window_decorations(),
            gpui::Decorations::Client { .. }
        )
    {
        return Vec::new();
    }
    resize_regions(window.viewport_size())
        .into_iter()
        .map(|(edge, cursor, bounds)| {
            div()
                .absolute()
                .occlude()
                .left(bounds.left())
                .top(bounds.top())
                .w(bounds.size.width)
                .h(bounds.size.height)
                .cursor(cursor)
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    cx.stop_propagation();
                    window.start_window_resize(edge);
                })
                .into_any_element()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn controls_fit_the_reserved_space_and_close_receives_clicks(cx: &mut gpui::TestAppContext) {
        struct Preview {
            close_requests: usize,
        }
        impl gpui::Render for Preview {
            fn render(
                &mut self,
                window: &mut Window,
                cx: &mut gpui::Context<Self>,
            ) -> impl IntoElement {
                div().size_full().child(
                    super::super::titlebar_drag_area()
                        .w_full()
                        .h(px(crate::theme::TITLE_BAR_HEIGHT))
                        .child(controls(
                            window,
                            cx.listener(|this, _, _, cx| {
                                this.close_requests += 1;
                                cx.notify();
                            }),
                        )),
                )
            }
        }
        let window = cx.update(|cx| {
            cx.open_window(gpui::WindowOptions::default(), |_, cx| {
                cx.new(|_| Preview { close_requests: 0 })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        visual.run_until_parked();
        let width = window
            .update(visual, |_, window, _| window.viewport_size().width)
            .unwrap();
        let mut right = width - px(CONTROLS_WIDTH);
        for id in ["window-minimize", "window-maximize", "window-close"] {
            let bounds = visual.debug_bounds(id).unwrap();
            assert!(bounds.left() >= right);
            assert!(bounds.right() <= width);
            right = bounds.right();
        }
        // Windows sends these clicks to its caption buttons, not to GPUI.
        if cfg!(target_os = "windows") {
            return;
        }
        let close = visual.debug_bounds("window-close").unwrap();
        visual.simulate_click(close.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |preview, _, _| {
                assert_eq!(preview.close_requests, 1)
            })
            .unwrap();
    }

    #[test]
    fn resize_regions_cover_edges_and_corners_without_covering_content() {
        let viewport = size(px(820.), px(520.));
        let regions = resize_regions(viewport);
        for (position, expected) in [
            ((1., 1.), ResizeEdge::TopLeft),
            ((410., 1.), ResizeEdge::Top),
            ((819., 1.), ResizeEdge::TopRight),
            ((819., 260.), ResizeEdge::Right),
            ((819., 519.), ResizeEdge::BottomRight),
            ((410., 519.), ResizeEdge::Bottom),
            ((1., 519.), ResizeEdge::BottomLeft),
            ((1., 260.), ResizeEdge::Left),
        ] {
            let hits: Vec<_> = regions
                .iter()
                .filter(|(_, _, bounds)| bounds.contains(&point(px(position.0), px(position.1))))
                .map(|(edge, _, _)| *edge)
                .collect();
            assert_eq!(hits, vec![expected]);
        }
        for position in [(410., 260.), (20., 20.), (800., 500.)] {
            assert!(regions.iter().all(|(_, _, bounds)| {
                !bounds.contains(&point(px(position.0), px(position.1)))
            }));
        }
    }
}
