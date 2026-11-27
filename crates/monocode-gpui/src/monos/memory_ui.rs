use super::*;
use mono::memory;

impl Monos {
    pub(super) fn memory_page(
        &mut self,
        mono: &Mono,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let files = self.files.clone();
        let mut rows = Vec::new();
        if self.memory_adding {
            rows.push(self.memory_editor(mono, None, window, cx));
        }
        if let Some(files) = &files {
            rows.extend(self.memory_facts(mono, &files.memory, window, cx));
        }
        let gauge = files.as_ref().map(|files| {
            let loaded = memory::within_budget(&files.memory);
            let lines = loaded.lines().count();
            let dropped = files.memory.lines().count().saturating_sub(lines);
            let share = (lines as f32 / memory::MAX_LINES as f32)
                .max(loaded.len() as f32 / memory::MAX_BYTES as f32)
                .min(1.);
            let tone = if dropped > 0 {
                gpui::Hsla::from(rgb(0xfb2c36)).opacity(0.7)
            } else if share >= 0.8 {
                gpui::Hsla::from(rgb(0xffb900)).opacity(0.7)
            } else {
                content(0.3)
            };
            div()
                .flex_shrink_0()
                .border_t_1()
                .border_color(content(0.04))
                .px_2()
                .pb_3()
                .child(
                    div()
                        .mt_2()
                        .px_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .text_color(content(0.45))
                        .child(
                            div()
                                .flex_1()
                                .h(px(4.))
                                .rounded_full()
                                .overflow_hidden()
                                .bg(content(0.08))
                                .child(
                                    div()
                                        .h_full()
                                        .w(gpui::relative(share))
                                        .rounded_full()
                                        .bg(tone),
                                ),
                        )
                        .child(if dropped > 0 {
                            format!("{dropped} lines not loading")
                        } else {
                            format!("{lines} / 200 lines")
                        }),
                )
        });
        div()
            .id("mono-memory-page")
            .debug_selector(|| "mono-memory-page".into())
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                Self::settings_page_header("Memory").when(files.is_some(), |d| {
                    d.child(
                        icon_button_enabled(
                            "mono-add-memory",
                            "Add memory",
                            "plus",
                            !self.memory_adding,
                        )
                        .size(px(28.))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                if this.memory_adding {
                                    return;
                                }
                                this.memory_edit = None;
                                this.memory_adding = true;
                                this.error = None;
                                let editor = this.field(
                                    "memory-fact",
                                    "New memory",
                                    String::new(),
                                    false,
                                    cx,
                                );
                                editor.update(cx, |f, cx| f.set_text(String::new(), cx));
                                window.focus(&editor.focus_handle(cx), cx);
                                cx.notify();
                            },
                        )),
                    )
                }),
            )
            .child(
                div()
                    .id("mono-memory-facts")
                    .debug_selector(|| "mono-memory-facts".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_2()
                    .flex()
                    .flex_col()
                    .gap_px()
                    .when(files.is_none(), |d| {
                        d.child(
                            div()
                                .px_2()
                                .py_6()
                                .text_center()
                                .text_size(px(12.))
                                .text_color(content(0.45))
                                .child("Loading…"),
                        )
                    })
                    .when(files.is_some() && rows.is_empty(), |d| {
                        d.child(
                            div()
                                .px_2()
                                .py_6()
                                .text_center()
                                .text_size(px(12.))
                                .text_color(content(0.45))
                                .child("Nothing remembered yet"),
                        )
                    })
                    .children(rows),
            )
            .children(gauge)
            .into_any_element()
    }
    fn memory_icon(dim: bool) -> gpui::Div {
        div()
            .size(px(28.))
            .flex_shrink_0()
            .rounded_md()
            .bg(content(0.06))
            .flex()
            .items_center()
            .justify_center()
            .child(icon("sticky-note", 14.).text_color(content(if dim { 0.3 } else { 0.6 })))
    }
    fn memory_editor(
        &mut self,
        mono: &Mono,
        original: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = original
            .map(|line| {
                format!(
                    "memory-edit:{}:{}",
                    mono.id,
                    mono::files::content_hash(line)
                )
            })
            .unwrap_or_else(|| "memory-fact".into());
        let editor = self.field(
            &key,
            if original.is_some() {
                "Edit memory"
            } else {
                "New memory"
            },
            original.map(memory::fact).unwrap_or_default(),
            false,
            cx,
        );
        editor.update(cx, |f, cx| {
            f.set_placeholder("Something it should remember", cx)
        });
        if self.field_blurs.insert(editor.entity_id()) {
            let (mono, row, field) = (mono.id.clone(), original.map(str::to_owned), key.clone());
            self.observations.push(cx.subscribe(
                &editor,
                move |this, _, _: &input::Submitted, cx| {
                    this.commit_memory_input(&mono, &field, row.as_deref(), false, cx)
                },
            ));
            let (mono, row, field) = (
                self.active.clone().unwrap_or_default(),
                original.map(str::to_owned),
                key.clone(),
            );
            self.observations.push(cx.on_blur(
                &editor.focus_handle(cx),
                window,
                move |this, _, cx| {
                    this.commit_memory_input(&mono, &field, row.as_deref(), true, cx)
                },
            ));
        }
        div()
            .id(SharedString::from(format!("memory-editor-{key}")))
            .flex()
            .items_start()
            .gap_2p5()
            .rounded_md()
            .bg(content(0.05))
            .px_2()
            .py_2()
            .child(Self::memory_icon(false))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .py(px(5.))
                    .text_color(content(0.9))
                    .child(editor),
            )
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.memory_adding = false;
                    this.memory_edit = None;
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .into_any_element()
    }
    pub(super) fn memory_facts(
        &mut self,
        mono: &Mono,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut result = Vec::new();
        for (index, line) in text
            .lines()
            .enumerate()
            .filter(|(_, l)| !l.trim().is_empty() && !l.trim().starts_with('#'))
        {
            if self.memory_edit.as_deref() == Some(line) {
                result.push(self.memory_editor(mono, Some(line), window, cx));
                continue;
            }
            let struck = line.contains("~~");
            let fact = memory::fact(line);
            let date = line
                .trim_start_matches("- ")
                .trim_start_matches("~~")
                .get(..10)
                .and_then(|day| chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
                .map(|day| day.format("%b %-d").to_string());
            let edit = line.to_string();
            let remove = edit.clone();
            let edit_button = edit.clone();
            result.push(
                div()
                    .id(("mono-memory-fact", index))
                    .group("memory-fact")
                    .rounded_md()
                    .flex()
                    .items_center()
                    .hover(|s| s.bg(content(0.05)))
                    .child(
                        div()
                            .id(("memory-fact-body", index))
                            .role(Role::Button)
                            .aria_label(format!("Edit memory: {fact}"))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_2p5()
                            .px_2()
                            .py_2()
                            .child(Self::memory_icon(struck))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(12.))
                                    .line_height(px(20.))
                                    .text_color(content(if struck { 0.35 } else { 0.85 }))
                                    .child(
                                        div()
                                            .truncate()
                                            .when(struck, |d| d.line_through())
                                            .child(fact),
                                    )
                                    .when_some(date, |d, date| {
                                        d.child(
                                            div()
                                                .text_size(px(11.))
                                                .line_height(px(16.))
                                                .text_color(content(0.4))
                                                .child(date),
                                        )
                                    }),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if struck {
                                    return;
                                }
                                this.memory_adding = false;
                                this.memory_edit = Some(edit.clone());
                                this.error = None;
                                let key = format!(
                                    "memory-edit:{}:{}",
                                    this.active.as_deref().unwrap_or_default(),
                                    mono::files::content_hash(&edit)
                                );
                                let field =
                                    this.field(&key, "Edit memory", memory::fact(&edit), false, cx);
                                window.focus(&field.focus_handle(cx), cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .pr_2()
                            .opacity(0.)
                            .group_hover("memory-fact", |s| s.opacity(1.))
                            .when(!struck, |d| {
                                d.child(
                                    icon_button(
                                        "mono-memory-edit",
                                        "Edit memory fact",
                                        "pencil-edit",
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            this.memory_adding = false;
                                            this.memory_edit = Some(edit_button.clone());
                                            let key = format!(
                                                "memory-edit:{}:{}",
                                                this.active.as_deref().unwrap_or_default(),
                                                mono::files::content_hash(&edit_button)
                                            );
                                            let field = this.field(
                                                &key,
                                                "Edit memory",
                                                memory::fact(&edit_button),
                                                false,
                                                cx,
                                            );
                                            window.focus(&field.focus_handle(cx), cx);
                                            cx.notify();
                                        },
                                    )),
                                )
                            })
                            .child(
                                icon_button("mono-memory-forget", "Forget memory fact", "trash")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.edit_memory_fact(&remove, None, cx)
                                    })),
                            ),
                    )
                    .into_any_element(),
            );
        }
        if let Some(original) = self.memory_edit.clone()
            && !text.lines().any(|line| line == original)
        {
            result.push(self.memory_editor(mono, Some(&original), window, cx));
        }
        result
    }
    fn commit_memory_input(
        &mut self,
        id: &str,
        key: &str,
        original: Option<&str>,
        close: bool,
        cx: &mut Context<Self>,
    ) {
        let active_editor = if original.is_some() {
            self.memory_edit.as_deref() == original
        } else {
            self.memory_adding
        };
        if !active_editor || self.active.as_deref() != Some(id) {
            return;
        }
        let text = self
            .value(key, cx)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() {
            if original.is_some() {
                self.memory_edit = None;
            } else {
                self.memory_adding = false;
            }
            cx.notify();
            return;
        }
        if !self.memory_pending.insert(key.to_string()) {
            return;
        }
        let (root, id, key, original) = (
            self.root.clone(),
            id.to_string(),
            key.to_string(),
            original.map(str::to_owned),
        );
        let (target, content) = (id.clone(), text.clone());
        let edited = original.clone();
        let work = cx.background_executor().spawn(async move {
            memory::edit(&root, &target, None, |memory| {
                if let Some(original) = &edited {
                    memory::edit_line(memory, original, Some(&content))
                } else {
                    Ok(memory::add(
                        memory,
                        &memory::entry(&content, &memory::date(), None)?,
                    ))
                }
            })
            .map(|_| ())
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.memory_pending.remove(&key);
                match result {
                    Ok(()) => {
                        if original.is_some() {
                            this.memory_edit = None;
                        } else if close {
                            this.memory_adding = false;
                        } else if let Some(field) = this.fields.get(&key)
                            && field
                                .read(cx)
                                .content
                                .split_whitespace()
                                .collect::<Vec<_>>()
                                .join(" ")
                                == text
                        {
                            field.update(cx, |f, cx| f.set_text(String::new(), cx));
                        }
                        this.error = None;
                        this.reload_files(&id, cx);
                    }
                    Err(error) => this.error = Some(error),
                }
                this.changed(cx);
            });
        })
        .detach();
    }
    fn edit_memory_fact(
        &mut self,
        original: &str,
        replacement: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.active.clone() else {
            return;
        };
        let (root, target, original) = (self.root.clone(), id.clone(), original.to_string());
        self.file_work(
            id,
            move || {
                memory::edit(&root, &target, None, |text| {
                    memory::edit_line(text, &original, replacement.as_deref())
                })
                .map(|_| ())
            },
            cx,
        );
    }
}
