//! Port of src/features/files/model/filePreview.ts: which files open in
//! the image and PDF viewers, content sniffing, and the size label.

use monocode_core::paths::basename;

const IMAGE_EXTENSIONS: [&str; 8] = [
    ".png", ".jpg", ".jpeg", ".gif", ".webp", ".avif", ".bmp", ".ico",
];

/// `%PDF-`.
const PDF_MAGIC: [u8; 5] = [0x25, 0x50, 0x44, 0x46, 0x2d];

/// `isImagePath`: the image viewer owns the file, decided before reading.
/// `.svg` is absent on purpose: it is text, so it opens in the editor,
/// which has its own rendered preview.
pub fn is_image_path(path: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&extension_of(path).as_str())
}

/// `isPdfPath`.
pub fn is_pdf_path(path: &str) -> bool {
    extension_of(path) == ".pdf"
}

/// `isPdfBytes`: the `%PDF-` header may sit anywhere in the first 1024
/// bytes.
pub fn is_pdf_bytes(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(1024)];
    head.windows(PDF_MAGIC.len())
        .any(|window| window == PDF_MAGIC)
}

/// `sniffImageMime`: the MIME type from the magic number, not the name.
pub fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(&[0x47, 0x49, 0x46, 0x38]) {
        return Some("image/gif");
    }
    if bytes.starts_with(&[0x42, 0x4d]) {
        return Some("image/bmp");
    }
    if bytes.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        return Some("image/x-icon");
    }
    // RIFF....WEBP: the four size bytes at offset 4 are skipped.
    if bytes.starts_with(&[0x52, 0x49, 0x46, 0x46])
        && bytes.get(8..).is_some_and(|rest| rest.starts_with(b"WEBP"))
    {
        return Some("image/webp");
    }
    // ....ftyp{avif,avis}: an ISO base media box, shared with HEIF and MP4.
    if bytes.get(4..).is_some_and(|rest| rest.starts_with(b"ftyp")) {
        let brand = bytes.get(8..12.min(bytes.len())).unwrap_or_default();
        if brand == b"avif" || brand == b"avis" {
            return Some("image/avif");
        }
    }
    None
}

/// `formatFileSize`: bytes in the units a file manager shows.
pub fn format_file_size(bytes: i64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    const UNITS: [&str; 3] = ["KB", "MB", "GB"];
    let mut size = bytes as f64 / 1024.0;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    let number = if size < 10.0 {
        monocode_core::js::to_fixed_1(size)
    } else {
        monocode_core::js::number_to_string(monocode_core::js::round(size))
    };
    format!("{number} {}", UNITS[unit])
}

fn extension_of(path: &str) -> String {
    let name = basename(path).to_lowercase();
    match name.rfind('.') {
        Some(dot) => name[dot..].to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_images_away_from_the_editor() {
        assert!(is_image_path("/w/shot.png"));
        assert!(is_image_path("/w/Photo.JPEG"));
        assert!(is_image_path("/w/icon.webp"));
    }

    #[test]
    fn leaves_text_svg_and_extensionless_files_to_the_editor() {
        assert!(!is_image_path("/w/main.rs"));
        assert!(!is_image_path("/w/logo.svg"));
        assert!(!is_image_path("/w/spec.pdf"));
        assert!(!is_image_path("/w/LICENSE"));
        assert!(!is_image_path("/w/.png/notes.txt"));
    }

    #[test]
    fn routes_pdfs_to_the_viewer_by_extension() {
        assert!(is_pdf_path("/w/spec.pdf"));
        assert!(is_pdf_path("/w/Report.PDF"));
        assert!(!is_pdf_path("/w/notes.md"));
        assert!(!is_pdf_path("/w/.pdf/notes.txt"));
    }

    #[test]
    fn finds_the_header_at_the_start_or_within_the_first_1024_bytes() {
        assert!(is_pdf_bytes(b"%PDF-1.7\n"));
        assert!(is_pdf_bytes(
            format!("{}%PDF-1.4", " ".repeat(500)).as_bytes()
        ));
        assert!(!is_pdf_bytes(
            format!("{}%PDF-1.4", " ".repeat(1024)).as_bytes()
        ));
        assert!(!is_pdf_bytes(b"<html>%PD"));
        assert!(!is_pdf_bytes(&[]));
    }

    #[test]
    fn identifies_each_supported_format_by_magic_number() {
        assert_eq!(
            sniff_image_mime(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
            Some("image/png")
        );
        assert_eq!(
            sniff_image_mime(&[0xff, 0xd8, 0xff, 0xe0]),
            Some("image/jpeg")
        );
        assert_eq!(
            sniff_image_mime(&[0x47, 0x49, 0x46, 0x38, 0x39, 0x61]),
            Some("image/gif")
        );
        assert_eq!(
            sniff_image_mime(&[0x52, 0x49, 0x46, 0x46, 1, 2, 3, 4, 0x57, 0x45, 0x42, 0x50]),
            Some("image/webp")
        );
        assert_eq!(
            sniff_image_mime(&[
                0, 0, 0, 0x20, 0x66, 0x74, 0x79, 0x70, 0x61, 0x76, 0x69, 0x66
            ]),
            Some("image/avif")
        );
    }

    #[test]
    fn refuses_content_that_is_not_a_supported_image() {
        assert_eq!(sniff_image_mime(b"<html><script>x()</script>"), None);
        assert_eq!(sniff_image_mime(b"%PDF-1.7"), None);
        assert_eq!(sniff_image_mime(&[0x89, 0x50]), None);
        assert_eq!(sniff_image_mime(&[]), None);
        // An MP4 shares the ftyp box with AVIF but is not an image.
        assert_eq!(
            sniff_image_mime(&[
                0, 0, 0, 0x20, 0x66, 0x74, 0x79, 0x70, 0x69, 0x73, 0x6f, 0x6d
            ]),
            None
        );
    }

    #[test]
    fn scales_to_the_largest_unit_that_keeps_a_leading_digit() {
        assert_eq!(format_file_size(0), "0 B");
        assert_eq!(format_file_size(900), "900 B");
        assert_eq!(format_file_size(2048), "2.0 KB");
        assert_eq!(format_file_size(48 * 1024), "48 KB");
        assert_eq!(format_file_size(3 * 1024 * 1024), "3.0 MB");
        assert_eq!(format_file_size(2 * 1024 * 1024 * 1024), "2.0 GB");
    }
}
