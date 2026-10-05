//! The automations page (AutomationsView.tsx): automation cards, the
//! template picker, the editor with its time and event trigger sentences,
//! and the run history.

pub mod data;
pub mod editor;
pub mod local_time;
pub mod model;
pub mod templates;
pub mod triggers;
pub mod view;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod view_tests;

use gpui::{AnyElement, Hsla, IntoElement as _, Styled as _};
use monocode_ui::{IconName, icon, u};

pub use data::{
    AutomationsData, AutomationsSnapshot, DraftTarget, LocalAutomations, NoSkills,
    ProviderConnections, SessionFolderOption,
};
pub use editor::{AutomationEditor, EditorStatus, EditorTab};
pub use model::{Automation, AutomationDraft, AutomationRun, AutomationTrigger};
pub use templates::{AutomationTemplate, TemplateCategory, TemplateIcon};
pub use view::AutomationsView;

use crate::widgets::provider_mark::{MarkProvider, provider_mark};
use model::AutomationTriggerKind;

/// `TriggerMark`: a clock for schedules, else the Inbox provider's logo.
pub fn trigger_mark(kind: AutomationTriggerKind, size: f32, ink: Hsla) -> AnyElement {
    let provider = match kind {
        AutomationTriggerKind::Time => {
            return icon(IconName::Clock)
                .size(u(size))
                .text_color(ink)
                .into_any_element();
        }
        AutomationTriggerKind::Github => MarkProvider::Github,
        AutomationTriggerKind::Linear => MarkProvider::Linear,
        AutomationTriggerKind::Jira => MarkProvider::Jira,
        AutomationTriggerKind::Gitlab => MarkProvider::Gitlab,
        AutomationTriggerKind::AzureDevops => MarkProvider::AzureDevops,
    };
    provider_mark(Some(provider), size, ink)
}
