//! Port of src/app/shell/PiUsage.tsx: the footer chip for a Pi session,
//! showing the subscription usage Pi's own saved OAuth account reports.
//!
//! The footer builds a new [`PiUsage`] for each session and model, as the
//! React `key` remounted it. Each one fetches through
//! [`UsageHost::fetch_pi_usage`], polls every `RATE_LIMIT_POLL_MS` while the
//! window is visible, and refreshes when the window comes back, at most once
//! per `RATE_LIMIT_MIN_REFETCH_MS` unless forced.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
    Window, div, prelude::FluentBuilder as _,
};
use monocode_core::HarnessId;
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, icon, provider_logo, u};

use super::host::UsageHost;
use super::model::{
    PiUsageProvider, ProviderRateLimits, RATE_LIMIT_MIN_REFETCH_MS, RATE_LIMIT_POLL_MS,
    RateLimitStatus, idle_rate_limits, pi_billing_provider, pi_usage_provider,
};
use super::style::{spin_icon, text};
use super::usage_chip::{ChipActions, ChipProps, Presentation, UsageProviderChip};
use crate::settings::providers::harness_logo;

/// `PiProviderUsage`: one provider's snapshot and poll.
struct ProviderUsage {
    provider: PiUsageProvider,
    limits: ProviderRateLimits,
    inflight: bool,
    last_fetch_at: i64,
    chip: Entity<UsageProviderChip>,
    _poll: Task<()>,
}

/// `PiUsage`.
pub struct PiUsage {
    host: Rc<dyn UsageHost>,
    model: Option<String>,
    now: i64,
    usage: Option<ProviderUsage>,
    /// Bumps when the provider changes, so a late answer for the old one is
    /// dropped (`disposed`).
    generation: u64,
    animate: bool,
    _activation: Option<Subscription>,
}

impl PiUsage {
    pub fn new(
        host: Rc<dyn UsageHost>,
        model: Option<String>,
        now: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let activation = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.refresh(false, cx);
            }
        });
        let mut this = Self {
            host,
            model: None,
            now,
            usage: None,
            generation: 0,
            animate: true,
            _activation: Some(activation),
        };
        this.set_model(model, now, window, cx);
        this
    }

    /// The session's model and the footer clock. A new billing provider
    /// starts over with its own snapshot.
    pub fn set_model(
        &mut self,
        model: Option<String>,
        now: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.now = now;
        let provider = pi_usage_provider(model.as_deref());
        self.model = model;
        if self.usage.as_ref().map(|usage| usage.provider) == provider {
            return;
        }
        self.generation += 1;
        self.usage = provider.map(|provider| self.start(provider, window, cx));
        if self.usage.is_some() {
            self.refresh(false, cx);
        }
        cx.notify();
    }

    /// The footer clock. The owner re-renders, so no notify.
    pub fn set_now(&mut self, now: i64) {
        self.now = now;
    }

    pub fn set_animate(&mut self, animate: bool, cx: &mut Context<Self>) {
        self.animate = animate;
        if let Some(usage) = &self.usage {
            usage.chip.update(cx, |chip, _| chip.set_animate(animate));
        }
    }

    /// The provider this session's usage comes from.
    pub fn provider(&self) -> Option<PiUsageProvider> {
        self.usage.as_ref().map(|usage| usage.provider)
    }

    /// The current snapshot.
    pub fn limits(&self) -> Option<&ProviderRateLimits> {
        self.usage.as_ref().map(|usage| &usage.limits)
    }

    pub fn chip(&self) -> Option<&Entity<UsageProviderChip>> {
        self.usage.as_ref().map(|usage| &usage.chip)
    }

    fn start(
        &mut self,
        provider: PiUsageProvider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ProviderUsage {
        let limits = idle_rate_limits(pi_billing_provider(provider));
        let host = self.host.clone();
        let props = self.chip_props(provider, &limits);
        let animate = self.animate;
        let chip = cx.new(|cx| {
            let mut chip = UsageProviderChip::new(host, props, ChipActions::default(), window, cx);
            chip.set_animate(animate);
            chip
        });
        let generation = self.generation;
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(RATE_LIMIT_POLL_MS as u64))
                    .await;
                let alive = this
                    .update(cx, |this, cx| {
                        if this.generation != generation {
                            return false;
                        }
                        this.refresh(false, cx);
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        });
        ProviderUsage {
            provider,
            limits,
            inflight: false,
            last_fetch_at: 0,
            chip,
            _poll: poll,
        }
    }

    /// `refresh`: unforced refreshes skip a hidden window and a fetch younger
    /// than `RATE_LIMIT_MIN_REFETCH_MS`.
    pub fn refresh(&mut self, force: bool, cx: &mut Context<Self>) {
        let now = self.host.now();
        let visible = self.host.window_visible(cx);
        let generation = self.generation;
        let Some(usage) = self.usage.as_mut() else {
            return;
        };
        if usage.inflight {
            return;
        }
        if !force && (!visible || now - usage.last_fetch_at < RATE_LIMIT_MIN_REFETCH_MS) {
            return;
        }
        usage.inflight = true;
        usage.limits = ProviderRateLimits {
            status: RateLimitStatus::Fetching,
            ..idle_rate_limits(pi_billing_provider(usage.provider))
        };
        let fetch = self.host.fetch_pi_usage(usage.provider, cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = fetch.await;
            this.update(cx, |this, cx| {
                let now = this.host.now();
                if this.generation != generation {
                    return;
                }
                if let Some(usage) = this.usage.as_mut() {
                    usage.limits = result;
                    usage.inflight = false;
                    usage.last_fetch_at = now;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// `visibilitychange`: the app calls this when the window is shown or
    /// hidden; a visible window refreshes unless it fetched recently.
    pub fn visibility_changed(&mut self, cx: &mut Context<Self>) {
        self.refresh(false, cx);
    }

    fn chip_props(&self, provider: PiUsageProvider, limits: &ProviderRateLimits) -> ChipProps {
        ChipProps {
            presentation: Some(Presentation {
                source_label: Some("Pi's saved OAuth account".into()),
                harness: HarnessId::Pi,
                label: match provider {
                    PiUsageProvider::Anthropic => "Pi · Anthropic".into(),
                    PiUsageProvider::OpenaiCodex => "Pi · OpenAI Codex".into(),
                },
            }),
            ..ChipProps::new(limits.clone(), self.now)
        }
    }

    fn render_unavailable(&self, cx: &Context<Self>) -> AnyElement {
        let model = self.model.as_deref();
        let title = if model.is_none_or(|model| model.is_empty() || model == "pi:default") {
            "Send a message so Pi can report its configured provider."
        } else {
            "Subscription usage is not supported for this Pi provider."
        };
        let _ = cx;
        div()
            .id("pi-usage-unavailable")
            .flex()
            .items_center()
            .gap(u(6.))
            .whitespace_nowrap()
            .tooltip(tooltip(title))
            .child(provider_logo(harness_logo(HarnessId::Pi)).size(12.))
            .child(text("pi · Usage unavailable"))
            .into_any_element()
    }
}

impl Render for PiUsage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(usage) = self.usage.as_ref() else {
            return self.render_unavailable(cx);
        };
        let props = self.chip_props(usage.provider, &usage.limits);
        let fetching = usage.limits.status == RateLimitStatus::Fetching;
        usage
            .chip
            .update(cx, |chip, _| chip.set_props(props, ChipActions::default()));
        let theme = Theme::of(cx).clone();
        let ink = theme.content(0.40);
        let glyph = if fetching {
            spin_icon("pi-refresh-spin", IconName::RefreshCw, 10., ink)
        } else {
            icon(IconName::RefreshCw)
                .size(u(10.))
                .text_color(ink)
                .into_any_element()
        };
        let hover_fill = theme.content(0.10);
        let hover_ink = theme.colors.content;
        let refresh = div()
            .id("pi-usage-refresh")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(u(24.))
            .rounded(u(theme.radius.sm))
            .text_color(ink)
            .tooltip(tooltip("Refresh Pi usage"))
            .debug_selector(|| "button:Refresh Pi usage".into())
            .child(glyph)
            .map(|el| {
                if fetching {
                    el.opacity(0.5)
                } else {
                    el.hover(move |s| s.bg(hover_fill).text_color(hover_ink))
                        .on_click(cx.listener(|this, _, _, cx| this.refresh(true, cx)))
                }
            });
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(u(6.))
            .child(usage.chip.clone())
            .child(refresh)
            .into_any_element()
    }
}
