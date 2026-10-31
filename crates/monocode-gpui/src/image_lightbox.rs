//! Native port of `src/shared/ui/ImageLightbox.tsx`.
use crate::theme::{black, icon, white};
use crate::tooltip::Tip;
use gpui::{
    App, Bounds, BoxShadow, Context, CursorStyle, Entity, EventEmitter, FocusHandle, Image,
    ImageFormat, ImgResourceLoader, KeyBinding, MouseButton, Pixels, Point, RenderImage, Role,
    SharedString, Size, Window, actions, canvas, div, img, point, prelude::*, px, size,
};
use std::{path::PathBuf, sync::Arc, time::Instant};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Source {
    Path(PathBuf),
    Embedded { data: Arc<str>, mime: SharedString },
}

#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = image_lightbox, no_json)]
pub(crate) struct Open {
    pub source: Source,
    pub alt: SharedString,
}

actions!(image_lightbox, [Close, ZoomIn, ZoomOut, ResetZoom]);

pub(crate) fn bind(cx: &mut App) {
    for modifier in ["cmd", "ctrl"] {
        cx.bind_keys([
            KeyBinding::new(&format!("{modifier}-+"), ZoomIn, Some("ImageLightbox")),
            KeyBinding::new(&format!("{modifier}-="), ZoomIn, Some("ImageLightbox")),
            KeyBinding::new(&format!("{modifier}--"), ZoomOut, Some("ImageLightbox")),
            KeyBinding::new(&format!("{modifier}-0"), ResetZoom, Some("ImageLightbox")),
        ]);
    }
    cx.bind_keys([KeyBinding::new("escape", Close, Some("ImageLightbox"))]);
}

const MIN_SCALE: f32 = 1.;
const MAX_SCALE: f32 = 8.;
const DOUBLE_CLICK_SCALE: f32 = 2.5;
const KEY_ZOOM_STEP: f32 = 1.25;

#[derive(Clone, Copy, Debug, PartialEq)]
struct View {
    scale: f32,
    x: f32,
    y: f32,
}
impl View {
    const IDENTITY: Self = Self {
        scale: 1.,
        x: 0.,
        y: 0.,
    };

    fn clamp(self, viewport: Size<Pixels>, image: Size<Pixels>) -> Self {
        if self.scale <= MIN_SCALE {
            return Self::IDENTITY;
        }
        let max_x = ((image.width.as_f32() * self.scale - viewport.width.as_f32()) / 2.).max(0.);
        let max_y = ((image.height.as_f32() * self.scale - viewport.height.as_f32()) / 2.).max(0.);
        Self {
            x: self.x.clamp(-max_x, max_x),
            y: self.y.clamp(-max_y, max_y),
            ..self
        }
    }

    fn zoom_at(
        self,
        scale: f32,
        pointer: Point<Pixels>,
        viewport: Size<Pixels>,
        image: Size<Pixels>,
    ) -> Self {
        let scale = scale.clamp(MIN_SCALE, MAX_SCALE);
        let dx = pointer.x.as_f32() - viewport.width.as_f32() / 2.;
        let dy = pointer.y.as_f32() - viewport.height.as_f32() / 2.;
        let ratio = scale / self.scale;
        Self {
            scale,
            x: dx - ratio * (dx - self.x),
            y: dy - ratio * (dy - self.y),
        }
        .clamp(viewport, image)
    }
}

fn fitted_size(natural: Size<Pixels>, viewport: Size<Pixels>) -> Size<Pixels> {
    let ratio = ((viewport.width.as_f32() - 48.).max(0.) / natural.width.as_f32().max(1.))
        .min((viewport.height.as_f32() - 48.).max(0.) / natural.height.as_f32().max(1.))
        .min(1.);
    size(natural.width * ratio, natural.height * ratio)
}

pub(crate) struct Closed;
pub(crate) struct ImageLightbox {
    source: Source,
    alt: SharedString,
    image: Option<Arc<RenderImage>>,
    failed: bool,
    focus: FocusHandle,
    restore: Option<FocusHandle>,
    viewport: Size<Pixels>,
    image_size: Size<Pixels>,
    view: View,
    transition: Option<(View, Instant)>,
    drag: Option<(View, Point<Pixels>)>,
}
impl EventEmitter<Closed> for ImageLightbox {}

impl ImageLightbox {
    pub(crate) fn new(open: Open, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let restore = window.focused(cx);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        if let Source::Embedded { data, mime } = &open.source {
            let data = data.clone();
            let format = ImageFormat::from_mime_type(mime).unwrap_or(ImageFormat::Png);
            let renderer = cx.svg_renderer();
            let work = cx.background_executor().spawn(async move {
                let bytes = monocode_core::composer::decode_image_data(&data).ok()?;
                Image::from_bytes(format, bytes)
                    .to_image_data(renderer)
                    .ok()
            });
            cx.spawn(async move |this, cx| {
                let image = work.await;
                let _ = this.update(cx, |this, cx| {
                    this.failed = image.is_none();
                    this.image = image;
                    cx.notify();
                });
            })
            .detach();
        }
        // GPUI keeps decoded files and atlas sprites until told; closing the
        // preview must give back the full-resolution copy.
        cx.on_release(|this, cx| {
            if let Some(image) = this.image.take() {
                cx.drop_image(image, None);
            }
            if let Source::Path(path) = &this.source {
                cx.remove_asset::<ImgResourceLoader>(&path.clone().into());
            }
        })
        .detach();
        Self {
            source: open.source,
            alt: open.alt,
            image: None,
            failed: false,
            focus,
            restore,
            viewport: window.viewport_size(),
            image_size: size(px(0.), px(0.)),
            view: View::IDENTITY,
            transition: None,
            drag: None,
        }
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(restore) = self.restore.take() {
            window.focus(&restore, cx);
        }
        cx.emit(Closed);
        cx.stop_propagation();
    }

    fn displayed_view(&self) -> View {
        let Some((from, born)) = self.transition else {
            return self.view;
        };
        let t = crate::reveal::bezier(
            (born.elapsed().as_secs_f32() / 0.06).min(1.),
            0.,
            0.,
            0.58,
            1.,
        );
        View {
            scale: from.scale + (self.view.scale - from.scale) * t,
            x: from.x + (self.view.x - from.x) * t,
            y: from.y + (self.view.y - from.y) * t,
        }
    }

    fn set_view(&mut self, next: View, cx: &mut Context<Self>) {
        let next = next.clamp(self.viewport, self.image_size);
        if next != self.view {
            self.transition = self
                .drag
                .is_none()
                .then(|| (self.displayed_view(), Instant::now()));
            self.view = next;
            cx.notify();
        }
    }

    fn zoom_at(&mut self, scale: f32, pointer: Point<Pixels>, cx: &mut Context<Self>) {
        self.set_view(
            self.view
                .zoom_at(scale, pointer, self.viewport, self.image_size),
            cx,
        );
    }

    fn zoom_key(&mut self, factor: f32, cx: &mut Context<Self>) {
        self.zoom_at(
            self.view.scale * factor,
            point(self.viewport.width / 2., self.viewport.height / 2.),
            cx,
        );
        cx.stop_propagation();
    }

    fn image_bounds(&self, view: View) -> Bounds<Pixels> {
        let dimensions = size(
            self.image_size.width * view.scale,
            self.image_size.height * view.scale,
        );
        Bounds::new(
            point(
                (self.viewport.width - dimensions.width) / 2. + px(view.x),
                (self.viewport.height - dimensions.height) / 2. + px(view.y),
            ),
            dimensions,
        )
    }
}

impl Render for ImageLightbox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.image.is_none()
            && !self.failed
            && let Source::Path(path) = &self.source
            && let Some(result) = window.use_asset::<ImgResourceLoader>(&path.clone().into(), cx)
        {
            match result {
                Ok(image) => self.image = Some(image),
                Err(_) => self.failed = true,
            }
        }
        self.viewport = window.viewport_size();
        if let Some(image) = &self.image {
            let natural = image.size(0);
            self.image_size = fitted_size(
                size(px(natural.width.0 as f32), px(natural.height.0 as f32)),
                self.viewport,
            );
        }
        self.view = self.view.clamp(self.viewport, self.image_size);
        let view = self.displayed_view().clamp(self.viewport, self.image_size);
        if self
            .transition
            .is_some_and(|(_, born)| born.elapsed().as_secs_f32() < 0.06)
        {
            window.request_animation_frame();
        } else {
            self.transition = None;
        }
        let bounds = self.image_bounds(view);
        let zoomed = self.view.scale > MIN_SCALE;
        div()
            .id("image-lightbox")
            .debug_selector(|| "image-lightbox".into())
            .role(Role::Dialog)
            .aria_label(format!("Image preview: {}", self.alt))
            .key_context("ImageLightbox")
            .absolute()
            .top_0()
            .left_0()
            .w(self.viewport.width)
            .h(self.viewport.height)
            .overflow_hidden()
            .on_action(cx.listener(|this, _: &Close, window, cx| this.close(window, cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_key(KEY_ZOOM_STEP, cx)))
            .on_action(
                cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_key(1. / KEY_ZOOM_STEP, cx)),
            )
            .on_action(cx.listener(|this, _: &ResetZoom, _, cx| {
                this.set_view(View::IDENTITY, cx);
                cx.stop_propagation();
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    if !this
                        .image_bounds(this.displayed_view())
                        .contains(&event.position)
                        || this.image.is_none()
                    {
                        this.close(window, cx);
                    } else if event.click_count == 2 {
                        this.drag = None;
                        if this.view.scale > MIN_SCALE {
                            this.set_view(View::IDENTITY, cx);
                        } else {
                            this.zoom_at(DOUBLE_CLICK_SCALE, event.position, cx);
                        }
                    } else if this.view.scale > MIN_SCALE {
                        this.transition = None;
                        this.drag = Some((this.view, event.position));
                        cx.notify();
                    }
                }),
            )
            .on_click(|_, _, cx| cx.stop_propagation())
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                if let Some((origin, pointer)) = this.drag {
                    if event.pressed_button == Some(MouseButton::Left) {
                        this.set_view(
                            View {
                                x: origin.x + (event.position.x - pointer.x).as_f32(),
                                y: origin.y + (event.position.y - pointer.y).as_f32(),
                                ..origin
                            },
                            cx,
                        );
                    } else {
                        this.drag = None;
                        cx.notify();
                    }
                }
                cx.stop_propagation();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.drag = None;
                    cx.notify();
                    cx.stop_propagation();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.drag = None;
                    cx.notify();
                }),
            )
            .on_scroll_wheel(cx.listener(|this, event: &gpui::ScrollWheelEvent, _, cx| {
                let delta = event.delta.pixel_delta(px(16.));
                if event.modifiers.control || event.modifiers.platform {
                    // GPUI's wheel deltas have the opposite sign to DOM wheel deltas.
                    this.zoom_at(
                        this.view.scale * (delta.y.as_f32() * 0.01).exp(),
                        event.position,
                        cx,
                    );
                } else if this.view.scale > MIN_SCALE {
                    this.set_view(
                        View {
                            x: this.view.x + delta.x.as_f32(),
                            y: this.view.y + delta.y.as_f32(),
                            ..this.view
                        },
                        cx,
                    );
                }
                cx.stop_propagation();
            }))
            .on_pinch(cx.listener(|this, event: &gpui::PinchEvent, _, cx| {
                this.zoom_at(this.view.scale * (1. + event.delta), event.position, cx);
                cx.stop_propagation();
            }))
            .child(
                canvas(
                    |_, _, _| {},
                    |bounds, _, window, _| {
                        let quad = gpui::fill(bounds, black(0.85));
                        #[cfg(any(target_os = "macos", target_os = "windows"))]
                        let quad = quad
                            .border_color(gpui::backdrop_blur_marker(4. * window.scale_factor()));
                        window.paint_quad(quad);
                    },
                )
                .absolute()
                .size_full(),
            )
            .when_some(self.image.clone(), |d, image| {
                d.child(
                    div()
                        .absolute()
                        .left(bounds.left())
                        .top(bounds.top())
                        .w(bounds.size.width)
                        .h(bounds.size.height)
                        .debug_selector(|| "lightbox-image".into())
                        .cursor(if !zoomed {
                            CursorStyle::PointingHand
                        } else if self.drag.is_some() {
                            CursorStyle::ClosedHand
                        } else {
                            CursorStyle::OpenHand
                        })
                        .shadow(vec![
                            BoxShadow::new(px(0.), px(25.), black(0.25))
                                .blur_radius(px(50.))
                                .spread_radius(px(-12.)),
                        ])
                        .child(img(image).size_full()),
                )
            })
            .when(self.failed, |d| {
                d.child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(white(0.8))
                        .child("Couldn’t load image"),
                )
            })
            .child(
                div()
                    .id("close-image-preview")
                    .group("image-preview-close")
                    .debug_selector(|| "close-image-preview".into())
                    .role(Role::Button)
                    .aria_label("Close image preview")
                    .tip("Close image preview")
                    .track_focus(&self.focus)
                    .absolute()
                    .top(px(16.))
                    .right(px(16.))
                    .size(px(36.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(white(0.15))
                    .bg(black(0.45))
                    .text_color(white(0.8))
                    .shadow_lg()
                    .cursor_pointer()
                    .hover(|s| s.bg(black(0.65)).text_color(white(1.)))
                    .focus_visible(|s| s.border_2().border_color(white(0.8)))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| this.close(window, cx)))
                    .child(
                        canvas(
                            |_, _, _| {},
                            |bounds, _, window, _| {
                                #[cfg(any(target_os = "macos", target_os = "windows"))]
                                window.paint_quad(
                                    gpui::fill(bounds, gpui::transparent_black())
                                        .corner_radii(px(18.))
                                        .border_color(gpui::backdrop_blur_marker(
                                            12. * window.scale_factor(),
                                        )),
                                );
                                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                                let _ = (bounds, window);
                            },
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        icon("close", 16.)
                            .text_color(white(0.8))
                            .group_hover("image-preview-close", |s| s.text_color(white(1.))),
                    ),
            )
    }
}

pub(crate) fn overlay(view: &Entity<ImageLightbox>) -> impl IntoElement {
    gpui::deferred(
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(view.clone()),
    )
    .with_priority(100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase, VisualTestContext,
        WindowBounds, WindowOptions,
    };

    #[test]
    fn fit_uses_reacts_24px_padding_without_upscaling_small_images() {
        let viewport = size(px(800.), px(600.));
        assert_eq!(
            fitted_size(size(px(100.), px(50.)), viewport),
            size(px(100.), px(50.))
        );
        assert_eq!(
            fitted_size(size(px(1600.), px(1200.)), viewport),
            size(px(736.), px(552.))
        );
    }

    #[test]
    fn zoom_keeps_the_pointer_pixel_fixed_and_clamps_pan_and_scale() {
        let viewport = size(px(800.), px(600.));
        let image = size(px(752.), px(552.));
        let pointer = point(px(500.), px(350.));
        let zoom = View::IDENTITY.zoom_at(2.5, pointer, viewport, image);
        assert_eq!(
            zoom,
            View {
                scale: 2.5,
                x: -150.,
                y: -75.
            }
        );
        assert_eq!((100. - zoom.x) / zoom.scale, 100.);
        assert_eq!((50. - zoom.y) / zoom.scale, 50.);
        assert_eq!(zoom.zoom_at(0.1, pointer, viewport, image), View::IDENTITY);
        assert_eq!(zoom.zoom_at(100., pointer, viewport, image).scale, 8.);
        assert_eq!(
            View {
                scale: 2.,
                x: 10000.,
                y: -10000.
            }
            .clamp(viewport, image),
            View {
                scale: 2.,
                x: 352.,
                y: -252.
            }
        );
        assert_eq!(
            View {
                scale: 8.,
                x: 100.,
                y: -100.
            }
            .clamp(viewport, size(px(20.), px(10.))),
            View {
                scale: 8.,
                x: 0.,
                y: 0.
            }
        );
    }

    #[gpui::test]
    fn lightbox_handles_keyboard_pinch_wheel_drag_double_click_and_resize(cx: &mut TestAppContext) {
        cx.update(bind);
        let window = cx.update(|cx| cx.open_window(WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(0.), px(0.)), size(px(800.), px(600.)),
            ))),
            ..Default::default()
        }, |window, cx| cx.new(|cx| {
            let mut preview = ImageLightbox::new(Open { source: Source::Path("unused.png".into()), alt: "Test image".into() }, window, cx);
            preview.image = Some(Image::from_bytes(ImageFormat::Svg, br##"<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="1200"><rect width="1600" height="1200" fill="#4da3f5"/></svg>"##.to_vec()).to_image_data(cx.svg_renderer()).unwrap());
            preview
        })).unwrap());
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        let modifier = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        visual.simulate_keystrokes(&format!("{modifier}-="));
        window
            .update(visual, |view, _, _| assert_eq!(view.view.scale, 1.25))
            .unwrap();
        visual.simulate_keystrokes(&format!("{modifier}--"));
        window
            .update(visual, |view, _, _| assert_eq!(view.view, View::IDENTITY))
            .unwrap();
        let pointer = point(px(500.), px(350.));
        visual.simulate_mouse_move(pointer, None, Default::default());
        visual.simulate_event(gpui::PinchEvent {
            position: pointer,
            delta: 1.,
            modifiers: Default::default(),
            phase: TouchPhase::Moved,
        });
        window
            .update(visual, |view, _, _| {
                assert_eq!(
                    view.view,
                    View {
                        scale: 2.,
                        x: -100.,
                        y: -50.
                    }
                )
            })
            .unwrap();
        visual.simulate_event(ScrollWheelEvent {
            position: pointer,
            delta: ScrollDelta::Pixels(point(px(20.), px(-30.))),
            modifiers: Default::default(),
            touch_phase: TouchPhase::Moved,
        });
        window
            .update(visual, |view, _, _| {
                assert_eq!(
                    view.view,
                    View {
                        scale: 2.,
                        x: -80.,
                        y: -80.
                    }
                )
            })
            .unwrap();
        visual.simulate_event(ScrollWheelEvent {
            position: pointer,
            delta: ScrollDelta::Lines(point(0., 1.)),
            modifiers: Modifiers {
                control: true,
                ..Default::default()
            },
            touch_phase: TouchPhase::Moved,
        });
        window
            .update(visual, |view, _, _| {
                assert!((view.view.scale - 2. * 0.16_f32.exp()).abs() < 0.0001)
            })
            .unwrap();
        visual.simulate_keystrokes(&format!("{modifier}-0"));
        // Double-click a real image hitbox, then drag without a CSS transition.
        visual.simulate_event(gpui::MouseDownEvent {
            button: MouseButton::Left,
            position: pointer,
            click_count: 2,
            modifiers: Default::default(),
            first_mouse: false,
        });
        visual.simulate_mouse_up(pointer, MouseButton::Left, Default::default());
        window
            .update(visual, |view, _, _| {
                assert_eq!(view.view.scale, 2.5);
                view.transition = None;
            })
            .unwrap();
        visual.simulate_mouse_down(pointer, MouseButton::Left, Default::default());
        visual.simulate_mouse_move(
            point(px(520.), px(360.)),
            Some(MouseButton::Left),
            Default::default(),
        );
        window
            .update(visual, |view, _, _| {
                assert_eq!(
                    view.view,
                    View {
                        scale: 2.5,
                        x: -130.,
                        y: -65.
                    }
                );
                assert!(view.transition.is_none());
            })
            .unwrap();
        visual.simulate_mouse_up(pointer, MouseButton::Left, Default::default());
        visual.simulate_resize(size(px(1000.), px(700.)));
        visual.refresh().unwrap();
        assert_eq!(
            visual.debug_bounds("image-lightbox").unwrap().size,
            size(px(1000.), px(700.))
        );
        visual.simulate_keystrokes(&format!("{modifier}-0"));
        window
            .update(visual, |view, _, _| assert_eq!(view.view, View::IDENTITY))
            .unwrap();
        visual.update(|_, cx| {
            let mut prefs = crate::settings::Preferences::default();
            prefs
                .keybindings
                .insert("View: Zoom In".into(), Some("alt-z".into()));
            crate::settings::apply_keybindings(&prefs, cx);
        });
        visual.simulate_keystrokes(&format!("{modifier}-="));
        window
            .update(visual, |view, _, _| assert_eq!(view.view, View::IDENTITY))
            .unwrap();
        visual.simulate_keystrokes("alt-z");
        window
            .update(visual, |view, _, _| {
                assert_eq!(view.view.scale, KEY_ZOOM_STEP)
            })
            .unwrap();
    }
}
