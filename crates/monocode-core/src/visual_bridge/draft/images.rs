//! Images in an HTML visual are embedded before the page leaves the agent's
//! process: the page itself never reads files or makes requests, and a
//! published visual keeps rendering offline.
use base64::Engine;
use regex::Regex;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

/// The written page, without its embedded images.
pub const MAX_MARKUP_BYTES: usize = crate::visual::MAX_SOURCE_BYTES;
/// The page with its images and data embedded.
pub const MAX_HTML_BYTES: usize = 10 * 1024 * 1024;
/// The page with its images embedded.
const MAX_IMAGE_PAGE_BYTES: usize = 6 * 1024 * 1024;
/// A file or download larger than this is refused before it is decoded.
const MAX_INPUT_BYTES: u64 = 20 * 1024 * 1024;
/// Raster images are scaled to fit this many pixels on their long side.
const MAX_SIDE: u32 = 1600;
/// Images at or below this size, and within MAX_SIDE, are embedded unchanged.
const KEEP_BYTES: usize = 400 * 1024;
const MAX_IMAGES: usize = 24;
const EXTENSIONS: &str = "png|jpe?g|gif|webp|svg|avif|bmp|ico";

/// Quoted strings and unquoted CSS `url(...)` arguments that could be images.
/// Quoted ones may contain spaces, as screenshot file names do.
fn references() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        let start = r"(?:https://|/|~/)";
        Regex::new(&format!(
            r#""(?P<double>{start}[^"<>\n]+?)"|'(?P<single>{start}[^'<>\n]+?)'|url\(\s*(?P<bare>{start}[^"'()\s]+?)\s*\)"#
        ))
        .unwrap()
    })
}

/// Whether the reference at `at` is a CSS `url()` argument.
fn in_css_url(html: &str, at: usize) -> bool {
    html[..at]
        .trim_end_matches(['"', '\''])
        .trim_end()
        .to_ascii_lowercase()
        .ends_with("url(")
}

fn has_image_extension(target: &str) -> bool {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN
        .get_or_init(|| Regex::new(&format!(r"(?i)\.(?:{EXTENSIONS})(?:[?#].*)?$")).unwrap())
        .is_match(target)
}

/// Whether `at` sits inside an `<img>`, `<source>` or SVG `<image>` tag.
fn in_image_tag(html: &str, at: usize) -> bool {
    let before = &html[..at];
    let Some(open) = before.rfind('<') else {
        return false;
    };
    if before[open..].contains('>') {
        return false;
    }
    let name: String = before[open + 1..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    matches!(name.as_str(), "img" | "source" | "image")
}

/// Replaces every local or https image reference with a `data:` URL.
/// Local paths must be absolute (or start with `~/`); https addresses are
/// embedded when they name an image file, or when they are an image tag's
/// source or a CSS `url()`.
pub fn embed_images(html: &str) -> Result<String, String> {
    if html.len() > MAX_MARKUP_BYTES {
        return Err("Visual HTML exceeds 64 KiB before images are embedded".into());
    }
    let mut targets = Vec::new();
    for capture in references().captures_iter(html) {
        let Some(target) = ["double", "single", "bare"]
            .iter()
            .find_map(|name| capture.name(name))
        else {
            continue;
        };
        let css = in_css_url(html, target.start());
        let text = target.as_str();
        let wanted = if text.starts_with("https://") {
            has_image_extension(text) || css || in_image_tag(html, target.start())
        } else {
            has_image_extension(text)
        };
        if wanted {
            targets.push(target.range());
        }
    }
    if targets.is_empty() {
        return Ok(html.to_owned());
    }
    let mut embedded: HashMap<&str, String> = HashMap::new();
    for range in &targets {
        let target = &html[range.clone()];
        if embedded.contains_key(target) {
            continue;
        }
        if embedded.len() == MAX_IMAGES {
            return Err(format!("A visual may embed at most {MAX_IMAGES} images"));
        }
        let (bytes, mime) = load(target).map_err(|error| format!("Image {target}: {error}"))?;
        embedded.insert(target, data_url(&bytes, mime));
    }
    let mut out = String::with_capacity(html.len());
    let mut last = 0;
    for range in targets {
        out.push_str(&html[last..range.start]);
        out.push_str(&embedded[&html[range.clone()]]);
        last = range.end;
    }
    out.push_str(&html[last..]);
    if out.len() > MAX_IMAGE_PAGE_BYTES {
        return Err(format!(
            "The page's images add up to more than {} MiB; use fewer or smaller images",
            MAX_IMAGE_PAGE_BYTES / 1024 / 1024
        ));
    }
    Ok(out)
}

fn data_url(bytes: &[u8], mime: &str) -> String {
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

fn load(target: &str) -> Result<(Vec<u8>, &'static str), String> {
    let bytes = if target.starts_with("https://") {
        download(target)?
    } else {
        read(&local_path(target)?)?
    };
    shrink(bytes)
}

fn local_path(target: &str) -> Result<PathBuf, String> {
    // A query or fragment never belongs to a file name.
    let path = target.split(['?', '#']).next().unwrap_or(target);
    let path = if Path::new(path).exists() {
        path.to_owned()
    } else {
        percent_decode(path)
    };
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
        Ok(Path::new(&home).join(rest))
    } else {
        Ok(PathBuf::from(path))
    }
}

/// `%20` and friends, as a browser would read them in a file URL.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |at: usize| bytes.get(at).and_then(|&b| (b as char).to_digit(16));
        match (bytes[i], hex(i + 1), hex(i + 2)) {
            (b'%', Some(high), Some(low)) => {
                out.push((high * 16 + low) as u8);
                i += 3;
            }
            (byte, _, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if length > MAX_INPUT_BYTES {
        return Err("larger than 20 MiB".into());
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_INPUT_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .redirects(5)
        .build();
    let response = agent.get(url).call().map_err(|error| match error {
        ureq::Error::Status(code, _) => format!("the server answered {code}"),
        ureq::Error::Transport(error) => error.to_string(),
    })?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err("larger than 20 MiB".into());
    }
    Ok(bytes)
}

/// The image's type from its first bytes; file names and servers can be wrong.
fn sniff(bytes: &[u8]) -> Option<&'static str> {
    let head = &bytes[..bytes.len().min(512)];
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if head.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some("image/webp")
    } else if head.len() >= 12
        && &head[4..8] == b"ftyp"
        && matches!(&head[8..12], b"avif" | b"avis")
    {
        Some("image/avif")
    } else if head.starts_with(b"BM") {
        Some("image/bmp")
    } else if head.starts_with(b"\0\0\x01\0") {
        Some("image/x-icon")
    } else {
        let text = String::from_utf8_lossy(head).to_ascii_lowercase();
        let text = text.trim_start_matches('\u{feff}').trim_start();
        (text.starts_with("<svg") || (text.starts_with("<?xml") && text.contains("<svg")))
            .then_some("image/svg+xml")
    }
}

/// Large rasters are scaled down and re-encoded; everything else is kept.
fn shrink(bytes: Vec<u8>) -> Result<(Vec<u8>, &'static str), String> {
    let mime = sniff(&bytes).ok_or("not a PNG, JPEG, GIF, WebP, AVIF, BMP, ICO or SVG image")?;
    // Animation and vectors survive only as they are; AVIF cannot be decoded here.
    if matches!(mime, "image/gif" | "image/svg+xml" | "image/avif") {
        return if bytes.len() > 2 * 1024 * 1024 {
            Err(format!("{mime} images must be under 2 MiB"))
        } else {
            Ok((bytes, mime))
        };
    }
    let image = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    let fits = image.width() <= MAX_SIDE && image.height() <= MAX_SIDE;
    if fits && bytes.len() <= KEEP_BYTES {
        return Ok((bytes, mime));
    }
    let image = if fits {
        image
    } else {
        image.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::CatmullRom)
    };
    let mut out = std::io::Cursor::new(Vec::new());
    if image.color().has_alpha() {
        image
            .write_to(&mut out, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        Ok((out.into_inner(), "image/png"))
    } else {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
            .encode_image(&image.to_rgb8())
            .map_err(|e| e.to_string())?;
        Ok((out.into_inner(), "image/jpeg"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32, alpha: bool) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let image: image::DynamicImage = if alpha {
            image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 128])).into()
        } else {
            image::RgbImage::from_fn(width, height, |x, y| {
                image::Rgb([(x % 251) as u8, (y % 241) as u8, ((x ^ y) % 239) as u8])
            })
            .into()
        };
        image.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn local_images_are_embedded_wherever_a_page_names_them() {
        let dir = tempfile::tempdir().unwrap();
        let shot = dir.path().join("shot one.png");
        std::fs::write(&shot, png(8, 8, false)).unwrap();
        let path = shot.to_str().unwrap();
        let html = format!(
            "<img src=\"{path}\"><div style=\"background:url('{path}')\"></div><script>const a='{path}'</script><p>'/not/an/image.txt'</p>"
        );
        let out = embed_images(&html).unwrap();
        assert_eq!(out.matches("data:image/png;base64,").count(), 3);
        assert!(!out.contains(path));
        assert!(out.contains("'/not/an/image.txt'"));
    }

    #[test]
    fn missing_or_fake_images_name_the_reference() {
        let error = embed_images("<img src=\"/no/such/file.png\">").unwrap_err();
        assert!(error.starts_with("Image /no/such/file.png:"), "{error}");
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("fake.png");
        std::fs::write(&fake, b"#!/bin/sh\necho hi").unwrap();
        let error = embed_images(&format!("<img src=\"{}\">", fake.display())).unwrap_err();
        assert!(error.contains("not a PNG"), "{error}");
    }

    #[test]
    fn scripts_and_stylesheets_from_cdns_are_left_alone() {
        let html = r#"<script src="https://cdn.jsdelivr.net/npm/chart.js@4.4.4"></script><link rel=stylesheet href="https://fonts.googleapis.com/css2?family=Inter">"#;
        assert_eq!(embed_images(html).unwrap(), html);
    }

    #[test]
    fn large_images_are_scaled_to_fit_and_transparency_is_kept() {
        let (bytes, mime) = shrink(png(3200, 1000, false)).unwrap();
        assert_eq!(mime, "image/jpeg");
        let image = image::load_from_memory(&bytes).unwrap();
        assert_eq!((image.width(), image.height()), (1600, 500));
        let (bytes, mime) = shrink(png(2400, 2400, true)).unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(image::load_from_memory(&bytes).unwrap().width(), 1600);
        let small = png(16, 16, false);
        assert_eq!(shrink(small.clone()).unwrap(), (small, "image/png"));
    }

    #[test]
    fn tilde_paths_resolve_from_home() {
        let path = local_path("~/Pictures/a%20b.png?v=2").unwrap();
        assert!(path.ends_with("Pictures/a b.png"), "{}", path.display());
    }
}
