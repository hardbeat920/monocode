//! Port of src/features/inbox/ui/InboxMedia.tsx: a remote image or video
//! from an issue body, fetched with the provider's credentials. Images show
//! inline. Videos use the native player's playback controls and show their
//! source link when the platform cannot play the format.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

#[cfg(not(target_os = "macos"))]
use gpui::prelude::FluentBuilder as _;
#[cfg(not(target_os = "macos"))]
use gpui::{AppContext as _, Entity, Subscription};
use gpui::{
    Context, Image, ImageFormat, InteractiveElement as _, IntoElement, ObjectFit,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, StyledImage as _,
    Task, Window, canvas, div, img,
};
use gpui_component::WindowExt as _;
#[cfg(not(target_os = "macos"))]
use gpui_component::slider::{Slider, SliderEvent, SliderState};
#[cfg(not(target_os = "macos"))]
use monocode_ui::widgets::button;
use monocode_ui::widgets::tooltip;
use monocode_ui::{Theme, u};
#[cfg(not(target_os = "macos"))]
use std::cell::Cell;
#[cfg(target_os = "linux")]
use std::cell::RefCell;

use crate::data::{InboxMediaKind, InboxServices};
use crate::style::palette;
use monocode_platform::video::{NativeVideo, VideoFile, VideoRect};

/// Converts AVIF to PNG bytes that GPUI can decode.
pub fn decode_avif_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use image::ImageEncoder as _;
    let pixels = monocode_editor::image_view::decode_avif_rgba(bytes)?;
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            pixels.as_raw(),
            pixels.width(),
            pixels.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| error.to_string())?;
    Ok(png)
}

enum LoadState {
    Loading,
    Ready(Arc<Image>),
    Video(Rc<NativeVideo>),
    Error(Option<String>),
}

/// The image format for a sniffed mime type.
pub fn image_format(mime: &str) -> Option<ImageFormat> {
    Some(match mime {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::Webp,
        "image/svg+xml" => ImageFormat::Svg,
        "image/bmp" => ImageFormat::Bmp,
        "image/tiff" => ImageFormat::Tiff,
        _ => return None,
    })
}

pub struct InboxMediaView {
    services: Rc<dyn InboxServices>,
    src: String,
    alt: String,
    state: LoadState,
    natural_size: Option<(u32, u32)>,
    _load: Task<()>,
    _visibility: Task<()>,
    #[cfg(not(target_os = "macos"))]
    seek: Entity<SliderState>,
    #[cfg(not(target_os = "macos"))]
    _seek: Option<Subscription>,
    #[cfg(not(target_os = "macos"))]
    volume: Entity<SliderState>,
    #[cfg(not(target_os = "macos"))]
    _volume: Option<Subscription>,
    #[cfg(not(target_os = "macos"))]
    fullscreen: Rc<Cell<bool>>,
    #[cfg(not(target_os = "macos"))]
    fullscreen_view: bool,
    #[cfg(target_os = "linux")]
    video_frame: Rc<RefCell<Option<Arc<gpui::RenderImage>>>>,
}

impl InboxMediaView {
    pub fn new(
        services: Rc<dyn InboxServices>,
        src: String,
        alt: String,
        cx: &mut Context<Self>,
    ) -> Self {
        #[cfg(not(target_os = "macos"))]
        let seek = cx.new(|_| SliderState::new().min(0.).max(1000.).step(1.));
        #[cfg(not(target_os = "macos"))]
        let seek_subscription = cx.subscribe(&seek, |this, _, event, cx| {
            if let SliderEvent::Change(value) = event
                && let LoadState::Video(video) = &this.state
            {
                video.seek(f64::from(value.end()) / 1000. * video.duration());
                cx.notify();
            }
        });
        #[cfg(not(target_os = "macos"))]
        let volume = cx.new(|_| {
            SliderState::new()
                .min(0.)
                .max(100.)
                .step(1.)
                .default_value(100.)
        });
        #[cfg(not(target_os = "macos"))]
        let volume_subscription = cx.subscribe(&volume, |this, _, event, cx| {
            if let SliderEvent::Change(value) = event
                && let LoadState::Video(video) = &this.state
            {
                video.set_volume(f64::from(value.end()) / 100.);
                if value.end() > 0. {
                    video.set_muted(false);
                }
                cx.notify();
            }
        });
        let task = services.fetch_media(&src, cx);
        let load = cx.spawn(async move |this, cx| {
            let result = task.await;
            let avif = if let Ok(media) = &result {
                if media.kind == InboxMediaKind::Image && media.mime == "image/avif" {
                    let bytes = media.bytes.clone();
                    Some(
                        cx.background_executor()
                            .spawn(async move { decode_avif_png(&bytes) })
                            .await,
                    )
                } else {
                    None
                }
            } else {
                None
            };
            let staged = if let Ok(media) = &result {
                if media.kind == InboxMediaKind::Video {
                    let bytes = media.bytes.clone();
                    let mime = media.mime.clone();
                    Some(
                        cx.background_executor()
                            .spawn(async move { VideoFile::new(&bytes, &mime) })
                            .await,
                    )
                } else {
                    None
                }
            } else {
                None
            };
            let _ = this.update(cx, |this, cx| {
                this.state = match result {
                    Ok(media) if media.kind == InboxMediaKind::Image => {
                        if let Some(avif) = avif {
                            match avif {
                                Ok(png) => LoadState::Ready(Arc::new(Image::from_bytes(
                                    ImageFormat::Png,
                                    png,
                                ))),
                                Err(error) => LoadState::Error(Some(error)),
                            }
                        } else {
                            match image_format(&media.mime) {
                                Some(format) => LoadState::Ready(Arc::new(Image::from_bytes(
                                    format,
                                    media.bytes.as_ref().clone(),
                                ))),
                                None => LoadState::Error(Some(format!(
                                    "Unsupported image type: {}",
                                    media.mime
                                ))),
                            }
                        }
                    }
                    Ok(_) => match staged
                        .expect("a video was staged")
                        .and_then(NativeVideo::new)
                    {
                        Ok(video) => LoadState::Video(Rc::new(video)),
                        Err(error) => LoadState::Error(Some(error)),
                    },
                    Err(error) => LoadState::Error(Some(error)),
                };
                cx.notify();
            });
        });
        let visibility = Self::watch_visibility(cx);
        Self {
            services,
            src,
            alt,
            state: LoadState::Loading,
            natural_size: None,
            _load: load,
            _visibility: visibility,
            #[cfg(not(target_os = "macos"))]
            seek,
            #[cfg(not(target_os = "macos"))]
            _seek: Some(seek_subscription),
            #[cfg(not(target_os = "macos"))]
            volume,
            #[cfg(not(target_os = "macos"))]
            _volume: Some(volume_subscription),
            #[cfg(not(target_os = "macos"))]
            fullscreen: Rc::new(Cell::new(false)),
            #[cfg(not(target_os = "macos"))]
            fullscreen_view: false,
            #[cfg(target_os = "linux")]
            video_frame: Rc::new(RefCell::new(None)),
        }
    }

    fn watch_visibility(cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if !this
                    .update(cx, |this, cx| match &this.state {
                        LoadState::Loading => true,
                        LoadState::Video(video) => {
                            video.suspend_if_idle(Duration::from_millis(250));
                            let size = video.natural_size();
                            if size != this.natural_size {
                                this.natural_size = size;
                                cx.notify();
                            }
                            #[cfg(not(target_os = "macos"))]
                            cx.notify();
                            true
                        }
                        _ => false,
                    })
                    .unwrap_or(false)
                {
                    break;
                }
            }
        })
    }

    #[cfg(not(target_os = "macos"))]
    fn open_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.fullscreen_view {
            if let LoadState::Video(video) = &self.state {
                video.detach();
            }
            window.remove_window();
            return;
        }
        let LoadState::Video(video) = &self.state else {
            return;
        };
        if self.fullscreen.replace(true) {
            return;
        }
        let video = video.clone();
        video.detach();
        #[cfg(target_os = "linux")]
        if let Some(image) = self.video_frame.borrow().as_ref() {
            let _ = window.drop_image(image.clone());
        }
        let origin = cx.entity();
        let fullscreen = self.fullscreen.clone();
        let result = cx.open_window(
            gpui::WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Fullscreen(window.bounds())),
                titlebar: None,
                window_decorations: Some(gpui::WindowDecorations::Client),
                ..Default::default()
            },
            |window, cx| {
                let media = cx.new(|cx| Self {
                    services: self.services.clone(),
                    src: self.src.clone(),
                    alt: self.alt.clone(),
                    state: LoadState::Video(video.clone()),
                    natural_size: video.natural_size(),
                    _load: Task::ready(()),
                    _visibility: Self::watch_visibility(cx),
                    seek: self.seek.clone(),
                    _seek: None,
                    volume: self.volume.clone(),
                    _volume: None,
                    fullscreen: fullscreen.clone(),
                    fullscreen_view: true,
                    #[cfg(target_os = "linux")]
                    video_frame: self.video_frame.clone(),
                });
                let player = cx.new(|cx| {
                    cx.on_release(|this: &mut FullscreenPlayer, cx| {
                        this.video.detach();
                        this.fullscreen.set(false);
                        this.origin.update(cx, |_, cx| cx.notify());
                    })
                    .detach();
                    FullscreenPlayer {
                        media,
                        origin,
                        video: video.clone(),
                        fullscreen,
                        focus: {
                            let focus = cx.focus_handle();
                            window.focus(&focus, cx);
                            focus
                        },
                    }
                });
                let close = video.clone();
                window.on_window_should_close(cx, move |_, _| {
                    close.detach();
                    true
                });
                cx.new(|cx| gpui_component::Root::new(player, window, cx))
            },
        );
        if let Err(error) = result {
            self.fullscreen.set(false);
            monocode_ui::widgets::Toasts::push_timed(
                monocode_ui::widgets::Toast::new(format!(
                    "Could not open fullscreen video: {error}"
                ))
                .kind(monocode_ui::widgets::ToastKind::Error),
                Duration::from_secs(5),
                cx,
            );
        }
        cx.notify();
    }
}

#[cfg(not(target_os = "macos"))]
struct FullscreenPlayer {
    media: Entity<InboxMediaView>,
    origin: Entity<InboxMediaView>,
    video: Rc<NativeVideo>,
    fullscreen: Rc<Cell<bool>>,
    focus: gpui::FocusHandle,
}
#[cfg(not(target_os = "macos"))]
impl Render for FullscreenPlayer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let close = self.video.clone();
        div()
            .track_focus(&self.focus)
            .key_context("FullscreenVideo")
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.colors.root_background)
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                let key = &event.keystroke;
                if key.key == "escape"
                    || (event.keystroke.key == "w"
                        && (event.keystroke.modifiers.platform
                            || event.keystroke.modifiers.control))
                {
                    this.video.detach();
                    window.remove_window();
                    cx.stop_propagation();
                } else if !key.modifiers.platform && !key.modifiers.control && !key.modifiers.alt {
                    let handled = match key.key.as_str() {
                        "space" => {
                            if this.video.is_playing() {
                                this.video.pause();
                            } else {
                                this.video.play();
                            }
                            true
                        }
                        "left" => {
                            this.video.seek((this.video.position() - 5.).max(0.));
                            true
                        }
                        "right" => {
                            this.video
                                .seek((this.video.position() + 5.).min(this.video.duration()));
                            true
                        }
                        "home" => {
                            this.video.seek(0.);
                            true
                        }
                        "end" => {
                            this.video.seek(this.video.duration());
                            true
                        }
                        "m" => {
                            this.video.set_muted(!this.video.is_muted());
                            true
                        }
                        "up" => {
                            this.video.set_volume(this.video.volume() + 0.05);
                            true
                        }
                        "down" => {
                            this.video.set_volume(this.video.volume() - 0.05);
                            true
                        }
                        _ => false,
                    };
                    if handled {
                        cx.stop_propagation();
                        cx.notify();
                    }
                }
            }))
            .child(
                div().flex().justify_end().p(u(8.)).child(
                    button("close-fullscreen-video", "Exit fullscreen")
                        .compact()
                        .on_click(move |_, window, _| {
                            close.detach();
                            window.remove_window();
                        }),
                ),
            )
            .child(div().flex_1().min_h_0().child(self.media.clone()))
    }
}

impl Render for InboxMediaView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(target_os = "macos")]
        let _ = window;
        let theme = Theme::of(cx).clone();
        let src = self.src.clone();
        let services = self.services.clone();
        match &self.state {
            LoadState::Loading => div()
                .my(u(8.))
                .h(u(128.))
                .w_full()
                .max_w(u(576.))
                .rounded(u(10.))
                .border_1()
                .border_color(theme.content(0.10))
                .bg(theme.content(0.06))
                .into_any_element(),
            LoadState::Ready(image) => {
                let label = if self.alt.trim().is_empty() {
                    "Image".to_string()
                } else {
                    self.alt.trim().to_string()
                };
                div()
                    .id("inbox-media")
                    .my(u(8.))
                    .w_full()
                    .max_w(u(576.))
                    .overflow_hidden()
                    .rounded(u(10.))
                    .border_1()
                    .border_color(theme.content(0.10))
                    .bg(theme.content(0.06))
                    .cursor(gpui::CursorStyle::PointingHand)
                    .tooltip(tooltip(label))
                    .on_click(move |_, _, cx| services.open_url(&src, cx))
                    .child(
                        img(image.clone())
                            .w_full()
                            .max_h(u(448.))
                            .object_fit(ObjectFit::Contain),
                    )
                    .into_any_element()
            }
            LoadState::Video(video) => {
                #[cfg(not(target_os = "macos"))]
                if self.fullscreen.get() && !self.fullscreen_view {
                    return div()
                        .my(u(8.))
                        .text_color(theme.content(0.60))
                        .child("Playing in fullscreen")
                        .into_any_element();
                }
                let video = video.clone();
                let aspect_ratio = self
                    .natural_size
                    .map_or(16. / 9., |(width, height)| width as f32 / height as f32);
                let weak = cx.entity().downgrade();
                #[cfg(target_os = "linux")]
                let frame_store = self.video_frame.clone();
                #[cfg(not(target_os = "macos"))]
                let controls = {
                    let duration = video.duration();
                    let position = video.position().min(duration.max(0.));
                    let value = if duration > 0. {
                        (position / duration * 1000.) as f32
                    } else {
                        0.
                    };
                    if (self.seek.read(cx).value().end() - value).abs() > 0.5 {
                        self.seek
                            .update(cx, |seek, cx| seek.set_value(value, window, cx));
                    }
                    let play = video.clone();
                    let back = video.clone();
                    let forward = video.clone();
                    div()
                        .flex()
                        .items_center()
                        .gap(u(10.))
                        .py(u(4.))
                        .child(
                            button(
                                "inbox-video-play",
                                if video.is_playing() { "Pause" } else { "Play" },
                            )
                            .ghost()
                            .compact()
                            .tooltip(if video.is_playing() {
                                "Pause video"
                            } else {
                                "Play video"
                            })
                            .on_click(cx.listener(
                                move |_, _, _, cx| {
                                    if play.is_playing() {
                                        play.pause();
                                    } else {
                                        play.play();
                                    }
                                    cx.notify();
                                },
                            )),
                        )
                        .child(
                            button("inbox-video-back", "-10s")
                                .ghost()
                                .compact()
                                .tooltip("Back 10 seconds")
                                .disabled(duration <= 0.)
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    back.seek((back.position() - 10.).max(0.));
                                    cx.notify();
                                })),
                        )
                        .child(
                            Slider::new(&self.seek)
                                .flex_1()
                                .min_w_0()
                                .disabled(duration <= 0.),
                        )
                        .child(div().text_size(u(11.)).child(format!(
                            "{} / {}",
                            video_time(position),
                            video_time(duration)
                        )))
                        .child(
                            button("inbox-video-forward", "+10s")
                                .ghost()
                                .compact()
                                .tooltip("Forward 10 seconds")
                                .disabled(duration <= 0.)
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    forward
                                        .seek((forward.position() + 10.).min(forward.duration()));
                                    cx.notify();
                                })),
                        )
                };
                #[cfg(not(target_os = "macos"))]
                let volume_controls = {
                    let value = (video.volume().clamp(0., 1.) * 100.) as f32;
                    if (self.volume.read(cx).value().end() - value).abs() > 0.5 {
                        self.volume
                            .update(cx, |volume, cx| volume.set_value(value, window, cx));
                    }
                    let mute = video.clone();
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap(u(10.))
                        .child(
                            button(
                                "inbox-video-mute",
                                if video.is_muted() { "Unmute" } else { "Mute" },
                            )
                            .ghost()
                            .compact()
                            .on_click(cx.listener(
                                move |_, _, _, cx| {
                                    mute.set_muted(!mute.is_muted());
                                    cx.notify();
                                },
                            )),
                        )
                        .child(
                            div()
                                .text_size(u(11.))
                                .text_color(theme.content(0.60))
                                .child("Volume"),
                        )
                        .child(Slider::new(&self.volume).w(u(96.)))
                        .child(
                            button(
                                "inbox-video-fullscreen",
                                if self.fullscreen_view {
                                    "Exit fullscreen"
                                } else {
                                    "Fullscreen"
                                },
                            )
                            .ghost()
                            .compact()
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_fullscreen(window, cx)),
                            ),
                        )
                };
                let preview = div()
                    .my(u(8.))
                    .w_full()
                    .max_w(u(576.))
                    .flex()
                    .flex_col()
                    .gap(u(4.));
                #[cfg(not(target_os = "macos"))]
                let preview = preview.when(self.fullscreen_view, |el| {
                    el.size_full().max_w_full().my_0()
                });
                let picture = canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        window.paint_quad(gpui::fill(bounds, gpui::rgb(0)));
                        if cx.has_active_drag()
                            || window.has_active_dialog(cx)
                            || window.has_active_sheet(cx)
                        {
                            video.hide();
                            return;
                        }
                        let clip = window.content_mask().bounds;
                        let rect = |b: gpui::Bounds<gpui::Pixels>| VideoRect {
                            x: f32::from(b.origin.x) as f64,
                            y: f32::from(b.origin.y) as f64,
                            width: f32::from(b.size.width) as f64,
                            height: f32::from(b.size.height) as f64,
                        };
                        let visible = rect(bounds).intersection(rect(clip));
                        if visible.width <= 0. || visible.height <= 0. {
                            video.hide();
                            return;
                        }
                        let error = video
                            .place(window, rect(bounds), rect(clip))
                            .err()
                            .or_else(|| video.failure());
                        if let Some(error) = error {
                            let weak = weak.clone();
                            cx.defer(move |cx| {
                                weak.update(cx, |view, cx| {
                                    view.state = LoadState::Error(Some(error));
                                    cx.notify();
                                })
                                .ok();
                            });
                        }
                        #[cfg(target_os = "linux")]
                        {
                            if let Some(frame) = video.new_frame()
                                && let Some(buffer) = image::RgbaImage::from_raw(
                                    frame.width,
                                    frame.height,
                                    frame.bgra,
                                )
                            {
                                let image =
                                    Arc::new(gpui::RenderImage::new(vec![image::Frame::new(
                                        buffer,
                                    )]));
                                if let Some(old) = frame_store.borrow_mut().replace(image) {
                                    let _ = window.drop_image(old);
                                }
                            }
                            if let Some(image) = frame_store.borrow().as_ref() {
                                let size = image.size(0);
                                let width = size.width.0 as f32;
                                let height = size.height.0 as f32;
                                let scale = (f32::from(bounds.size.width) / width)
                                    .min(f32::from(bounds.size.height) / height);
                                let fitted =
                                    gpui::size(gpui::px(width * scale), gpui::px(height * scale));
                                let origin = gpui::point(
                                    bounds.origin.x + (bounds.size.width - fitted.width) / 2.,
                                    bounds.origin.y + (bounds.size.height - fitted.height) / 2.,
                                );
                                let _ = window.paint_image(
                                    gpui::Bounds::new(origin, fitted),
                                    Default::default(),
                                    image.clone(),
                                    0,
                                    false,
                                );
                            }
                        }
                        // Hiding before the next paint removes a cached player's
                        // native view as soon as this page leaves the window.
                        let video = video.clone();
                        window.on_next_frame(move |_, _| video.hide());
                        window.request_animation_frame();
                    },
                )
                .w_full();
                #[cfg(target_os = "macos")]
                let picture = picture.aspect_ratio(aspect_ratio).max_h(u(448.));
                #[cfg(not(target_os = "macos"))]
                let picture = picture
                    .when(!self.fullscreen_view, |el| {
                        el.aspect_ratio(aspect_ratio).max_h(u(448.))
                    })
                    .when(self.fullscreen_view, |el| el.h_auto().flex_1().min_h_0());
                let preview = preview.child(picture);
                #[cfg(not(target_os = "macos"))]
                let preview = preview.child(controls).child(volume_controls);
                preview
                    .child(
                        div()
                            .id("inbox-video-source")
                            .text_size(u(11.))
                            .text_color(palette::sky_400())
                            .cursor_pointer()
                            .child("Open video source")
                            .on_click(move |_, _, cx| services.open_url(&src, cx)),
                    )
                    .into_any_element()
            }
            LoadState::Error(error) => {
                let label = if self.alt.trim().is_empty() {
                    self.src.clone()
                } else {
                    self.alt.trim().to_string()
                };
                let hover = palette::sky_400();
                div()
                    .id("inbox-media-link")
                    .text_color(monocode_ui::color::with_alpha(palette::sky_400(), 0.9))
                    .tooltip(tooltip(
                        error.clone().unwrap_or_else(|| "Open media source".into()),
                    ))
                    .hover(move |s| s.text_color(hover).underline())
                    .on_click(move |_, _, cx| services.open_url(&src, cx))
                    .child(label)
                    .into_any_element()
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn video_time(seconds: f64) -> String {
    let seconds = if seconds.is_finite() {
        seconds.max(0.) as u64
    } else {
        0
    };
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod avif_tests {
    use super::decode_avif_png;

    #[test]
    fn authenticated_avif_keeps_red_and_blue_channels_in_the_png_preview() {
        let encoded = decode_avif_png(include_bytes!(
            "../../../editor/tests/fixtures/red-blue.avif"
        ))
        .unwrap();
        let pixels = image::load_from_memory(&encoded).unwrap().to_rgba8();
        assert_eq!(pixels.dimensions(), (32, 16));
        let red = pixels.get_pixel(4, 8).0;
        let blue = pixels.get_pixel(24, 8).0;
        assert!(red[0] > 240 && red[2] < 10 && red[3] == 255, "{red:?}");
        assert!(blue[2] > 240 && blue[0] < 10 && blue[3] == 255, "{blue:?}");
    }
}
