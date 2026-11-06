use super::*;
use std::path::PathBuf;

impl Conversation {
    pub(crate) fn request_folder(&mut self, cx: &mut Context<Self>) {
        if self.settings.mono.is_none() {
            self.completion
                .update(cx, |completion, cx| completion.dismiss(cx));
            cx.emit(ChooseFolder(self.composer_anchor.clone()));
            cx.notify();
        }
    }
    pub(crate) fn folder_chosen(&mut self, cx: &mut Context<Self>) {
        self.folder_selected = true;
        let text = self.input.read(cx).content.clone();
        if text.trim().eq_ignore_ascii_case("/add-to-folder") {
            self.input
                .update(cx, |input, cx| input.set_text(format!("{text} "), cx));
        }
    }
    pub(crate) fn set_app_environment(
        &mut self,
        environment: std::collections::HashMap<String, String>,
    ) {
        self.app_environment = environment;
    }
    pub(crate) fn enable_mode(&mut self, id: &str, cx: &mut Context<Self>) {
        let enabled = match id {
            "draft" => self.follow_ups.draft_selected,
            "plan" => self.settings.plan,
            _ => true,
        };
        if !enabled {
            self.toggle_mode(id, cx);
        }
    }
    pub(crate) fn clear_mode(&mut self, id: &str, cx: &mut Context<Self>) {
        let enabled = match id {
            "draft" => &mut self.follow_ups.draft_selected,
            "plan" => &mut self.settings.plan,
            _ => return,
        };
        if *enabled {
            // The input has already been edited. Clearing its old mode must
            // preserve the replacement text and the user's caret position.
            *enabled = false;
            self.save_settings(cx);
            cx.notify();
        }
    }
    pub(super) fn clear_submitted_mode(&mut self, cx: &mut Context<Self>) {
        // The accepted turn or queue item already owns its mode. The empty
        // composer starts a fresh draft, matching Composer.tsx's submit reset.
        self.settings.plan = false;
        self.input
            .update(cx, |input, cx| input.selected_command(None, cx));
        self.save_settings(cx);
        cx.notify();
    }
    pub(crate) fn toggle_mode(&mut self, id: &str, cx: &mut Context<Self>) {
        if id != "draft" {
            self.follow_ups.draft_selected = false;
        }
        match id {
            "draft" => {
                self.follow_ups.draft_selected = !self.follow_ups.draft_selected;
                self.settings.plan = false;
            }
            "plan" => self.settings.plan = !self.settings.plan,
            _ => return,
        }
        let selected = if self.follow_ups.draft_selected {
            Some("draft")
        } else if self.settings.plan {
            Some("plan")
        } else {
            None
        };
        self.input.update(cx, |input, cx| {
            let leading = input::Command::leading(&input.content);
            if leading.map(input::Command::name) != selected {
                let body = leading.map_or(input.content.as_ref(), |mode| {
                    input.content[mode.name().len() + 1..].trim_start()
                });
                let next =
                    selected.map_or_else(|| body.to_owned(), |name| format!("/{name} {body}"));
                input.set_text(next, cx);
            }
        });
        self.save_settings(cx);
        cx.notify();
    }
    pub(super) fn consume_mode_command(
        &mut self,
        mut text: String,
        cx: &mut Context<Self>,
    ) -> String {
        if let Some(rest) = monocode_core::composer::consume_command(&text, "plan") {
            let rest = rest.to_owned();
            self.settings.plan = true;
            self.save_settings(cx);
            text = rest;
        }
        text
    }
    pub(super) fn choose_attachments(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach files".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await {
                let _ = this.update(cx, |this, cx| this.add_attachments(paths, cx));
            }
        })
        .detach();
    }
    pub(super) fn paste_attachments(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(clipboard) = cx.read_from_clipboard() else {
            return false;
        };
        let paths: Vec<_> = clipboard
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                gpui::ClipboardEntry::ExternalPaths(paths) => Some(paths.paths().to_vec()),
                _ => None,
            })
            .flatten()
            .collect();
        if !paths.is_empty() {
            self.add_attachments(paths, cx);
            return true;
        }
        let images: Vec<_> = clipboard
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                gpui::ClipboardEntry::Image(image) => Some(image.clone()),
                _ => None,
            })
            .collect();
        if images.is_empty() {
            return false;
        }
        if self.attaching {
            return true;
        }
        self.attaching = true;
        let store = self.store.clone();
        let id = self.summary.id;
        let mut all = self.attachments.clone();
        let existing = all.len();
        let work = cx.background_executor().spawn(async move {
            if all.len() + images.len() > monocode_core::composer::MAX_ATTACHMENTS
                || images.iter().map(|i| i.bytes().len() as u64).sum::<u64>()
                    > monocode_core::composer::MAX_EMBED_BYTES
            {
                return Err("Attach up to 20 files totalling 20 MB.".to_owned());
            }
            // Kept with the conversation so deleting it removes them.
            let root = locked(&store)?
                .thread_attachments_dir(id)?
                .unwrap_or_else(|| std::env::temp_dir().join("monocode-attachments"));
            std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
            for image in images {
                let path = root.join(format!(
                    "pasted-{}.{}",
                    monocode_core::mono::new_id(),
                    image.format().extension()
                ));
                std::fs::write(&path, image.bytes()).map_err(|e| e.to_string())?;
                all.push(path);
            }
            monocode_core::composer::validate_attachments(&all)?;
            Ok(all.split_off(existing))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.finish_attachment_read(result, cx);
            });
        })
        .detach();
        cx.notify();
        true
    }
    pub(super) fn add_attachments(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        for path in paths {
            if !self.attachments.contains(&path) && !self.pending_attachments.contains(&path) {
                self.pending_attachments.push(path);
            }
        }
        // Rapid successive drops must wait for the active read, rather than
        // silently losing the later files (including during an image paste).
        if self.attaching || self.pending_attachments.is_empty() {
            return;
        }
        let incoming = std::mem::take(&mut self.pending_attachments);
        let mut all = self.attachments.clone();
        all.extend(incoming.iter().cloned());
        self.attaching = true;
        let work = cx.background_executor().spawn(async move {
            monocode_core::composer::validate_attachments(&all).map(|_| incoming)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.finish_attachment_read(result, cx);
            });
        })
        .detach();
        cx.notify();
    }
    fn finish_attachment_read(
        &mut self,
        result: Result<Vec<PathBuf>, String>,
        cx: &mut Context<Self>,
    ) {
        self.attaching = false;
        match result {
            Ok(paths) => {
                // Keep removals made while the background validation ran.
                for path in paths {
                    if !self.attachments.contains(&path) {
                        self.attachments.push(path);
                    }
                }
                self.attachment_error = None;
            }
            Err(error) => self.attachment_error = Some(error),
        }
        let pending = std::mem::take(&mut self.pending_attachments);
        self.add_attachments(pending, cx);
        cx.notify();
    }
    pub(super) fn attachment_chips(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        // Removed chips release their thumbnail sprites with the entity.
        let attachments = &self.attachments;
        self.attachment_photos
            .retain(|path, _| attachments.contains(path));
        if self.attachments.is_empty() && self.attachment_error.is_none() && !self.attaching {
            return None;
        }
        for path in &self.attachments {
            if let Some(mime) = monocode_core::composer::image_mime(path)
                && !self.attachment_photos.contains_key(path)
            {
                let origin = crate::transcript::attachments::Origin::Path(path.clone());
                let photo = cx
                    .new(|cx| crate::transcript::attachments::Photo::new(origin, mime.into(), cx));
                self.attachment_photos.insert(path.clone(), photo);
            }
        }
        Some(
            div()
                .debug_selector(|| "composer-attachments".into())
                .px_3()
                .pt_3()
                .flex()
                .flex_wrap()
                .gap_1p5()
                .children(self.attachments.iter().enumerate().map(|(ix, path)| {
                    let label = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let photo = self.attachment_photos.get(path).cloned();
                    let image = monocode_core::composer::image_mime(path).is_some();
                    let remove = div()
                        .id(("remove-attachment", ix))
                        .role(Role::Button)
                        .aria_label(format!("Remove {label}"))
                        .cursor_pointer()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(image, |d| {
                            d.absolute()
                                .top(px(-4.))
                                .right(px(-4.))
                                .size(px(20.))
                                .rounded_full()
                                .bg(content(0.2))
                                .text_color(content(0.7))
                                .hover(|s| s.bg(content(0.3)).text_color(content(1.)))
                        })
                        .when(!image, |d| {
                            d.h(px(26.))
                                .px(px(6.))
                                .rounded_md()
                                .bg(content(0.1))
                                .text_color(content(1.))
                                .hover(|s| s.bg(content(0.15)))
                        })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(icon("close", if image { 12. } else { 10. }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if ix < this.attachments.len() {
                                this.attachments.remove(ix);
                                cx.notify();
                            }
                        }));
                    if image {
                        let preview = crate::image_lightbox::Open {
                            source: crate::image_lightbox::Source::Path(path.clone()),
                            alt: label.clone().into(),
                        };
                        return div()
                            .id(("attachment", ix))
                            .relative()
                            .size(px(36.))
                            .flex_shrink_0()
                            .role(Role::Button)
                            .focusable()
                            .debug_selector(move || format!("composer-image-{ix}"))
                            .aria_label(format!("Open {label} full screen"))
                            .cursor(CursorStyle::PointingHand)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                window.dispatch_action(Box::new(preview.clone()), cx);
                            })
                            .children(photo)
                            .child(remove)
                            .into_any_element();
                    }
                    div()
                        .id(("attachment", ix))
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(content(0.06))
                        .text_size(px(11.))
                        .child(crate::file_icons::file_type_icon(&label, false, false, 14.))
                        .child(div().max_w(px(190.)).truncate().child(label))
                        .child(remove)
                        .into_any_element()
                }))
                .when(self.attaching, |d| {
                    d.child(
                        div()
                            .text_size(px(11.))
                            .text_color(content(0.5))
                            .child("Attaching…"),
                    )
                })
                .when_some(self.attachment_error.clone(), |d, error| {
                    d.child(
                        div()
                            .text_size(px(11.))
                            .text_color(tint(AMBER_400, 0.9))
                            .child(error),
                    )
                })
                .into_any_element(),
        )
    }
    pub(crate) fn external_outcome(&self, cx: &gpui::App) -> (bool, String) {
        let rows = self.transcript.read(cx).source_rows();
        let start = rows.iter().rposition(|row| row.role == "user").unwrap_or(0);
        let text = monocode_core::response::prose(&rows[start..]);
        (
            self.status == "Ready",
            monocode_core::response::limit(&format!("{}\n{}", self.status, text), 12_000),
        )
    }
    pub(crate) fn ready_for_external(&self, cx: &gpui::App) -> bool {
        self.history_ready()
            && !self.editing_last_turn.pending()
            && !self.busy
            && !self.stopping
            && !self.preparing_worktree
            && !self.attaching
            && self.input.read(cx).content.is_empty()
            && self.attachments.is_empty()
    }
    /// Managed runs must wait for history restoration and worktree setup as
    /// well as the provider, including submissions deferred by the editor.
    pub(crate) fn external_submission_pending(&self) -> bool {
        self.busy
            || self.stopping
            || self.preparing_worktree
            || self.attaching
            || self.account_switching
            || self.loading
            || (!self.history_actions.is_empty() && self.history_ready())
            || self.editing_last_turn.pending()
            || self.mono_external_pending()
    }
    /// An evicted history can be restored for a managed run; a user's draft
    /// or an already accepted message must keep ownership of this composer.
    pub(crate) fn available_for_managed_run(&self, cx: &gpui::App) -> bool {
        !self.busy
            && !self.stopping
            && !self.preparing_worktree
            && !self.attaching
            && !self.account_switching
            && !self.editing_last_turn.pending()
            && !self.mono_external_pending()
            && self.input.read(cx).content.is_empty()
            && self.attachments.is_empty()
            && self.follow_ups.state.messages.is_empty()
            && self.follow_ups.state.draft.is_none()
            && !self.settings.worktree_removed
    }
}
