//! Port of src/features/providers/ui/HarnessUpdateNotice.tsx: the glass
//! card in the top right that offers CLI updates found at launch, updates
//! them in place, and says when the model picker has the new models.
//!
//! The app puts this view in its overlay layer, over a `relative()` root,
//! the way it places the toast stack.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, Context, EventEmitter, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window,
    canvas, div, prelude::FluentBuilder as _,
};
use monocode_core::HarnessId;
use monocode_ui::styled::glass_backdrop;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, provider_logo, u};

use super::host::HarnessUpdateHost;
use super::model::{HarnessUpdate, compare_semver, parse_version};
use super::style::{spin_icon, text};
use crate::settings::providers::harness_logo;

/// `RowState`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RowState {
    #[default]
    Idle,
    Updating,
    Updated(String),
    Failed(String),
}

/// What the notice tells its owner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HarnessUpdateNoticeEvent {
    /// `onHeightChange`: the card's height in CSS px, 0 once it is gone, so
    /// toasts can sit below it.
    HeightChanged(f32),
}

/// `HarnessUpdateNotice`.
pub struct HarnessUpdateNotice {
    host: Rc<dyn HarnessUpdateHost>,
    updates: Vec<HarnessUpdate>,
    rows: HashMap<HarnessId, RowState>,
    top_offset: f32,
    height: Rc<Cell<f32>>,
    _check: Task<()>,
    _updated: Option<Subscription>,
}

impl EventEmitter<HarnessUpdateNoticeEvent> for HarnessUpdateNotice {}

impl HarnessUpdateNotice {
    /// Starts the launch check. `top_offset` is the distance from the
    /// window top in CSS px (12 by default).
    pub fn new(host: Rc<dyn HarnessUpdateHost>, top_offset: f32, cx: &mut Context<Self>) -> Self {
        // Every window listens, so the one that ran the update tells the
        // others to pick up the new CLI's models too.
        let listener = host.clone();
        let updated = host.on_harness_updated(
            Box::new(move |harness, cx| listener.refresh_catalogs(harness, cx).detach()),
            cx,
        );
        let check = host.check_for_updates(cx);
        let check = cx.spawn(async move |this, cx| {
            let updates = check.await;
            this.update(cx, |this, cx| {
                this.updates = updates;
                cx.notify();
            })
            .ok();
        });
        Self {
            host,
            updates: Vec::new(),
            rows: HashMap::new(),
            top_offset,
            height: Rc::new(Cell::new(0.)),
            _check: check,
            _updated: updated,
        }
    }

    pub fn updates(&self) -> &[HarnessUpdate] {
        &self.updates
    }

    pub fn row(&self, harness: HarnessId) -> RowState {
        self.rows.get(&harness).cloned().unwrap_or_default()
    }

    pub fn set_top_offset(&mut self, top_offset: f32, cx: &mut Context<Self>) {
        self.top_offset = top_offset;
        cx.notify();
    }

    /// `start`: update these harnesses side by side.
    pub fn start(&mut self, targets: Vec<HarnessUpdate>, cx: &mut Context<Self>) {
        for update in targets {
            self.rows.insert(update.harness, RowState::Updating);
            let run = run_update(self.host.clone(), update.clone(), cx);
            cx.spawn(async move |this, cx| {
                let result = run.await;
                this.update(cx, |this, cx| {
                    this.rows.insert(update.harness, result);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    /// `dismiss`: only for this run; the next launch checks and offers again.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.host.dismiss_updates(cx);
        self.updates.clear();
        cx.notify();
    }

    fn pending(&self) -> Vec<HarnessUpdate> {
        self.updates
            .iter()
            .filter(|update| {
                matches!(
                    self.row(update.harness),
                    RowState::Idle | RowState::Failed(_)
                )
            })
            .cloned()
            .collect()
    }

    /// Reports the card's height after layout, when it changed.
    fn report_height(&mut self, height: f32, cx: &mut Context<Self>) {
        if (self.height.get() - height).abs() < 0.5 {
            return;
        }
        self.height.set(height);
        cx.emit(HarnessUpdateNoticeEvent::HeightChanged(height));
    }

    fn render_row(&self, update: &HarnessUpdate, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let state = self.row(update.harness);
        let harness = update.harness;
        let mut line = div()
            .flex()
            .items_center()
            .gap(u(8.))
            .child(provider_logo(harness_logo(harness)).size(16.))
            .child(
                text(harness.title())
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_px(13.)
                    .medium(),
            );
        line = match &state {
            RowState::Updated(version) => line.child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(4.))
                    .text_px(11.)
                    .text_color(theme.colors.success)
                    .child(icon(IconName::Check).size(u(14.)))
                    .child(text(format!("Updated to {version}"))),
            ),
            _ => {
                let updating = state == RowState::Updating;
                let label = match state {
                    RowState::Updating => "Updating",
                    RowState::Failed(_) => "Retry",
                    _ => "Update",
                };
                let fill = theme.content(0.10);
                let hover = theme.content(0.15);
                let target = update.clone();
                let selector = format!("button:{label}:{}", harness.as_str());
                let mut button = div()
                    .id(SharedString::from(format!("update-{}", harness.as_str())))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(6.))
                    .rounded(u(theme.radius.md))
                    .bg(fill)
                    .px(u(8.))
                    .py(u(4.))
                    .text_px(11.)
                    .medium()
                    .debug_selector(move || selector)
                    .when(updating, |el| {
                        el.child(spin_icon(
                            SharedString::from(format!("updating-{}", harness.as_str())),
                            IconName::Loader,
                            12.,
                            theme.colors.content,
                        ))
                    })
                    .child(label);
                if updating {
                    button = button.opacity(0.6);
                } else {
                    button = button.hover(move |s| s.bg(hover)).on_click(
                        cx.listener(move |this, _, _, cx| this.start(vec![target.clone()], cx)),
                    );
                }
                line.child(
                    text(format!("{} → {}", update.installed, update.latest))
                        .flex_none()
                        .font_family(theme.fonts.mono.clone())
                        .text_px(11.)
                        .text_color(theme.content(0.50)),
                )
                .child(button)
            }
        };
        div()
            .px(u(12.))
            .py(u(10.))
            .child(line)
            .when_some(
                match &state {
                    RowState::Failed(error) => Some(error.clone()),
                    _ => None,
                },
                |el, error| {
                    let ink = theme.colors.danger_soft;
                    el.child(
                        text(error)
                            .mt(u(6.))
                            .line_clamp(2)
                            .text_px(11.)
                            .leading(theme.leading.relaxed)
                            .text_color(gpui::Hsla {
                                a: ink.a * 0.9,
                                ..ink
                            }),
                    )
                },
            )
            .into_any_element()
    }
}

/// `runUpdate`. Some updaters exit cleanly without installing anything, so
/// success is the version the CLI reports afterwards, not the exit code. Its
/// models reload before the row says so, so the picker is current by then.
fn run_update(
    host: Rc<dyn HarnessUpdateHost>,
    update: HarnessUpdate,
    cx: &mut Context<HarnessUpdateNotice>,
) -> Task<RowState> {
    let harness = update.harness;
    let run = host.update_cli(harness, cx);
    cx.spawn(async move |_, cx| {
        if let Err(error) = run.await {
            return RowState::Failed(error);
        }
        let after = match cx.update(|cx| host.installed_version(harness, cx)).await {
            Ok(after) => after,
            Err(error) => return RowState::Failed(error),
        };
        let version = parse_version(after.as_deref().unwrap_or(""));
        match version {
            Some(version) if compare_semver(&version, &update.latest) >= 0 => {
                cx.update(|cx| host.refresh_catalogs(harness, cx)).await;
                cx.update(|cx| host.announce_updated(harness, cx));
                RowState::Updated(version)
            }
            version => RowState::Failed(format!(
                "Still on {} after updating.",
                version.unwrap_or(update.installed)
            )),
        }
    })
}

impl Render for HarnessUpdateNotice {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.updates.is_empty() {
            self.report_height(0., cx);
            return div().into_any_element();
        }
        let theme = Theme::of(cx).clone();
        let busy = self
            .updates
            .iter()
            .any(|update| self.row(update.harness) == RowState::Updating);
        let pending = self.pending();
        let any_updated = self
            .updates
            .iter()
            .any(|update| matches!(self.row(update.harness), RowState::Updated(_)));
        let viewport =
            f32::from(window.viewport_size().width) / f32::from(window.rem_size()) * 16.0;
        let width = 340f32.min(viewport - 24.);
        let hover = theme.content(0.10);
        let hover_ink = theme.colors.content;
        let mut header = div()
            .flex()
            .items_center()
            .gap(u(8.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .px(u(12.))
            .py(u(8.))
            .child(
                text(if self.updates.len() == 1 {
                    "Harness update available"
                } else {
                    "Harness updates available"
                })
                .min_w_0()
                .flex_1()
                .truncate()
                .text_px(12.)
                .semibold(),
            );
        if pending.len() > 1 {
            header = header.child(
                div()
                    .id("update-all")
                    .rounded(u(theme.radius.md))
                    .px(u(8.))
                    .py(u(4.))
                    .text_px(11.)
                    .medium()
                    .text_color(theme.content(0.70))
                    .hover(move |s| s.bg(hover).text_color(hover_ink))
                    .debug_selector(|| "button:Update all".into())
                    .on_click(cx.listener(move |this, _, _, cx| this.start(pending.clone(), cx)))
                    .child("Update all"),
            );
        }
        let mut close = div()
            .id("dismiss-updates")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(u(24.))
            .rounded(u(theme.radius.md))
            .text_color(theme.content(0.40))
            .debug_selector(|| "button:Dismiss harness updates".into())
            .child(icon(IconName::X).size(u(12.)));
        if busy {
            close = close.opacity(0.4);
        } else {
            close = close
                .hover(move |s| s.bg(hover).text_color(hover_ink))
                .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx)));
        }
        header = header.child(close);
        let mut rows = div().flex().flex_col();
        for (index, update) in self.updates.clone().iter().enumerate() {
            let row = self.render_row(update, cx);
            rows = rows.child(if index > 0 {
                div()
                    .border_t_1()
                    .border_color(theme.colors.stroke)
                    .child(row)
                    .into_any_element()
            } else {
                row
            });
        }
        let weak = cx.entity().downgrade();
        let probe = canvas(
            move |bounds, window, cx| {
                let height = f32::from(bounds.size.height) / f32::from(window.rem_size()) * 16.0;
                let weak = weak.clone();
                cx.defer(move |cx| {
                    weak.update(cx, |this, cx| this.report_height(height, cx))
                        .ok();
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        div()
            .id("harness-update-notice")
            .absolute()
            .top(u(self.top_offset))
            .right(u(12.))
            .w(u(width))
            .overflow_hidden()
            .rounded(u(theme.radius.xl))
            .border_1()
            .border_color(theme.content(0.10))
            .shadow_xl()
            .text_color(theme.colors.content)
            .occlude()
            .debug_selector(|| "status:Harness updates".into())
            .child(probe)
            .child(glass_backdrop(
                theme.radius.xl,
                24.,
                theme.colors.popover_backdrop,
            ))
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .child(header)
                    .child(rows)
                    .child(
                        text(if any_updated {
                            "Model picker refreshed with the new version’s models."
                        } else {
                            "New models often need the latest version."
                        })
                        .border_t_1()
                        .border_color(theme.colors.stroke)
                        .px(u(12.))
                        .py(u(8.))
                        .text_px(11.)
                        .text_color(theme.content(0.50)),
                    ),
            )
            .into_any_element()
    }
}
