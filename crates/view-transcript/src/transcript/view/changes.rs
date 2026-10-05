//! The changes card after the latest reply. Port of
//! src/features/sessions/ui/SessionReview.tsx, with the checkpoint calls
//! replaced by events: the host runs Undo, Keep, and Review.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use monocode_core::paths::basename;
use monocode_ui::styled::{UiStyled as _, format_integer};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, file_type_icon, icon, u};

use crate::transcript::model::plan::Row;

use super::style::palette;
use super::{ChangedFile, TranscriptEvent, TranscriptView, eid};

impl TranscriptView {
    pub(super) fn render_changes(&mut self, row: &Row, cx: &mut Context<Self>) -> AnyElement {
        if self.changes.is_empty() {
            return div().into_any_element();
        }
        let theme = Theme::of(cx).clone();
        let files = self.changes.clone();
        let expanded = self.toggled("review:expanded", false);
        let acting = self.changes_busy;
        let can_undo_all = !self.undo_locked && files.iter().all(|file| file.undoable);
        let visible: Vec<&ChangedFile> = if expanded {
            files.iter().collect()
        } else {
            files.iter().take(3).collect()
        };
        let hidden = files.len() - visible.len();
        let additions: i64 = files.iter().map(|file| file.additions).sum();
        let deletions: i64 = files.iter().map(|file| file.deletions).sum();
        let count = files.len();

        let quiet_button = |part: &str, label: &'static str, enabled: bool| {
            div()
                .id(eid(&row.key, part))
                .flex()
                .items_center()
                .h(u(28.))
                .px(u(10.))
                .rounded(u(6.))
                .text_px(11.)
                .text_color(theme.content(0.5))
                .when(enabled, |el| {
                    el.cursor_pointer()
                        .hover(|s| s.bg(theme.content(0.08)).text_color(theme.colors.content))
                })
                .when(!enabled, |el| el.opacity(0.35))
                .child(label)
        };
        let undo_title = if can_undo_all {
            "Undo all session changes"
        } else if self.undo_locked {
            "Undo is unavailable while another session is running in this project"
        } else {
            "Undo is unavailable because a file changed outside this session"
        };
        let header = div()
            .flex()
            .items_center()
            .gap(u(10.))
            .min_w_0()
            .px(u(12.))
            .py(u(10.))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(u(32.))
                    .rounded(u(8.))
                    .bg(theme.content(0.08))
                    .child(
                        icon(IconName::FileDiff)
                            .size(u(16.))
                            .text_color(theme.content(0.55)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_px(12.)
                            .line_height(u(16.))
                            .medium()
                            .text_color(theme.content(0.8))
                            .child(format!(
                                "Changed {count} {}",
                                if count == 1 { "file" } else { "files" }
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(u(6.))
                            .mt(u(-2.))
                            .text_px(11.)
                            .line_height(u(16.))
                            .semibold()
                            .tabular()
                            .child(
                                div()
                                    .text_color(theme.colors.diff_add_fg)
                                    .child(format!("+{}", format_integer(additions))),
                            )
                            .child(
                                div()
                                    .text_color(theme.colors.diff_del_fg)
                                    .child(format!("-{}", format_integer(deletions))),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(2.))
                    .child({
                        let enabled = !acting && can_undo_all;
                        quiet_button("undo", "Undo", enabled)
                            .tooltip(tooltip(undo_title))
                            .when(enabled, |el| {
                                el.on_click(cx.listener(|this, _, _, cx| {
                                    this.changes_busy = true;
                                    cx.emit(TranscriptEvent::UndoChanges);
                                    cx.notify();
                                }))
                            })
                    })
                    .child(
                        quiet_button("keep", "Keep", !acting)
                            .tooltip(tooltip("Keep all session changes and dismiss this card"))
                            .when(!acting, |el| {
                                el.on_click(cx.listener(|this, _, _, cx| {
                                    this.changes_busy = true;
                                    cx.emit(TranscriptEvent::KeepChanges);
                                    cx.notify();
                                }))
                            }),
                    )
                    .child(
                        div()
                            .id(eid(&row.key, "review"))
                            .flex()
                            .items_center()
                            .h(u(28.))
                            .px(u(10.))
                            .rounded(u(6.))
                            .border_1()
                            .border_color(theme.content(0.12))
                            .bg(theme.content(0.08))
                            .text_px(11.)
                            .medium()
                            .text_color(theme.content(0.75))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.content(0.12)).text_color(theme.colors.content))
                            .tooltip(tooltip("Review changes"))
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(TranscriptEvent::ReviewChanges { path: None })
                            }))
                            .child("Review"),
                    ),
            );
        let mut list = div()
            .id(eid(&row.key, "files"))
            .flex()
            .flex_col()
            .border_t(px(1.))
            .border_color(theme.colors.stroke)
            .py(u(4.))
            .when(expanded, |el| el.max_h(u(256.)).overflow_y_scroll());
        for file in visible {
            let path = file.path.clone();
            let counts: AnyElement = if file.exact {
                div()
                    .flex()
                    .flex_none()
                    .gap(u(8.))
                    .text_px(11.)
                    .semibold()
                    .tabular()
                    .child(
                        div()
                            .text_color(theme.colors.diff_add_fg)
                            .child(format!("+{}", format_integer(file.additions))),
                    )
                    .child(
                        div()
                            .text_color(theme.colors.diff_del_fg)
                            .child(format!("-{}", format_integer(file.deletions))),
                    )
                    .into_any_element()
            } else {
                div()
                    .flex_none()
                    .text_px(11.)
                    .medium()
                    .text_color(palette::amber_300_80())
                    .child("Mixed changes")
                    .into_any_element()
            };
            list = list.child(
                div()
                    .id(eid(&row.key, &format!("file:{}", file.relative)))
                    .flex()
                    .items_center()
                    .gap(u(8.))
                    .h(u(32.))
                    .px(u(12.))
                    .min_w_0()
                    .text_color(theme.content(0.65))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.content(0.05)).text_color(theme.colors.content))
                    .tooltip(tooltip(file.relative.clone()))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(TranscriptEvent::ReviewChanges {
                            path: Some(path.clone()),
                        })
                    }))
                    .child(file_type_icon(basename(&file.relative)).size(15.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(theme.fonts.mono.clone())
                            .text_px(12.)
                            .child(file.relative.clone()),
                    )
                    .child(counts),
            );
        }
        let more = (files.len() > 3).then(|| {
            div()
                .id(eid(&row.key, "more"))
                .flex()
                .items_center()
                .gap(u(6.))
                .h(u(32.))
                .w_full()
                .px(u(12.))
                .border_t(px(1.))
                .border_color(theme.colors.stroke)
                .text_px(11.)
                .text_color(theme.content(0.45))
                .cursor_pointer()
                .hover(|s| s.bg(theme.content(0.05)).text_color(theme.content(0.7)))
                .on_click(
                    cx.listener(|this, _, _, cx| this.toggle("review:expanded".into(), false, cx)),
                )
                .child(
                    icon(if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .size(u(14.))
                    .text_color(theme.content(0.45)),
                )
                .child(if expanded {
                    "Show fewer files".to_string()
                } else {
                    format!(
                        "Show {hidden} more {}",
                        if hidden == 1 { "file" } else { "files" }
                    )
                })
        });
        div()
            .px(u(16.))
            .pt(u(4.))
            .pb(u(8.))
            .font_family(theme.fonts.sans.clone())
            .child(
                div()
                    .overflow_hidden()
                    .rounded(u(12.))
                    .border_1()
                    .border_color(theme.content(0.12))
                    .bg(theme.content(0.03))
                    .child(header)
                    .child(list)
                    .children(more),
            )
            .into_any_element()
    }
}
