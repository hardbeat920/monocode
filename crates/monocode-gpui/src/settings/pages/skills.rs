use crate::{
    settings::{
        SettingsView,
        management::Target,
        widgets::{icon_action, note, page_header, secondary_button},
    },
    theme::*,
};
use gpui::{
    AnyElement, ClipboardItem, Context, Focusable, Role, SharedString, div, prelude::*, px,
};
impl SettingsView {
    fn skill_create_form(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let e = self.management.as_ref().expect("open skill form");
        let name = e.name.clone();
        let busy = e.busy;
        let scope = e.scope.clone();
        let error = e.error.clone();
        let raw = name.read(cx).content.to_string();
        let valid =
            monocode_core::skills::is_valid_skill_name(&monocode_core::skills::slug_name(&raw));
        let project = !self.selected_project.is_empty() && self.selected_project != "~";
        name.update(cx, |i, _| i.set_read_only(busy));
        div()
            .px(px(10.))
            .py_2()
            .child(
                div()
                    .mb_2()
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(content(0.5))
                    .child("Create an editable SKILL.md."),
            )
            .child(
                div()
                    .mb_2()
                    .px_2()
                    .py(px(6.))
                    .rounded_md()
                    .bg(content(0.1))
                    .child(name),
            )
            .child(
                div().mb_2().flex().gap_1().children(
                    [
                        ("project", "Project", ".agents/skills"),
                        ("user", "Personal", "~/.agents/skills"),
                    ]
                    .into_iter()
                    .map(|(value, label, hint)| {
                        let disabled = busy || (value == "project" && !project);
                        div()
                            .id(value)
                            .role(Role::Button)
                            .aria_label(label)
                            .flex_1()
                            .min_w_0()
                            .rounded_md()
                            .px_2()
                            .py(px(6.))
                            .bg(content(if scope == value { 0.16 } else { 0.1 }))
                            .when(disabled, |d| d.opacity(0.4))
                            .text_color(content(if scope == value { 1. } else { 0.7 }))
                            .child(div().text_size(px(12.)).line_height(px(16.)).child(label))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(10.))
                                    .line_height(px(14.))
                                    .text_color(content(0.4))
                                    .child(hint),
                            )
                            .when(!disabled, |d| {
                                d.cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if let Some(e) = &mut this.management {
                                            e.scope = value.into();
                                        }
                                        cx.notify();
                                    }))
                            })
                    }),
                ),
            )
            .when_some(error, |d, error| {
                d.child(
                    div()
                        .mb_2()
                        .text_size(px(12.))
                        .text_color(content(0.7))
                        .child(error),
                )
            })
            .when(!raw.trim().is_empty() && !valid, |d| {
                d.child(
                    div()
                        .mb_2()
                        .text_size(px(12.))
                        .text_color(content(0.5))
                        .child("Use lowercase letters, numbers, and hyphens."),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .id("cancel-skill")
                            .role(Role::Button)
                            .aria_label("Cancel")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(content(0.5))
                            .when(busy, |d| d.opacity(0.4))
                            .when(!busy, |d| {
                                d.cursor_pointer()
                                    .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.close_management(window, cx)
                                    }))
                            }),
                    )
                    .child(
                        div()
                            .id("create-skill")
                            .role(Role::Button)
                            .aria_label("Create skill")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(content(0.2))
                            .text_size(px(12.))
                            .line_height(px(16.))
                            .text_color(content(1.))
                            .when(busy || !valid, |d| d.opacity(0.4))
                            .when(!busy && valid, |d| {
                                d.cursor_pointer().on_click(
                                    cx.listener(|this, _, _, cx| this.submit_management(cx)),
                                )
                            })
                            .child(if busy { "Creating…" } else { "Create" }),
                    ),
            )
            .into_any_element()
    }
    fn preview_skill(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        self.skill_preview = Some(path.clone());
        self.skill_preview_text = None;
        self.skill_documents.clear();
        self.skill_preview_generation += 1;
        let generation = self.skill_preview_generation;
        let work = cx.background_executor().spawn(async move {
            let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
            if size > 8 * 1024 * 1024 {
                return Err("SKILL.md is too large to preview.".into());
            }
            std::fs::read_to_string(path)
                .map(super::super::skill_document::DocumentData::new)
                .map_err(|e| format!("Could not read SKILL.md. {e}"))
        });
        self.skill_preview_task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.skill_preview_generation == generation {
                    this.skill_preview_text = Some(result);
                    this.skill_documents.clear();
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }
    pub(super) fn skills_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let query = self.skill_query.trim().to_lowercase();
        let skills = self
            .discovery
            .skills
            .iter()
            .filter(|s| {
                format!(
                    "{} {} {} {}",
                    s.name,
                    s.description,
                    s.path.display(),
                    s.provider
                )
                .to_lowercase()
                .contains(&query)
            })
            .cloned()
            .collect::<Vec<_>>();
        let adding = self
            .management
            .as_ref()
            .is_some_and(|e| matches!(e.target, Target::Skill));
        let busy = self.management.as_ref().is_some_and(|e| e.busy);
        let preview_open = self
            .skill_preview
            .as_ref()
            .is_some_and(|p| self.discovery.skills.iter().any(|s| &s.path == p));
        let split = self
            .skill_surface
            .get()
            .is_some_and(|b| b.size.width >= px(768.));
        let mut list = div()
            .mx_auto()
            .w_full()
            .max_w(px(1024.))
            .py_8()
            .map(|d| if preview_open { d.px_4() } else { d.px_8() })
            .child(page_header(
                "Skills",
                "Discover and manage file skills from project, personal, and harness folders.",
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .pb_3()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(content(0.4))
                                    .flex_shrink_0()
                                    .child(if self.discovery_loading {
                                        "…".into()
                                    } else {
                                        format!(
                                            "{} {}",
                                            skills.len(),
                                            if skills.len() == 1 { "skill" } else { "skills" }
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .h(px(28.))
                                    .max_w(px(208.))
                                    .min_w_0()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(content(0.1))
                                    .px_2()
                                    .child(icon("search-01", 14.))
                                    .child(
                                        div().min_w_0().flex_1().child(self.skill_filter.clone()),
                                    ),
                            )
                            .child(
                                icon_action(
                                    "refresh-skills",
                                    "Refresh skills",
                                    "refresh-cw",
                                    24.,
                                    14.,
                                    0.45,
                                    self.discovery_loading,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        if !this.discovery_loading {
                                            this.refresh(cx);
                                        }
                                    },
                                )),
                            ),
                    )
                    .child(
                        secondary_button(
                            "add-skill",
                            if adding {
                                "Close skill form"
                            } else {
                                "Add skill"
                            },
                            false,
                            busy,
                        )
                        .debug_selector(|| "settings-add-skill".into())
                        .child(if adding { "Close" } else { "Add skill" })
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this
                                .management
                                .as_ref()
                                .is_some_and(|e| matches!(e.target, Target::Skill))
                            {
                                this.close_management(window, cx);
                            } else {
                                this.open_management(Target::Skill, window, cx);
                            }
                        })),
                    ),
            );
        if adding {
            list = list.child(
                div()
                    .debug_selector(|| "settings-skill-form".into())
                    .mb_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(content(0.1))
                    .bg(content(0.03))
                    .child(self.skill_create_form(cx)),
            );
        }
        if self.discovery_loading {
            list = list.child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(content(0.45))
                    .child("Loading skills…"),
            );
        } else {
            let mut rows = Vec::new();
            if skills.is_empty() {
                rows.push(
                    div()
                        .px_3()
                        .py_3()
                        .text_size(px(12.))
                        .text_color(content(0.45))
                        .child(if self.discovery.skills.is_empty() {
                            "No skills yet."
                        } else {
                            "No matching skills"
                        })
                        .into_any_element(),
                );
            }
            let count = skills.len();
            for (ix, skill) in skills.into_iter().enumerate() {
                let path = skill.path.clone();
                let skill_path = path.display().to_string();
                let enabled = !self.prefs.disabled_skill_paths.contains(&skill_path);
                let preview = path.clone();
                let copy = path.clone();
                let reveal = path.clone();
                let name = skill.name.clone();
                rows.push(
                    div()
                        .id(SharedString::from(format!("settings-skill-{ix}")))
                        .when(!enabled, |d| d.opacity(0.5))
                        .when(self.skill_preview.as_ref() == Some(&path), |d| {
                            d.bg(content(0.05))
                        })
                        .when(ix + 1 < count, |d| {
                            d.border_b_1().border_color(content(0.05))
                        })
                        .px_3()
                        .py_2()
                        .line_height(px(16.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .id("preview-name")
                                        .role(Role::Button)
                                        .aria_label(format!("Preview {}", skill.name))
                                        .min_w_0()
                                        .flex_1()
                                        .truncate()
                                        .text_size(px(12.))
                                        .cursor_pointer()
                                        .child(skill.name.clone())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.preview_skill(preview.clone(), cx);
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .rounded_full()
                                        .bg(content(0.1))
                                        .px(px(6.))
                                        .py(px(2.))
                                        .text_size(px(10.))
                                        .text_color(content(0.6))
                                        .child(if skill.scope == "user" {
                                            "PERSONAL"
                                        } else if skill.scope == "builtin" {
                                            "MONOCODE"
                                        } else {
                                            "PROJECT"
                                        }),
                                )
                                .child(
                                    div()
                                        .w(px(80.))
                                        .flex_shrink_0()
                                        .truncate()
                                        .text_right()
                                        .text_size(px(11.))
                                        .text_color(content(0.4))
                                        .child(skill.provider),
                                )
                                .child(self.toggle(
                                    "include-skill",
                                    format!("Include {} in MonoCode catalog", skill.name),
                                    enabled,
                                    false,
                                    cx,
                                    move |this, on, cx| {
                                        this.update(cx, |p| {
                                            p.disabled_skill_paths.retain(|p| p != &skill_path);
                                            if !on {
                                                p.disabled_skill_paths.push(skill_path.clone());
                                            }
                                        })
                                    },
                                )),
                        )
                        .when(!skill.description.is_empty(), |d| {
                            d.child(
                                div()
                                    .mt(px(2.))
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(content(0.55))
                                    .child(skill.description),
                            )
                        })
                        .child(
                            div()
                                .mt(px(2.))
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    div()
                                        .min_w_0()
                                        .flex_1()
                                        .truncate()
                                        .text_size(px(11.))
                                        .text_color(content(0.35))
                                        .child(path.display().to_string()),
                                )
                                .child(
                                    icon_action(
                                        "skill-preview",
                                        format!("Preview skill {name}"),
                                        "eye",
                                        20.,
                                        12.,
                                        0.4,
                                        false,
                                    )
                                    .debug_selector(|| "skill-preview-action".into())
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.preview_skill(path.clone(), cx);
                                            cx.notify();
                                        },
                                    )),
                                )
                                .child(
                                    icon_action(
                                        "skill-copy",
                                        format!("Copy path of {name}"),
                                        "copy",
                                        20.,
                                        12.,
                                        0.4,
                                        false,
                                    )
                                    .debug_selector(|| "skill-copy-action".into())
                                    .on_click(
                                        move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                copy.display().to_string(),
                                            ))
                                        },
                                    ),
                                )
                                .child(
                                    icon_action(
                                        "skill-reveal",
                                        format!("Reveal {name} in file explorer"),
                                        "folder-open",
                                        20.,
                                        12.,
                                        0.4,
                                        false,
                                    )
                                    .debug_selector(|| "skill-reveal-action".into())
                                    .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
                                ),
                        )
                        .into_any_element(),
                );
            }
            list = list.child(
                div()
                    .overflow_hidden()
                    .rounded_lg()
                    .border_1()
                    .border_color(content(0.1))
                    .children(rows),
            );
        }
        list = list.child(div().pt_3().text_size(px(12.)).line_height(px(16.)).text_color(content(0.4))
            .child("Hidden skills stay on disk but leave MonoCode's picker. Provider-managed skills and commands are unchanged."));
        let surface = self.skill_surface.clone();
        let view = cx.weak_entity();
        let mut layout = div()
            .id("skills-surface")
            .debug_selector(|| "skills-surface".into())
            .relative()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .when(!split, |d| d.flex_col())
            .child(
                gpui::canvas(
                    move |bounds, window, _| {
                        let split = bounds.size.width >= px(768.);
                        let changed = surface
                            .get()
                            .is_none_or(|old| (old.size.width >= px(768.)) != split);
                        surface.set(Some(bounds));
                        if changed {
                            let view = view.clone();
                            window.on_next_frame(move |_, cx| {
                                let _ = view.update(cx, |_, cx| cx.notify());
                            });
                            window.request_animation_frame();
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .id("skills-list-scroll")
                    .debug_selector(|| "skills-list-scroll".into())
                    .min_w_0()
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(list),
            );
        if let Some(skill) = self
            .skill_preview
            .as_ref()
            .and_then(|p| self.discovery.skills.iter().find(|s| &s.path == p))
            .cloned()
        {
            let source = self
                .prefs
                .skill_source_paths
                .contains(&skill.path.display().to_string());
            let document = self
                .skill_preview_text
                .as_ref()
                .and_then(|r| r.as_ref().ok())
                .cloned()
                .map(|data| {
                    self.skill_documents
                        .entry((skill.path.clone(), source))
                        .or_insert_with(|| {
                            cx.new(|_| {
                                super::super::skill_document::SkillDocument::new(data, source)
                            })
                        })
                        .clone()
                });
            layout = layout.child(
                div()
                    .id("skill-preview-pane")
                    .debug_selector(|| "skill-preview-pane".into())
                    .role(Role::Region)
                    .aria_label("Skill preview")
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .when(split, |d| d.max_w(px(720.)).border_l_1())
                    .when(!split, |d| d.border_t_1())
                    .border_color(content(0.1))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .px_4()
                            .pt_4()
                            .pb_2()
                            .flex_shrink_0()
                            .flex()
                            .items_start()
                            .gap_2()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .text_size(px(16.))
                                    .line_height(px(24.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(skill.name),
                            )
                            .child(
                                icon_action(
                                    "close-skill-preview",
                                    "Close skill preview",
                                    "x",
                                    24.,
                                    14.,
                                    0.45,
                                    false,
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.skill_preview = None;
                                        window.focus(
                                            &this.skill_filter.read(cx).focus_handle(cx),
                                            cx,
                                        );
                                        cx.notify();
                                    },
                                )),
                            ),
                    )
                    .child(
                        div()
                            .px_4()
                            .pt_1()
                            .pb_3()
                            .flex_shrink_0()
                            .border_b_1()
                            .border_color(content(0.1))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .line_height(px(16.))
                                    .text_color(content(0.5))
                                    .child(skill.path.display().to_string()),
                            )
                            .child(
                                div()
                                    .mt_3()
                                    .flex()
                                    .justify_end()
                                    .child(self.skill_mode_toggle(source, cx)),
                            ),
                    )
                    .child(div().min_w_0().min_h_0().flex_1().child(
                        if let Some(Err(error)) = &self.skill_preview_text {
                            div()
                                .px_4()
                                .py_5()
                                .id("skill-read-error")
                                .role(Role::Alert)
                                .text_size(px(12.))
                                .text_color(gpui::rgb(0xf87171))
                                .child(error.clone())
                                .into_any_element()
                        } else if let Some(document) = document {
                            document.into_any_element()
                        } else {
                            note("Loading skill…")
                        },
                    )),
            );
        }
        vec![layout.into_any_element()]
    }
    fn skill_mode_toggle(&self, source: bool, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("skill-document-mode")
            .role(Role::TabList)
            .aria_label("Markdown view")
            .flex()
            .rounded_md()
            .border_1()
            .border_color(content(0.1))
            .bg(content(0.1))
            .p(px(2.))
            .children(
                [(false, "Preview"), (true, "Source")]
                    .into_iter()
                    .map(|(value, label)| {
                        div()
                            .id(label)
                            .debug_selector(move || {
                                format!("settings-skill-document-{}", label.to_lowercase())
                            })
                            .role(Role::Tab)
                            .aria_label(label)
                            .aria_selected(source == value)
                            .rounded(px(4.))
                            .px_2()
                            .py(px(2.))
                            .text_size(px(11.))
                            .line_height(px(16.))
                            .text_color(content(if source == value { 1. } else { 0.45 }))
                            .when(source == value, |d| d.bg(content(0.12)))
                            .cursor_pointer()
                            .hover(|s| s.text_color(content(0.8)))
                            .child(label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(path) = &this.skill_preview {
                                    let path = path.display().to_string();
                                    this.update(cx, |p| {
                                        p.skill_source_paths.retain(|s| s != &path);
                                        if value {
                                            p.skill_source_paths.push(path);
                                        }
                                    });
                                }
                            }))
                    }),
            )
            .into_any_element()
    }
}
