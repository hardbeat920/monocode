//! The full pages that replace the main column: Search, Inbox, Notes,
//! Automations, and Settings.

pub mod automations;
pub mod connections;
pub mod inbox;
pub mod notes;
pub mod search;
pub mod settings;

use gpui::{AnyView, App, Window};

use crate::slots::Page;

/// `AppSlots::page`: the view for a full page.
pub fn page_view(page: Page, window: &mut Window, cx: &mut App) -> Option<AnyView> {
    match page {
        Page::Search => search::page(window, cx),
        Page::Inbox => inbox::page(window, cx),
        Page::Notes => notes::page(window, cx),
        Page::Automations => automations::page(window, cx),
        Page::Settings => settings::page(window, cx),
    }
}
