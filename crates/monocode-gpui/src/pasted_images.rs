//! Shared worker-side preparation for chat and Quick Composer clipboard images.
use gpui::{Image, ImageFormat};
use monocode_core::composer::{MAX_ATTACHMENTS, MAX_EMBED_BYTES, validate_attachments};
use std::{
    io::Cursor,
    path::{Path, PathBuf},
};

pub(crate) struct PreparedImages {
    paths: Vec<PathBuf>,
}

impl PreparedImages {
    pub(crate) fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub(crate) fn keep(mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.paths)
    }
}

impl Drop for PreparedImages {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub(crate) fn save(
    images: Vec<Image>,
    root: &Path,
    existing: &[PathBuf],
) -> Result<PreparedImages, String> {
    if existing.len() + images.len() > MAX_ATTACHMENTS {
        return Err(format!("Attach up to {MAX_ATTACHMENTS} files at a time."));
    }
    validate_attachments(existing)?;
    let images = images
        .into_iter()
        .map(normalize)
        .collect::<Result<Vec<_>, _>>()?;
    let existing_bytes = existing.iter().try_fold(0u64, |total, path| {
        path.metadata()
            .map(|meta| total + meta.len())
            .map_err(|error| error.to_string())
    })?;
    if existing_bytes
        + images
            .iter()
            .map(|image| image.bytes().len() as u64)
            .sum::<u64>()
        > MAX_EMBED_BYTES
    {
        return Err("Attachments must total 20 MB or less.".into());
    }
    std::fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let mut prepared = PreparedImages { paths: Vec::new() };
    for image in images {
        let path = root.join(format!(
            "pasted-{}.{}",
            monocode_core::mono::new_id(),
            image.format().extension()
        ));
        prepared.paths.push(path.clone());
        std::fs::write(&path, image.bytes()).map_err(|error| error.to_string())?;
    }
    Ok(prepared)
}

fn normalize(image: Image) -> Result<Image, String> {
    if image.format() != ImageFormat::Bmp {
        return Ok(image);
    }
    let mut decoded = image::load_from_memory_with_format(image.bytes(), image::ImageFormat::Bmp)
        .map_err(|error| format!("Could not decode the pasted bitmap: {error}"))?;
    // Windows-synthesized DIBV5 often declares an alpha mask over zeroed bytes.
    if decoded
        .as_rgba8()
        .is_some_and(|rgba| rgba.pixels().all(|pixel| pixel[3] == 0))
    {
        decoded = image::DynamicImage::ImageRgb8(decoded.to_rgb8());
    }
    let mut png = Cursor::new(Vec::new());
    decoded
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|error| format!("Could not encode the pasted image: {error}"))?;
    Ok(Image::from_bytes(ImageFormat::Png, png.into_inner()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_pixels_and_supported_encodings_survive_preparation() {
        let bitmap = crate::fixtures::clipboard_bitmap();
        let original = image::load_from_memory(bitmap.bytes()).unwrap().to_rgba8();
        let converted = normalize(bitmap).unwrap();
        assert_eq!(converted.format(), ImageFormat::Png);
        assert_eq!(
            image::load_from_memory(converted.bytes())
                .unwrap()
                .to_rgba8(),
            original
        );
        for format in [
            ImageFormat::Png,
            ImageFormat::Jpeg,
            ImageFormat::Gif,
            ImageFormat::Webp,
        ] {
            let input = Image::from_bytes(format, vec![1, 2, 3]);
            let output = normalize(input).unwrap();
            assert_eq!(output.format(), format);
            assert_eq!(output.bytes(), [1, 2, 3]);
        }
    }

    #[test]
    fn bitmap_with_zeroed_alpha_becomes_opaque_but_real_alpha_survives() {
        let bitmap = |alpha: [u8; 2]| {
            let pixels = image::RgbaImage::from_raw(
                2,
                1,
                vec![18, 52, 86, alpha[0], 171, 205, 239, alpha[1]],
            )
            .unwrap();
            let mut bmp = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(pixels)
                .write_to(&mut bmp, image::ImageFormat::Bmp)
                .unwrap();
            Image::from_bytes(ImageFormat::Bmp, bmp.into_inner())
        };
        let decode = |image: Image| {
            image::load_from_memory(normalize(image).unwrap().bytes())
                .unwrap()
                .to_rgba8()
                .into_raw()
        };
        assert_eq!(
            decode(bitmap([0, 0])),
            [18, 52, 86, 255, 171, 205, 239, 255]
        );
        assert_eq!(
            decode(bitmap([0, 128])),
            [18, 52, 86, 0, 171, 205, 239, 128]
        );
    }

    #[test]
    fn invalid_or_excessive_images_leave_no_files() {
        let root = std::env::temp_dir().join(format!(
            "monocode-pasted-test-{}",
            monocode_core::mono::new_id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let invalid = Image::from_bytes(ImageFormat::Bmp, b"invalid bitmap".to_vec());
        assert!(save(vec![invalid], &root, &[]).is_err());
        let too_many = (0..=MAX_ATTACHMENTS)
            .map(|_| crate::fixtures::clipboard_bitmap())
            .collect();
        assert!(save(too_many, &root, &[]).is_err());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0);
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn prepared_images_are_removed_unless_the_composer_accepts_them() {
        let root = std::env::temp_dir().join(format!(
            "monocode-pasted-test-{}",
            monocode_core::mono::new_id()
        ));
        let prepared = save(vec![crate::fixtures::clipboard_bitmap()], &root, &[]).unwrap();
        let rejected = prepared.paths()[0].clone();
        assert!(rejected.is_file());
        drop(prepared);
        assert!(!rejected.exists());
        let accepted = save(vec![crate::fixtures::clipboard_bitmap()], &root, &[])
            .unwrap()
            .keep();
        assert!(accepted[0].is_file());
        std::fs::remove_file(&accepted[0]).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
}
