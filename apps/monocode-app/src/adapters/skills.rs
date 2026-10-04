//! `monocode_view_pages::skills::SkillsData` over the skill files.

use gpui::{AnyView, App, AppContext as _, Window};
use monocode_view_settings::settings::SlotContext;

/// The Settings page's Skills section.
pub fn skills_slot(ctx: &SlotContext, window: &mut Window, cx: &mut App) -> AnyView {
    let _ = (ctx, window);
    cx.new(|_| gpui::Empty).into()
}
