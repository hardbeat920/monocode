//! The webview only sees text on paste. Finder puts file URLs on the native
//! pasteboard as `public.file-url` items, one per file.

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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::file_paths_from;
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypeString};
    use objc2_foundation::NSString;

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
}
