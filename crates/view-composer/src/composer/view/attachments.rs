//! Attachments: `addAttachments`, `removeAttachment`, `attachFromPicker`,
//! the textarea's `onPaste`, and the drop handlers from Composer.tsx, with
//! the native clipboard reads of src/platform/tauri/clipboard.ts.
//!
//! GPUI's clipboard already carries copied file paths and images on macOS.
//! `monocode-platform`'s pasteboard is the fallback for a `file://` paste
//! and for a screenshot the GPUI read did not surface.

use gpui::{AppContext as _, ClipboardEntry, Context, ExternalPaths, Task, Window};
use monocode_core::Attachment;
use monocode_core::attachment::{MAX_ATTACHMENTS, merge_attachments};

use super::super::model::clipboard::{
    CLIPBOARD_IMAGE_NAME, ClipboardFile, NO_CLIPBOARD_IMAGE, clipboard_path_batch,
    is_file_reference_text, message_files_from_metadata, nothing_to_attach_message,
    too_many_copied_files_message,
};
use super::super::prompt_input::{Paste, normalize_newlines};
use super::Composer;

/// What the native clipboard held when GPUI's read had no files.
enum NativePaste {
    Paths(Vec<String>),
    Image(Vec<u8>),
    Nothing,
}

impl Composer {
    /// `addAttachments`.
    pub fn add_attachments(
        &mut self,
        incoming: Vec<Attachment>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !monocode_core::harness::harness_supports_attachments(self.props.harness)
            || incoming.is_empty()
        {
            return;
        }
        self.attachments = merge_attachments(&self.attachments, &incoming);
        self.paste_error = None;
        self.draft_revision += 1;
        self.sync_has_value();
        self.focus(window, cx);
        cx.notify();
    }

    /// `removeAttachment`.
    pub fn remove_attachment(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .attachment_preview
            .as_ref()
            .is_some_and(|preview| preview.id == id)
        {
            self.close_attachment_preview(window, cx);
        }
        if let Some(index) = self.attachments.iter().position(|file| file.id == id) {
            let removed = self.attachments.remove(index);
            if !self.borrowed_attachment_ids.remove(&removed.id) {
                self.host.revoke_attachment(&removed, cx);
            }
        }
        self.draft_revision += 1;
        self.paste_error = None;
        self.sync_has_value();
        self.focus(window, cx);
        cx.notify();
    }

    /// `attachFromPicker`: the + menu's Upload file.
    pub(crate) fn attach_from_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.attachments_supported() {
            return;
        }
        let task = self.host.pick_attachments(window, cx);
        self.attach_when_ready(task, None, window, cx);
    }

    /// Files dropped on the composer or its session pane
    /// (`attachmentsFromPaths`).
    pub fn drop_paths(&mut self, paths: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        self.file_drag = false;
        if !self.props.enabled
            || self.props.disabled
            || !self.attachments_supported()
            || paths.is_empty()
        {
            cx.notify();
            return;
        }
        let task = self.host.attachments_from_paths(paths, cx);
        self.attach_when_ready(task, None, window, cx);
    }

    /// Shows or hides the "Drop files to attach" overlay, for an owner that
    /// tracks drags over the whole session pane.
    pub fn set_file_drag(&mut self, over: bool, cx: &mut Context<Self>) {
        let next =
            over && self.props.enabled && !self.props.disabled && self.attachments_supported();
        if next != self.file_drag {
            self.file_drag = next;
            cx.notify();
        }
    }

    pub(crate) fn on_external_drop(
        &mut self,
        paths: &ExternalPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = paths
            .paths()
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        self.drop_paths(paths, window, cx);
    }

    /// Adds what `task` yields unless the draft was retired meanwhile, in
    /// which case the files are released.
    fn attach_when_ready(
        &mut self,
        task: Task<Vec<Attachment>>,
        generation: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let task = cx.spawn_in(window, async move |this, cx| {
            let pasted = task.await;
            this.update_in(cx, |this, window, cx| {
                if generation.is_some_and(|generation| generation != this.paste_generation) {
                    for file in &pasted {
                        this.host.revoke_attachment(file, cx);
                    }
                } else {
                    this.add_attachments(pasted, window, cx);
                }
            })
            .ok();
        });
        self._tasks.push(task);
    }

    /// Tracks a paste so Send waits for it (`rememberPaste`).
    fn remember_paste(&mut self, task: Task<()>) {
        self.pastes_in_flight += 1;
        self._tasks.push(task);
    }

    /// `onPaste`, captured before the prompt's own paste. Returns true when
    /// the composer took the paste.
    pub(crate) fn on_paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if self.props.disabled || self.prompt.read(cx).is_composing() {
            return;
        }
        if self.paste(window, cx) {
            cx.stop_propagation();
        }
    }

    pub(crate) fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.paste_error = None;
        let item = cx.read_from_clipboard();
        let text = item
            .as_ref()
            .and_then(|item| item.text())
            .map(|text| normalize_newlines(&text))
            .unwrap_or_default();

        // A message copied in MonoCode carries its files with it.
        let message_files = item
            .as_ref()
            .and_then(|item| {
                item.entries().iter().find_map(|entry| match entry {
                    ClipboardEntry::String(string) => string.metadata.clone(),
                    _ => None,
                })
            })
            .and_then(|metadata| message_files_from_metadata(&metadata));
        if let Some(files) = message_files {
            if !text.is_empty() {
                self.prompt
                    .update(cx, |prompt, cx| prompt.insert(&text, cx));
            }
            if !self.attachments_supported() {
                return true;
            }
            self.paste_files(files, window, cx);
            return true;
        }

        let paths: Vec<String> = item
            .as_ref()
            .map(|item| {
                item.entries()
                    .iter()
                    .filter_map(|entry| match entry {
                        ClipboardEntry::ExternalPaths(paths) => Some(
                            paths
                                .paths()
                                .iter()
                                .map(|path| path.to_string_lossy().into_owned())
                                .collect::<Vec<_>>(),
                        ),
                        _ => None,
                    })
                    .flatten()
                    .collect()
            })
            .unwrap_or_default();
        let images: Vec<ClipboardFile> = item
            .as_ref()
            .map(|item| {
                item.entries()
                    .iter()
                    .filter_map(|entry| match entry {
                        ClipboardEntry::Image(image) => Some(ClipboardFile {
                            name: image_name(image.format),
                            mime_type: image.format.mime_type().to_string(),
                            bytes: image.bytes.clone(),
                        }),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();

        if !paths.is_empty() {
            if self.attachments_supported() {
                self.paste_paths(paths, None, window, cx);
            }
            return true;
        }
        if !images.is_empty() {
            if self.attachments_supported() {
                self.paste_files(images, window, cx);
            }
            return true;
        }

        // GPUI saw text only. A screenshot or a file manager copy can still
        // live on the native clipboard.
        if !self.attachments_supported() {
            return false;
        }
        if !text.is_empty() && !is_file_reference_text(&text) {
            return false;
        }
        let captured = is_file_reference_text(&text).then(|| {
            let prompt = self.prompt.read(cx);
            (prompt.text().to_string(), prompt.selection())
        });
        let generation = self.paste_generation;
        let want_image = text.is_empty();
        let read = cx.background_spawn(async move {
            match monocode_platform::pasteboard::clipboard_file_paths() {
                Ok(paths) => {
                    let paths: Vec<String> = paths
                        .into_iter()
                        .filter(|path| !path.trim().is_empty())
                        .collect();
                    if !paths.is_empty() {
                        return Ok(NativePaste::Paths(paths));
                    }
                }
                Err(error) => return Err(error),
            }
            if !want_image {
                return Ok(NativePaste::Nothing);
            }
            match monocode_platform::pasteboard::clipboard_image() {
                Ok(bytes) if !bytes.is_empty() => Ok(NativePaste::Image(bytes)),
                Ok(_) => Ok(NativePaste::Nothing),
                Err(error) if error == NO_CLIPBOARD_IMAGE => Ok(NativePaste::Nothing),
                Err(error) => Err(error),
            }
        });
        let task = cx.spawn_in(window, async move |this, cx| {
            let result = read.await;
            this.update_in(cx, |this, window, cx| {
                if this.paste_generation != generation {
                    this.paste_settled(window, cx);
                    return;
                }
                match result {
                    Ok(NativePaste::Paths(paths)) => {
                        this.paste_paths(paths, captured.map(|c| (c, text.clone())), window, cx);
                    }
                    Ok(NativePaste::Image(bytes)) => {
                        this.paste_files(
                            vec![ClipboardFile {
                                name: CLIPBOARD_IMAGE_NAME.into(),
                                mime_type: "image/png".into(),
                                bytes,
                            }],
                            window,
                            cx,
                        );
                    }
                    Ok(NativePaste::Nothing) => {
                        if let Some(captured) = captured {
                            this.insert_restored_text(captured, &text, cx);
                        }
                    }
                    Err(error) => this.paste_error = Some(error),
                }
                this.paste_settled(window, cx);
                cx.notify();
            })
            .ok();
        });
        self.remember_paste(task);
        true
    }

    /// Pasted bytes become attachments (`attachmentsFromFiles`).
    fn paste_files(
        &mut self,
        files: Vec<ClipboardFile>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let generation = self.paste_generation;
        let read = self.host.attachments_from_files(files, cx);
        let task = cx.spawn_in(window, async move |this, cx| {
            let pasted = read.await;
            this.update_in(cx, |this, window, cx| {
                if this.paste_generation != generation {
                    for file in &pasted {
                        this.host.revoke_attachment(file, cx);
                    }
                } else {
                    this.add_attachments(pasted, window, cx);
                }
                this.paste_settled(window, cx);
            })
            .ok();
        });
        self.remember_paste(task);
    }

    /// Copied paths become attachments, filling one turn's quota with what
    /// the filesystem accepts (`attachmentsFromClipboardPaths`). A path that
    /// was moved yields nothing, so paths go a batch at a time. `captured`
    /// is the withheld `file://` text to put back if nothing attached.
    fn paste_paths(
        &mut self,
        paths: Vec<String>,
        captured: Option<((String, std::ops::Range<usize>), String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let generation = self.paste_generation;
        let host = self.host.clone();
        let task = cx.spawn_in(window, async move |this, cx| {
            let mut files: Vec<Attachment> = Vec::new();
            let mut consumed = 0;
            while consumed < paths.len() && files.len() < MAX_ATTACHMENTS {
                let batch = clipboard_path_batch(&paths, consumed).to_vec();
                let count = batch.len();
                let Ok(read) = cx.update(|_, cx| host.attachments_from_paths(batch, cx)) else {
                    return;
                };
                for file in read.await {
                    if files.len() >= MAX_ATTACHMENTS {
                        break;
                    }
                    files.push(file);
                }
                consumed += count;
            }
            this.update_in(cx, |this, window, cx| {
                if this.paste_generation != generation {
                    for file in &files {
                        this.host.revoke_attachment(file, cx);
                    }
                    this.paste_settled(window, cx);
                    return;
                }
                if files.is_empty() {
                    this.paste_error = Some(nothing_to_attach_message(paths.len()));
                    if let Some((captured, text)) = captured {
                        this.insert_restored_text(captured, &text, cx);
                    }
                } else {
                    let attached = files.len();
                    this.add_attachments(files, window, cx);
                    if consumed < paths.len() {
                        this.paste_error =
                            Some(too_many_copied_files_message(attached, paths.len()));
                    }
                }
                this.paste_settled(window, cx);
                cx.notify();
            })
            .ok();
        });
        self.remember_paste(task);
    }

    /// `insertRestoredText`: insert at the captured range, or at the caret if
    /// the draft moved on.
    fn insert_restored_text(
        &mut self,
        captured: (String, std::ops::Range<usize>),
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let (value, range) = captured;
        self.prompt.update(cx, |prompt, cx| {
            if prompt.text() == value {
                prompt.replace_range(range, text, cx);
            } else {
                prompt.insert(text, cx);
            }
        });
    }
}

fn image_name(format: gpui::ImageFormat) -> String {
    match format {
        gpui::ImageFormat::Png => CLIPBOARD_IMAGE_NAME.into(),
        other => {
            let ext = other.mime_type().rsplit('/').next().unwrap_or("png");
            format!("clipboard-image.{ext}")
        }
    }
}
