use gpui::{AnyElement, Context, Render, Role, Window, div, prelude::*, px, rgb};
use monocode_core::usage::{self, Limit};

use crate::theme::content;

pub(crate) fn label(limit: &Limit, now: i64) -> String {
    if !limit.resume_at_reset {
        return "Usage limit".into();
    }
    match limit.resets_at.map(|at| at.saturating_add(30_000)) {
        Some(at) if at > now => format!(
            "Resumes in {}",
            usage::duration((at - now).saturating_add(59_999))
        ),
        Some(_) => "Resume pending".into(),
        None => "Waiting for reset".into(),
    }
}

pub(crate) fn description(limit: &Limit, now: i64) -> String {
    let when = limit.resets_at.and_then(|at| {
        chrono::DateTime::from_timestamp_millis(at).map(|date| {
            date.with_timezone(&chrono::Local)
                .format("%b %-d, %-I:%M %p %Z")
                .to_string()
        })
    });
    if limit.resume_at_reset {
        match when {
            Some(when) if limit.resets_at.is_some_and(|at| at > now) => {
                format!(
                    "Will continue automatically after the limit resets at {when}. Keep Monocode running."
                )
            }
            Some(_) => "Waiting to retry automatically. Keep Monocode running.".into(),
            None => {
                "Auto-resume is on. Checking when the limit resets. Keep Monocode running.".into()
            }
        }
    } else {
        match when {
            Some(when) if limit.resets_at.is_some_and(|at| at <= now) => {
                format!("Auto-resume is off. Limit reset at {when}.")
            }
            Some(when) => format!("Auto-resume is off. Limit resets at {when}."),
            None => "Auto-resume is off. Reset time is unavailable.".into(),
        }
    }
}

pub(crate) fn sidebar_status(id: i64, limit: &Limit, narrow: bool) -> AnyElement {
    let now = usage::now_ms();
    let description = description(limit, now);
    div()
        .id(("session-usage-limit", id as usize))
        .debug_selector(move || format!("session-usage-limit-{id}"))
        .role(Role::Status)
        .aria_label(description.clone())
        .tooltip(move |_, cx| cx.new(|_| ScheduleTooltip(description.clone())).into())
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap_1()
        .max_w(px(150.))
        .text_size(px(11.))
        .line_height(px(16.5))
        .text_color(if crate::theme::is_light() {
            rgb(0x855b00)
        } else {
            rgb(0xffb900)
        })
        .child(
            gpui::svg()
                .path(if limit.resume_at_reset {
                    "clock"
                } else {
                    "circle-alert"
                })
                .size(px(12.))
                .flex_shrink_0(),
        )
        .child(
            div().min_w_0().truncate().child(
                if narrow
                    && limit.resume_at_reset
                    && limit
                        .resets_at
                        .is_some_and(|at| at.saturating_add(30_000) > now)
                {
                    "Scheduled".into()
                } else {
                    label(limit, now)
                },
            ),
        )
        .into_any_element()
}

struct ScheduleTooltip(String);
impl Render for ScheduleTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut background = crate::theme::shell_fill();
        background.a = 1.;
        div()
            .px_2()
            .py_1()
            .max_w(px(320.))
            .rounded_md()
            .bg(background)
            .border_1()
            .border_color(content(0.12))
            .text_size(px(12.))
            .line_height(px(18.))
            .text_color(content(0.85))
            .child(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_labels_distinguish_cancelled_unknown_reset_and_due_retry() {
        let mut limit = Limit {
            resets_at: Some(60_000),
            resume_at_reset: false,
        };
        assert_eq!(label(&limit, 0), "Usage limit");
        assert!(description(&limit, 0).starts_with("Auto-resume is off."));
        limit.resume_at_reset = true;
        assert_eq!(label(&limit, 0), "Resumes in 2m");
        assert_eq!(label(&limit, 60_000), "Resumes in 1m");
        assert_eq!(label(&limit, 90_000), "Resume pending");
        assert!(description(&limit, 0).contains("Keep Monocode running."));
        limit.resets_at = None;
        assert_eq!(label(&limit, 0), "Waiting for reset");
    }
}
