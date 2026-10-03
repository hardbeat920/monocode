//! Workbench panes. Ports of src/features/workspace/ui (the split tree,
//! surface tabs, tab group menu, and workspace picker), the tab motion and
//! drag hooks, and the session surfaces from src/features/sessions/ui: the
//! empty session, the model welcome screens, provider sign-in, the session
//! review card, and the notices.
//!
//! The views take the layout model from `monocode-layout` and report what
//! the user did through events. They do not touch the engine; the app wires
//! them to its `Workspace`.

pub mod agent_tab_view;
pub mod animated_reorder;
pub mod drag_resize;
pub mod empty_session;
pub mod notices;
pub mod pane_tree;
pub mod pixel_art;
pub mod provider_sign_in;
pub mod reorder;
pub mod session_review;
pub mod sortable;
pub mod speech_bubble;
pub mod submenu;
pub mod surface_tabs;
pub mod tab_close_motion;
pub mod tab_group_menu;
pub mod tab_width_motion;
pub mod welcome;
pub mod workspace_picker;

#[cfg(test)]
mod test_support;

/// Installs the panes' key bindings: ⌘⇧G for the workspace mode toggle.
pub fn init(cx: &mut gpui::App) {
    workspace_picker::init(cx);
}
