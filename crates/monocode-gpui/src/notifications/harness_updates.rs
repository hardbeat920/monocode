//! Native counterpart of HarnessUpdateNotice.tsx. The parent positions this
//! entity in the shared top-right notification stack.
use crate::theme::*;
use crate::tooltip::Tip;
use gpui::{Context, EventEmitter, Render, Role, Window, div, prelude::*, px};
use monocode_core::{
    harness_updates::{self, Update},
    provider::Kind,
};
use std::collections::HashMap;

pub(crate) enum HarnessUpdatesEvent {
    Changed,
    Updated(Kind),
}
#[derive(Clone)]
enum RowState {
    Updating,
    Updated(String),
    Failed(String),
}
pub(crate) struct HarnessUpdates {
    updates: Vec<Update>,
    rows: HashMap<Kind, RowState>,
    checking: bool,
    generation: u64,
}
impl EventEmitter<HarnessUpdatesEvent> for HarnessUpdates {}
impl HarnessUpdates {
    pub(crate) fn new(hidden: Vec<String>, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            updates: Vec::new(),
            rows: HashMap::new(),
            checking: false,
            generation: 0,
        };
        if harness_updates::claim_launch_check() {
            this.check_now(hidden, cx);
        }
        this
    }
    pub(crate) fn is_visible(&self) -> bool {
        !self.updates.is_empty()
    }
    fn busy(&self) -> bool {
        self.rows.values().any(|s| matches!(s, RowState::Updating))
    }
    /// Explicit refresh is permitted after the launch claim or a dismissal.
    pub(crate) fn check_now(&mut self, hidden: Vec<String>, cx: &mut Context<Self>) {
        if self.checking || self.busy() {
            return;
        }
        self.checking = true;
        self.generation += 1;
        let generation = self.generation;
        let work = cx
            .background_executor()
            .spawn(async move { harness_updates::find_updates(&hidden) });
        cx.spawn(async move |this, cx| {
            let updates = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.checking = false;
                this.updates = updates;
                this.rows.clear();
                cx.emit(HarnessUpdatesEvent::Changed);
                cx.notify();
            });
        })
        .detach();
    }
    pub(crate) fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.busy() {
            return;
        }
        self.generation += 1;
        self.checking = false;
        self.updates.clear();
        self.rows.clear();
        cx.emit(HarnessUpdatesEvent::Changed);
        cx.notify();
    }
    fn start(&mut self, targets: Vec<Update>, cx: &mut Context<Self>) {
        for update in targets {
            let kind = update.harness;
            if matches!(
                self.rows.get(&kind),
                Some(RowState::Updating | RowState::Updated(_))
            ) {
                continue;
            }
            self.rows.insert(kind, RowState::Updating);
            let work = cx
                .background_executor()
                .spawn(async move { harness_updates::run_update(&update) });
            cx.spawn(async move |this, cx| {
                let result = work.await;
                let _ = this.update(cx, |this, cx| {
                    let state = match result {
                        Ok(version) => {
                            cx.emit(HarnessUpdatesEvent::Updated(kind));
                            RowState::Updated(version)
                        }
                        Err(error) => RowState::Failed(error),
                    };
                    this.rows.insert(kind, state);
                    cx.emit(HarnessUpdatesEvent::Changed);
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }
}
impl Render for HarnessUpdates {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_visible() {
            return div().into_any_element();
        }
        let busy = self.busy();
        let pending = self
            .updates
            .iter()
            .filter(|u| {
                !matches!(
                    self.rows.get(&u.harness),
                    Some(RowState::Updating | RowState::Updated(_))
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        let updated = self
            .rows
            .values()
            .any(|s| matches!(s, RowState::Updated(_)));
        let mut header = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(12.))
            .py(px(8.))
            .border_b_1()
            .border_color(content(0.1))
            .child(
                div()
                    .flex_1()
                    .text_size(px(12.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(if self.updates.len() == 1 {
                        "Harness update available"
                    } else {
                        "Harness updates available"
                    }),
            );
        if pending.len() > 1 {
            header = header.child(
                div()
                    .id("update-all-harnesses")
                    .role(Role::Button)
                    .aria_label("Update all")
                    .cursor_pointer()
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .text_size(px(11.))
                    .text_color(content(0.7))
                    .hover(|s| s.bg(content(0.1)))
                    .on_click(cx.listener(move |this, _, _, cx| this.start(pending.clone(), cx)))
                    .child("Update all"),
            );
        }
        header = header.child(
            div()
                .id("dismiss-harness-updates")
                .role(Role::Button)
                .aria_label("Dismiss harness updates")
                .tip("Dismiss harness updates")
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .text_color(content(if busy { 0.16 } else { 0.4 }))
                .when(!busy, |el| {
                    el.cursor_pointer()
                        .hover(|s| s.bg(content(0.1)))
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx)))
                })
                .child(icon("x", 12.)),
        );
        let mut panel = div().relative().child(header);
        for update in &self.updates {
            let state = self.rows.get(&update.harness);
            let mut line = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(provider_icon(update.harness.id(), 16.))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(13.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(update.harness.label()),
                );
            if let Some(RowState::Updated(version)) = state {
                line = line.child(
                    div()
                        .text_size(px(11.))
                        .text_color(gpui::rgb(0x34d399))
                        .child(format!("✓ Updated to {version}")),
                );
            } else {
                let updating = matches!(state, Some(RowState::Updating));
                let target = update.clone();
                line = line
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_family("monospace")
                            .text_color(content(0.5))
                            .child(format!("{} → {}", update.installed, update.latest)),
                    )
                    .child(
                        div()
                            .id(gpui::SharedString::from(format!(
                                "update-harness-{}",
                                update.harness.id()
                            )))
                            .px(px(8.))
                            .py(px(4.))
                            .rounded(px(6.))
                            .bg(content(0.1))
                            .text_size(px(11.))
                            .opacity(if updating { 0.6 } else { 1. })
                            .when(!updating, |el| {
                                el.cursor_pointer().hover(|s| s.bg(content(0.15))).on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.start(vec![target.clone()], cx)
                                    }),
                                )
                            })
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .when(updating, |el| {
                                el.child(crate::pulse::looping(
                                    "harness-update-spinner",
                                    std::time::Duration::from_secs(1),
                                    crate::pulse::Rate::Smooth,
                                    move |delta| {
                                        icon("loader", 12.).with_transformation(
                                            gpui::Transformation::rotate(gpui::percentage(delta)),
                                        )
                                    },
                                ))
                            })
                            .child(if updating {
                                "Updating"
                            } else if matches!(state, Some(RowState::Failed(_))) {
                                "Retry"
                            } else {
                                "Update"
                            }),
                    );
            }
            let mut row = div()
                .px(px(12.))
                .py(px(10.))
                .border_b_1()
                .border_color(content(0.1))
                .child(line);
            if let Some(RowState::Failed(error)) = state {
                row = row.child(
                    div()
                        .mt(px(6.))
                        .text_size(px(11.))
                        .text_color(gpui::rgb(0xfca5a5))
                        .child(error.clone()),
                );
            }
            panel = panel.child(row);
        }
        super::surface::glass_frame()
            .id("harness-update-notice")
            .role(Role::Status)
            .aria_label("Harness updates")
            .child(
                panel.child(
                    div()
                        .px(px(12.))
                        .py(px(8.))
                        .text_size(px(11.))
                        .text_color(content(0.5))
                        .child(if updated {
                            "Model picker refreshed with the new version’s models."
                        } else {
                            "New models often need the latest version."
                        }),
                ),
            )
            .into_any_element()
    }
}
