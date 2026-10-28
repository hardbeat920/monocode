//! Small public provider avatars. No provider credentials leave their adapter.
use std::{
    io::{Cursor, Read},
    time::Duration,
};

pub fn png(url: &str) -> Result<Vec<u8>, String> {
    let parsed = url::Url::parse(url).map_err(|_| "Invalid avatar URL".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Invalid avatar URL".into());
    }
    let response = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(8))
        .build()
        .get(url)
        .set("User-Agent", "MonoCode")
        .call()
        .map_err(|_| "Avatar unavailable".to_string())?;
    let mut bytes = vec![];
    response
        .into_reader()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Avatar unavailable".to_string())?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("Avatar is too large".into());
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "Invalid avatar".to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| "Invalid avatar".to_string())?
        .thumbnail(64, 64)
        .into_rgba8();
    crate::project_appearance::png_bytes(image)
}
