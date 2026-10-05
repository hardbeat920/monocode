//! The skills section (SkillsPage.tsx), which the settings page embeds.

pub mod data;
pub mod view;

#[cfg(test)]
mod tests;

pub use data::{DiscoveredSkill, LocalSkills, SkillsData};
pub use view::SkillsPage;
