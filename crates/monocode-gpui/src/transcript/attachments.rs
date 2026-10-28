use super::*;
use gpui::{AnyElement, ImageSource};
use std::path::PathBuf;

#[derive(Clone)]
pub(crate) struct Attachment {
    name: SharedString,
    path: Option<PathBuf>,
    mime: SharedString,
    image: bool,
    data: Option<Arc<str>>,
}

impl Attachment {
    fn from_value(mut value: serde_json::Value) -> Self {
        let path = value["path"]
            .as_str()
            .filter(|p| !p.is_empty())
            .map(PathBuf::from);
        let name = value["name"]
            .as_str()
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .or_else(|| path.as_ref()?.file_name()?.to_str().map(str::to_owned))
            .unwrap_or_else(|| "Image".into());
        let mime = value["mimeType"].as_str().unwrap_or("").to_owned();
        let image = value["kind"] == "image"
            || mime.starts_with("image/")
            || path
                .as_ref()
                .is_some_and(|path| monocode_core::composer::image_mime(path).is_some());
        let data = match value["data"].take() {
            serde_json::Value::String(data) if !data.is_empty() => Some(Arc::from(data)),
            _ => path
                .as_ref()
                .and_then(|path| path.to_str())
                .filter(|path| monocode_core::remote::attachments::is_attachment_source(path))
                .map(Arc::from),
        };
        Self {
            name: name.into(),
            path,
            mime: mime.into(),
            image,
            data,
        }
    }

    /// Everything a chip's size depends on. Embedded image data is skipped:
    /// it can be large, and chips have a fixed size.
    pub(super) fn hash_layout(&self, state: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        (&self.name, &self.path, &self.mime, self.image).hash(state);
    }

    pub(super) fn from_paths(paths: &[PathBuf]) -> Arc<[Self]> {
        let value = monocode_core::transcript::attachments::from_paths(paths);
        value
            .as_array()
            .into_iter()
            .flatten()
            .cloned()
            .map(Self::from_value)
            .collect()
    }
}

pub(crate) fn message(rows: &[Row], end: usize, text: &str) -> (Option<String>, Arc<[Attachment]>) {
    if let Some(manifest) = rows
        .get(end)
        .filter(|row| row.role == monocode_core::transcript::attachments::ROLE)
    {
        let files: Vec<serde_json::Value> =
            serde_json::from_str(&manifest.text).unwrap_or_default();
        return (
            None,
            files.into_iter().map(Attachment::from_value).collect(),
        );
    }
    let (body, paths) = monocode_core::transcript::attachments::legacy_tail(text);
    (
        (!paths.is_empty()).then(|| body.to_owned()),
        Attachment::from_paths(&paths),
    )
}

/// Where a chip's pixels come from: the transcript's embedded copy, or a file.
#[derive(Clone)]
pub(crate) enum Origin {
    Embedded(Arc<str>),
    Path(PathBuf),
}

impl Origin {
    fn of(file: &Attachment) -> Option<Self> {
        file.data
            .clone()
            .map(Self::Embedded)
            .or_else(|| file.path.clone().map(Self::Path))
    }

    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Embedded(a), Self::Embedded(b)) => {
                Arc::ptr_eq(a, b)
                    || (monocode_core::remote::attachments::is_attachment_source(b) && a == b)
            }
            (Self::Path(a), Self::Path(b)) => a == b,
            _ => false,
        }
    }

    fn read(&self) -> Result<Vec<u8>, String> {
        match self {
            Self::Embedded(data) => monocode_core::composer::decode_image_data(data),
            Self::Path(path) => std::fs::read(path).map_err(|error| error.to_string()),
        }
    }
}

/// A chip covers 36px at up to 3x, so it keeps a sprite this tall, never the
/// full image. Opening the lightbox decodes the original separately.
const THUMBNAIL_SIDE: u32 = 108;

/// Images are decoded off-thread when their chip is first built. Releasing the
/// entity frees the GPU sprite too; GPUI keeps atlas entries until told.
pub(crate) struct Photo {
    origin: Origin,
    bytes: usize,
    mime: SharedString,
    source: Option<ImageSource>,
}

impl Photo {
    pub(crate) fn new(origin: Origin, mime: SharedString, cx: &mut Context<Self>) -> Self {
        let format = match mime.as_ref() {
            "image/jpeg" | "image/jpg" => gpui::ImageFormat::Jpeg,
            "image/gif" => gpui::ImageFormat::Gif,
            "image/webp" => gpui::ImageFormat::Webp,
            "image/svg+xml" => gpui::ImageFormat::Svg,
            "image/bmp" => gpui::ImageFormat::Bmp,
            "image/tiff" => gpui::ImageFormat::Tiff,
            _ => gpui::ImageFormat::Png,
        };
        let read = origin.clone();
        let work = cx.background_executor().spawn(async move {
            let bytes = read.read()?;
            Ok::<_, String>(
                match monocode_core::composer::thumbnail_bgra(&bytes, THUMBNAIL_SIDE) {
                    Ok(pixels) => Ok(Arc::new(gpui::RenderImage::new(vec![image::Frame::new(
                        pixels,
                    )]))),
                    // SVG and formats the decoder lacks still go through GPUI.
                    Err(_) => Err(Arc::new(gpui::Image::from_bytes(format, bytes))),
                },
            )
        });
        cx.spawn(async move |this, cx| {
            let source = work.await.ok().map(|decoded| match decoded {
                Ok(sprite) => ImageSource::Render(sprite),
                Err(encoded) => ImageSource::from(encoded),
            });
            let _ = this.update(cx, |this, cx| {
                this.source = source;
                cx.notify();
            });
        })
        .detach();
        cx.on_release(|this, cx| {
            if let Some(source) = this.source.take() {
                source.evict(cx);
            }
        })
        .detach();
        Self {
            bytes: match &origin {
                Origin::Embedded(data) => {
                    monocode_core::remote::attachments::attachment_source_size(data)
                        .unwrap_or(data.len())
                }
                Origin::Path(_) => 0,
            },
            origin,
            mime,
            source: None,
        }
    }

    pub(crate) fn matches(&self, origin: &Origin, mime: &SharedString) -> bool {
        self.origin.same(origin) && &self.mime == mime
    }
}

impl Render for Photo {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if let Some(source) = &self.source {
            attachment_thumbnail(source.clone())
        } else {
            div()
                .size(px(36.))
                .flex_shrink_0()
                .rounded_lg()
                .bg(content(0.04))
                .flex()
                .items_center()
                .justify_center()
                .child(icon("file", 14.))
                .into_any_element()
        }
    }
}

impl TranscriptView {
    pub(super) fn attachment_row(
        &mut self,
        files: &[Attachment],
        key: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .flex_wrap()
            .gap_1p5()
            .children(files.iter().enumerate().map(|(ix, file)| {
                if file.image {
                    let photo = if let Some(origin) = Origin::of(file) {
                        let key = format!("{key}-{ix}");
                        let cached = self
                            .attachment_images
                            .get(&key)
                            .filter(|photo| photo.read(cx).matches(&origin, &file.mime))
                            .cloned();
                        let photo = cached.unwrap_or_else(|| {
                            let photo = cx.new(|cx| Photo::new(origin, file.mime.clone(), cx));
                            self.attachment_images.insert(key.clone(), photo.clone());
                            // Bound decoded image ownership while scrolling long histories.
                            while self.attachment_images.len() > 32
                                || self
                                    .attachment_images
                                    .values()
                                    .map(|photo| photo.read(cx).bytes)
                                    .sum::<usize>()
                                    > 64 * 1024 * 1024
                            {
                                let Some(old) = self
                                    .attachment_images
                                    .keys()
                                    .find(|old| **old != key)
                                    .cloned()
                                else {
                                    break;
                                };
                                self.attachment_images.remove(&old);
                            }
                            photo
                        });
                        photo.into_any_element()
                    } else {
                        icon("file", 16.).into_any_element()
                    };
                    let source = file
                        .data
                        .as_ref()
                        .map(|data| crate::image_lightbox::Source::Embedded {
                            data: data.clone(),
                            mime: file.mime.clone(),
                        })
                        .or_else(|| file.path.clone().map(crate::image_lightbox::Source::Path));
                    let alt = file.name.clone();
                    div()
                        .id(SharedString::from(format!("attachment-{key}-{ix}")))
                        .size(px(36.))
                        .flex_shrink_0()
                        .role(Role::Button)
                        .focusable()
                        .debug_selector(move || format!("transcript-image-{ix}"))
                        .aria_label(format!("Open {} full screen", file.name))
                        .when_some(source, |d, source| {
                            d.cursor(gpui::CursorStyle::PointingHand)
                                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    window.dispatch_action(
                                        Box::new(crate::image_lightbox::Open {
                                            source: source.clone(),
                                            alt: alt.clone(),
                                        }),
                                        cx,
                                    );
                                })
                        })
                        .child(photo)
                        .into_any_element()
                } else {
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .px_1()
                        .py_1()
                        .rounded_md()
                        .bg(content(0.06))
                        .child(crate::file_icons::file_type_icon(
                            &file.name, false, false, 16.,
                        ))
                        .child(
                            div()
                                .max_w(px(140.))
                                .truncate()
                                .text_size(px(11.))
                                .child(file.name.clone()),
                        )
                        .into_any_element()
                }
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gpui::{
        Context, ImageSource, ImgResourceLoader, IntoElement, Render, Resource, Styled,
        TestAppContext, Window, img, px,
    };

    struct Viewer(ImageSource);

    impl Render for Viewer {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            img(self.0.clone()).size(px(2.))
        }
    }

    #[gpui::test]
    fn evicting_a_decoded_source_frees_its_sprite(cx: &mut TestAppContext) {
        let path = std::env::temp_dir().join(format!("photo-evict-{}.png", std::process::id()));
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .save(&path)
            .unwrap();
        let resource = Resource::Path(path.clone().into());
        let source = ImageSource::Resource(resource.clone());
        let (_view, cx) = cx.add_window_view(|_, _| Viewer(source.clone()));
        cx.run_until_parked();
        let image = cx
            .update(|_, cx| cx.fetch_asset::<ImgResourceLoader>(&resource))
            .and_then(Result::ok)
            .expect("the image should have decoded");
        assert!(cx.update(|window, _| window.has_image_atlas_entry(&image)));

        // Views release their images from outside any window update.
        let app: &TestAppContext = cx;
        app.update(|cx| source.evict(cx));
        assert!(!cx.update(|window, _| window.has_image_atlas_entry(&image)));
        assert!(!cx.update(|_, cx| source.is_asset_cached(cx)));
        let _ = std::fs::remove_file(path);
    }
}
