//! The webview only sees text on paste. Finder puts file URLs on the native
//! pasteboard as `public.file-url` items, one per file, and screenshot tools
//! put image data there, so pasted images are read from the native clipboard
//! instead of the paste event.
//!
//! Moved from src-tauri/src/pasteboard.rs.

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

/// A Wayland clipboard read through the normal data-device offer.
///
/// arboard's data-control protocol is not implemented by Mutter, so on GNOME
/// it falls back to X11 and misses the Wayland clipboard entirely. `wl-paste`
/// reads the offer a focused client is allowed to see.
enum WlPaste {
    /// Bytes of the requested type.
    Got(Vec<u8>),
    /// `wl-paste` ran, and the clipboard has no such type.
    Empty,
    /// Not a Wayland session, or `wl-paste` is not installed.
    Unavailable,
    /// The clipboard could not be read.
    Failed,
}

fn wl_paste(mime: &str) -> WlPaste {
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return WlPaste::Unavailable;
    }
    let mut child = match std::process::Command::new("wl-paste")
        .args(["--no-newline", "--type", mime])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return WlPaste::Unavailable,
        Err(_) => return WlPaste::Failed,
    };
    let mut stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => return WlPaste::Failed,
    };
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 16 * 1024];
        loop {
            if buf.len() as u64 > crate::MAX_ATTACHMENT_EMBED_BYTES {
                return Err(());
            }
            match std::io::Read::read(&mut stdout, &mut chunk) {
                Ok(0) => return Ok(buf),
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(_) => return Err(()),
            }
        }
    });
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let bytes = reader.join().unwrap_or(Err(()));
                return match bytes {
                    Err(()) => WlPaste::Failed,
                    Ok(bytes) if status.success() => WlPaste::Got(bytes),
                    Ok(_) => WlPaste::Empty,
                };
            }
            Ok(None) if started.elapsed() < std::time::Duration::from_secs(2) => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return WlPaste::Failed;
            }
            Err(_) => return WlPaste::Failed,
        }
    }
}

/// File paths from a `text/uri-list`, skipping comments and non-file URIs.
///
/// macOS reads `public.file-url` instead, so this stays out of that library
/// build. `cargo clippy --all-targets` still typechecks it via the tests.
#[cfg(any(test, not(target_os = "macos")))]
fn paths_from_uri_list(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let paths = text
        .split(['\n', '\r'])
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| url::Url::parse(line).ok())
        .filter(|url| url.scheme() == "file")
        .filter_map(|url| url.to_file_path().ok())
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    clean_clipboard_paths(paths)
}

#[cfg(not(target_os = "macos"))]
enum WlPaths {
    Found(Vec<String>),
    Unavailable,
    Failed,
}

#[cfg(not(target_os = "macos"))]
fn wayland_file_paths() -> WlPaths {
    match wl_paste("text/uri-list") {
        WlPaste::Got(bytes) => WlPaths::Found(paths_from_uri_list(&bytes)),
        WlPaste::Empty => WlPaths::Found(Vec::new()),
        WlPaste::Unavailable => WlPaths::Unavailable,
        WlPaste::Failed => WlPaths::Failed,
    }
}

/// Paths for files copied in a file manager, empty when it holds none.
///
/// A file manager puts a URI list on the clipboard (`text/uri-list` on X11 and
/// Wayland, `public.file-url` on macOS, `CF_HDROP` on Windows) that a paste
/// event does not surface, so the webview sees nothing to attach.
///
/// An empty clipboard is an empty list, not a failure: only a clipboard that
/// cannot be read at all is an error, so the caller can tell "nothing was
/// copied" from "the clipboard is unreachable" without inspecting a message.
pub fn clipboard_file_paths() -> Result<Vec<String>, String> {
    #[cfg(target_os = "macos")]
    {
        Ok(clean_clipboard_paths(file_paths_from(
            &objc2_app_kit::NSPasteboard::generalPasteboard(),
        )))
    }
    #[cfg(not(target_os = "macos"))]
    {
        // A missing uri-list is an empty clipboard. Falling through to X11
        // would read a different clipboard than the one the user copied from.
        match wayland_file_paths() {
            WlPaths::Found(paths) => return Ok(paths),
            WlPaths::Failed => {
                return Err("The clipboard could not be read on this system.".into());
            }
            WlPaths::Unavailable => {}
        }
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get().file_list()) {
            Ok(paths) => Ok(clean_clipboard_paths(paths)),
            Err(arboard::Error::ContentNotAvailable) => Ok(Vec::new()),
            Err(_) => Err("The clipboard could not be read on this system.".into()),
        }
    }
}

/// Drop the line terminator a URI list leaves on every path.
///
/// A file manager writes `text/uri-list` CRLF-terminated and arboard splits on
/// `\n` without trimming, so each path arrives as `/home/me/a.pdf\r` and
/// matches no file on disk.
fn clean_clipboard_paths<T: AsRef<std::path::Path>>(paths: Vec<T>) -> Vec<String> {
    paths
        .iter()
        .map(|path| {
            path.as_ref()
                .to_string_lossy()
                .trim_end_matches(['\r', '\n'])
                .to_string()
        })
        .filter(|path| !path.is_empty())
        .collect()
}

pub fn copy_file_to_clipboard(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let path = crate::expand_home(&path);
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
pub fn clipboard_image() -> Result<Vec<u8>, String> {
    clipboard_png()
}

/// Bounds the PNG encode, not the decode: arboard has already turned the
/// clipboard bytes into RGBA by the time we see them, so this only rejects an
/// image too large to re-encode. 40 MP covers an 8000x5000 capture.
const MAX_CLIPBOARD_PIXELS: u64 = 40_000_000;

const EMPTY_CLIPBOARD_IMAGE: &str = "The clipboard does not contain an image.";

fn clipboard_png() -> Result<Vec<u8>, String> {
    match wl_paste("image/png") {
        WlPaste::Got(bytes) if bytes.is_empty() => {
            return Err(EMPTY_CLIPBOARD_IMAGE.into());
        }
        WlPaste::Got(bytes) => return bounded_png(bytes),
        WlPaste::Empty => return Err(EMPTY_CLIPBOARD_IMAGE.into()),
        WlPaste::Failed => {
            return Err("The clipboard could not be read on this system.".into());
        }
        WlPaste::Unavailable => {}
    }
    let image = match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_image()) {
        Ok(image) => image,
        // An empty clipboard is not a failure. The raw arboard message is not
        // something to put in the composer.
        Err(arboard::Error::ContentNotAvailable) => return Err(EMPTY_CLIPBOARD_IMAGE.into()),
        Err(_) => return Err("The clipboard could not be read on this system.".into()),
    };
    let (width, height) = u32::try_from(image.width)
        .ok()
        .zip(u32::try_from(image.height).ok())
        .ok_or(EMPTY_CLIPBOARD_IMAGE)?;
    let png = encode_png(width, height, &image.bytes)?;
    // Guard the encoded size, not the pixels: a 4K screenshot is 33 MB of RGBA
    // but only a few MB of PNG, and that is what the harness receives.
    bounded_png(png)
}

fn bounded_png(png: Vec<u8>) -> Result<Vec<u8>, String> {
    if png.len() as u64 > crate::MAX_ATTACHMENT_EMBED_BYTES {
        return Err(format!(
            "Clipboard image is too large to attach (maximum {} MB).",
            crate::MAX_ATTACHMENT_EMBED_BYTES / 1024 / 1024
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
mod path_tests {
    use super::clean_clipboard_paths;
    // `Url::to_file_path` rejects `file:///home/...` on Windows.
    #[cfg(unix)]
    use super::paths_from_uri_list;

    /// What arboard hands back for a `text/uri-list` copied in a file manager.
    #[test]
    fn drops_the_carriage_return_a_uri_list_leaves_behind() {
        assert_eq!(
            clean_clipboard_paths(vec![
                "/home/dev/All_BTech_Affiliated_2022_23.pdf\r",
                "/home/dev/notes.md\r\n",
            ]),
            vec![
                "/home/dev/All_BTech_Affiliated_2022_23.pdf".to_string(),
                "/home/dev/notes.md".to_string(),
            ]
        );
    }

    #[test]
    fn keeps_spaces_that_belong_to_the_name() {
        assert_eq!(
            clean_clipboard_paths(vec!["/home/dev/My Report.pdf\r"]),
            vec!["/home/dev/My Report.pdf".to_string()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn reads_a_uri_list_the_wayland_clipboard_publishes() {
        assert_eq!(
            paths_from_uri_list(
                b"file:///home/dev/My%20Report.pdf\r\n# comment\nfile:///home/dev/notes.md\nhttps://example.com/x\n"
            ),
            vec![
                "/home/dev/My Report.pdf".to_string(),
                "/home/dev/notes.md".to_string(),
            ]
        );
    }

    #[test]
    fn drops_empty_entries() {
        assert!(clean_clipboard_paths(vec!["\r", "\n", ""]).is_empty());
    }

    /// The chain a file-manager copy takes: the path arboard hands over, through
    /// the cleaner, to something the filesystem can stat. Without the carriage
    /// return trimmed this resolves to nothing.
    #[test]
    fn a_uri_list_line_resolves_to_the_file_it_names() {
        let dir = std::env::temp_dir().join(format!("monocode-uri-list-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("All_BTech_Affiliated_2022_23.pdf");
        std::fs::write(&file, b"%PDF-1.7").unwrap();

        // arboard strips the `file://` prefix and percent-decodes, then leaves
        // the CRLF terminator attached.
        let from_clipboard = format!("{}\r\n", file.to_string_lossy());
        let infos = monocode_git::fs::inspect_paths(clean_clipboard_paths(vec![from_clipboard]));

        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(
            infos.len(),
            1,
            "a copied file must resolve to an attachment"
        );
        assert_eq!(infos[0].name, "All_BTech_Affiliated_2022_23.pdf");
        assert!(!infos[0].is_dir);
    }
}

#[cfg(test)]
mod png_tests {
    use super::{MAX_CLIPBOARD_PIXELS, encode_png};
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
