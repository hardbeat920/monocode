//! The template gallery's shapes, mirroring the engine's
//! automations/templates.rs (a port of automationTemplates.ts). The engine
//! owns the template list; [`super::AutomationsData::templates`] hands it to
//! the view.

use monocode_ui::IconName;

use super::model::TemplateTrigger;

/// `AutomationTemplateCategoryId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateCategory {
    Popular,
    Review,
    Security,
    Incidents,
    Research,
    Environment,
}

impl TemplateCategory {
    pub const fn id(self) -> &'static str {
        match self {
            TemplateCategory::Popular => "popular",
            TemplateCategory::Review => "review",
            TemplateCategory::Security => "security",
            TemplateCategory::Incidents => "incidents",
            TemplateCategory::Research => "research",
            TemplateCategory::Environment => "environment",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            TemplateCategory::Popular => "Popular",
            TemplateCategory::Review => "Code Review",
            TemplateCategory::Security => "Security",
            TemplateCategory::Incidents => "Incidents & Triage",
            TemplateCategory::Research => "Data & Research",
            TemplateCategory::Environment => "Environment",
        }
    }
}

/// `AUTOMATION_TEMPLATE_CATEGORIES`, in gallery order.
pub const AUTOMATION_TEMPLATE_CATEGORIES: [TemplateCategory; 6] = [
    TemplateCategory::Popular,
    TemplateCategory::Review,
    TemplateCategory::Security,
    TemplateCategory::Incidents,
    TemplateCategory::Research,
    TemplateCategory::Environment,
];

/// `AutomationTemplateIcon`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateIcon {
    Search,
    Alert,
    File,
    Check,
    Lock,
    Pr,
    Inbox,
    Gauge,
    Terminal,
    Note,
}

impl TemplateIcon {
    /// `TEMPLATE_ICONS`.
    pub fn icon(self) -> IconName {
        match self {
            TemplateIcon::Search => IconName::Search,
            TemplateIcon::Alert => IconName::AlertCircle,
            TemplateIcon::File => IconName::File,
            TemplateIcon::Check => IconName::CheckCircle,
            TemplateIcon::Lock => IconName::Lock,
            TemplateIcon::Pr => IconName::GitPullRequest,
            TemplateIcon::Inbox => IconName::Inbox,
            TemplateIcon::Gauge => IconName::Gauge,
            TemplateIcon::Terminal => IconName::Terminal,
            TemplateIcon::Note => IconName::StickyNote,
        }
    }
}

/// `AutomationTemplate`. `category` is never `Popular`; `popular` marks the
/// templates that also show there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationTemplate {
    pub id: String,
    pub category: TemplateCategory,
    pub popular: bool,
    pub icon: TemplateIcon,
    pub name: String,
    pub description: String,
    pub prompt: String,
    pub trigger: TemplateTrigger,
    pub trigger_label: String,
}

/// `templatesForCategory`.
pub fn templates_for_category(
    templates: &[AutomationTemplate],
    category: TemplateCategory,
) -> Vec<AutomationTemplate> {
    templates
        .iter()
        .filter(|template| match category {
            TemplateCategory::Popular => template.popular,
            category => template.category == category,
        })
        .cloned()
        .collect()
}
