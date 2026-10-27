use super::*;
use monocode_core::mono::files;

impl Monos {
    pub(crate) fn reset_defaults(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> gpui::Task<Result<(), String>> {
        let root = self.root.clone();
        let id = id.to_owned();
        let target = id.clone();
        let work = cx.background_executor().spawn(async move {
            let current = files::load(&root, &target, None)?;
            files::save(
                &root,
                &target,
                None,
                "SOUL.md",
                mono::DEFAULT_SOUL,
                Some(&current.soul_hash),
            )?;
            files::load(&root, &target, None)
        });
        cx.spawn(async move |this, cx| {
            let files = work.await?;
            this.update(cx, |this, cx| {
                let mono = this.roster.get_mut(&id).ok_or("Mono no longer exists")?;
                mono.name.clear();
                let key = format!("{id}:SOUL.md");
                this.fields.remove(&format!("file:{key}"));
                this.file_versions.remove(&key);
                if this.active.as_deref() == Some(&id) {
                    this.files = Some(files);
                }
                this.configure_chat(&id, cx);
                this.save(cx);
                Ok(())
            })
            .map_err(|e| e.to_string())?
        })
    }
    fn file_editor(
        &mut self,
        path: &str,
        text: String,
        hash: String,
        cx: &mut Context<Self>,
    ) -> Entity<Input> {
        let key = format!("{}:{path}", self.active.as_deref().unwrap_or_default());
        let field_key = format!("file:{key}");
        if let Some((old, old_hash)) = self.file_versions.get(&key)
            && old_hash != &hash
            && !self.saving_files.contains(&key)
            && let Some(field) = self
                .fields
                .get(&field_key)
                .filter(|f| f.read(cx).content.as_str() == old)
        {
            field.update(cx, |f, cx| f.set_text(text.clone(), cx));
            self.file_versions
                .insert(key.clone(), (text.clone(), hash.clone()));
        }
        self.file_versions
            .entry(key)
            .or_insert((text.clone(), hash));
        self.field(&field_key, "Markdown", text, true, cx)
    }
    pub(super) fn soul_page(&mut self, mono: &Mono, cx: &mut Context<Self>) -> AnyElement {
        let body = if let Some(files) = self.files.clone() {
            let editor = self.file_editor(
                "SOUL.md",
                files.soul.unwrap_or_default(),
                files.soul_hash,
                cx,
            );
            editor.update(cx, |v, cx| {
                v.set_placeholder("Who it is, and what it should always keep in mind", cx)
            });
            let focus = editor.focus_handle(cx);
            div()
                .id("mono-soul-editor")
                .debug_selector(|| "mono-soul-editor".into())
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px_3()
                .py_3()
                .font_family(crate::fonts::mono())
                .text_size(px(13.))
                .line_height(px(20.))
                .text_color(content(0.85))
                .on_mouse_down(MouseButton::Left, move |_, w, cx| w.focus(&focus, cx))
                .child(editor)
                .into_any_element()
        } else {
            div()
                .flex_1()
                .p_6()
                .text_center()
                .text_size(px(12.))
                .text_color(content(0.45))
                .child("Loading…")
                .into_any_element()
        };
        let _ = mono;
        div()
            .id("mono-soul-page")
            .debug_selector(|| "mono-soul-page".into())
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(Self::settings_page_header("Soul"))
            .child(body)
            .into_any_element()
    }
    pub(super) fn settings_page_header(title: &'static str) -> gpui::Div {
        div()
            .h(px(40.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1()
            .pl_4()
            .pr(px(44.))
            .border_b_1()
            .border_color(content(0.04))
            .text_size(px(13.))
            .font_weight(gpui::FontWeight::MEDIUM)
            .child(div().flex_1().min_w_0().child(title))
    }
    pub(super) fn bind_settings_blurs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let fields = self
            .fields
            .iter()
            .filter(|(key, _)| key.starts_with("file:"))
            .map(|(_, field)| field.clone())
            .collect::<Vec<_>>();
        for field in fields {
            if self.field_blurs.insert(field.entity_id()) {
                self.observations.push(cx.on_blur(
                    &field.focus_handle(cx),
                    window,
                    |this, _, cx| this.save_open_file(cx),
                ));
            }
        }
    }
    pub(super) fn save_open_file(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.active.clone() else {
            return;
        };
        let prefix = format!("{id}:");
        for (key, (old, hash)) in self.file_versions.clone() {
            let Some(path) = key.strip_prefix(&prefix).map(str::to_owned) else {
                continue;
            };
            let Some(field) = self.fields.get(&format!("file:{key}")) else {
                continue;
            };
            let text = field.read(cx).content.to_string();
            if text == old || hash.is_empty() || !self.saving_files.insert(key.clone()) {
                continue;
            }
            let root = self.root.clone();
            let id = id.clone();
            let target = id.clone();
            let content = text.clone();
            let work = cx.background_executor().spawn(async move {
                files::save(&root, &target, None, &path, &content, Some(&hash))
            });
            cx.spawn(async move |this, cx| {
                let result = work.await;
                let _ = this.update(cx, |this, cx| {
                    this.saving_files.remove(&key);
                    match result {
                        Ok(hash) => {
                            this.file_versions.insert(key, (text, hash));
                            this.reload_files(&id, cx);
                        }
                        Err(e) => {
                            this.error = Some(format!(
                                "Could not save: {e}. Your edits are still in the editor."
                            ))
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }

    pub(super) fn file_work(
        &mut self,
        id: String,
        work: impl FnOnce() -> Result<(), String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let work = cx.background_executor().spawn(async move { work() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.reload_files(&id, cx);
                    }
                    Err(e) => this.error = Some(e),
                }
                this.changed(cx);
            });
        })
        .detach();
    }
    pub(super) fn reset_conversation(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prompt = window.prompt(
            gpui::PromptLevel::Warning,
            "Reset conversation?",
            Some("The chat will be cleared. Its soul, memory and habits stay."),
            &["Cancel", "Reset"],
            cx,
        );
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            if prompt.await.ok() != Some(1) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                if let Some(view) = this.chats.remove(&id) {
                    view.update(cx, |v, cx| v.mono_stop(cx));
                }
                if let Some(mono) = this.roster.get_mut(&id) {
                    mono.session_id = None;
                    mono.unavailable_session = None;
                    mono.seen_rows = 0;
                    mono.outbox.clear();
                }
                this.save(cx);
                this.ensure_chat(&id, cx);
            });
        })
        .detach();
    }
    pub(super) fn delete_mono(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = window.prompt(
            gpui::PromptLevel::Warning,
            "Delete mono?",
            Some("Its scheduled habits stop. Sessions and notes it created stay saved."),
            &["Cancel", "Delete"],
            cx,
        );
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            if prompt.await.ok() != Some(1) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                if let Some(view) = this.chats.remove(&id) {
                    view.update(cx, |v, cx| v.mono_stop(cx));
                }
                for run in this.running.values().filter(|r| r.mono == id) {
                    run.view.update(cx, |v, cx| v.mono_stop(cx));
                }
                this.roster.monos.retain(|m| m.id != id);
                if this.active.as_ref() == Some(&id) {
                    this.active = None;
                }
                this.save(cx);
            });
        })
        .detach();
    }
}
