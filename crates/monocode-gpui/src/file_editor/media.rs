//! BinaryFileView's image canvas and native video player. No file is executed
//! as HTML; playback is paused when its tab leaves the visible workspace.
use super::*;
use crate::popover::{Anchor, AnchorSlot, Menu, MenuAction, MenuEvent, menu_popover};
use gpui::{
    Bounds, ClipboardEntry, ClipboardItem, ExternalPaths, ImageSource, MouseButton, Point,
    RenderImage, ScrollHandle, ScrollWheelEvent, canvas, fill, point, size,
};

#[cfg(target_os = "macos")]
#[path = "video.rs"]
mod video;

pub(super) fn prepare_document(doc: Document) -> Result<Document, String> {
    #[cfg(target_os = "macos")]
    if let Document::Image {
        ref bytes,
        format: "avif",
        original_size,
        ..
    } = doc
    {
        use objc2::AnyThread;
        use objc2_app_kit::NSImage;
        let data = objc2_foundation::NSData::with_bytes(bytes);
        let image =
            NSImage::initWithData(NSImage::alloc(), &data).ok_or("Couldn’t decode this image")?;
        let dimensions = image.size();
        let data = image
            .TIFFRepresentation()
            .ok_or("Couldn’t decode this image")?;
        return Ok(Document::Image {
            bytes: data.to_vec(),
            format: "tiff",
            original_size,
            width: dimensions.width as u32,
            height: dimensions.height as u32,
        });
    }
    Ok(doc)
}

pub(super) enum MediaEvent {
    Retry,
}
pub(super) struct MediaPreview {
    path: String,
    image: Option<Arc<Image>>,
    /// Decoded here rather than by `img(Image)`: GPUI keeps those decodes and
    /// sprites for the life of the app, and closing a tab must give them back.
    decoded: Option<Arc<RenderImage>>,
    width: u32,
    height: u32,
    size: u64,
    format: String,
    zoom: Option<f32>,
    bounds: AnchorSlot,
    scroll: ScrollHandle,
    pointer: Option<Point<gpui::Pixels>>,
    visible: bool,
    copied: bool,
    modified: Option<std::time::SystemTime>,
    duration: Option<f64>,
    failed: bool,
    menu: Option<(Point<gpui::Pixels>, Entity<Menu>, Subscription)>,
    #[cfg(target_os = "macos")]
    video: Option<video::Video>,
    _poll: Option<Task<()>>,
    _source: Option<Arc<document::TemporaryMedia>>,
}
impl EventEmitter<MediaEvent> for MediaPreview {}
fn file_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.;
    let mut unit = 0;
    let units = ["KB", "MB", "GB"];
    while value >= 1024. && unit < 2 {
        value /= 1024.;
        unit += 1;
    }
    if value < 10. {
        format!("{value:.1} {}", units[unit])
    } else {
        format!("{value:.0} {}", units[unit])
    }
}
impl MediaPreview {
    pub(super) fn image(
        path: String,
        image: Arc<Image>,
        width: u32,
        height: u32,
        bytes: usize,
        format: &str,
        cx: &mut Context<Self>,
    ) -> Self {
        let renderer = cx.svg_renderer();
        let encoded = image.clone();
        let work = cx
            .background_executor()
            .spawn(async move { encoded.to_image_data(renderer).ok() });
        cx.spawn(async move |this, cx| {
            let decoded = work.await;
            let _ = this.update(cx, |this, cx| {
                this.decoded = decoded;
                cx.notify();
            });
        })
        .detach();
        cx.on_release(|this, cx| {
            if let Some(decoded) = this.decoded.take() {
                cx.drop_image(decoded, None);
            }
        })
        .detach();
        Self {
            path,
            image: Some(image),
            decoded: None,
            width,
            height,
            size: bytes as u64,
            format: format.into(),
            zoom: None,
            bounds: Default::default(),
            scroll: Default::default(),
            pointer: None,
            visible: true,
            copied: false,
            modified: None,
            duration: None,
            failed: false,
            menu: None,
            #[cfg(target_os = "macos")]
            video: None,
            _poll: None,
            _source: None,
        }
    }
    pub(super) fn video(
        path: String,
        bytes: u64,
        modified: Option<std::time::SystemTime>,
        source: Option<Arc<document::TemporaryMedia>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Self {
            format: PathBuf::from(&path)
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            path: path.clone(),
            image: None,
            decoded: None,
            width: 0,
            height: 0,
            size: bytes,
            zoom: None,
            bounds: Default::default(),
            scroll: Default::default(),
            pointer: None,
            visible: true,
            copied: false,
            modified,
            duration: None,
            failed: false,
            menu: None,
            #[cfg(target_os = "macos")]
            video: Some(video::Video::new(
                source
                    .as_ref()
                    .map(|source| source.path().to_string_lossy().into_owned())
                    .unwrap_or(path),
            )),
            _poll: None,
            _source: source,
        };
        this._poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        #[cfg(target_os = "macos")]
                        if this.visible
                            && let Some(video) = &this.video
                            && let Some((width, height, duration, failed)) = video.metadata()
                            && (this.width != width
                                || this.height != height
                                || this.duration != Some(duration)
                                || this.failed != failed)
                        {
                            this.width = width;
                            this.height = height;
                            this.duration = Some(duration);
                            this.failed = failed;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
        this
    }
    pub(super) fn same_video(&self, size: u64, modified: Option<std::time::SystemTime>) -> bool {
        self.image.is_none() && self.size == size && self.modified == modified
    }
    pub(super) fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        #[cfg(target_os = "macos")]
        if !visible && let Some(video) = &self.video {
            video.hide();
        }
    }
    fn scale(&self) -> f32 {
        self.zoom.unwrap_or_else(|| {
            self.bounds.get().map_or(1., |b| {
                ((b.size.width.as_f32() - 32.) / self.width.max(1) as f32)
                    .min((b.size.height.as_f32() - 32.) / self.height.max(1) as f32)
                    .clamp(0.001, 1.)
            })
        })
    }
    fn zoom_at(
        &mut self,
        value: Option<f32>,
        anchor: Option<Point<gpui::Pixels>>,
        cx: &mut Context<Self>,
    ) {
        let old_scale = self.scale();
        let old_offset = self.scroll.offset();
        let previous = self.zoom;
        self.zoom = value.map(|v| v.clamp(0.1, 16.));
        if let Some(bounds) = self.bounds.get() {
            let pointer = anchor.unwrap_or(bounds.center());
            let position = |scale: f32| {
                point(
                    bounds.left()
                        + px(16.
                            + ((bounds.size.width.as_f32() - 32. - self.width as f32 * scale)
                                / 2.)
                                .max(0.)),
                    bounds.top()
                        + px(16.
                            + ((bounds.size.height.as_f32() - 32. - self.height as f32 * scale)
                                / 2.)
                                .max(0.)),
                )
            };
            let local = pointer - position(old_scale) - old_offset;
            let new_scale = self.scale();
            let offset = pointer - position(new_scale) - local * (new_scale / old_scale);
            self.scroll.set_offset(if value.is_none() {
                point(px(0.), px(0.))
            } else {
                point(offset.x.min(px(0.)), offset.y.min(px(0.)))
            });
        }
        if previous != self.zoom {
            cx.notify();
        }
    }
    fn copy_original(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem {
            entries: vec![ClipboardEntry::ExternalPaths(ExternalPaths(
                vec![PathBuf::from(&self.path)].into(),
            ))],
        });
        self.menu = None;
        self.copied = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.copied = false;
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn image_menu(&mut self, at: Point<gpui::Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let menu = cx.new(|cx| {
            Menu::new(
                "Image actions",
                vec![MenuAction::new("copy", "Copy Original File").into()],
                window,
                cx,
            )
        });
        let sub = cx.subscribe(&menu, |this, _, event: &MenuEvent, cx| {
            if matches!(event, MenuEvent::Picked(..)) {
                this.copy_original(cx);
            }
            this.menu = None;
            cx.notify();
        });
        self.menu = Some((at, menu, sub));
    }
    pub(super) fn zoom_key(&mut self, e: &gpui::KeyDownEvent, cx: &mut Context<Self>) {
        if self.image.is_none()
            || (!e.keystroke.modifiers.platform && !e.keystroke.modifiers.control)
        {
            return;
        }
        match e.keystroke.key.as_str() {
            "+" | "=" => self.zoom_at(Some(self.scale() * 1.5), self.pointer, cx),
            "-" => self.zoom_at(Some(self.scale() / 1.5), self.pointer, cx),
            "0" => self.zoom_at(None, None, cx),
            _ => return,
        }
        cx.stop_propagation();
    }
    fn wheel(&mut self, e: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        if e.modifiers.control || e.modifiers.platform {
            let delta = e.delta.pixel_delta(px(16.)).y.as_f32();
            self.zoom_at(
                Some(self.scale() * (-delta * 0.01).exp()),
                Some(e.position),
                cx,
            );
            cx.stop_propagation();
        } else {
            cx.propagate();
        }
    }
}
impl gpui::Render for MediaPreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_image = self.image.is_some();
        #[cfg(target_os = "macos")]
        if self.failed
            && let Some(video) = &self.video
        {
            video.hide();
        }
        #[cfg(target_os = "macos")]
        if !window.is_window_active()
            && let Some(video) = &self.video
        {
            video.pause();
        }
        let body = if self.image.is_some() {
            let source = match &self.decoded {
                Some(decoded) => ImageSource::Render(decoded.clone()),
                None => ImageSource::Custom(Arc::new(|_, _| None)),
            };
            let bounds = self.bounds.clone();
            let viewport_scroll = self.scroll.clone();
            let entity = cx.entity().downgrade();
            let scale = self.scale();
            let view_size = self
                .bounds
                .get()
                .map(|b| b.size)
                .unwrap_or(size(px(400.), px(300.)));
            div()
                .id("media-image-viewport")
                .relative()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .overflow_scroll()
                .track_scroll(&self.scroll)
                .on_scroll_wheel(cx.listener(Self::wheel))
                .on_pinch(cx.listener(|this, e: &gpui::PinchEvent, _, cx| {
                    this.zoom_at(Some(this.scale() * (1. + e.delta)), Some(e.position), cx);
                    cx.stop_propagation();
                }))
                .on_mouse_move(cx.listener(|this, e: &gpui::MouseMoveEvent, _, _| {
                    this.pointer = Some(e.position)
                }))
                .on_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, _, cx| {
                    if !e.keystroke.modifiers.platform && !e.keystroke.modifiers.control {
                        return;
                    }
                    match e.keystroke.key.as_str() {
                        "+" | "=" => this.zoom_at(Some(this.scale() * 1.5), this.pointer, cx),
                        "-" => this.zoom_at(Some(this.scale() / 1.5), this.pointer, cx),
                        "0" => this.zoom_at(None, None, cx),
                        _ => return,
                    }
                    cx.stop_propagation();
                }))
                .child(
                    canvas(
                        |_, _, _| {},
                        move |_, _, w, cx| {
                            let b = viewport_scroll.bounds();
                            if bounds.get() != Some(b) {
                                bounds.set(Some(b));
                                let _ = entity.update(cx, |_, cx| cx.notify());
                            }
                            let tile = 8.;
                            for row in 0..(b.size.height.as_f32() / tile).ceil() as usize {
                                for col in 0..(b.size.width.as_f32() / tile).ceil() as usize {
                                    if (row + col) % 2 == 0 {
                                        w.paint_quad(fill(
                                            Bounds::new(
                                                b.origin
                                                    + point(
                                                        px(col as f32 * tile),
                                                        px(row as f32 * tile),
                                                    ),
                                                size(px(tile), px(tile)),
                                            ),
                                            gpui::rgba(0x8080801a),
                                        ));
                                    }
                                }
                            }
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .child(
                    div()
                        .p_4()
                        .w(px(
                            (self.width as f32 * scale + 32.).max(view_size.width.as_f32())
                        ))
                        .h(px(
                            (self.height as f32 * scale + 32.).max(view_size.height.as_f32())
                        ))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            img(source)
                                .w(px(self.width.max(1) as f32 * scale))
                                .h(px(self.height.max(1) as f32 * scale))
                                .object_fit(ObjectFit::Contain)
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, e: &gpui::MouseDownEvent, _, cx| {
                                        this.zoom_at(
                                            if this.zoom.is_none() { Some(1.) } else { None },
                                            Some(e.position),
                                            cx,
                                        );
                                    }),
                                )
                                .on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(|this, e: &gpui::MouseDownEvent, w, cx| {
                                        this.image_menu(e.position, w, cx)
                                    }),
                                ),
                        ),
                )
                .into_any_element()
        } else if self.failed {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .p_6()
                .text_size(px(12.))
                .text_color(content(0.5))
                .child("Couldn’t play this video")
                .child("This video’s format or codec is unsupported, or the file is damaged.")
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .id("retry-video")
                                .role(Role::Button)
                                .aria_label("Retry")
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(content(0.1))
                                .child("Retry")
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(MediaEvent::Retry))),
                        )
                        .child(
                            div()
                                .id("open-video-externally")
                                .role(Role::Button)
                                .aria_label("Open externally")
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(content(0.1))
                                .child("Open externally")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.open_url(&format!(
                                        "file://{}",
                                        this.path
                                            .replace('%', "%25")
                                            .replace(' ', "%20")
                                            .replace('#', "%23")
                                            .replace('?', "%3F")
                                    ))
                                })),
                        )
                        .child(
                            div()
                                .id("reveal-video")
                                .role(Role::Button)
                                .aria_label("Reveal")
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(content(0.1))
                                .child("Reveal")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let path = PathBuf::from(&this.path);
                                    cx.background_executor()
                                        .spawn(async move {
                                            let _ = monocode_core::files::reveal_path(&path);
                                        })
                                        .detach();
                                })),
                        )
                        .child(
                            div()
                                .id("copy-video-path")
                                .role(Role::Button)
                                .aria_label("Copy path")
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(content(0.1))
                                .child("Copy path")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        this.path.clone(),
                                    ))
                                })),
                        ),
                )
                .into_any_element()
        } else {
            #[cfg(target_os = "macos")]
            {
                self.video
                    .as_ref()
                    .map(|v| v.element())
                    .unwrap_or_else(|| div().into_any_element())
            }
            #[cfg(not(target_os = "macos"))]
            {
                div()
                    .flex_1()
                    .child("Video playback is unavailable on this platform")
                    .into_any_element()
            }
        };
        let footer = div()
            .h(px(32.))
            .px_3()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_3()
            .border_t_1()
            .border_color(stroke())
            .text_size(px(11.))
            .text_color(content(0.5))
            .child(if self.width > 0 {
                format!("{} × {}", self.width, self.height)
            } else {
                "—".into()
            })
            .when_some(
                self.duration.filter(|d| d.is_finite() && *d >= 0.),
                |d, duration| {
                    d.child(format!(
                        "{}:{:02}",
                        duration as u64 / 60,
                        duration as u64 % 60
                    ))
                },
            )
            .child(file_size(self.size))
            .child(self.format.to_uppercase())
            .child(div().flex_1())
            .when(is_image, |d| {
                d.when(cfg!(target_os = "macos"), |d| {
                    d.child(
                        icon_action(
                            "copy-original-image",
                            if self.copied {
                                "Copied"
                            } else {
                                "Copy original file"
                            },
                            if self.copied { "check" } else { "copy" },
                            true,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.copy_original(cx))),
                    )
                })
                .child(
                    icon_action("image-zoom-out", "Zoom out", "minus", true).on_click(cx.listener(
                        |this, _, _, cx| this.zoom_at(Some(this.scale() / 1.5), None, cx),
                    )),
                )
                .child(
                    div()
                        .id("image-fit")
                        .role(Role::Button)
                        .aria_label("Fit to window")
                        .w(px(44.))
                        .text_center()
                        .child(
                            self.zoom
                                .map_or("Fit".into(), |z| format!("{:.0}%", z * 100.)),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.zoom_at(None, None, cx))),
                )
                .child(
                    icon_action("image-zoom-in", "Zoom in", "plus", true).on_click(cx.listener(
                        |this, _, _, cx| this.zoom_at(Some(this.scale() * 1.5), None, cx),
                    )),
                )
            });
        div()
            .relative()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(body)
            .child(footer)
            .children(
                self.menu
                    .as_ref()
                    .map(|(at, menu, _)| menu_popover(menu, Anchor::Point(*at), cx).gap(0.)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, WindowBounds, WindowOptions};
    #[gpui::test]
    fn image_zoom_clamps_preserves_the_pointer_and_fits_after_panning(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(400.), px(300.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|cx| {
                        MediaPreview::image(
                            "/tmp/image.png".into(),
                            Arc::new(Image::from_bytes(
                                ImageFormat::Png,
                                include_bytes!("../../assets/monocode.png").to_vec(),
                            )),
                            800,
                            400,
                            1024,
                            "png",
                            cx,
                        )
                    })
                },
            )
            .unwrap()
        });
        cx.run_until_parked();
        window
            .update(cx, |m, _, cx| {
                let bounds = m.bounds.get().unwrap();
                let old_scale = m.scale();
                let anchor = bounds.center();
                m.zoom_at(Some(old_scale * 2.), Some(anchor), cx);
                assert!(m.scroll.offset().x < px(0.));
                m.zoom_at(Some(100.), None, cx);
                assert_eq!(m.zoom, Some(16.));
                m.zoom_at(Some(0.001), None, cx);
                assert_eq!(m.zoom, Some(0.1));
                m.zoom_at(None, None, cx);
                assert_eq!(m.zoom, None);
                assert_eq!(m.scroll.offset(), point(px(0.), px(0.)));
                assert!((m.scale() - old_scale).abs() < 0.001);
            })
            .unwrap();
        assert_eq!(file_size(1024), "1.0 KB");
        assert_eq!(file_size(24 * 1024), "24 KB");
    }
}
