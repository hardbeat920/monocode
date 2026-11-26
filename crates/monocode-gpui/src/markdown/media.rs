//! Async Markdown image previews with a bounded GPU sprite cache shared across
//! virtualized transcript rows (supporting local files, remote host files,
//! data URIs, and HTTP(S) images) and click-to-open ImageLightbox integration.
use super::*;
use gpui::{
    App, CursorStyle, EntityId, Image, ImageFormat, IntoElement, MouseButton, RenderImage,
    RenderOnce, Role, SharedString, Task, Window, div, img, prelude::*, px,
};
use std::{
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

const MAX_DISPLAY_WIDTH: f32 = 560.;
const MAX_DISPLAY_HEIGHT: f32 = 360.;
const MAX_DECODE_SIDE: u32 = 1600;
const CACHE_BYTES: usize = 64 << 20;
const KEEP_RECENT: Duration = Duration::from_secs(2);
const ERROR_RETRY_AFTER: Duration = Duration::from_secs(5);
const MAX_LOCAL_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ImageTarget {
    DataUri {
        data: Arc<str>,
        mime: SharedString,
    },
    Http(SharedString),
    Remote {
        environment: SharedString,
        cwd: SharedString,
        path: SharedString,
    },
    Local(PathBuf),
}

impl ImageTarget {
    fn lightbox_source(&self) -> crate::image_lightbox::Source {
        match self {
            Self::DataUri { data, mime } => crate::image_lightbox::Source::Embedded {
                data: data.clone(),
                mime: mime.clone(),
            },
            Self::Http(url) => crate::image_lightbox::Source::Url(url.clone()),
            Self::Remote {
                environment,
                cwd,
                path,
            } => crate::image_lightbox::Source::RemoteImage {
                environment: environment.clone(),
                cwd: cwd.clone(),
                path: path.clone(),
            },
            Self::Local(path) => crate::image_lightbox::Source::Path(path.clone()),
        }
    }

    fn fallback_name(&self) -> Option<String> {
        let raw = match self {
            Self::DataUri { .. } => return None,
            Self::Http(url) => url
                .as_ref()
                .split(['?', '#'])
                .next()
                .unwrap_or(url.as_ref()),
            Self::Remote { path, .. } => path.as_ref(),
            Self::Local(path) => path.to_str()?,
        };
        raw.rsplit(['/', '\\'])
            .next()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
    }

    fn cache_key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish()
    }
}

#[derive(Clone)]
struct DecodedImage {
    render: Arc<RenderImage>,
    width: u32,
    height: u32,
}

enum CacheEntry {
    Loading {
        views: HashSet<EntityId>,
        _task: Task<()>,
    },
    Ready {
        result: Result<DecodedImage, String>,
        bytes: usize,
        used: Instant,
    },
}

#[derive(Default)]
struct MarkdownImageCache {
    entries: HashMap<u64, CacheEntry>,
    bytes: usize,
    generation: u64,
    trimming: bool,
}
impl gpui::Global for MarkdownImageCache {}

pub(crate) fn generation(cx: &App) -> u64 {
    cx.try_global::<MarkdownImageCache>()
        .map_or(0, |cache| cache.generation)
}

fn decoded_bytes(image: &RenderImage) -> usize {
    let size = image.size(0);
    (size.width.0.max(0) as usize)
        .saturating_mul(size.height.0.max(0) as usize)
        .saturating_mul(4)
}

fn expand_home(path: &str) -> PathBuf {
    if path == "~" || path.starts_with("~/") || path.starts_with("~\\") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            let suffix = path
                .strip_prefix("~/")
                .or_else(|| path.strip_prefix("~\\"))
                .unwrap_or("");
            return if suffix.is_empty() {
                PathBuf::from(home)
            } else {
                PathBuf::from(home).join(suffix)
            };
        }
    }
    PathBuf::from(path)
}

fn normalize_local_path(raw: &str, source_cwd: Option<&str>) -> PathBuf {
    let expanded = expand_home(raw);
    let joined = if expanded.is_absolute() {
        expanded
    } else if let Some(base) = source_cwd.filter(|cwd| !cwd.is_empty()) {
        Path::new(base).join(expanded)
    } else {
        expanded
    };
    let mut clean = PathBuf::new();
    for component in joined.components() {
        match component {
            std::path::Component::ParentDir => {
                clean.pop();
            }
            std::path::Component::CurDir => {}
            other => clean.push(other),
        }
    }
    clean
}

fn resolve_target(url: &str, source_cwd: Option<&str>) -> Option<ImageTarget> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("data:image/") {
        let mime = trimmed
            .strip_prefix("data:")
            .or_else(|| trimmed.get(5..))
            .and_then(|rest| rest.split([';', ',']).next())
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or("image/png")
            .to_ascii_lowercase();
        return Some(ImageTarget::DataUri {
            data: Arc::from(trimmed),
            mime: mime.into(),
        });
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(ImageTarget::Http(SharedString::from(trimmed.to_owned())));
    }
    let decoded = monocode_core::remote::attachments::decode_file_url_or_path(trimmed)?;
    if let Some(remote) = source_cwd.and_then(monocode_core::remote::parse_remote_path) {
        return Some(ImageTarget::Remote {
            environment: remote.environment_id.into(),
            cwd: remote.host_path.into(),
            path: decoded.into(),
        });
    }
    Some(ImageTarget::Local(normalize_local_path(
        &decoded, source_cwd,
    )))
}

fn load_target_bytes(target: &ImageTarget) -> Result<(Vec<u8>, String), String> {
    match target {
        ImageTarget::DataUri { data, mime } => {
            let bytes = monocode_core::composer::decode_image_data(data)?;
            let detected = monocode_core::remote::attachments::detect_image_mime(
                &bytes,
                Some(mime.as_ref()),
                "",
            )
            .unwrap_or_else(|| mime.to_string());
            Ok((bytes, detected))
        }
        ImageTarget::Http(url) => monocode_core::remote::attachments::read_http_image(url),
        ImageTarget::Remote {
            environment,
            cwd,
            path,
        } => monocode_core::remote::attachments::read_remote_image(environment, cwd, path),
        ImageTarget::Local(path) => {
            let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
            let meta = file.metadata().map_err(|e| e.to_string())?;
            if !meta.is_file() {
                return Err("Not a regular file".into());
            }
            if meta.len() > MAX_LOCAL_IMAGE_BYTES {
                return Err("Image is larger than 20 MB.".into());
            }
            let mut bytes = Vec::with_capacity(meta.len().min(MAX_LOCAL_IMAGE_BYTES) as usize);
            file.take(MAX_LOCAL_IMAGE_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() as u64 > MAX_LOCAL_IMAGE_BYTES {
                return Err("Image is larger than 20 MB.".into());
            }
            let path_str = path.to_string_lossy();
            let mime =
                monocode_core::remote::attachments::detect_image_mime(&bytes, None, &path_str)
                    .ok_or_else(|| "Unsupported image format".to_string())?;
            Ok((bytes, mime))
        }
    }
}

fn decode_target(
    target: &ImageTarget,
    renderer: &gpui::SvgRenderer,
) -> Result<DecodedImage, String> {
    let (bytes, mime) = load_target_bytes(target)?;
    if mime != "image/svg+xml"
        && let Ok(pixels) = monocode_core::composer::fit_bgra(&bytes, MAX_DECODE_SIDE)
    {
        let (width, height) = pixels.dimensions();
        let render = Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]));
        return Ok(DecodedImage {
            render,
            width,
            height,
        });
    }
    let format = ImageFormat::from_mime_type(&mime).unwrap_or(ImageFormat::Png);
    let render = Image::from_bytes(format, bytes)
        .to_image_data(renderer.clone())
        .map_err(|error| error.to_string())?;
    let size = render.size(0);
    Ok(DecodedImage {
        render,
        width: size.width.0.max(0) as u32,
        height: size.height.0.max(0) as u32,
    })
}

fn display_size(width: u32, height: u32) -> (f32, f32) {
    if width == 0 || height == 0 {
        return (280., 160.);
    }
    let (w, h) = (width as f32, height as f32);
    let scale = (MAX_DISPLAY_WIDTH / w).min(MAX_DISPLAY_HEIGHT / h).min(1.);
    ((w * scale).round().max(24.), (h * scale).round().max(24.))
}

fn finish_load(key: u64, result: Result<DecodedImage, String>, cx: &mut App) {
    let (views, should_trim) = {
        let cache = cx.default_global::<MarkdownImageCache>();
        let views = match cache.entries.remove(&key) {
            Some(CacheEntry::Loading { views, .. }) => views,
            _ => HashSet::new(),
        };
        let bytes = result
            .as_ref()
            .map_or(0, |decoded| decoded_bytes(&decoded.render));
        cache.bytes += bytes;
        cache.generation = cache.generation.wrapping_add(1);
        cache.entries.insert(
            key,
            CacheEntry::Ready {
                result,
                bytes,
                used: Instant::now(),
            },
        );
        let should_trim = cache.bytes > CACHE_BYTES && !cache.trimming;
        if should_trim {
            cache.trimming = true;
        }
        (views, should_trim)
    };
    for view in views {
        cx.notify(view);
    }
    if should_trim {
        cx.defer(|cx| trim_cache(cx));
    }
}

fn trim_cache(cx: &mut App) {
    let dropped: Vec<Arc<RenderImage>> = {
        let cache = cx.default_global::<MarkdownImageCache>();
        cache.trimming = false;
        let now = Instant::now();
        let mut idle: Vec<(Instant, u64)> = cache
            .entries
            .iter()
            .filter_map(|(&key, entry)| match entry {
                CacheEntry::Ready {
                    result: Ok(_),
                    used,
                    ..
                } if now.duration_since(*used) >= KEEP_RECENT => Some((*used, key)),
                _ => None,
            })
            .collect();
        idle.sort_unstable();
        let mut dropped = Vec::new();
        for (_, key) in idle {
            if cache.bytes <= CACHE_BYTES {
                break;
            }
            if let Some(CacheEntry::Ready {
                result: Ok(decoded),
                bytes,
                ..
            }) = cache.entries.remove(&key)
            {
                cache.bytes = cache.bytes.saturating_sub(bytes);
                dropped.push(decoded.render);
            }
        }
        dropped
    };
    if !dropped.is_empty() {
        for image in dropped {
            cx.drop_image(image, None);
        }
        cx.refresh_windows();
    }
}

fn request_image(
    target: &ImageTarget,
    view: EntityId,
    cx: &mut App,
) -> Option<Result<DecodedImage, String>> {
    let key = target.cache_key();
    {
        let cache = cx.default_global::<MarkdownImageCache>();
        let expired_error = matches!(
            cache.entries.get(&key),
            Some(CacheEntry::Ready {
                result: Err(_),
                used,
                ..
            }) if used.elapsed() >= ERROR_RETRY_AFTER
        );
        if expired_error {
            cache.entries.remove(&key);
        }
        match cache.entries.get_mut(&key) {
            Some(CacheEntry::Ready { result, used, .. }) => {
                *used = Instant::now();
                return Some(result.clone());
            }
            Some(CacheEntry::Loading { views, .. }) => {
                views.insert(view);
                return None;
            }
            None => {}
        }
    }
    let renderer = cx.svg_renderer();
    let target_clone = target.clone();
    let work = cx
        .background_executor()
        .spawn(async move { decode_target(&target_clone, &renderer) });
    let task = cx.spawn(async move |cx| {
        let result = work.await;
        let _ = cx.update(|cx| finish_load(key, result, cx));
    });
    cx.default_global::<MarkdownImageCache>().entries.insert(
        key,
        CacheEntry::Loading {
            views: HashSet::from([view]),
            _task: task,
        },
    );
    None
}

#[derive(IntoElement)]
pub(crate) struct MarkdownImage {
    pub id: SharedString,
    pub url: String,
    pub alt: String,
    pub source_cwd: Option<String>,
}

impl RenderOnce for MarkdownImage {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(target) = resolve_target(&self.url, self.source_cwd.as_deref()) else {
            let label = if self.alt.trim().is_empty() {
                self.url.clone()
            } else {
                self.alt.clone()
            };
            return div()
                .id(self.id)
                .text_size(px(13.))
                .text_color(content(0.5))
                .child(label)
                .into_any_element();
        };
        let state = request_image(&target, window.current_view(), cx);
        let alt_clean = self.alt.trim();
        let display_label = if !alt_clean.is_empty() && alt_clean != "Image" {
            alt_clean.to_owned()
        } else {
            target.fallback_name().unwrap_or_else(|| "Image".to_owned())
        };
        let aria_label: SharedString = display_label.clone().into();
        let lightbox_source = target.lightbox_source();
        let url_for_menu = self.url.clone();

        match state {
            Some(Ok(decoded)) => {
                let (width, height) = display_size(decoded.width, decoded.height);
                div()
                    .id(self.id)
                    .debug_selector(|| "markdown-image".into())
                    .role(Role::Button)
                    .aria_label(format!("Open {aria_label} full screen"))
                    .w(px(width))
                    .h(px(height))
                    .max_w_full()
                    .rounded(px(10.))
                    .overflow_hidden()
                    .border_1()
                    .border_color(content(0.1))
                    .bg(content(0.04))
                    .cursor(CursorStyle::PointingHand)
                    .hover(|s| s.border_color(content(0.24)))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        window.dispatch_action(
                            Box::new(crate::image_lightbox::Open {
                                source: lightbox_source.clone(),
                                alt: aria_label.clone(),
                            }),
                            cx,
                        );
                    })
                    .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                        link_menu(&url_for_menu, event.position, window, cx);
                        cx.stop_propagation();
                    })
                    .child(
                        img(decoded.render)
                            .size_full()
                            .object_fit(gpui::ObjectFit::Contain),
                    )
                    .into_any_element()
            }
            None => div()
                .id(self.id)
                .debug_selector(|| "markdown-image-loading".into())
                .role(Role::Image)
                .aria_label(format!("Loading {aria_label}"))
                .w(px(280.))
                .max_w_full()
                .h(px(140.))
                .rounded(px(10.))
                .border_1()
                .border_color(content(0.08))
                .bg(content(0.05))
                .flex()
                .items_center()
                .justify_center()
                .gap_2()
                .px_3()
                .child(icon("image-add", 15.).text_color(content(0.4)))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(content(0.45))
                        .child(display_label),
                )
                .into_any_element(),
            Some(Err(error)) => {
                let retry_key = target.cache_key();
                let view_id = window.current_view();
                div()
                    .id(self.id)
                    .debug_selector(|| "markdown-image-error".into())
                    .role(Role::Button)
                    .aria_label(aria_label)
                    .max_w(px(460.))
                    .px_3()
                    .py_2()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(content(0.1))
                    .bg(content(0.04))
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor(CursorStyle::PointingHand)
                    .hover(|s| s.border_color(content(0.22)))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        cx.default_global::<MarkdownImageCache>()
                            .entries
                            .remove(&retry_key);
                        cx.notify(view_id);
                    })
                    .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                        link_menu(&url_for_menu, event.position, window, cx);
                        cx.stop_propagation();
                    })
                    .child(icon("image-add", 14.).text_color(content(0.45)))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(content(0.65))
                                    .child(display_label),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(content(0.4))
                                    .child(error),
                            ),
                    )
                    .into_any_element()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_local_remote_data_and_http_targets() {
        assert_eq!(
            resolve_target(
                "file:///Users/me/%E8%A1%8C%E4%B8%9A/shot%201.jpg",
                Some("remote://host-1/Users/me/repo"),
            ),
            Some(ImageTarget::Remote {
                environment: "host-1".into(),
                cwd: "/Users/me/repo".into(),
                path: "/Users/me/行业/shot 1.jpg".into(),
            })
        );
        assert_eq!(
            resolve_target("./out/a.png", Some("/Users/me/repo")),
            Some(ImageTarget::Local(PathBuf::from(
                "/Users/me/repo/out/a.png"
            )))
        );
        assert_eq!(
            resolve_target("https://example.com/a.png", Some("remote://env/repo")),
            Some(ImageTarget::Http("https://example.com/a.png".into()))
        );
        assert!(matches!(
            resolve_target("data:image/png;base64,iVBORw0KGgo=", None),
            Some(ImageTarget::DataUri { mime, .. }) if mime == "image/png"
        ));
    }

    #[test]
    fn display_size_preserves_aspect_ratio_without_upscaling_small_images() {
        assert_eq!(display_size(2612, 1002), (560., 215.));
        assert_eq!(display_size(1170, 2532), (166., 360.));
        assert_eq!(display_size(32, 32), (32., 32.));
    }
}
