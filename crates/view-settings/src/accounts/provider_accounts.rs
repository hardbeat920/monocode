//! Port of `ProviderAccountsSettings` and `ProviderAccountEditor` in
//! src/features/settings/ui/SettingsView.tsx: the Accounts card at the top
//! of the Providers page, with each provider's named accounts, their status
//! and usage, and add, rename, and remove.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div, prelude::FluentBuilder as _,
};
use gpui_component::input::{InputEvent, InputState};
use monocode_core::HarnessId;
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, provider_logo, u};

use super::account_usage::{
    account_status_label, account_usage_meters, account_usage_refresh, provider_account_subtitle,
};
use super::host::UsageHost;
use super::model::{
    ACCOUNT_USAGE_CLOCK_MS, PROVIDER_ACCOUNT_PROVIDERS, ProviderAccount, ProviderAccountIdentity,
    RateLimitProvider, account_status, identity_key, identity_organization_tag,
};
use super::style::{spin_icon, text};
use crate::settings::chrome::{Reveal, group};
use crate::settings::controls::plain_input;
use crate::settings::providers::harness_logo;

/// `AccountEditor`: adding (no account id) or renaming.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountEditor {
    pub provider: HarnessId,
    pub account_id: Option<String>,
}

/// `ProviderAccountsSettings`.
pub struct ProviderAccountsSettings {
    host: Rc<dyn UsageHost>,
    reveal: Reveal,
    editor: Option<AccountEditor>,
    label: Entity<InputState>,
    working: Option<String>,
    error: Option<String>,
    identities: HashMap<String, Option<ProviderAccountIdentity>>,
    identities_key: Option<String>,
    identities_task: Option<Task<()>>,
    loaded_key: Option<String>,
    /// `inflight` in `useProviderAccountUsage`.
    loads: usize,
    now: i64,
    _clock: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl ProviderAccountsSettings {
    pub(crate) fn keep(&mut self, subscription: Subscription) {
        self._subscriptions.push(subscription);
    }
    pub fn new(host: Rc<dyn UsageHost>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let label = cx.new(|cx| InputState::new(window, cx).placeholder("Work or Personal"));
        let mut subscriptions = vec![cx.subscribe_in(
            &label,
            window,
            |this, input, event, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                InputEvent::Change => {
                    // `maxLength={48}`.
                    let value = input.read(cx).value();
                    if value.chars().count() > 48 {
                        let clipped: String = value.chars().take(48).collect();
                        input.update(cx, |input, cx| input.set_value(clipped, window, cx));
                    }
                    cx.notify();
                }
                _ => {}
            },
        )];
        let weak = cx.entity().downgrade();
        if let Some(observe) = host.observe(
            Box::new(move |cx| {
                weak.update(cx, |_, cx| cx.notify()).ok();
            }),
            cx,
        ) {
            subscriptions.push(observe);
        }
        let clock = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(
                        ACCOUNT_USAGE_CLOCK_MS as u64,
                    ))
                    .await;
                if this
                    .update(cx, |this, cx| {
                        this.now = this.host.now();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            now: host.now(),
            host,
            reveal: Reveal::default(),
            editor: None,
            label,
            working: None,
            error: None,
            identities: HashMap::new(),
            identities_key: None,
            identities_task: None,
            loaded_key: None,
            loads: 0,
            _clock: clock,
            _subscriptions: subscriptions,
        }
    }

    /// The page's reveal state, so the card flashes when search finds it.
    pub fn set_reveal(&mut self, reveal: Reveal, cx: &mut Context<Self>) {
        self.reveal = reveal;
        cx.notify();
    }

    pub fn editor(&self) -> Option<&AccountEditor> {
        self.editor.as_ref()
    }

    /// The account name field of the open editor.
    pub fn label_input(&self) -> &Entity<InputState> {
        &self.label
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn accounts(&self, cx: &gpui::App) -> Vec<ProviderAccount> {
        PROVIDER_ACCOUNT_PROVIDERS
            .iter()
            .flat_map(|provider| self.host.provider_accounts(*provider, cx))
            .collect()
    }

    /// The `useProviderAccountUsage` load effect: accounts without a
    /// snapshot load once.
    fn sync_usage(&mut self, cx: &mut Context<Self>) {
        let accounts = self.accounts(cx);
        let key = accounts
            .iter()
            .map(identity_key)
            .collect::<Vec<_>>()
            .join("|");
        if self.loaded_key.as_deref() == Some(key.as_str()) {
            return;
        }
        self.loaded_key = Some(key);
        let targets: Vec<ProviderAccount> = accounts
            .into_iter()
            .filter(|account| {
                RateLimitProvider::from_harness(account.provider).is_some_and(|provider| {
                    self.host.rate_limits(provider, &account.id, cx).is_none()
                })
            })
            .collect();
        self.load(targets, false, cx);
    }

    fn load(&mut self, targets: Vec<ProviderAccount>, force: bool, cx: &mut Context<Self>) {
        if targets.is_empty() {
            return;
        }
        self.loads += 1;
        let loads: Vec<Task<_>> = targets
            .iter()
            .filter_map(|account| {
                let provider = RateLimitProvider::from_harness(account.provider)?;
                Some(self.host.load_rate_limits(provider, &account.id, force, cx))
            })
            .collect();
        cx.notify();
        cx.spawn(async move |this, cx| {
            futures::future::join_all(loads).await;
            this.update(cx, |this, cx| {
                this.loads -= 1;
                this.now = this.host.now();
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `refresh`: reload every account.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let accounts = self.accounts(cx);
        self.load(accounts, true, cx);
    }

    fn sync_identities(&mut self, cx: &mut Context<Self>) {
        let accounts = self.accounts(cx);
        let key = accounts
            .iter()
            .map(|account| format!("{}={}", identity_key(account), account.label))
            .collect::<Vec<_>>()
            .join("|");
        if self.identities_key.as_deref() == Some(key.as_str()) {
            return;
        }
        self.identities_key = Some(key);
        let load = self.host.account_identities(&accounts, cx);
        self.identities_task = Some(cx.spawn(async move |this, cx| {
            let identities = load.await;
            this.update(cx, |this, cx| {
                this.identities = identities;
                cx.notify();
            })
            .ok();
        }));
    }

    /// `startAdd`.
    pub fn start_add(&mut self, provider: HarnessId, window: &mut Window, cx: &mut Context<Self>) {
        self.open_editor(
            AccountEditor {
                provider,
                account_id: None,
            },
            "",
            window,
            cx,
        );
    }

    /// `startRename`.
    pub fn start_rename(
        &mut self,
        account: &ProviderAccount,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_editor(
            AccountEditor {
                provider: account.provider,
                account_id: Some(account.id.clone()),
            },
            &account.label,
            window,
            cx,
        );
    }

    fn open_editor(
        &mut self,
        editor: AccountEditor,
        label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.error = None;
        self.editor = Some(editor);
        let label = label.to_string();
        self.label.update(cx, |input, cx| {
            input.set_value(label, window, cx);
            input.set_disabled(false, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        self.editor = None;
        cx.notify();
    }

    fn set_working(&mut self, working: Option<String>, cx: &mut Context<Self>) {
        let busy = working.is_some();
        self.working = working;
        self.label
            .update(cx, |input, cx| input.set_disabled(busy, cx));
        cx.notify();
    }

    /// `submitEditor`.
    pub fn submit(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.clone() else {
            return;
        };
        let label = self.label.read(cx).value().to_string();
        if label.trim().is_empty() || self.working.is_some() {
            return;
        }
        let key = match &editor.account_id {
            Some(id) => format!("rename:{}:{id}", editor.provider),
            None => format!("add:{}", editor.provider),
        };
        self.set_working(Some(key), cx);
        self.error = None;
        if let Some(id) = &editor.account_id {
            let result = self
                .host
                .rename_provider_account(editor.provider, id, &label, cx);
            match result {
                Ok(()) => self.editor = None,
                Err(error) => self.error = Some(fallback(error, "Could not save this account")),
            }
            self.set_working(None, cx);
            return;
        }
        let account = match self.host.new_provider_account(editor.provider, &label, cx) {
            Ok(account) => account,
            Err(error) => {
                self.error = Some(fallback(error, "Could not save this account"));
                self.set_working(None, cx);
                return;
            }
        };
        let login = self
            .host
            .login_harness(editor.provider, Some(&account.id), cx);
        cx.spawn(async move |this, cx| {
            let result = login.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.host.save_provider_account(&account, cx);
                        this.editor = None;
                    }
                    Err(error) => {
                        this.error = Some(fallback(error, "Could not save this account"));
                    }
                }
                this.set_working(None, cx);
            })
            .ok();
        })
        .detach();
    }

    /// `removeAccount`: confirm, delete the credentials, then the account
    /// and its cached usage.
    pub fn remove(&mut self, account: ProviderAccount, cx: &mut Context<Self>) {
        if account.is_default() || self.working.is_some() {
            return;
        }
        let confirm = self.host.confirm_remove_account(&account, cx);
        cx.spawn(async move |this, cx| {
            if !confirm.await {
                return;
            }
            let removal = this.update(cx, |this, cx| {
                this.set_working(
                    Some(format!("remove:{}:{}", account.provider, account.id)),
                    cx,
                );
                this.error = None;
                this.host
                    .remove_provider_account_credentials(account.provider, &account.id, cx)
            });
            let Ok(removal) = removal else {
                return;
            };
            let result = removal.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.host
                            .remove_provider_account(account.provider, &account.id, cx);
                        if let Some(provider) = RateLimitProvider::from_harness(account.provider) {
                            this.host.clear_rate_limits(provider, &account.id, cx);
                        }
                        if this.editor.as_ref().is_some_and(|editor| {
                            editor.provider == account.provider
                                && editor.account_id.as_deref() == Some(account.id.as_str())
                        }) {
                            this.editor = None;
                        }
                    }
                    Err(error) => {
                        this.error = Some(fallback(error, "Could not remove this account"));
                    }
                }
                this.set_working(None, cx);
            })
            .ok();
        })
        .detach();
    }

    /// `ProviderAccountEditor`.
    fn render_editor(
        &self,
        editor: &AccountEditor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let adding = editor.account_id.is_none();
        let working = self.working.is_some();
        let value = self.label.read(cx).value().to_string();
        let focused = self.label.read(cx).focus_handle(cx).is_focused(window);
        let aria = format!(
            "{} {} account",
            if adding { "New" } else { "Rename" },
            editor.provider.title()
        );
        let submit_label = if adding {
            if working {
                "Waiting for browser…"
            } else {
                "Sign in and add"
            }
        } else {
            "Save"
        };
        let cancel_hover = theme.content(0.10);
        let hover_ink = theme.colors.content;
        let mut cancel = div()
            .id("editor-cancel")
            .flex()
            .flex_none()
            .items_center()
            .h(u(24.))
            .rounded(u(4.5))
            .bg(theme.content(0.05))
            .px(u(10.))
            .text_px(11.)
            .text_color(theme.content(0.45))
            .debug_selector(|| "button:Cancel".into())
            .child("Cancel");
        if working {
            cancel = cancel.opacity(0.4);
        } else {
            cancel = cancel
                .hover(move |s| s.bg(cancel_hover).text_color(hover_ink))
                .on_click(cx.listener(|this, _, _, cx| this.cancel(cx)));
        }
        let ink = theme.colors.background_base;
        let disabled = working || value.trim().is_empty();
        let submit_hover = theme.content(0.85);
        let mut submit = div()
            .id("editor-submit")
            .ml(u(4.))
            .flex()
            .flex_none()
            .items_center()
            .gap(u(6.))
            .h(u(24.))
            .rounded(u(4.5))
            .bg(theme.colors.content)
            .px(u(10.))
            .text_px(11.)
            .medium()
            .text_color(ink)
            .debug_selector(move || format!("button:{submit_label}"))
            .when(working, |el| {
                el.child(spin_icon("editor-spin", IconName::Loader, 12., ink))
            })
            .child(submit_label);
        if disabled {
            submit = submit.opacity(0.4);
        } else {
            submit = submit
                .hover(move |s| s.bg(submit_hover))
                .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)));
        }
        let input = plain_input(&self.label, cx);
        div()
            .flex()
            .h(u(48.))
            .items_center()
            .border_b_1()
            .border_color(theme.content(0.05))
            .px(u(16.))
            .py(u(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .pr(u(4.))
                    .h(u(32.))
                    .min_w_0()
                    .flex_1()
                    .overflow_hidden()
                    .rounded(u(theme.radius.md))
                    .border_1()
                    .border_color(if focused {
                        theme.accent(0.45)
                    } else {
                        theme.content(0.10)
                    })
                    .bg(theme.content(0.04))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .px(u(10.))
                            .text_px(12.)
                            .when(working, |el| el.opacity(0.5))
                            .debug_selector(move || format!("input:{aria}"))
                            .child(input),
                    )
                    .child(cancel)
                    .child(submit),
            )
            .into_any_element()
    }

    fn render_account(
        &self,
        account: &ProviderAccount,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let identity = self
            .identities
            .get(&identity_key(account))
            .cloned()
            .flatten();
        let org = identity_organization_tag(identity.as_ref());
        // `usage.usage[accountUsageKey(account)]`: the same `provider:id` key.
        let limits = RateLimitProvider::from_harness(account.provider)
            .and_then(|provider| self.host.rate_limits(provider, &account.id, cx));
        let working = self.working.is_some();
        let removing = self.working.as_deref()
            == Some(format!("remove:{}:{}", account.provider, account.id).as_str());
        let row_id = format!("{}-{}", account.provider, account.id);
        let fallback = if account.is_default() {
            "Provider CLI profile"
        } else {
            "Isolated profile"
        };
        let subtitle = provider_account_subtitle(
            SharedString::from(format!("subtitle-{row_id}")),
            identity.as_ref(),
            Some(fallback),
            theme.content(0.30),
            None,
            self.host.mask_emails(cx),
        );
        let icon_button = |id: String, label: String, title: &'static str| {
            let selector = format!("button:{label}");
            div()
                .id(SharedString::from(id))
                .flex()
                .items_center()
                .justify_center()
                .size(u(28.))
                .rounded(u(theme.radius.md))
                .tooltip(tooltip(title))
                .debug_selector(move || selector)
        };
        let hover = theme.content(0.10);
        let hover_ink = theme.colors.content;
        let mut rename = icon_button(
            format!("rename-{row_id}"),
            format!("Rename {}", account.label),
            "Rename account",
        )
        .text_color(theme.content(0.40))
        .child(icon(IconName::Pencil).size(u(14.)));
        if working {
            rename = rename.opacity(0.35);
        } else {
            let target = account.clone();
            rename = rename
                .hover(move |s| s.bg(hover).text_color(hover_ink))
                .on_click(
                    cx.listener(move |this, _, window, cx| this.start_rename(&target, window, cx)),
                );
        }
        let remove = (!account.is_default()).then(|| {
            let danger = theme.colors.danger;
            let danger_fill = gpui::Hsla { a: 0.1, ..danger };
            let mut remove = icon_button(
                format!("remove-{row_id}"),
                format!("Remove {}", account.label),
                "Remove account",
            )
            .text_color(theme.content(0.35))
            .child(if removing {
                spin_icon(
                    SharedString::from(format!("removing-{row_id}")),
                    IconName::Loader,
                    14.,
                    danger,
                )
            } else {
                icon(IconName::Trash2).size(u(14.)).into_any_element()
            });
            if working {
                remove = remove.opacity(0.35);
            } else {
                let target = account.clone();
                remove = remove
                    .hover(move |s| s.bg(danger_fill).text_color(danger))
                    .on_click(cx.listener(move |this, _, _, cx| this.remove(target.clone(), cx)));
            }
            remove
        });
        let row_selector = format!("account:{}", account.label);
        div()
            .flex()
            .h(u(48.))
            .items_center()
            .gap(u(12.))
            .border_b_1()
            .border_color(theme.content(0.05))
            .px(u(16.))
            .py(u(8.))
            .debug_selector(move || row_selector)
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .flex()
                            .min_w_0()
                            .items_center()
                            .gap(u(6.))
                            .child(
                                text(account.label.clone())
                                    .truncate()
                                    .text_px(12.)
                                    .text_color(theme.content(0.85)),
                            )
                            .when_some(org, |el, org| {
                                el.child(
                                    text(org)
                                        .flex_none()
                                        .max_w(u(128.))
                                        .truncate()
                                        .rounded(u(theme.radius.sm))
                                        .bg(theme.content(0.07))
                                        .px(u(4.))
                                        .text_px(9.)
                                        .line_height(u(16.))
                                        .text_color(theme.content(0.50)),
                                )
                            }),
                    )
                    .child(
                        div()
                            .mt(u(2.))
                            .flex()
                            .min_w_0()
                            .items_center()
                            .gap(u(10.))
                            .text_px(10.)
                            .child(div().flex_none().child(account_status_label(
                                SharedString::from(format!("status-{row_id}")),
                                &account_status(limits.as_ref(), self.now),
                                10.,
                                cx,
                            )))
                            .children(subtitle),
                    ),
            )
            .children(account_usage_meters(
                SharedString::from(format!("meters-{row_id}")),
                limits.as_ref(),
                self.now,
                self.host.show_remaining_usage(cx),
                window,
                cx,
            ))
            .child(
                div()
                    .flex()
                    .w(u(96.))
                    .flex_none()
                    .items_center()
                    .justify_end()
                    .gap(u(4.))
                    .when(account.is_default(), |el| {
                        el.child(
                            div()
                                .mr(u(4.))
                                .text_px(10.)
                                .medium()
                                .text_color(theme.content(0.30))
                                .child("DEFAULT"),
                        )
                    })
                    .child(rename)
                    .children(remove),
            )
            .into_any_element()
    }
}

fn fallback(error: String, message: &str) -> String {
    if error.is_empty() {
        message.into()
    } else {
        error
    }
}

impl Render for ProviderAccountsSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_usage(cx);
        self.sync_identities(cx);
        let theme = Theme::of(cx).clone();
        let refreshing = self.loads > 0;
        let refresh = account_usage_refresh(
            refreshing,
            cx.listener(|this, _, _, cx| this.refresh(cx)),
            cx,
        );
        let working = self.working.is_some();
        let mut card = group(&self.reveal, "Accounts")
            .id("provider-accounts")
            .first(true)
            .description("Create isolated sign-ins for providers that support account profiles. Account switching stays available from the usage control in the footer.")
            .action(refresh);
        let count = PROVIDER_ACCOUNT_PROVIDERS.len();
        for (index, provider) in PROVIDER_ACCOUNT_PROVIDERS.iter().copied().enumerate() {
            let accounts = self.host.provider_accounts(provider, cx);
            let adding = self
                .editor
                .as_ref()
                .is_some_and(|editor| editor.provider == provider && editor.account_id.is_none());
            let hover = theme.content(0.10);
            let hover_ink = theme.colors.content;
            let add_selector = format!("button:Add account:{}", provider.as_str());
            let mut add = div()
                .id(SharedString::from(format!("add-{}", provider.as_str())))
                .flex()
                .flex_none()
                .items_center()
                .gap(u(6.))
                .rounded(u(theme.radius.md))
                .border_1()
                .border_color(theme.content(0.10))
                .px(u(10.))
                .py(u(4.))
                .text_px(12.)
                .text_color(theme.content(0.70))
                .debug_selector(move || add_selector)
                .child(icon(IconName::Plus).size(u(14.)))
                .child("Add account");
            if working {
                add = add.opacity(0.4);
            } else {
                add =
                    add.hover(move |s| s.bg(hover).text_color(hover_ink))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.start_add(provider, window, cx)
                        }));
            }
            let mut list = div()
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(theme.content(0.05))
                .bg(theme.content(0.015))
                .pl(u(40.));
            for account in &accounts {
                let editing = self.editor.as_ref().is_some_and(|editor| {
                    editor.provider == provider
                        && editor.account_id.as_deref() == Some(account.id.as_str())
                });
                list = list.child(if editing {
                    let editor = self.editor.clone().expect("editing");
                    self.render_editor(&editor, window, cx)
                } else {
                    self.render_account(account, window, cx)
                });
            }
            if adding {
                let editor = self.editor.clone().expect("adding");
                list = list.child(self.render_editor(&editor, window, cx));
            }
            let section = div()
                .when(index + 1 < count, |el| {
                    el.border_b_1().border_color(theme.content(0.05))
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(u(16.))
                        .px(u(16.))
                        .py(u(14.))
                        .child(
                            div()
                                .flex()
                                .min_w_0()
                                .flex_1()
                                .items_center()
                                .gap(u(10.))
                                .child(
                                    div()
                                        .flex()
                                        .flex_none()
                                        .items_center()
                                        .justify_center()
                                        .size(u(28.))
                                        .rounded(u(theme.radius.lg))
                                        .bg(theme.content(0.05))
                                        .border_1()
                                        .border_color(theme.content(0.06))
                                        .child(provider_logo(harness_logo(provider)).size(16.)),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .child(
                                            text(provider.title())
                                                .text_px(13.)
                                                .medium()
                                                .text_color(theme.colors.content),
                                        )
                                        .child(
                                            text(format!(
                                                "{} {}",
                                                accounts.len(),
                                                if accounts.len() == 1 {
                                                    "account"
                                                } else {
                                                    "accounts"
                                                }
                                            ))
                                            .mt(u(2.))
                                            .text_px(11.)
                                            .text_color(theme.content(0.40)),
                                        ),
                                ),
                        )
                        .child(add),
                )
                .child(list);
            card = card.child(section);
        }
        if let Some(error) = self.error.clone() {
            card = card.child(
                div()
                    .border_t_1()
                    .border_color(theme.content(0.05))
                    .px(u(16.))
                    .py(u(10.))
                    .text_px(11.)
                    .line_height(u(16.))
                    .text_color(theme.colors.danger)
                    .debug_selector(|| "alert".into())
                    .child(text(error)),
            );
        }
        card
    }
}
