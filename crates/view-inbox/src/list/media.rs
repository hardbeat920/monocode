//! Port of src/features/inbox/ui/InboxMedia.tsx: a remote image or video
//! from an issue body, fetched with the provider's credentials. Images show
//! inline and open in the browser; videos and failures show as a link.
// TODO(port): GPUI has no video element, so videos show as their link.

use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    Context, Image, ImageFormat, InteractiveElement as _, IntoElement, ObjectFit,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, StyledImage as _,
    Task, Window, div, img,
};
use monocode_ui::widgets::tooltip;
use monocode_ui::{Theme, u};

use crate::data::{InboxMediaKind, InboxServices};
use crate::style::palette;

enum LoadState {
    Loading,
    Ready(Arc<Image>),
    Error,
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
    _load: Task<()>,
}

impl InboxMediaView {
    pub fn new(
        services: Rc<dyn InboxServices>,
        src: String,
        alt: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let task = services.fetch_media(&src, cx);
        let load = cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.state = match result {
                    Ok(media) if media.kind == InboxMediaKind::Image => {
                        match image_format(&media.mime) {
                            Some(format) => LoadState::Ready(Arc::new(Image::from_bytes(
                                format,
                                media.bytes.as_ref().clone(),
                            ))),
                            None => LoadState::Error,
                        }
                    }
                    _ => LoadState::Error,
                };
                cx.notify();
            });
        });
        Self {
            services,
            src,
            alt,
            state: LoadState::Loading,
            _load: load,
        }
    }
}

impl Render for InboxMediaView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            LoadState::Error => {
                let label = if self.alt.trim().is_empty() {
                    self.src.clone()
                } else {
                    self.alt.trim().to_string()
                };
                let hover = palette::sky_400();
                div()
                    .id("inbox-media-link")
                    .text_color(monocode_ui::color::with_alpha(palette::sky_400(), 0.9))
                    .hover(move |s| s.text_color(hover).underline())
                    .on_click(move |_, _, cx| services.open_url(&src, cx))
                    .child(label)
                    .into_any_element()
            }
        }
    }
}
