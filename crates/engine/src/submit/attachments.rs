//! Port of the IO half of src/features/sessions/model/attachments.ts: build
//! attachments from paths and pasted bytes, and load vision images before a
//! turn is sent. The pure half lives in `monocode_core::attachment`.
//!
//! The file picker and the clipboard belong to the composer view. It passes
//! the paths it picked to [`attachments_from_paths`] and pasted bytes to
//! [`attachment_from_bytes`]. Object URLs do not exist here, so a pasted
//! image has no `preview_url`; the view draws it from `data`.

use std::collections::HashSet;
use std::sync::Arc;

use base64::Engine as _;
use futures::FutureExt;
use futures::future::BoxFuture;
use monocode_core::attachment::{
    Attachment, AttachmentKind, FOLDER_MIME, MAX_EMBED_BYTES, fallback_name, is_vision_image,
    kind_from_mime, mime_from_file, mime_from_name, normalize_image_mime, skip_name,
};
use monocode_core::js;

/// `PathInfo` from `inspect_paths`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathInfo {
    pub path: String,
    pub name: String,
    pub size: i64,
    pub is_dir: bool,
}

/// The three backend calls attachments make.
pub trait AttachmentIo: Send + Sync {
    /// `inspect_paths`: metadata for paths that exist.
    fn inspect_paths(
        &self,
        paths: Vec<String>,
    ) -> BoxFuture<'static, Result<Vec<PathInfo>, String>>;
    /// `read_file_base64`.
    fn read_file_base64(&self, path: String) -> BoxFuture<'static, Result<String, String>>;
    /// `write_attachment`: persist a pasted blob and return its path.
    fn write_attachment(
        &self,
        name: String,
        data: String,
    ) -> BoxFuture<'static, Result<String, String>>;
}

/// The real IO over `monocode_git::fs`, on smol's blocking pool.
pub struct LocalAttachmentIo;

impl AttachmentIo for LocalAttachmentIo {
    fn inspect_paths(
        &self,
        paths: Vec<String>,
    ) -> BoxFuture<'static, Result<Vec<PathInfo>, String>> {
        smol::unblock(move || {
            Ok(monocode_git::fs::inspect_paths(paths)
                .into_iter()
                .map(|info| PathInfo {
                    path: info.path,
                    name: info.name,
                    size: info.size as i64,
                    is_dir: info.is_dir,
                })
                .collect())
        })
        .boxed()
    }

    fn read_file_base64(&self, path: String) -> BoxFuture<'static, Result<String, String>> {
        smol::unblock(move || monocode_git::fs::read_file_base64(path)).boxed()
    }

    fn write_attachment(
        &self,
        name: String,
        data: String,
    ) -> BoxFuture<'static, Result<String, String>> {
        smol::unblock(move || monocode_git::fs::write_attachment(name, data)).boxed()
    }
}

/// The default IO.
pub fn local_io() -> Arc<dyn AttachmentIo> {
    Arc::new(LocalAttachmentIo)
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// `attachmentsFromPaths`: unique, non-blank paths become attachments. Small
/// vision images are read inline.
pub async fn attachments_from_paths(
    io: &dyn AttachmentIo,
    paths: &[String],
) -> Result<Vec<Attachment>, String> {
    let mut seen = HashSet::new();
    let unique: Vec<String> = paths
        .iter()
        .filter(|path| !js::trim(path).is_empty())
        .filter(|path| seen.insert(path.as_str()))
        .cloned()
        .collect();
    if unique.is_empty() {
        return Ok(Vec::new());
    }
    let infos = io.inspect_paths(unique).await?;
    let mut out = Vec::new();
    for info in infos {
        if let Some(file) = attachment_from_path(io, info).await {
            out.push(file);
        }
    }
    Ok(out)
}

async fn attachment_from_path(io: &dyn AttachmentIo, info: PathInfo) -> Option<Attachment> {
    if skip_name(&info.name) {
        return None;
    }
    // A folder's name says nothing about its contents, and a harness handed a
    // resource_link for a directory has nothing to open.
    let mime_type = if info.is_dir {
        FOLDER_MIME.to_string()
    } else {
        mime_from_name(&info.name)
    };
    let kind = kind_from_mime(&mime_type);
    let mut file = Attachment {
        id: new_id(),
        name: info.name,
        mime_type: mime_type.clone(),
        kind,
        size: info.size,
        path: Some(info.path.clone()),
        ..Attachment::default()
    };
    if is_vision_image(&mime_type) && info.size > 0 && info.size <= MAX_EMBED_BYTES {
        // On failure the file falls back to a resource_link the agent can read.
        if let Ok(data) = io.read_file_base64(info.path).await {
            file.data = Some(data);
        }
    }
    Some(file)
}

/// A file from the clipboard or a drop that has no path: its name, the type
/// the platform reported, and its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PastedFile {
    pub name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

/// `attachmentFromBlob`: vision images travel inline; anything else is
/// written to a temp file so the agent gets a path.
pub async fn attachment_from_bytes(io: &dyn AttachmentIo, file: &PastedFile) -> Option<Attachment> {
    if skip_name(&file.name) {
        return None;
    }
    let mime_type = mime_from_file(&file.name, &file.mime_type);
    let kind = kind_from_mime(&mime_type);
    let trimmed = js::trim(&file.name);
    let name = if trimmed.is_empty() {
        fallback_name(&mime_type).to_string()
    } else {
        trimmed.to_string()
    };
    let size = file.bytes.len() as i64;
    // `readBlobBase64` gives up on files over the inline limit.
    let data = (size <= MAX_EMBED_BYTES)
        .then(|| base64::engine::general_purpose::STANDARD.encode(&file.bytes));
    if kind == AttachmentKind::Image && is_vision_image(&mime_type) && data.is_some() {
        return Some(Attachment {
            id: new_id(),
            name,
            mime_type: normalize_image_mime(&mime_type),
            kind,
            size,
            data,
            ..Attachment::default()
        });
    }
    let data = data?;
    let path = io.write_attachment(name.clone(), data).await.ok()?;
    Some(Attachment {
        id: new_id(),
        name,
        mime_type,
        kind,
        size,
        path: Some(path),
        ..Attachment::default()
    })
}

/// `attachmentsFromFiles`: files with a native path go through
/// [`attachments_from_paths`]; the rest are pasted bytes.
pub async fn attachments_from_files(
    io: &dyn AttachmentIo,
    paths: &[String],
    pasted: &[PastedFile],
) -> Result<Vec<Attachment>, String> {
    let mut out = Vec::new();
    let native: Vec<String> = paths
        .iter()
        .filter(|path| !js::trim(path).is_empty())
        .cloned()
        .collect();
    if !native.is_empty() {
        out.extend(attachments_from_paths(io, &native).await?);
    }
    for file in pasted {
        if let Some(item) = attachment_from_bytes(io, file).await {
            out.push(item);
        }
    }
    Ok(out)
}

/// `prepareAttachments`: read the bytes of small vision images that only
/// have a path, so they travel inline. Anything else is sent as it is.
pub async fn prepare_attachments(io: &dyn AttachmentIo, files: &[Attachment]) -> Vec<Attachment> {
    let reads = files.iter().map(|file| async move {
        let has_data = file.data.as_deref().is_some_and(|data| !data.is_empty());
        let Some(path) = file.path.clone().filter(|path| !path.is_empty()) else {
            return file.clone();
        };
        if has_data || !is_vision_image(&file.mime_type) || file.size > MAX_EMBED_BYTES {
            return file.clone();
        }
        match io.read_file_base64(path).await {
            Ok(data) => Attachment {
                data: Some(data),
                ..file.clone()
            },
            Err(_) => file.clone(),
        }
    });
    futures::future::join_all(reads).await
}

/// A clipboard entry: what `filesFromClipboard` reads from a `File`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardFile {
    pub name: String,
    pub mime_type: String,
}

/// `filesFromClipboard`: the longer of the file list and the file items
/// (WebKit often truncated the list to its first file), without the unnamed
/// TIFF twin macOS adds next to a screenshot.
pub fn files_from_clipboard<T: Clone>(
    list: &[T],
    items: &[T],
    describe: impl Fn(&T) -> ClipboardFile,
) -> Vec<T> {
    let files = if items.len() > list.len() {
        items
    } else {
        list
    };
    drop_mac_screenshot_twins(files, describe)
}

fn drop_mac_screenshot_twins<T: Clone>(
    files: &[T],
    describe: impl Fn(&T) -> ClipboardFile,
) -> Vec<T> {
    let unnamed_tiff = |file: &ClipboardFile| {
        let mime = file.mime_type.to_lowercase();
        if mime != "image/tiff" && mime != "image/tif" {
            return false;
        }
        let name = js::trim(&file.name).to_lowercase();
        name.is_empty() || name == "image.tiff" || name == "image.tif" || name == "image"
    };
    let has_other_image = files.iter().any(|file| {
        let file = describe(file);
        file.mime_type.starts_with("image/") && !unnamed_tiff(&file)
    });
    if !has_other_image {
        return files.to_vec();
    }
    files
        .iter()
        .filter(|file| !unnamed_tiff(&describe(file)))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    /// The `invoke` mock: canned results and a call log.
    #[derive(Default)]
    struct FakeIo {
        infos: Vec<PathInfo>,
        read: Option<Result<String, String>>,
        written: Option<String>,
        calls: Mutex<Vec<(String, String)>>,
    }

    impl AttachmentIo for FakeIo {
        fn inspect_paths(
            &self,
            paths: Vec<String>,
        ) -> BoxFuture<'static, Result<Vec<PathInfo>, String>> {
            self.calls
                .lock()
                .push(("inspect_paths".into(), paths.join(",")));
            futures::future::ready(Ok(self.infos.clone())).boxed()
        }

        fn read_file_base64(&self, path: String) -> BoxFuture<'static, Result<String, String>> {
            self.calls.lock().push(("read_file_base64".into(), path));
            let result = self.read.clone().unwrap_or(Err("read failed".into()));
            futures::future::ready(result).boxed()
        }

        fn write_attachment(
            &self,
            name: String,
            data: String,
        ) -> BoxFuture<'static, Result<String, String>> {
            self.calls
                .lock()
                .push(("write_attachment".into(), format!("{name}:{data}")));
            futures::future::ready(
                self.written
                    .clone()
                    .ok_or_else(|| "write failed".to_string()),
            )
            .boxed()
        }
    }

    fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
        futures::executor::block_on(future)
    }

    // attachmentsPreparation.test.ts
    #[test]
    fn persists_a_pasted_file_before_handing_it_to_the_harness() {
        for (name, mime) in [
            ("report.pdf", "application/pdf"),
            ("transcript.md", "text/markdown"),
            ("server.log", "text/plain"),
        ] {
            let path = format!("/tmp/monocode-attachments/{name}");
            let io = FakeIo {
                written: Some(path.clone()),
                ..FakeIo::default()
            };
            let pasted = PastedFile {
                name: name.into(),
                mime_type: mime.into(),
                bytes: b"contents".to_vec(),
            };
            let files = block_on(attachments_from_files(&io, &[], &[pasted])).unwrap();
            assert_eq!(
                io.calls.lock()[0],
                (
                    "write_attachment".to_string(),
                    format!("{name}:Y29udGVudHM=")
                )
            );
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].name, name);
            assert_eq!(files[0].mime_type, mime);
            assert_eq!(files[0].kind, AttachmentKind::File);
            assert_eq!(files[0].path.as_deref(), Some(path.as_str()));
            assert_eq!(files[0].data, None);
            assert_eq!(block_on(prepare_attachments(&io, &files)), files);
            assert_eq!(
                monocode_core::attachment::persistable_attachment(&files[0])
                    .path
                    .as_deref(),
                Some(path.as_str())
            );
            assert_eq!(io.calls.lock().len(), 1);
        }
    }

    #[test]
    fn keeps_large_disk_documents_as_paths_without_reading_them_into_the_prompt() {
        let io = FakeIo {
            infos: vec![PathInfo {
                path: "/tmp/large.md".into(),
                name: "large.md".into(),
                size: MAX_EMBED_BYTES + 1,
                is_dir: false,
            }],
            ..FakeIo::default()
        };
        let files = block_on(attachments_from_paths(&io, &["/tmp/large.md".into()])).unwrap();
        let files = block_on(prepare_attachments(&io, &files));
        assert_eq!(files[0].path.as_deref(), Some("/tmp/large.md"));
        assert_eq!(files[0].mime_type, "text/markdown");
        assert_eq!(files[0].data, None);
        assert_eq!(io.calls.lock().len(), 1);
    }

    #[test]
    fn retains_the_image_path_when_byte_loading_fails() {
        let io = FakeIo {
            infos: vec![PathInfo {
                path: "/tmp/image.png".into(),
                name: "image.png".into(),
                size: 100,
                is_dir: false,
            }],
            ..FakeIo::default()
        };
        let files = block_on(attachments_from_paths(&io, &["/tmp/image.png".into()])).unwrap();
        let files = block_on(prepare_attachments(&io, &files));
        assert_eq!(files[0].path.as_deref(), Some("/tmp/image.png"));
        assert_eq!(files[0].mime_type, "image/png");
        assert_eq!(files[0].data, None);
    }

    #[test]
    fn inlines_small_vision_images_and_marks_folders() {
        let io = FakeIo {
            infos: vec![
                PathInfo {
                    path: "/tmp/shot.png".into(),
                    name: "shot.png".into(),
                    size: 10,
                    is_dir: false,
                },
                PathInfo {
                    path: "/repo/src".into(),
                    name: "src".into(),
                    size: 0,
                    is_dir: true,
                },
                PathInfo {
                    path: "/tmp/.DS_Store".into(),
                    name: ".DS_Store".into(),
                    size: 1,
                    is_dir: false,
                },
            ],
            read: Some(Ok("AAAA".into())),
            ..FakeIo::default()
        };
        let files = block_on(attachments_from_paths(
            &io,
            &[
                "/tmp/shot.png".into(),
                "/tmp/shot.png".into(),
                " ".into(),
                "/repo/src".into(),
            ],
        ))
        .unwrap();
        assert_eq!(io.calls.lock()[0].1, "/tmp/shot.png,/repo/src");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].data.as_deref(), Some("AAAA"));
        assert_eq!(files[1].mime_type, FOLDER_MIME);
    }

    #[test]
    fn keeps_pasted_vision_images_inline() {
        let io = FakeIo::default();
        let pasted = PastedFile {
            name: String::new(),
            mime_type: "image/jpg".into(),
            bytes: vec![1, 2, 3],
        };
        let file = block_on(attachment_from_bytes(&io, &pasted)).unwrap();
        assert_eq!(file.name, "image.jpg");
        assert_eq!(file.mime_type, "image/jpeg");
        assert_eq!(file.data.as_deref(), Some("AQID"));
        assert!(io.calls.lock().is_empty());
    }

    // attachments.test.ts: filesFromClipboard
    fn clip(name: &str, mime: &str) -> ClipboardFile {
        ClipboardFile {
            name: name.into(),
            mime_type: mime.into(),
        }
    }

    #[test]
    fn returns_every_file_item_when_the_files_list_is_truncated() {
        let a = clip("a.png", "image/png");
        let b = clip("b.png", "image/png");
        assert_eq!(
            files_from_clipboard(
                std::slice::from_ref(&a),
                &[a.clone(), b.clone()],
                Clone::clone
            ),
            vec![a, b]
        );
    }

    #[test]
    fn drops_the_unnamed_tiff_twin_of_a_png_screenshot() {
        let png = clip("image.png", "image/png");
        let tiff = clip("image.tiff", "image/tiff");
        assert_eq!(
            files_from_clipboard(
                std::slice::from_ref(&png),
                &[png.clone(), tiff],
                Clone::clone
            ),
            vec![png]
        );
    }

    #[test]
    fn keeps_a_real_named_tiff_next_to_a_png() {
        let png = clip("diagram.png", "image/png");
        let tiff = clip("scan.tiff", "image/tiff");
        let both = vec![png, tiff];
        assert_eq!(files_from_clipboard(&both, &both, Clone::clone), both);
    }
}
