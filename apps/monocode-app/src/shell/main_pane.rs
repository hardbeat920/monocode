//! The main pane area: the active tab's split tree. Port of the pane
//! layout in src/features/workspace/ui (`LayoutView`): each leaf is a
//! session pane (transcript and composer) or a file surface (editor or
//! diff), and splits share their space by the stored sizes. Sash dragging
//! and pane drops land with the M3 workspace work.

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
    Window, div, relative,
};
use monocode_core::session::session_display_title;
use monocode_engine::runtime::Engine;
use monocode_layout::{LayoutNode, SplitDir, WorkspaceTab, find_surface_pane, leaf_ids};
use monocode_ui::widgets::{icon_button, spinner};
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};

use super::Shell;

impl Shell {
    pub(super) fn render_main_pane(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let tab = self
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.read(cx).active_tab().cloned());
        let body: AnyElement = match tab {
            Some(tab) => {
                let split = leaf_ids(&tab.layout).len() > 1;
                self.sync_pane_focus(&tab, window, cx);
                self.render_node(&tab.layout, &tab, split, window, cx)
            }
            None => div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(spinner("workspace-loading").color(theme.content(0.45)))
                .into_any_element(),
        };
        div()
            .id("main-pane")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(body)
    }

    /// The focused pane's composer takes the model hotkeys.
    fn sync_pane_focus(&mut self, tab: &WorkspaceTab, window: &mut Window, cx: &mut Context<Self>) {
        for (id, pane) in &self.session_panes {
            let focused = *id == tab.focused_id;
            pane.update(cx, |pane, cx| pane.set_focused(focused, window, cx));
        }
    }

    fn render_node(
        &mut self,
        node: &LayoutNode,
        tab: &WorkspaceTab,
        split: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match node {
            LayoutNode::Leaf(leaf) => {
                if find_surface_pane(tab, &leaf.id).is_some() {
                    match self.file_pane(&leaf.id, window, cx) {
                        Some(pane) => pane.into_any_element(),
                        None => div().flex_1().into_any_element(),
                    }
                } else {
                    let pane = self.session_pane(&leaf.id, window, cx);
                    if split {
                        self.render_split_session(&leaf.id, tab, pane.into_any_element(), cx)
                    } else {
                        pane.into_any_element()
                    }
                }
            }
            LayoutNode::Split(node) => {
                let theme = Theme::of(cx).clone();
                let row = node.dir == SplitDir::Right;
                let mut container = div().flex().flex_1().min_w_0().min_h_0().size_full();
                container = if row {
                    container.flex_row()
                } else {
                    container.flex_col()
                };
                for (index, child) in node.children.iter().enumerate() {
                    let size = node.sizes.get(index).copied().unwrap_or(1.0) as f32;
                    let mut cell = div()
                        .flex()
                        .flex_col()
                        .min_w_0()
                        .min_h_0()
                        .flex_basis(relative(size))
                        .flex_grow(1.)
                        .flex_shrink(1.);
                    if index > 0 {
                        cell = if row {
                            cell.border_l_1()
                        } else {
                            cell.border_t_1()
                        }
                        .border_color(theme.colors.stroke);
                    }
                    let child = self.render_node(child, tab, split, window, cx);
                    container = container.child(cell.child(child));
                }
                container.into_any_element()
            }
        }
    }

    /// A session pane inside a split gets the `inSplit` header: focus dot,
    /// title, and close.
    fn render_split_session(
        &self,
        session_id: &str,
        tab: &WorkspaceTab,
        pane: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let focused = tab.focused_id == session_id;
        let title = Engine::sessions(cx)
            .read(cx)
            .get(session_id)
            .map(|session| session_display_title(&session.title, session.harness))
            .unwrap_or_default();
        let close_id = session_id.to_string();
        let header = div()
            .flex()
            .flex_none()
            .h(u(theme.metrics.toolbar_height))
            .items_center()
            .gap(u(6.))
            .px(u(8.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .child(
                icon(IconName::GripVertical)
                    .size(u(14.))
                    .text_color(theme.content(0.35)),
            )
            .child(div().flex_none().size(u(8.)).rounded_full().bg(if focused {
                theme.colors.accent
            } else {
                gpui::transparent_black()
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_px(theme.text.label)
                    .child(title),
            )
            .child(
                icon_button(
                    gpui::ElementId::from(gpui::SharedString::from(format!(
                        "pane-close-{session_id}"
                    ))),
                    IconName::X,
                )
                .size(20.)
                .icon_size(12.)
                .tooltip("Close Pane (⌘W)")
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(workspace) = &this.workspace {
                        workspace
                            .update(cx, |workspace, cx| {
                                workspace.close_pane(Some(&close_id), cx)
                            })
                            .detach();
                    }
                })),
            );
        div()
            .id(gpui::SharedString::from(format!("split-{session_id}")))
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(header)
            .child(pane)
            .into_any_element()
    }
}
