//! Port of src/features/providers/ui/ProviderAccountUsage.tsx and
//! ProviderAccountSubtitle.tsx: the status label, the usage meters, the
//! refresh button, and the plan and email line under an account.

use gpui::{
    AnyElement, App, ClickEvent, ElementId, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, relative,
};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::model::{
    AccountStatus, AccountStatusTone, ProviderAccountIdentity, ProviderRateLimits, RateLimitStatus,
    RateLimitWindow, clamp_used_percent, format_reset_duration, format_usage_percent,
    format_window_label,
};
use super::style::{
    bar_color, css_percent, pulse, spin_icon, status_dot_color, status_text_color, text,
};
use crate::settings::private_email::private_email;

/// `AccountStatusLabel`: dot and word, as "● Ready" or "● Exhausted back in
/// 31m". `size` is the font size in CSS px.
pub fn account_status_label(
    id: impl Into<ElementId>,
    status: &AccountStatus,
    size: f32,
    cx: &App,
) -> AnyElement {
    let id = id.into();
    let theme = Theme::of(cx);
    let title: SharedString = match &status.detail {
        Some(detail) => format!("{} · {detail}", status.label).into(),
        None => status.label.clone().into(),
    };
    let dot = div()
        .flex_none()
        .size(u(6.))
        .rounded_full()
        .bg(status_dot_color(status.tone, cx));
    let dot = if status.tone == AccountStatusTone::Checking {
        pulse(
            ElementId::NamedChild(std::sync::Arc::new(id.clone()), "pulse".into()),
            dot,
        )
    } else {
        dot.into_any_element()
    };
    let label = text(status.label.clone()).text_color(status_text_color(status.tone, cx));
    let label = if status.tone == AccountStatusTone::Unknown {
        label.min_w_0().truncate()
    } else {
        label.flex_none()
    };
    div()
        .id(id)
        .flex()
        .min_w_0()
        .items_center()
        .gap(u(6.))
        .text_px(size)
        .whitespace_nowrap()
        .tooltip(tooltip(title))
        .child(dot)
        .child(label)
        .when_some(status.detail.clone(), |el, detail| {
            el.child(
                text(detail)
                    .min_w_0()
                    .truncate()
                    .text_color(theme.content(0.40)),
            )
        })
        .into_any_element()
}

/// `AccountUsageRefresh`: the card's refresh button.
pub fn account_usage_refresh(
    refreshing: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let theme = Theme::of(cx);
    let ink = theme.content(0.40);
    let glyph = if refreshing {
        spin_icon("account-usage-refresh-spin", IconName::RefreshCw, 14., ink)
    } else {
        icon(IconName::RefreshCw)
            .size(u(14.))
            .text_color(ink)
            .into_any_element()
    };
    let hover_fill = theme.content(0.10);
    let hover_ink = theme.colors.content;
    div()
        .id("account-usage-refresh")
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(u(28.))
        .rounded(u(theme.radius.md))
        .text_color(ink)
        .tooltip(tooltip("Refresh usage limits"))
        .debug_selector(|| "button:Refresh usage limits".into())
        .child(glyph)
        .map(|el| {
            if refreshing {
                el.opacity(0.4)
            } else {
                el.hover(move |s| s.bg(hover_fill).text_color(hover_ink))
                    .on_click(on_click)
            }
        })
        .into_any_element()
}

/// `meterWindows`: the titled 5h, weekly, and monthly windows an account
/// has data for.
pub fn meter_windows(limits: Option<&ProviderRateLimits>) -> Vec<(&'static str, RateLimitWindow)> {
    let Some(limits) = limits else {
        return Vec::new();
    };
    [
        ("5h", limits.session),
        ("Weekly", limits.weekly),
        ("Monthly", limits.monthly),
    ]
    .into_iter()
    .filter_map(|(title, window)| window.map(|window| (title, window)))
    .collect()
}

/// How wide a meter is: `w-36`, or a share of its row (`min-w-0 flex-1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeterWidth {
    Fixed,
    Flex,
}

/// Account timestamps in the system locale and time zone.
pub fn format_locale_date_time(ms: i64) -> String {
    monocode_platform::date_time::format_local(
        ms,
        monocode_platform::date_time::DateTimeStyle::DateTime,
    )
}

/// `UsageMeter`: "5h · 2h" over "58% left" and a 4px bar of what remains.
pub fn usage_meter(
    id: impl Into<ElementId>,
    title: &str,
    window: &RateLimitWindow,
    now: i64,
    width: MeterWidth,
    cx: &App,
) -> AnyElement {
    let theme = Theme::of(cx);
    let pct = clamp_used_percent(window.used_percent);
    let remaining = 100.0 - pct;
    let full = pct >= 100.0 && window.resets_at.is_none_or(|resets_at| resets_at > now);
    let reset = match window.resets_at {
        None => format_window_label(window.window_minutes),
        Some(resets_at) if resets_at <= now => "reset due".into(),
        Some(resets_at) => format_reset_duration(resets_at - now),
    };
    let left = format!("{} left", format_usage_percent(remaining));
    let aria = format!("{title} limit remaining");
    let value_now = monocode_core::js::round(remaining) as i64;
    let fill_selector = format!("fill:{aria}={}", css_percent(remaining));
    let bar_selector = format!("progressbar:{aria}={value_now}");
    let mut meter = div().id(id).flex().flex_col();
    meter = match width {
        MeterWidth::Fixed => meter.flex_none().w(u(144.)),
        MeterWidth::Flex => meter.min_w_0().flex_1(),
    };
    if let Some(resets_at) = window.resets_at {
        meter = meter.tooltip(tooltip(format!(
            "Resets {}",
            format_locale_date_time(resets_at)
        )));
    }
    meter
        .child(
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .gap(u(8.))
                .text_px(10.)
                .line_height(u(12.))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.content(0.40))
                        .child(format!("{title} · {reset}")),
                )
                .child(text(left).flex_none().tabular().map(|el| {
                    if full {
                        el.medium().text_color(theme.colors.danger)
                    } else {
                        el.text_color(theme.content(0.60))
                    }
                })),
        )
        .child(
            div()
                .mt(u(6.))
                .h(u(4.))
                .overflow_hidden()
                .rounded_full()
                .bg(theme.content(0.10))
                .debug_selector(move || bar_selector)
                .child(
                    div()
                        .h_full()
                        .w(relative((remaining / 100.0) as f32))
                        .rounded_full()
                        .bg(bar_color(pct, cx))
                        .debug_selector(move || fill_selector),
                ),
        )
        .into_any_element()
}

fn meter_skeleton(id: impl Into<ElementId>, cx: &App) -> AnyElement {
    let theme = Theme::of(cx);
    pulse(
        id,
        div()
            .flex_none()
            .w(u(144.))
            .child(
                div()
                    .h(u(12.))
                    .w(u(80.))
                    .rounded(u(theme.radius.sm))
                    .bg(theme.content(0.10)),
            )
            .child(
                div()
                    .mt(u(6.))
                    .h(u(4.))
                    .rounded_full()
                    .bg(theme.content(0.10)),
            ),
    )
}

/// `AccountUsageMeters`: compact 5h and weekly meters for one account row.
/// Hidden below the `sm` breakpoint (640px), as `hidden sm:flex`.
pub fn account_usage_meters(
    id: impl Into<ElementId>,
    limits: Option<&ProviderRateLimits>,
    now: i64,
    window: &Window,
    cx: &App,
) -> Option<AnyElement> {
    let viewport = f32::from(window.viewport_size().width) / f32::from(window.rem_size()) * 16.0;
    if viewport < 640.0 {
        return None;
    }
    let id: ElementId = id.into();
    let child =
        |name: &'static str| ElementId::NamedChild(std::sync::Arc::new(id.clone()), name.into());
    let row = div().flex().flex_none().gap(u(16.));
    let windows = meter_windows(limits);
    if windows.is_empty() {
        let loading = limits.is_none_or(|limits| {
            matches!(
                limits.status,
                RateLimitStatus::Idle | RateLimitStatus::Fetching
            )
        });
        return Some(if loading {
            row.child(meter_skeleton(child("skeleton-a"), cx))
                .child(meter_skeleton(child("skeleton-b"), cx))
                .into_any_element()
        } else {
            // The row's status line already explains why there is no data.
            row.child(div().w(u(304.))).into_any_element()
        });
    }
    Some(
        row.children(windows.into_iter().map(|(title, window)| {
            usage_meter(child(title), title, &window, now, MeterWidth::Fixed, cx)
        }))
        .into_any_element(),
    )
}

/// `ProviderAccountSubtitle`: "Pro · email" with the email masked until
/// clicked, or `fallback` when the identity has neither. `color` and `size`
/// are the className the caller passed.
pub fn provider_account_subtitle(
    id: impl Into<ElementId>,
    identity: Option<&ProviderAccountIdentity>,
    fallback: Option<&str>,
    color: Hsla,
    size: Option<f32>,
) -> Option<AnyElement> {
    let plan = identity.and_then(|identity| identity.plan.clone());
    let email = identity.and_then(|identity| identity.email.clone());
    let with_size = |el: gpui::Div| match size {
        Some(size) => el.text_px(size),
        None => el,
    };
    if plan.is_none() && email.is_none() {
        let fallback = fallback?;
        return Some(
            with_size(
                text(fallback.to_string())
                    .min_w_0()
                    .truncate()
                    .text_color(color),
            )
            .into_any_element(),
        );
    }
    let id: ElementId = id.into();
    let both = plan.is_some() && email.is_some();
    Some(
        with_size(
            div()
                .flex()
                .min_w_0()
                .items_baseline()
                .gap(u(4.))
                .whitespace_nowrap()
                .text_color(color),
        )
        .when_some(plan, |el, plan| el.child(text(plan).flex_none()))
        .when(both, |el| el.child(div().flex_none().child("·")))
        .when_some(email, |el, email| {
            el.child(private_email(
                ElementId::NamedChild(std::sync::Arc::new(id), email.clone().into()),
                email,
            ))
        })
        .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::model::{RateLimitProvider, idle_rate_limits};

    #[test]
    fn lists_meter_windows_in_order() {
        let window = RateLimitWindow {
            used_percent: 10.0,
            window_minutes: 300,
            resets_at: None,
        };
        let limits = ProviderRateLimits {
            session: Some(window),
            monthly: Some(window),
            ..idle_rate_limits(RateLimitProvider::Claude)
        };
        let titles: Vec<&str> = meter_windows(Some(&limits))
            .into_iter()
            .map(|(title, _)| title)
            .collect();
        assert_eq!(titles, ["5h", "Monthly"]);
        assert!(meter_windows(None).is_empty());
    }
}
