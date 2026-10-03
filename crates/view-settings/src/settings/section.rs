//! What every section view is built with: the store, the platform, the
//! hosts, and the page's reveal state.

use gpui::{App, Entity};
use monocode_core::Platform;
use monocode_settings::Kv;

use super::chrome::Reveal;
use super::host::SettingsHosts;

/// The page's `RevealedSetting` context, shared with each section.
#[derive(Default)]
pub struct RevealState(pub Reveal);

#[derive(Clone)]
pub struct SectionContext {
    pub kv: Kv,
    pub platform: Platform,
    pub hosts: SettingsHosts,
    pub reveal: Entity<RevealState>,
}

impl SectionContext {
    /// The reveal state for this frame.
    pub fn reveal(&self, cx: &App) -> Reveal {
        self.reveal.read(cx).0.clone()
    }
}
