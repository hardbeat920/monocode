//! The webview only sees text on paste. Finder puts file URLs on the native
//! pasteboard as `public.file-url` items, one per file, and screenshot tools
//! put image data there, so pasted images are read from the native clipboard
//! instead of the paste event.

#[cfg(target_os = "macos")]
fn write_file_to(pb: &objc2_app_kit::NSPasteboard, path: &std::path::Path) -> Result<(), String> {
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::NSPasteboardWriting;
    use objc2_foundation::{NSArray, NSString, NSURL};

    let path = path
        .to_str()
        .ok_or_else(|| "The file path is not valid UTF-8".to_string())?;
    let url = NSURL::fileURLWithPath(&NSString::from_str(path));
    let object = ProtocolObject::<dyn NSPasteboardWriting>::from_retained(url);
    let objects = NSArray::from_retained_slice(&[object]);

    pb.clearContents();
    if pb.writeObjects(&objects) {
        Ok(())
    } else {
        Err("macOS refused to copy the file to the clipboard".into())
    }
}

#[cfg(target_os = "macos")]
fn file_paths_from(pb: &objc2_app_kit::NSPasteboard) -> Vec<String> {
    let Some(items) = pb.pasteboardItems() else {
        return Vec::new();
    };
    let file_url = unsafe { objc2_app_kit::NSPasteboardTypeFileURL };
    items
        .iter()
        .filter_map(|item| item.stringForType(file_url))
        .filter_map(|s| url::Url::parse(&s.to_string()).ok())
        .filter_map(|u| u.to_file_path().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

/// Paths for files copied in a file manager, empty when it holds none.
///
/// A file manager puts a URI list on the clipboard (`text/uri-list` on X11 and
/// Wayland, `public.file-url` on macOS, `CF_HDROP` on Windows) that a paste
/// event does not surface, so the webview sees nothing to attach.
#[tauri::command(async)]
pub fn clipboard_file_paths() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        file_paths_from(&objc2_app_kit::NSPasteboard::generalPasteboard())
    }
    #[cfg(not(target_os = "macos"))]
    {
        arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.get().file_list())
            .map(|paths| {
                paths
                    .iter()
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[tauri::command]
pub fn copy_file_to_clipboard(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let path = crate::fs::expand_home(&path);
        let metadata =
            std::fs::metadata(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        if !metadata.is_file() {
            return Err(format!("{} is not a file", path.display()));
        }
        write_file_to(&objc2_app_kit::NSPasteboard::generalPasteboard(), &path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err("Copying files to the clipboard is only supported on macOS".into())
    }
}

/// Read an image off the native clipboard as PNG bytes.
///
/// Screenshot tools put `image/png` on the system clipboard, which the
/// webview's paste event never surfaces as a file, so `clipboardData.files`
/// stays empty and the paste looks like a no-op. This is the fallback for
/// that: callers only reach it when the paste event carried no file, so a
/// clipboard without an image just reports the empty clipboard.
#[tauri::command]
pub async fn clipboard_image() -> Result<tauri::ipc::Response, String> {
    let bytes = tauri::async_runtime::spawn_blocking(clipboard_png)
        .await
        .map_err(|e| e.to_string())??;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Screenshots are decoded to RGBA before they reach us, so the clipboard
/// image is bounded by pixels here and by encoded size afterwards.
const MAX_CLIPBOARD_PIXELS: u64 = 100_000_000;

fn clipboard_png() -> Result<Vec<u8>, String> {
    let image = arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_image())
        .map_err(|error| error.to_string())?;
    let (width, height) = u32::try_from(image.width)
        .ok()
        .zip(u32::try_from(image.height).ok())
        .ok_or("The clipboard does not contain an image.")?;
    let png = encode_png(width, height, &image.bytes)?;
    // Guard the encoded size, not the pixels: a 4K screenshot is 33 MB of RGBA
    // but only a few MB of PNG, and that is what the harness receives.
    if png.len() as u64 > crate::fs::MAX_ATTACHMENT_EMBED_BYTES {
        return Err(format!(
            "Clipboard image is too large to attach (maximum {} MB).",
            crate::fs::MAX_ATTACHMENT_EMBED_BYTES / 1024 / 1024
        ));
    }
    Ok(png)
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 {
        return Err("The clipboard does not contain an image.".into());
    }
    if u64::from(width) * u64::from(height) > MAX_CLIPBOARD_PIXELS {
        return Err("Clipboard image has too many pixels to attach.".into());
    }
    if rgba.len() != width as usize * height as usize * 4 {
        return Err("The clipboard image could not be read.".into());
    }

    // arboard hands back decoded pixels, so the clipboard's original encoding
    // is gone; PNG keeps the screenshot lossless for the harness.
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
        writer
            .write_image_data(rgba)
            .map_err(|error| error.to_string())?;
    }
    Ok(png)
}

#[cfg(test)]
mod png_tests {
    use super::{encode_png, MAX_CLIPBOARD_PIXELS};
    use std::io::Cursor;

    #[test]
    fn encodes_rgba_pixels_as_a_png() {
        let png = encode_png(1, 1, &[10, 20, 30, 255]).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        let reader = png::Decoder::new(Cursor::new(&png)).read_info().unwrap();
        let info = reader.info();
        assert_eq!((info.width, info.height), (1, 1));
    }

    #[test]
    fn keeps_pixels_lossless() {
        let rgba = [0x00, 0x7f, 0xff, 0x80];
        let png = encode_png(1, 1, &rgba).unwrap();
        let mut reader = png::Decoder::new(Cursor::new(&png)).read_info().unwrap();
        let mut out = vec![0; 4];
        reader.next_frame(&mut out).unwrap();
        assert_eq!(out, rgba);
    }

    #[test]
    fn rejects_an_empty_or_malformed_image() {
        assert!(encode_png(0, 1, &[]).is_err());
        assert!(encode_png(1, 1, &[]).is_err());
        assert!(encode_png(2, 2, &[0; 8]).is_err());
    }

    #[test]
    fn rejects_an_image_wide_enough_to_exhaust_memory() {
        let side = (MAX_CLIPBOARD_PIXELS as f64).sqrt() as u32 + 1;
        assert!(encode_png(side, side, &[]).is_err());
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{file_paths_from, write_file_to};
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypeString};
    use objc2_foundation::NSString;
    use std::path::Path;

    #[test]
    fn reads_file_urls_from_a_private_pasteboard() {
        let pb = NSPasteboard::pasteboardWithUniqueName();
        pb.clearContents();
        let ok = pb.setString_forType(
            &NSString::from_str("file:///tmp/finder%20copy.txt"),
            unsafe { NSPasteboardTypeFileURL },
        );
        assert!(ok);
        assert_eq!(
            file_paths_from(&pb),
            vec!["/tmp/finder copy.txt".to_string()]
        );
    }

    #[test]
    fn ignores_pasteboards_without_file_urls() {
        let pb = NSPasteboard::pasteboardWithUniqueName();
        pb.clearContents();
        pb.setString_forType(&NSString::from_str("hello"), unsafe {
            NSPasteboardTypeString
        });
        assert!(file_paths_from(&pb).is_empty());
    }

    #[test]
    fn writes_original_file_url_to_a_private_pasteboard() {
        let pb = NSPasteboard::pasteboardWithUniqueName();
        write_file_to(&pb, Path::new("/tmp/original image.png")).unwrap();
        assert_eq!(
            file_paths_from(&pb),
            vec!["/tmp/original image.png".to_string()]
        );
    }
}
