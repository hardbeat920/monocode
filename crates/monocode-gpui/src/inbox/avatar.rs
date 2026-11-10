use super::*;
use gpui::{AnyElement, Asset, Image, ImageFormat, Window, div, img, prelude::*, px};
use std::sync::Arc;

struct Avatar;
impl Asset for Avatar {
    type Source = String;
    type Output = Result<Arc<Image>, String>;
    #[expect(
        clippy::manual_async_fn,
        reason = "An async method captures the non-Send App reference."
    )]
    fn load(
        url: String,
        _: &mut App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        async move {
            model::avatar_png(&url)
                .map(|bytes| Arc::new(Image::from_bytes(ImageFormat::Png, bytes)))
        }
    }
}

pub(crate) fn image(
    provider: &str,
    name: &str,
    url: &str,
    side: f32,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let url = if url.trim().is_empty() && provider == "github" && !name.trim().is_empty() {
        format!("https://github.com/{}.png?size=64", name.trim())
    } else {
        url.trim().to_string()
    };
    let image = (!url.is_empty())
        .then(|| window.use_asset::<Avatar>(&url, cx))
        .flatten()
        .and_then(Result::ok);
    let initial = name
        .trim()
        .chars()
        .next()
        .unwrap_or('?')
        .to_uppercase()
        .to_string();
    if let Some(image) = image {
        img(image)
            .size(px(side))
            .rounded_full()
            .object_fit(gpui::ObjectFit::Cover)
            .into_any_element()
    } else {
        div()
            .size(px(side))
            .rounded_full()
            .bg(content(0.12))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px((side * 0.45).max(9.)))
            .text_color(content(0.55))
            .child(initial)
            .into_any_element()
    }
}

pub(super) fn person(
    provider: &str,
    name: &str,
    url: &str,
    side: f32,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let avatar = image(provider, name, url, side, window, cx);
    div()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .child(avatar)
        .child(div().min_w_0().truncate().child(name.to_string()))
        .into_any_element()
}
