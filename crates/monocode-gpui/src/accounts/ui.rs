use super::masked::masked_text;
use super::*;
use crate::{
    settings::widgets::{primary_button, secondary_button},
    theme::*,
};
use gpui::{AnyElement, App, Div, FontWeight, MouseButton, Role, SharedString, div, px};
fn account_button(
    id: SharedString,
    label: SharedString,
    path: &'static str,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .size_7()
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .text_color(content(0.4))
        .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
        .child(icon(path, 14.))
}

impl Accounts {
    fn editor_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(editor) = &self.editor else {
            return div().into_any_element();
        };
        if editor.surface == Surface::Footer {
            let back = cx.listener(|this, _: &(), window, cx| {
                if !this.working {
                    if let Some(focus) = this.editor.take().and_then(|e| e.previous_focus) {
                        window.focus(&focus, cx);
                    }
                    this.error = None;
                    cx.notify();
                }
            });
            return div()
                .child(back_header(
                    "cancel-account-edit",
                    "Back to accounts",
                    format!("Add {} account", editor.kind.label()),
                    move |window, cx| back(&(), window, cx),
                ))
                .child(
                    div()
                        .mt_1()
                        .px_1()
                        .text_size(px(10.))
                        .line_height(px(16.))
                        .text_color(content(0.4))
                        .child(
                            "Give this account a local name, then finish sign-in in your browser.",
                        ),
                )
                .child(
                    div()
                        .mt_3()
                        .text_size(px(10.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(content(0.55))
                        .child("Account name"),
                )
                .child(
                    div()
                        .mt(px(6.))
                        .h_8()
                        .w_full()
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .rounded_lg()
                        .bg(content(0.04))
                        .border_1()
                        .border_color(content(0.1))
                        .text_size(px(11.))
                        .when(self.working, |d| d.opacity(0.55))
                        .child(editor.input.clone()),
                )
                .child(
                    primary_button(
                        "save-account-edit",
                        "Sign in and add account",
                        self.working || editor.input.read(cx).content.trim().is_empty(),
                    )
                    .mt_3()
                    .h_8()
                    .w_full()
                    .rounded_lg()
                    .justify_center()
                    .gap(px(6.))
                    .text_size(px(11.))
                    .when(self.working, |d| {
                        d.child(crate::pulse::looping(
                            "footer-account-spinner",
                            std::time::Duration::from_secs(1),
                            crate::pulse::Rate::Smooth,
                            move |delta| {
                                icon("refresh-cw", 14.).with_transformation(
                                    gpui::Transformation::rotate(gpui::percentage(delta)),
                                )
                            },
                        ))
                    })
                    .child(if self.working {
                        "Waiting for browser…"
                    } else {
                        "Sign in and add account"
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.submit_editor(cx))),
                )
                .children(self.error.as_ref().map(|error| {
                    div()
                        .mt_2()
                        .text_size(px(10.))
                        .line_height(px(16.))
                        .text_color(gpui::rgb(0xf87171))
                        .child(error.clone())
                }))
                .into_any_element();
        }
        let adding = editor.id.is_none();
        div()
            .h(px(48.))
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(content(0.05))
            .flex()
            .items_center()
            .child(
                div()
                    .h_8()
                    .min_w_0()
                    .flex_1()
                    .pr_1()
                    .rounded_md()
                    .overflow_hidden()
                    .border_1()
                    .border_color(content(0.1))
                    .bg(content(0.04))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .px(px(10.))
                            .when(self.working, |d| d.opacity(0.5))
                            .child(editor.input.clone()),
                    )
                    .child(
                        secondary_button("cancel-account-edit", "Cancel", false, self.working)
                            .h_6()
                            .border_0()
                            .px(px(10.))
                            .rounded(px(4.5))
                            .bg(content(0.05))
                            .text_size(px(11.))
                            .text_color(content(0.45))
                            .child("Cancel")
                            .on_click(cx.listener(|this, _, window, cx| {
                                if !this.working {
                                    if let Some(focus) =
                                        this.editor.take().and_then(|e| e.previous_focus)
                                    {
                                        window.focus(&focus, cx);
                                    }
                                    this.error = None;
                                    this.signin = None;
                                    cx.notify();
                                }
                            })),
                    )
                    .child(
                        primary_button(
                            "save-account-edit",
                            if adding {
                                "Sign in and add"
                            } else {
                                "Save account"
                            },
                            self.working || editor.input.read(cx).content.trim().is_empty(),
                        )
                        .ml_1()
                        .h_6()
                        .px(px(10.))
                        .rounded(px(4.5))
                        .gap(px(6.))
                        .text_size(px(11.))
                        .when(self.working, |d| {
                            d.child(crate::pulse::looping(
                                "account-editor-spinner",
                                std::time::Duration::from_secs(1),
                                crate::pulse::Rate::Smooth,
                                move |delta| {
                                    icon("refresh-cw", 12.).with_transformation(
                                        gpui::Transformation::rotate(gpui::percentage(delta)),
                                    )
                                },
                            ))
                        })
                        .child(if adding {
                            if self.working {
                                "Waiting for browser…"
                            } else {
                                "Sign in and add"
                            }
                        } else {
                            "Save"
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.submit_editor(cx))),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn settings_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows = Vec::new();
        for kind in accounts::PROVIDERS {
            let accounts = self.book.list(kind);
            let count = accounts.len();
            let adding = self.editor.as_ref().is_some_and(|e| {
                e.kind == kind && e.id.is_none() && e.surface == Surface::Settings
            });
            let header = div()
                .px_4()
                .py(px(14.))
                .flex()
                .items_center()
                .gap_4()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .size_7()
                                .rounded_lg()
                                .bg(content(0.05))
                                .border_1()
                                .border_color(content(0.06))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(provider_icon(kind.id(), 16.)),
                        )
                        .child(
                            div()
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(kind.label()),
                                )
                                .child(
                                    div()
                                        .mt(px(2.))
                                        .text_size(px(11.))
                                        .text_color(content(0.4))
                                        .child(format!(
                                            "{count} {}",
                                            if count == 1 { "account" } else { "accounts" }
                                        )),
                                ),
                        ),
                )
                .when(accounts::supports_profiles(kind), |d| {
                    d.child(
                        secondary_button(
                            SharedString::from(format!("add-account-{}", kind.id())),
                            "Add account",
                            false,
                            self.working,
                        )
                        .child(icon("plus", 14.))
                        .child("Add account")
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.start_edit(kind, None, Surface::Settings, window, cx)
                            },
                        )),
                    )
                });
            let mut body = div()
                .border_t_1()
                .border_color(content(0.05))
                .bg(content(0.015))
                .pl_10();
            for account in accounts {
                if self.editor.as_ref().is_some_and(|e| {
                    e.kind == kind
                        && e.id.as_ref() == Some(&account.id)
                        && e.surface == Surface::Settings
                }) {
                    body = body.child(self.editor_row(cx));
                    continue;
                }
                let id = account.id.clone();
                let rename_id = id.clone();
                let key = (kind, id.clone());
                let identity = self.identities.get(&key);
                let status = self.status(kind, &id);
                let subtitle = identity
                    .and_then(|i| i.email.clone().or(i.name.clone()))
                    .unwrap_or_else(|| {
                        if account.is_default() {
                            "Provider CLI profile".into()
                        } else {
                            "Isolated profile".into()
                        }
                    });
                let masked = self.mask_emails
                    && identity.and_then(|i| i.email.as_ref()).is_some()
                    && !self.revealed.contains(&key);
                let org = identity
                    .and_then(|i| i.organization.as_ref().or(i.plan.as_ref()))
                    .cloned();
                let mut controls = div()
                    .w(px(96.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_1();
                if account.is_default() {
                    controls = controls.child(
                        div()
                            .mr_1()
                            .text_size(px(10.))
                            .text_color(content(0.3))
                            .child("DEFAULT"),
                    );
                }
                controls = controls.child(
                    account_button(
                        SharedString::from(format!("rename-{id}-{}", kind.id())),
                        SharedString::from(format!("Rename {}", account.label)),
                        "pencil-edit-02",
                    )
                    .size_7()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.start_edit(
                            kind,
                            Some(rename_id.clone()),
                            Surface::Settings,
                            window,
                            cx,
                        )
                    })),
                );
                if !account.is_default() {
                    let remove = account.clone();
                    controls = controls.child(
                        account_button(
                            SharedString::from(format!("remove-{id}-{}", kind.id())),
                            SharedString::from(format!("Remove {}", account.label)),
                            "delete-02",
                        )
                        .size_7()
                        .on_click(cx.listener(
                            move |this, _, window, cx| this.remove(remove.clone(), window, cx),
                        )),
                    );
                }
                let row = div()
                    .h_12()
                    .border_b_1()
                    .border_color(content(0.05))
                    .px_4()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .truncate()
                                            .child(account.label.clone()),
                                    )
                                    .children(org.map(|org| {
                                        div()
                                            .px_1()
                                            .rounded_sm()
                                            .bg(content(0.07))
                                            .text_size(px(9.))
                                            .text_color(content(0.5))
                                            .child(org)
                                    })),
                            )
                            .child(
                                div()
                                    .mt(px(2.))
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .text_size(px(10.))
                                    .child(status_label(&status).flex_shrink_0())
                                    .child(
                                        div()
                                            .id(SharedString::from(format!(
                                                "account-email-{}-{id}",
                                                kind.id()
                                            )))
                                            .when(
                                                self.mask_emails
                                                    && identity
                                                        .and_then(|i| i.email.as_ref())
                                                        .is_some(),
                                                |d| {
                                                    d.role(Role::Button).aria_label(if masked {
                                                        "Reveal email"
                                                    } else {
                                                        "Hide email"
                                                    })
                                                },
                                            )
                                            .text_color(content(0.3))
                                            .relative()
                                            .truncate()
                                            .child(masked_text(subtitle, masked))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if this.mask_emails && !this.revealed.remove(&key) {
                                                    this.revealed.insert(key.clone());
                                                }
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    .child(self.meters(kind, &id))
                    .child(controls);
                body = body.child(row);
            }
            if adding {
                body = body.child(self.editor_row(cx));
            }
            rows.push(
                div()
                    .border_b_1()
                    .border_color(content(0.05))
                    .child(header)
                    .child(body)
                    .into_any_element(),
            );
        }
        if let Some(error) = &self.error {
            rows.push(
                div()
                    .id("account-settings-error")
                    .role(Role::Alert)
                    .px_4()
                    .py_2()
                    .text_size(px(11.))
                    .text_color(gpui::rgb(0xf87171))
                    .child(error.clone())
                    .into_any_element(),
            );
        }
        rows
    }
    fn meters(&self, kind: Kind, id: &str) -> AnyElement {
        let key = Key::new(kind, self.profile(id), None);
        let Some(snapshot) = self.usage.get(&key).and_then(|s| s.snapshot.as_ref()) else {
            return div().into_any_element();
        };
        let now = usage::now_ms();
        div()
            .flex()
            .items_center()
            .gap_3()
            .children(snapshot.windows.iter().map(|w| {
                let used = if w.resets_at.is_some_and(|at| at <= now) {
                    0.
                } else {
                    w.used_percent
                };
                let percent = if self.show_remaining {
                    100. - used
                } else {
                    used
                };
                div()
                    .w(px(64.))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_size(px(9.))
                            .text_color(content(0.4))
                            .child(w.label())
                            .child(format!("{percent:.0}%")),
                    )
                    .child(
                        div()
                            .relative()
                            .mt_1()
                            .h(px(3.))
                            .rounded_full()
                            .bg(content(0.08))
                            .overflow_hidden()
                            .child(
                                div()
                                    .h_full()
                                    .w(px(64. * percent as f32 / 100.))
                                    .rounded_full()
                                    .bg(if used >= 100. {
                                        gpui::rgb(0xf87171).into()
                                    } else if used >= 80. {
                                        gpui::rgb(0xfbbf24).into()
                                    } else {
                                        content(0.55)
                                    }),
                            )
                            .children(pace_marker(w, now, self.show_remaining)),
                    )
            }))
            .into_any_element()
    }
    /// ProviderAccountSubtitle: "Pro · email", the email blurred until clicked.
    pub(crate) fn subtitle(&self, kind: Kind, id: &str, cx: &mut Context<Self>) -> Option<Div> {
        let identity = self.identities.get(&(kind, id.to_owned()))?;
        if identity.plan.is_none() && identity.email.is_none() {
            return None;
        }
        let key = (kind, id.to_owned());
        let masked = self.mask_emails && !self.revealed.contains(&key);
        Some(
            div()
                .flex()
                .items_baseline()
                .gap_1()
                .min_w_0()
                .children(
                    identity
                        .plan
                        .clone()
                        .map(|plan| div().flex_shrink_0().child(plan)),
                )
                .when(identity.plan.is_some() && identity.email.is_some(), |d| {
                    d.child("·")
                })
                .children(identity.email.clone().map(|email| {
                    div()
                        .id(SharedString::from(format!(
                            "account-email-{}-{id}",
                            kind.id()
                        )))
                        .role(Role::Button)
                        .aria_label(if masked { "Reveal email" } else { "Hide email" })
                        .min_w_0()
                        .relative()
                        .truncate()
                        .child(masked_text(email, masked))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.mask_emails && !this.revealed.remove(&key) {
                                this.revealed.insert(key.clone());
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }))
                })),
        )
    }
    pub(crate) fn picker(
        &self,
        kind: Kind,
        selected: &str,
        project: &str,
        session: Option<i64>,
        on_back: impl Fn(&mut Window, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let adding = self
            .editor
            .as_ref()
            .is_some_and(|e| e.kind == kind && e.surface == Surface::Footer);
        if adding {
            return self.editor_row(cx);
        }
        let now = usage::now_ms();
        let mut rows = div()
            .id("provider-account-choices")
            .role(Role::Group)
            .aria_label(format!("{} accounts", kind.label()))
            .mt_2()
            .flex()
            .flex_col()
            .gap_1();
        for (index, account) in self.book.list(kind).into_iter().enumerate() {
            let status = self.status(kind, &account.id);
            let active = account.id == selected;
            let id = account.id.clone();
            let project = project.to_owned();
            let org = self
                .identities
                .get(&(kind, id.clone()))
                .and_then(organization_tag);
            let windows = self
                .usage
                .get(&Key::new(kind, self.profile(&id), None))
                .and_then(|s| s.snapshot.as_ref())
                .map(|s| s.windows.clone())
                .unwrap_or_default();
            let highlighted = self
                .picker
                .as_ref()
                .is_some_and(|p| p.kind == kind && p.index == index);
            let fill = if active {
                accent(if highlighted { 0.14 } else { 0.1 })
            } else {
                content(if highlighted { 0.075 } else { 0.035 })
            };
            rows = rows.child(
                div()
                    .id(SharedString::from(format!("pick-account-{}", id)))
                    .role(Role::MenuItem)
                    .aria_label(account.label.clone())
                    .px(px(10.))
                    .py_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(if active { accent(0.2) } else { content(0.06) })
                    .bg(fill)
                    .hover(move |s| s.bg(if active { accent(0.14) } else { content(0.075) }))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap_3()
                    .text_size(px(11.))
                    .text_color(content(if active { 1. } else { 0.7 }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .py(px(2.))
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap(px(6.))
                                    .min_w_0()
                                    .child(div().flex_shrink_0().child(account.label))
                                    .children(
                                        self.subtitle(kind, &id, cx).map(|d| {
                                            d.text_size(px(10.)).text_color(content(0.35))
                                        }),
                                    )
                                    .children(org.map(|org| {
                                        div()
                                            .max_w(px(96.))
                                            .flex_shrink_0()
                                            .truncate()
                                            .px_1()
                                            .rounded_sm()
                                            .bg(content(0.07))
                                            .text_size(px(9.))
                                            .line_height(px(16.))
                                            .text_color(content(0.5))
                                            .child(org)
                                    })),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .min_w_0()
                                    .text_size(px(10.))
                                    .child(status_label(&status).map(|d| {
                                        if windows.is_empty() {
                                            d.min_w_0()
                                        } else {
                                            d.flex_shrink_0()
                                        }
                                    }))
                                    .when(!windows.is_empty(), |d| {
                                        d.child(
                                            div().flex().flex_1().min_w_0().gap(px(10.)).children(
                                                windows
                                                    .iter()
                                                    .map(|w| meter(w, now, self.show_remaining)),
                                            ),
                                        )
                                    }),
                            ),
                    )
                    .children(active.then(|| icon("check", 14.).text_color(accent(1.))))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.choose(kind, id.clone(), project.clone(), session, cx)
                    })),
            );
        }
        let list_row = |id: &'static str, label: &'static str, ink: f32| {
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label)
                .h_8()
                .w_full()
                .px(px(10.))
                .rounded_lg()
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(11.))
                .text_color(content(ink))
                .cursor_pointer()
                .hover(|s| s.bg(content(0.07)).text_color(content(1.)))
        };
        div()
            .track_focus(&self.picker_focus)
            .on_key_down(cx.listener(Self::picker_key))
            .child(back_header(
                "back-to-usage",
                "Back to usage",
                format!("{} accounts", kind.label()),
                on_back,
            ))
            .child(
                div()
                    .mt_1()
                    .px_1()
                    .text_size(px(10.))
                    .line_height(px(16.))
                    .text_color(content(0.4))
                    .child("Choosing an account moves the current conversation to it."),
            )
            .child(rows)
            .when(accounts::supports_profiles(kind), |d| {
                d.child(
                    list_row("footer-add-account", "Add account", 0.55)
                        .mt_2()
                        .when(self.working, |d| d.opacity(0.4))
                        .child(icon("plus", 14.))
                        .child("Add account")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if !this.working {
                                this.start_edit(kind, None, Surface::Footer, window, cx)
                            }
                        })),
                )
            })
            .child(
                list_row("manage-provider-accounts", "Manage accounts", 0.45)
                    .mt(px(2.))
                    .child("Manage accounts…")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(AccountEvent::Manage))),
            )
            .into_any_element()
    }
}

/// Back arrow and title, as at the top of React's account popovers.
fn back_header(
    id: &'static str,
    label: &'static str,
    title: String,
    on_back: impl Fn(&mut Window, &mut App) + 'static,
) -> Div {
    div()
        .h_7()
        .flex()
        .items_center()
        .gap_1()
        .child(
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label)
                .size_6()
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(|s| s.bg(content(0.1)))
                .child(icon("arrow-left", 14.).text_color(content(0.45)))
                .on_click(move |_, window, cx| on_back(window, cx)),
        )
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
}

/// identityOrganizationTag: "Jane's Organization" is a personal workspace.
fn organization_tag(identity: &accounts::Identity) -> Option<String> {
    let name = identity.organization.as_deref()?.trim();
    if name.is_empty() {
        None
    } else if name.ends_with("'s Organization") || name.ends_with("’s Organization") {
        Some("Personal".into())
    } else {
        Some(name.into())
    }
}

/// React's barClass, shared with the footer chip and usage cards.
pub(crate) fn bar_color(used: f64) -> gpui::Hsla {
    if used >= 90. {
        gpui::rgb(0xf87171).into()
    } else if used >= 80. {
        gpui::rgb(0xfbbf24).into()
    } else {
        content(0.45)
    }
}

/// Even-pace tick: where the fill would sit if usage were spread evenly.
pub(crate) fn pace_marker(w: &usage::Window, now: i64, show_remaining: bool) -> Option<Div> {
    let pace = w.pace(now)?;
    let at = if show_remaining { 100. - pace } else { pace };
    Some(
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(gpui::relative(at as f32 / 100.))
            .w(px(1.5))
            .bg(content(0.8)),
    )
}

fn accent(alpha: f32) -> gpui::Hsla {
    let mut color = accent_color();
    color.a = alpha;
    color
}

/// AccountStatusLabel: "● Ready" or "● Exhausted back in 31m".
pub(crate) fn status_label(status: &Status) -> Div {
    let tinted = |hex: u32, alpha: f32| {
        let mut color: gpui::Hsla = gpui::rgb(hex).into();
        color.a = alpha;
        color
    };
    let (dot, ink) = match status.tone {
        Tone::Ready => (tinted(0x34d399, 1.), content(0.6)),
        Tone::Low => (tinted(0xfbbf24, 1.), tinted(0xfbbf24, 0.9)),
        Tone::Exhausted => (tinted(0xf87171, 1.), tinted(0xf87171, 0.9)),
        Tone::Checking | Tone::Unknown => (content(0.25), content(0.35)),
    };
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .min_w_0()
        .child(div().size(px(6.)).flex_shrink_0().rounded_full().bg(dot))
        .child(
            div()
                .text_color(ink)
                .map(|d| {
                    if status.tone == Tone::Unknown {
                        d.min_w_0().truncate()
                    } else {
                        d.flex_shrink_0()
                    }
                })
                .child(status.label.clone()),
        )
        .children(status.detail.clone().map(|detail| {
            div()
                .min_w_0()
                .truncate()
                .text_color(content(0.4))
                .child(detail)
        }))
}

/// UsageMeter with React's short "5h · 3h 10m" title for account rows.
fn meter(w: &usage::Window, now: i64, show_remaining: bool) -> Div {
    let used = w.used_percent.clamp(0., 100.);
    let remaining = 100. - used;
    let shown = if show_remaining { remaining } else { used };
    let full = used >= 100. && w.resets_at.is_none_or(|at| at > now);
    let reset = match w.resets_at {
        None => w.label(),
        Some(at) if at <= now => "reset due".into(),
        Some(at) => usage::duration(at - now),
    };
    div()
        .flex_1()
        .min_w_0()
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .gap_2()
                .text_size(px(10.))
                .line_height(px(12.))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(content(0.4))
                        .child(format!("{} · {reset}", w.label())),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .map(|d| {
                            if full {
                                d.font_weight(FontWeight::MEDIUM)
                                    .text_color(gpui::rgb(0xf87171))
                            } else {
                                d.text_color(content(0.6))
                            }
                        })
                        .child(if show_remaining {
                            format!("{remaining:.0}% left")
                        } else if full {
                            "Full".into()
                        } else {
                            format!("{used:.0}%")
                        }),
                ),
        )
        .child(
            div()
                .relative()
                .mt(px(6.))
                .h_1()
                .rounded_full()
                .bg(content(0.1))
                .overflow_hidden()
                .child(
                    div()
                        .h_full()
                        .w(gpui::relative(shown as f32 / 100.))
                        .rounded_full()
                        .bg(bar_color(used)),
                )
                .children(pace_marker(w, now, show_remaining)),
        )
}

impl Render for Accounts {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(signin) = &self.signin else {
            return div().into_any_element();
        };
        if !self.has_modal_auth() {
            return div().into_any_element();
        }
        let kind = signin.kind;
        let complete = signin.complete;
        let failed = signin.error.is_some();
        let action = signin.action;
        let title = if complete {
            format!(
                "Signed {} {}",
                if action == AuthAction::SignIn {
                    "in to"
                } else {
                    "out of"
                },
                kind.label()
            )
        } else if failed {
            "Authentication required".into()
        } else {
            "Waiting for browser…".into()
        };
        let error = signin.error.clone();
        let retry_id = signin.profile.id.clone();
        let adding = signin.adding.clone();
        if !self.dialog_presented {
            self.dialog_presented = true;
            self.previous_focus = self
                .editor
                .as_ref()
                .filter(|e| signin.adding.is_some() && e.surface == Surface::Settings)
                .and_then(|e| e.previous_focus.clone())
                .or_else(|| window.focused(cx));
            window.focus(&self.dialog_focus, cx);
        }
        div()
            .absolute()
            .inset_0()
            .occlude()
            .child(crate::modal::scrim("provider-signin-scrim"))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("provider-signin-panel")
                    .role(Role::Dialog)
                    .aria_label("Authentication required")
                    .track_focus(&self.dialog_focus)
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                        if event.keystroke.key == "escape" {
                            this.close_auth(window, cx);
                            cx.stop_propagation();
                        }
                    }))
                    .w(px(400.))
                    .relative()
                    .rounded(px(crate::modal::RADIUS))
                    .border_1()
                    .border_color(content(crate::modal::BORDER))
                    .child(crate::modal::surface(crate::modal::RADIUS - 1.))
                    .p_6()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(div().w_full().flex().justify_end().child(
                        icon_button("close-provider-signin", "Close sign-in", "close").on_click(
                            cx.listener(|this, _, window, cx| {
                                this.close_auth(window, cx);
                            }),
                        ),
                    ))
                    .child(
                        div()
                            .size_16()
                            .rounded(px(16.))
                            .bg(content(0.06))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(provider_icon(kind.id(), 36.)),
                    )
                    .child(
                        div()
                            .mt(px(14.))
                            .text_size(px(15.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(title),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_size(px(11.))
                            .text_color(content(0.45))
                            .child(if complete {
                                "You can retry your last message now."
                            } else {
                                "The provider CLI opens its browser to complete sign-in."
                            }),
                    )
                    .child(
                        secondary_button(
                            "provider-auth-action",
                            if complete {
                                "Continue"
                            } else if failed {
                                "Retry sign-in"
                            } else {
                                "Waiting for browser"
                            },
                            true,
                            (!complete && !failed) || self.working,
                        )
                        .mt_4()
                        .child(if complete {
                            "Continue"
                        } else if failed {
                            "Retry sign-in"
                        } else {
                            "Waiting for browser…"
                        })
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                if complete {
                                    this.close_auth(window, cx);
                                } else if failed {
                                    this.begin_auth(kind, &retry_id, action, adding.clone(), cx);
                                }
                            },
                        )),
                    )
                    .children(error.map(|error| {
                        div()
                            .id("provider-signin-error")
                            .role(Role::Alert)
                            .mt_2()
                            .text_size(px(10.))
                            .text_color(gpui::rgb(0xf87171))
                            .child(error)
                    })),
            )
            .into_any_element()
    }
}

impl Accounts {
    fn close_auth(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(signin) = self.signin.take() {
            signin.cancel.store(true, Ordering::Relaxed);
            if !signin.complete {
                self.working = false;
                if signin.adding.is_some() {
                    cx.background_executor()
                        .spawn(async move {
                            accounts::stop_processes(signin.kind, &signin.profile);
                            let _ = signin.profile.remove_credentials(signin.kind);
                        })
                        .detach();
                }
            }
        }
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
        cx.notify();
    }
}
