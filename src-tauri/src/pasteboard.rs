//! The webview only sees text on paste. Finder puts file URLs on the native
//! pasteboard as `public.file-url` items, one per file.

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

#[tauri::command]
pub fn clipboard_file_paths() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        file_paths_from(&objc2_app_kit::NSPasteboard::generalPasteboard())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
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
