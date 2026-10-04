//! Shared actionable updates and the installed-version marker.

use crate::adapters::settings::updater::{self, UpdateState};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, Global, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div,
};
use monocode_app::boot::AppServices;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};
use monocode_updater::update_notice::{InstalledUpdate, consume_installed_update};
use monocode_view_settings::settings::UpdatePhase;
use std::rc::Rc;

struct SharedNotices(Entity<Notices>);
impl Global for SharedNotices {}

struct Notices {
    update: Entity<UpdateState>,
    installed: Option<InstalledUpdate>,
    _subscription: Subscription,
}

fn ensure(cx: &mut App) -> Option<Entity<Notices>> {
    AppServices::try_global(cx)?;
    if let Some(notices) = cx.try_global::<SharedNotices>() {
        return Some(notices.0.clone());
    }
    let installed = consume_installed_update(&AppServices::global(cx).kv);
    let update = updater::state(cx);
    let notices = cx.new(|cx| Notices {
        _subscription: cx.observe(&update, |_, _, cx| cx.notify()),
        update,
        installed,
    });
    cx.set_global(SharedNotices(notices.clone()));
    Some(notices)
}

pub fn view(cx: &mut App) -> AnyElement {
    ensure(cx)
        .map(IntoElement::into_any_element)
        .unwrap_or_else(|| div().into_any_element())
}

pub fn installed(cx: &mut App) -> Option<String> {
    ensure(cx)?
        .read(cx)
        .installed
        .as_ref()
        .map(|update| update.version.clone())
}

pub fn dismiss(cx: &mut App) {
    if let Some(notices) = ensure(cx) {
        notices.update(cx, |notices, cx| {
            notices.installed = None;
            cx.notify();
        });
    }
}

fn actionable(phase: UpdatePhase) -> bool {
    matches!(phase, UpdatePhase::Available | UpdatePhase::Downloading)
}

impl Render for Notices {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.update.read(cx).snapshot.clone();
        if self.installed.is_none() && !actionable(snapshot.phase) {
            return div().into_any_element();
        }
        let theme = Theme::of(cx).clone();
        let mut column = div().flex().flex_col().gap(u(6.)).px(u(8.)).pt(u(8.));
        if let Some(update) = &self.installed {
            column = column.child(super::update_rail_card::card(&update.version, cx));
        }
        if actionable(snapshot.phase) {
            let busy = snapshot.phase == UpdatePhase::Downloading;
            let label = if busy {
                snapshot
                    .progress
                    .map(|progress| format!("Downloading {progress}%"))
                    .unwrap_or_else(|| "Downloading...".into())
            } else {
                format!(
                    "Update to {}",
                    snapshot
                        .available_version
                        .as_deref()
                        .unwrap_or("new version")
                )
            };
            column = column.child(
                div()
                    .id("sidebar-install-update")
                    .flex()
                    .w_full()
                    .items_center()
                    .gap(u(8.))
                    .p(u(8.))
                    .rounded(u(8.))
                    .bg(theme.accent(if busy { 0.05 } else { 0.15 }))
                    .text_color(theme.colors.content)
                    .cursor_pointer()
                    .child(
                        icon(IconName::ArrowDownCircle)
                            .size(u(16.))
                            .text_color(theme.colors.accent),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_px(12.)
                            .medium()
                            .child(label),
                    )
                    .child(
                        div()
                            .text_px(11.)
                            .text_color(theme.content(0.4))
                            .child(format!("v{}", snapshot.current_version)),
                    )
                    .on_click(move |_, _, cx| {
                        if !busy {
                            updater::install(Rc::new(|_, _| {}), cx).detach();
                        }
                    }),
            );
        }
        column.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sidebar_updates_only_show_available_and_downloading() {
        assert!(actionable(UpdatePhase::Available));
        assert!(actionable(UpdatePhase::Downloading));
        for phase in [
            UpdatePhase::Idle,
            UpdatePhase::Checking,
            UpdatePhase::Current,
            UpdatePhase::Error,
        ] {
            assert!(!actionable(phase));
        }
    }
}
