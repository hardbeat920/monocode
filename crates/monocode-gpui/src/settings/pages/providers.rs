use crate::{
    settings::{
        SettingsView,
        widgets::{note, secondary_button},
    },
    theme::*,
};
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
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
        let binaries = self.discovery.binaries.clone();
        let mut clis = Vec::new();
        for (index, kind) in Kind::ALL.into_iter().enumerate() {
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
            if let Some(model) = &selected_model {
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
                    format!(
                        "{} {} available.",
                        models.len(),
                        if models.len() == 1 { "model" } else { "models" }
                    )
                })
                .unwrap_or_else(|| kind.unavailable_hint());
            clis.push(self.row(None, index == 0, label, Some(description.into()), controls));
        }
        if self.discovery_loading {
            clis.insert(0, note("Checking installed CLIs…"));
        }
        let advanced=vec![
            self.row(Some("idle-agents"),true,"Keep idle agents ready",Some("Keep up to this many idle agents running for faster follow-ups. Uses additional memory.".into()),vec![self.segmented("idle-agents","Keep idle agents ready",&p.idle_agents.to_string(),&[("0","None"),("1","1"),("2","2"),("3","3"),("5","5")],cx,|this,value,cx|if let Ok(value)=value.parse(){this.update(cx,|p|p.idle_agents=value)})]),
            self.row(Some("claude-hooks"),false,"Claude Code hooks",Some("Run your configured Claude Code hooks. Changes apply on the next turn.".into()),vec![self.toggle("claude-hooks","Claude Code hooks",p.claude_hooks,false,cx,|this,on,cx|this.update(cx,|p|p.claude_hooks=on))])];
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
