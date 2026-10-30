use crate::{
    settings::{
        ModelChoice, Preferences, SettingsView,
        widgets::{note, secondary_button},
    },
    theme::*,
};
use gpui::{AnyElement, Context, Focusable, SharedString, div, prelude::*, px};
use monocode_core::provider::Kind;
impl SettingsView {
    pub(super) fn providers_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = self.prefs.clone();
        let accounts = if let Some(center) = crate::accounts::get(cx) {
            center.update(cx, |center, cx| {
                center.mask_emails = p.mask_emails;
                center.show_remaining = p.show_remaining_usage;
                for kind in monocode_core::accounts::PROVIDERS {
                    center.load_provider(kind, false, cx);
                }
                center.settings_rows(cx)
            })
        } else {
            vec![note("Provider accounts load with the workspace.")]
        };
        let usage = vec![
            self.row(
                Some("show-remaining-usage"),
                true,
                "Show remaining usage",
                Some("Show remaining capacity in usage meters.".into()),
                vec![self.toggle(
                    "show-remaining-usage",
                    "Show remaining usage",
                    p.show_remaining_usage,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.show_remaining_usage = on),
                )],
            ),
            self.row(
                Some("mask-emails"),
                false,
                "Mask account emails",
                Some("Hide email addresses until clicked.".into()),
                vec![self.toggle(
                    "mask-emails",
                    "Mask account emails",
                    p.mask_emails,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.mask_emails = on),
                )],
            ),
        ];
        let mut options = vec![(
            "global".into(),
            SharedString::from("Global"),
            Some(icon("globe", 14.).into_any_element()),
        )];
        options.extend(self.ctx.projects.iter().map(|(path, label)| {
            (
                path.clone(),
                SharedString::from(label.clone()),
                Some(icon("folder", 14.).into_any_element()),
            )
        }));
        let scope = self.provider_scope.clone();
        let action = self.select(
            "provider-scope",
            "Provider defaults scope",
            &scope,
            options,
            None,
            cx,
            |this, value, cx| {
                this.provider_scope = value;
                cx.notify();
            },
        );
        let mode = p.new_chat_mode().to_owned();
        // Global per-provider models only steer new chats in "pinned" mode;
        // a project's model always does.
        let model_selects = scope != "global" || mode == "pinned";
        let binaries = self.discovery.binaries.clone();
        let mut clis = vec![
            self.row(
                Some("new-chat-model"),
                true,
                "New chats start with",
                Some(
                    match mode.as_str() {
                        "default" => "Each provider's own default model.",
                        "pinned" => "The model chosen for each provider below.",
                        _ => "The model you last picked for each provider.",
                    }
                    .into(),
                ),
                vec![self.segmented(
                    "new-chat-model",
                    "New chats start with",
                    &mode,
                    &[
                        ("last", "Last used"),
                        ("default", "Provider default"),
                        ("pinned", "Chosen model"),
                    ],
                    cx,
                    |this, mode, cx| this.update(cx, |p| p.new_chat_model = Some(mode.into())),
                )],
            ),
        ];
        for kind in Kind::ALL {
            let installed = binaries
                .iter()
                .find(|(k, _)| *k == kind)
                .and_then(|(_, p)| p.as_ref());
            let project = if scope == "global" {
                None
            } else {
                p.project_providers.get(&scope)
            };
            let selected = project
                .and_then(|p| p.provider.as_deref())
                .or(p.default_provider.as_deref())
                == Some(kind.id());
            let globally_hidden = p.hidden_providers.iter().any(|id| id == kind.id());
            let locked = scope != "global" && globally_hidden;
            let visible = !globally_hidden
                && !project.is_some_and(|p| p.hidden.iter().any(|id| id == kind.id()));
            let models = self
                .ctx
                .catalogs
                .get(kind.id())
                .filter(|models| !models.is_empty())
                .or_else(|| self.model_catalogs.get(&kind))
                .cloned()
                .unwrap_or_default();
            let selected_model = project
                .and_then(|p| p.models.get(kind.id()))
                .or(p.default_models.get(kind.id()))
                .cloned()
                .or_else(|| models.first().map(|m| m.0.clone()));
            let label = div()
                .flex()
                .items_center()
                .gap_2()
                .child(provider_icon(kind.id(), 16.))
                .child(kind.label())
                .child({
                    let slot = self
                        .binary_anchors
                        .borrow_mut()
                        .entry(kind)
                        .or_default()
                        .clone();
                    secondary_button(
                        SharedString::from(format!("binary-{}", kind.id())),
                        format!("Show {} CLI details", kind.label()),
                        false,
                        false,
                    )
                    .relative()
                    .size_6()
                    .p_0()
                    .justify_center()
                    .child(slot.probe())
                    .child(icon("folder-open", 14.))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_management(
                            super::super::management::Target::Cli(kind),
                            window,
                            cx,
                        )
                    }))
                })
                .child(
                    div()
                        .rounded_full()
                        .bg(content(0.1))
                        .px(px(6.))
                        .py(px(2.))
                        .text_size(px(10.))
                        .text_color(content(0.6))
                        .child(if installed.is_some() {
                            "INSTALLED"
                        } else {
                            "NOT INSTALLED"
                        }),
                );
            let mut controls = vec![
                secondary_button(
                    SharedString::from(format!("default-provider-{}", kind.id())),
                    if selected {
                        "Default"
                    } else {
                        "Use by default"
                    },
                    false,
                    selected || selected_model.is_none() || installed.is_none(),
                )
                .child(if selected {
                    "Default"
                } else {
                    "Use by default"
                })
                .when(
                    !selected && selected_model.is_some() && installed.is_some(),
                    |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| {
                            let scope = this.provider_scope.clone();
                            this.update(cx, |p| {
                                if scope == "global" {
                                    p.default_provider = Some(kind.id().into());
                                } else {
                                    p.project_providers.entry(scope).or_default().provider =
                                        Some(kind.id().into());
                                }
                            })
                        }))
                    },
                )
                .into_any_element(),
            ];
            if let Some(model) = selected_model.as_ref().filter(|_| model_selects) {
                controls.insert(
                    0,
                    self.select(
                        kind.id(),
                        kind.label(),
                        model,
                        models
                            .iter()
                            .map(|(id, name)| (id.clone(), SharedString::from(name.clone()), None))
                            .collect(),
                        None,
                        cx,
                        move |this, model, cx| {
                            let scope = this.provider_scope.clone();
                            this.update(cx, |p| {
                                if scope == "global" {
                                    p.default_models.insert(kind.id().into(), model);
                                } else {
                                    p.project_providers
                                        .entry(scope)
                                        .or_default()
                                        .models
                                        .insert(kind.id().into(), model);
                                }
                            });
                        },
                    ),
                );
            }
            let hidden_models = if scope == "global" {
                p.hidden_models.clone()
            } else {
                p.hidden_models_for(&scope)
            }
            .into_iter()
            .filter(|c| {
                c.provider == kind.id()
                    && models.iter().any(|(id, _)| c.model.as_deref() == Some(id))
            })
            .count();
            let open = self.models_open == Some(kind);
            if installed.is_some() && !models.is_empty() {
                controls.push(
                    secondary_button(
                        SharedString::from(format!("models-{}", kind.id())),
                        format!("Choose which {} models the picker shows", kind.label()),
                        false,
                        false,
                    )
                    .aria_expanded(open)
                    .gap_1()
                    .child("Models")
                    .child(icon(if open { "chevron-up" } else { "chevron-down" }, 12.))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let open = this.models_open != Some(kind);
                        this.models_open = open.then_some(kind);
                        this.model_query.clear();
                        this.model_filter.update(cx, |input, cx| {
                            input.set_text(String::new(), cx);
                            if open {
                                window.focus(&input.focus_handle(cx), cx);
                            }
                        });
                        cx.notify();
                    }))
                    .into_any_element(),
                );
            }
            if installed.is_some() {
                controls.push(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(content(0.5))
                                .child(if locked {
                                    "Hidden globally"
                                } else {
                                    "Show in picker"
                                }),
                        )
                        .child(self.toggle(
                            kind.id(),
                            "Show provider in the model picker",
                            visible,
                            locked,
                            cx,
                            move |this, on, cx| {
                                let scope = this.provider_scope.clone();
                                this.update(cx, |p| {
                                    let hidden = if scope == "global" {
                                        &mut p.hidden_providers
                                    } else {
                                        &mut p.project_providers.entry(scope).or_default().hidden
                                    };
                                    hidden.retain(|id| id != kind.id());
                                    if !on {
                                        hidden.push(kind.id().into());
                                    }
                                })
                            },
                        ))
                        .into_any_element(),
                );
            }
            let description = installed
                .map(|_| {
                    let mut text = format!(
                        "{} {} available",
                        models.len(),
                        if models.len() == 1 { "model" } else { "models" }
                    );
                    if hidden_models > 0 {
                        text.push_str(&format!(", {hidden_models} hidden"));
                    }
                    text + "."
                })
                .unwrap_or_else(|| kind.unavailable_hint());
            clis.push(self.row(None, false, label, Some(description.into()), controls));
            if open && installed.is_some() {
                clis.push(self.model_visibility(kind, &scope, &models, cx));
            }
        }
        if self.discovery_loading {
            clis.insert(1, note("Checking installed CLIs…"));
        }
        let advanced=vec![
            self.row(Some("idle-agents"),true,"Keep idle agents ready",Some("Keep up to this many idle agents running for faster follow-ups. Uses additional memory.".into()),vec![self.segmented("idle-agents","Keep idle agents ready",&p.idle_agents.to_string(),&[("0","None"),("1","1"),("2","2"),("3","3"),("5","5")],cx,|this,value,cx|if let Ok(value)=value.parse(){this.update(cx,|p|p.idle_agents=value)})]),
            self.row(Some("claude-hooks"),false,"Claude Code hooks",Some("Run your configured Claude Code hooks. Changes apply on the next turn.".into()),vec![self.toggle("claude-hooks","Claude Code hooks",p.claude_hooks,false,cx,|this,on,cx|this.update(cx,|p|p.claude_hooks=on))]),
            self.row(Some("claude-entrypoint"),false,"Identify as Monocode to Claude (experimental)",Some("Report Monocode as the client instead of the default sdk-cli, which community tests show uses the 5-hour limit up to ~2x faster. Anthropic hasn't confirmed this or whether relabeling is allowed. Applies to new sessions.".into()),vec![self.toggle("claude-entrypoint","Identify as Monocode to Claude",p.claude_entrypoint,false,cx,|this,on,cx|this.update(cx,|p|p.claude_entrypoint=on))])];
        vec![
            self.group(
                Some("provider-accounts"),
                true,
                "Accounts",
                Some("Manage sign-ins and choose accounts for each project.".into()),
                Some(self.refresh_button("refresh-provider-accounts", "Refresh", cx)),
                accounts,
            ),
            self.group(None, false, "Usage and privacy", None, None, usage),
            self.group(
                Some("agent-clis"),
                false,
                "Agent CLIs",
                Some("Choose available agents and default models for new conversations.".into()),
                Some(action),
                clis,
            ),
            self.group(None, false, "Advanced", None, None, advanced),
        ]
    }
}
impl SettingsView {
    /// The open CLI's models, each with a switch for the model picker.
    /// At project scope a model hidden globally stays off and locked.
    fn model_visibility(
        &mut self,
        kind: Kind,
        scope: &str,
        models: &[(String, String)],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = &self.prefs;
        let choice = |id: &str| ModelChoice {
            provider: kind.id().into(),
            model: Some(id.into()),
        };
        let globally = |id: &str| p.hidden_models.contains(&choice(id));
        let hidden_here = |id: &str| {
            if scope == "global" {
                globally(id)
            } else {
                p.project_providers
                    .get(scope)
                    .is_some_and(|s| s.hidden_models.contains(&choice(id)))
            }
        };
        let query = self.model_query.trim().to_lowercase();
        let shown: Vec<(String, SharedString)> = models
            .iter()
            .filter(|(id, _)| id != "default")
            .map(|(id, name)| {
                (
                    id.clone(),
                    crate::pickers::model_display_name(kind, id, name),
                )
            })
            .filter(|(id, name)| {
                query.is_empty()
                    || name.to_lowercase().contains(&query)
                    || id.to_lowercase().contains(&query)
            })
            .collect();
        let editable: Vec<String> = shown
            .iter()
            .filter(|(id, _)| scope == "global" || !globally(id))
            .map(|(id, _)| id.clone())
            .collect();
        let any_hidden = editable.iter().any(|id| hidden_here(id));
        let any_shown = editable.iter().any(|id| !hidden_here(id));
        let bulk = |label: &'static str, hide: bool, enabled: bool, cx: &mut Context<Self>| {
            let ids = editable.clone();
            secondary_button(
                SharedString::from(format!("models-{}-{label}", kind.id())),
                label,
                false,
                !enabled,
            )
            .child(label)
            .when(enabled, |d| {
                d.on_click(cx.listener(move |this, _, _, cx| {
                    let ids = ids.clone();
                    let scope = this.provider_scope.clone();
                    this.update(cx, |p| {
                        for id in &ids {
                            set_hidden(p, &scope, kind, id, hide);
                        }
                    })
                }))
            })
            .into_any_element()
        };
        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .px_4()
            .py_2()
            .child(
                div()
                    .h(px(28.))
                    .max_w(px(240.))
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
                    .child(div().min_w_0().flex_1().child(self.model_filter.clone())),
            )
            .child(div().flex_1())
            .child(bulk("Show all", false, any_hidden, cx))
            .child(bulk("Hide all", true, any_shown, cx));
        let rows: Vec<AnyElement> = shown
            .iter()
            .map(|(id, name)| {
                let locked = scope != "global" && globally(id);
                let visible = !locked && !hidden_here(id);
                let model = id.clone();
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .py(px(6.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .items_baseline()
                            .gap_2()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.))
                                    .text_color(content(if visible { 1. } else { 0.5 }))
                                    .child(name.clone()),
                            )
                            .when(name.as_ref() != id, |d| {
                                d.child(
                                    div()
                                        .truncate()
                                        .text_size(px(11.))
                                        .text_color(content(0.35))
                                        .child(id.clone()),
                                )
                            }),
                    )
                    .when(locked, |d| {
                        d.child(
                            div()
                                .text_size(px(12.))
                                .text_color(content(0.5))
                                .child("Hidden globally"),
                        )
                    })
                    .child(self.toggle(
                        format!("model-{}-{id}", kind.id()),
                        format!("Show {name} in the model picker"),
                        visible,
                        locked,
                        cx,
                        move |this, on, cx| {
                            let scope = this.provider_scope.clone();
                            this.update(cx, |p| set_hidden(p, &scope, kind, &model, !on))
                        },
                    ))
                    .into_any_element()
            })
            .collect();
        div()
            .debug_selector(move || format!("settings-models-{}", kind.id()))
            .mx_4()
            .mb_3()
            .rounded_md()
            .border_1()
            .border_color(content(0.07))
            .bg(content(0.02))
            .child(header)
            .when(rows.is_empty(), |d| d.child(note("No matching models.")))
            .child(
                div()
                    .id(SharedString::from(format!("models-list-{}", kind.id())))
                    .max_h(px(320.))
                    .overflow_y_scroll()
                    .pb_1()
                    .children(rows),
            )
            .into_any_element()
    }
}

/// Hide or show a model at one scope: "global" or a project path.
fn set_hidden(p: &mut Preferences, scope: &str, kind: Kind, model: &str, hide: bool) {
    let choice = ModelChoice {
        provider: kind.id().into(),
        model: Some(model.into()),
    };
    let hidden = if scope == "global" {
        &mut p.hidden_models
    } else {
        &mut p
            .project_providers
            .entry(scope.into())
            .or_default()
            .hidden_models
    };
    hidden.retain(|old| old != &choice);
    if hide {
        hidden.push(choice);
    }
}
