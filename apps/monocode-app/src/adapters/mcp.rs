//! `monocode_view_pages::mcp::McpData` over the MCP settings cache.

use gpui::{AnyView, App, AppContext as _, Window};
use monocode_view_settings::settings::SlotContext;

/// The Settings page's MCP section.
pub fn mcp_settings_slot(ctx: &SlotContext, window: &mut Window, cx: &mut App) -> AnyView {
    let _ = (ctx, window);
    cx.new(|_| gpui::Empty).into()
}
