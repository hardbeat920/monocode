//! One file per Settings page, each a port of its SettingsView.tsx function.
mod appearance;
mod archive;
mod chat;
mod connections;
mod general;
mod inbox;
mod keybindings;
mod mcp;
mod mobile;
mod monos;
mod permissions;
mod providers;
mod skills;
mod worktrees;

pub(super) use appearance::parse_hex;
pub(crate) use appearance::{ACCENTS, swatch};
pub(super) use connections::ConnectionsState;
pub(crate) use keybindings::command_shortcut;
pub(super) use mobile::MobileState;

use super::{Section, SettingsView};
use gpui::{AnyElement, Context, Window};

impl SettingsView {
    pub(super) fn page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        match self.section {
            Section::General => self.general_page(cx),
            Section::Connections => {
                // As on the earlier desktop and in the phone app's copy:
                // Settings → Connections holds both machines and phones.
                let mut page = self.connections_page(cx);
                page.extend(self.mobile_page(cx));
                page
            }
            Section::Appearance => self.appearance_page(window, cx),
            Section::Keybindings => self.keybindings_page(cx),
            Section::Chat => self.chat_page(cx),
            Section::Providers => self.providers_page(cx),
            Section::Mcp => self.mcp_page(cx),
            Section::Permissions => self.permissions_page(cx),
            Section::Skills => self.skills_page(cx),
            Section::Monos => self.monos_page(cx),
            Section::Inbox => self.inbox_page(cx),
            Section::Archive => self.archive_page(cx),
            Section::Worktrees => self.worktrees_page(cx),
        }
    }
}
