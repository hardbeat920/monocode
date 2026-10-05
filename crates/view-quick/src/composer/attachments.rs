//! Port of src/features/quick-composer/ui/useQuickAttachments.ts: pick,
//! paste, drop, and capture attachments; keep them through a dismiss;
//! release capture files the draft drops.
//!
//! One collection runs at a time. A screenshot pasted while one runs is read
//! from the clipboard at once, so a later clipboard change cannot replace
//! it, and attached when the running collection ends.

use std::rc::Rc;

use gpui::{
    App, AsyncWindowContext, ClipboardEntry, Context, ExternalPaths, Task, WeakEntity, Window,
};
use monocode_core::Attachment;
use monocode_view_composer::composer::model::clipboard::ClipboardFile;
use monocode_view_composer::composer::prompt_input::normalize_newlines;

use super::QuickComposer;
use crate::host::{HostTask, NativeClipboard, QuickComposerHost};
use crate::model::attachments::{PasteAction, accept_incoming, capture_paths, paste_action};

type AfterCollect = Box<dyn FnOnce(&mut QuickComposer, &mut Context<QuickComposer>)>;

/// The draft's attachments.
#[derive(Default)]
pub struct QuickAttachments {
    pub(crate) files: Vec<Attachment>,
    /// A collection is running (`loading`).
    pub(crate) loading: bool,
    /// A clipboard read captured while a collection ran.
    queued: Option<Task<Result<NativeClipboard, String>>>,
    task: Option<Task<()>>,
    /// The composer went away (`alive.current = false`).
    released: bool,
}

impl QuickAttachments {
    pub fn files(&self) -> &[Attachment] {
        &self.files
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn has_queued_paste(&self) -> bool {
        self.queued.is_some()
    }

    /// `clear`: drop every file and release their captures.
    pub(crate) fn clear(&mut self, host: &Rc<dyn QuickComposerHost>, cx: &mut App) {
        let files = std::mem::take(&mut self.files);
        for file in &files {
            host.revoke_attachment(file, cx);
        }
        let paths = capture_paths(&files);
        if !paths.is_empty() {
            host.release_captures(paths, cx);
        }
    }

    /// The unmount cleanup.
    pub(crate) fn release(&mut self, host: &Rc<dyn QuickComposerHost>, cx: &mut App) {
        self.released = true;
        self.queued = None;
        self.task = None;
        self.clear(host, cx);
    }
}

/// How a collection reads its files.
type Read = Box<
    dyn FnOnce(
        &mut QuickComposer,
        &mut Window,
        &mut Context<QuickComposer>,
    ) -> HostTask<Vec<Attachment>>,
>;

impl QuickComposer {
    /// `supported`: the provider takes attachments and nothing is starting.
    pub(crate) fn attachments_enabled(&self) -> bool {
        self.attachments_supported() && !self.busy
    }

    /// `canCollect`: `collect` bails on the same conditions, so a paste
    /// asks first.
    pub(crate) fn can_collect(&self) -> bool {
        self.attachments_enabled() && !self.attachments.loading
    }

    /// `collect`: read files, keep the new ones, write pasted bytes to
    /// disk, and add them. Errors land in the toolbar.
    fn collect(&mut self, read: Read, window: &mut Window, cx: &mut Context<Self>) {
        self.collect_then(read, None, window, cx);
    }

    fn collect_then(
        &mut self,
        read: Read,
        after: Option<AfterCollect>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_collect() {
            if let Some(after) = after {
                after(self, cx);
            }
            return;
        }
        self.attachments.loading = true;
        self.error = None;
        cx.notify();
        let reading = read(self, window, cx);
        let host = self.host.clone();
        self.attachments.task = Some(cx.spawn_in(window, async move |this, cx| {
            let incoming = reading.await;
            let outcome = finish_collect(&this, host, incoming, cx).await;
            this.update_in(cx, |this, window, cx| {
                if let Err(err) = outcome
                    && !this.attachments.released
                {
                    this.error = Some(err);
                }
                this.attachments.loading = false;
                this.attachments.task = None;
                if let Some(after) = after {
                    after(this, cx);
                }
                this.drain_queued(window, cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// The clipboard read captured during the last collection.
    fn drain_queued(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(queued) = self.attachments.queued.take() else {
            return;
        };
        if self.attachments.released || !self.attachments_enabled() {
            let host = self.host.clone();
            cx.spawn(async move |_, cx| {
                if let Ok(read) = queued.await {
                    cx.update(|cx| {
                        for file in &read.files {
                            host.revoke_attachment(file, cx);
                        }
                    });
                }
            })
            .detach();
            return;
        }
        let this = cx.entity().downgrade();
        self.collect(
            Box::new(move |_, _, cx| {
                cx.spawn(async move |_, cx| {
                    let read = queued.await?;
                    if let Some(warning) = read.warning {
                        this.update(cx, |this, cx| {
                            this.error = Some(warning);
                            cx.notify();
                        })
                        .ok();
                    }
                    Ok(read.files)
                })
            }),
            window,
            cx,
        );
    }

    /// `chooseFiles`: the file dialog.
    pub fn choose_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.collect(
            Box::new(|this, window, cx| this.host.pick_attachments(window, cx)),
            window,
            cx,
        );
    }

    /// Dropped files (the native drop).
    pub fn drop_paths(&mut self, paths: Vec<String>, window: &mut Window, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        self.collect(
            Box::new(move |this, _, cx| this.host.attachments_from_paths(paths, cx)),
            window,
            cx,
        );
    }

    pub(crate) fn on_drop_paths(
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

    /// `takeScreenshot`: the interactive capture. Cancelling keeps the
    /// draft; a capture the draft does not keep is released.
    pub fn take_screenshot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let captured: Rc<std::cell::RefCell<Option<String>>> = Rc::default();
        let slot = captured.clone();
        let read: Read = Box::new(move |this, window, cx| {
            let capture = this.host.capture_screenshot(window, cx);
            let host = this.host.clone();
            cx.spawn(async move |_, cx| {
                let Some(path) = capture.await? else {
                    return Ok(Vec::new());
                };
                *slot.borrow_mut() = Some(path.clone());
                cx.update(|cx| host.attachments_from_paths(vec![path], cx))
                    .await
            })
        });
        // Inspection, capacity checks, and closing can reject a new capture.
        let after = Box::new(move |this: &mut Self, cx: &mut Context<Self>| {
            let Some(path) = captured.borrow_mut().take() else {
                return;
            };
            let kept = this
                .attachments
                .files
                .iter()
                .any(|file| file.path.as_deref() == Some(path.as_str()));
            if !kept {
                this.host.release_captures(vec![path], cx);
            }
        });
        self.collect_then(read, Some(after), window, cx);
    }

    /// `remove`: drop one file and release it if it was a capture.
    pub fn remove_attachment(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(index) = self.attachments.files.iter().position(|file| file.id == id) {
            let removed = self.attachments.files.remove(index);
            self.host.revoke_attachment(&removed, cx);
            let paths = capture_paths(std::slice::from_ref(&removed));
            if !paths.is_empty() {
                self.host.release_captures(paths, cx);
            }
            cx.notify();
        }
    }

    /// `onPaste`, captured before the prompt's own paste. Returns true when
    /// the composer took the paste.
    pub(crate) fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let item = cx.read_from_clipboard();
        let text = item
            .as_ref()
            .and_then(|item| item.text())
            .map(|text| normalize_newlines(&text))
            .unwrap_or_default();
        let mut paths: Vec<String> = Vec::new();
        let mut images: Vec<ClipboardFile> = Vec::new();
        for entry in item.as_ref().map(|item| item.entries()).unwrap_or_default() {
            match entry {
                ClipboardEntry::ExternalPaths(list) => paths.extend(
                    list.paths()
                        .iter()
                        .map(|path| path.to_string_lossy().into_owned()),
                ),
                ClipboardEntry::Image(image) => images.push(ClipboardFile {
                    name: format!("clipboard-image.{}", image_extension(image.format)),
                    mime_type: image.format.mime_type().to_string(),
                    bytes: image.bytes.clone(),
                }),
                _ => {}
            }
        }
        let has_files = !paths.is_empty() || !images.is_empty();
        match paste_action(
            has_files,
            &text,
            self.attachments_enabled(),
            self.can_collect(),
        ) {
            PasteAction::Text => false,
            PasteAction::Files => {
                if !paths.is_empty() {
                    self.drop_paths(paths, window, cx);
                } else {
                    self.collect(
                        Box::new(move |this, _, cx| this.host.attachments_from_files(images, cx)),
                        window,
                        cx,
                    );
                }
                true
            }
            PasteAction::Queue => {
                if self.attachments.queued.is_none() {
                    self.attachments.queued = Some(self.host.native_clipboard("", cx));
                }
                true
            }
            PasteAction::Native { restore_text } => {
                // Captured before the read, so the text goes back where it
                // was pasted if nothing attaches.
                let selection = self.prompt.read(cx).selection();
                let restore = restore_text.then(|| (text.clone(), selection));
                let this = cx.entity().downgrade();
                self.collect(
                    Box::new(move |this_ref, _, cx| {
                        let read = this_ref.host.native_clipboard(&text, cx);
                        cx.spawn(async move |_, cx| {
                            let read = read.await?;
                            let empty = read.files.is_empty();
                            this.update(cx, |this, cx| {
                                if let Some(warning) = read.warning.clone() {
                                    this.error = Some(warning);
                                }
                                if empty && let Some((text, selection)) = restore {
                                    this.prompt.update(cx, |prompt, cx| {
                                        prompt.replace_range(selection, &text, cx)
                                    });
                                }
                                cx.notify();
                            })
                            .ok();
                            Ok(read.files)
                        })
                    }),
                    window,
                    cx,
                );
                true
            }
        }
    }
}

fn image_extension(format: gpui::ImageFormat) -> &'static str {
    match format {
        gpui::ImageFormat::Png => "png",
        gpui::ImageFormat::Jpeg => "jpg",
        gpui::ImageFormat::Gif => "gif",
        gpui::ImageFormat::Webp => "webp",
        gpui::ImageFormat::Svg => "svg",
        gpui::ImageFormat::Bmp => "bmp",
        gpui::ImageFormat::Tiff => "tiff",
        _ => "png",
    }
}

/// The rest of `collect` once the files are read.
async fn finish_collect(
    this: &WeakEntity<QuickComposer>,
    host: Rc<dyn QuickComposerHost>,
    incoming: Result<Vec<Attachment>, String>,
    cx: &mut AsyncWindowContext,
) -> Result<(), String> {
    let incoming = incoming?;
    let accepted = this
        .update(cx, |this, cx| {
            let collected = accept_incoming(&this.attachments.files, incoming);
            for file in &collected.rejected {
                host.revoke_attachment(file, cx);
            }
            if let Some(error) = collected.error {
                this.error = Some(error);
            }
            collected.accepted
        })
        .map_err(|err| err.to_string())?;
    if accepted.is_empty() {
        return Ok(());
    }
    let store = this
        .update(cx, |_, cx| host.store_attachments(accepted.clone(), cx))
        .map_err(|err| err.to_string())?;
    match store.await {
        Ok(stored) => this
            .update(cx, |this, cx| {
                if this.attachments.released {
                    for file in &stored {
                        host.revoke_attachment(file, cx);
                    }
                    return;
                }
                // Read the current list again: files may have been removed
                // while loading.
                this.attachments.files.extend(stored);
            })
            .map_err(|err| err.to_string()),
        Err(err) => {
            this.update(cx, |_, cx| {
                for file in &accepted {
                    host.revoke_attachment(file, cx);
                }
            })
            .ok();
            Err(err)
        }
    }
}
