//! A window that shows one slot view on its own, for `--view` screenshots
//! of a page or a sidebar tab without the shell. It restores the saved
//! workspace the way the shell does, sets it as the window's workspace, and
//! then draws the slot.

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    Styled as _, Task, Window, div,
};
use monocode_app::boot::{self, AppServices};
use monocode_app::bridge::ActiveWorkspace;
use monocode_engine::workspace::Workspace;
use monocode_ui::widgets::{spinner, toast_stack};
use monocode_ui::{Theme, u};

use crate::shell::SidebarTab;
use crate::slots::{AppSlots, Page, window_workspace};

/// Which slot the window shows.
#[derive(Clone, Copy, Debug)]
pub enum SlotKind {
    Page(Page),
    SidebarTab(SidebarTab),
    Workspace,
}

pub struct Standalone {
    kind: SlotKind,
    workspace: Option<Entity<Workspace>>,
    _restore: Option<Task<()>>,
}

impl Standalone {
    pub fn build(kind: SlotKind, window: &mut Window, cx: &mut App) -> AnyView {
        cx.new(|cx| Self::new(kind, window, cx)).into()
    }

    fn new(kind: SlotKind, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let restore = (AppServices::try_global(cx).is_some() && window_workspace(cx).is_none())
            .then(|| {
                let restore = boot::restore_workspace(cx);
                cx.spawn_in(window, async move |this, cx| {
                    let restored = restore.await;
                    this.update_in(cx, |this, window, cx| {
                        let workspace = cx.new(|cx| Workspace::new(restored.config, cx));
                        ActiveWorkspace::set(workspace.downgrade(), cx);
                        crate::slots::register_workspace(workspace.clone(), window, cx);
                        this.workspace = Some(workspace);
                        cx.notify();
                    })
                    .ok();
                })
            });
        Self {
            kind,
            workspace: None,
            _restore: restore,
        }
    }
}

impl Render for Standalone {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let slots = AppSlots::get(cx);
        let view = match self.kind {
            SlotKind::Page(page) => slots.page.and_then(|factory| factory(page, window, cx)),
            SlotKind::SidebarTab(tab) => slots
                .sidebar_tab
                .and_then(|factory| factory(tab, window, cx)),
            SlotKind::Workspace => slots.workspace.map(|factory| factory(window, cx)),
        };
        let body = match view {
            Some(view) => view.into_any_element(),
            None => div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(spinner("standalone-loading").color(theme.content(0.45)))
                .into_any_element(),
        };
        let column = match self.kind {
            SlotKind::SidebarTab(_) => div()
                .flex()
                .flex_col()
                .h_full()
                .w(u(300.))
                .border_r_1()
                .border_color(theme.colors.stroke)
                .bg(theme.colors.sidebar_glass)
                .child(body),
            _ => div()
                .flex()
                .flex_col()
                .size_full()
                .bg(theme.colors.body_glass)
                .child(body),
        };
        div()
            .flex()
            .size_full()
            .bg(theme.colors.root_background)
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone())
            .child(column)
            .child(toast_stack())
    }
}
