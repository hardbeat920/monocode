//! What the skills page reads and changes. `DiscoveredSkill` mirrors
//! `monocode_process::skills::DiscoveredSkill`; the calls match the engine's
//! skill catalog (`SkillCatalog::load_disabled_skill_paths`,
//! `save_disabled_skill_paths`, `invalidate_skills`, `create_blank_skill`,
//! `subscribe_changes`) and the file calls the page made directly.

use std::collections::HashMap;

use gpui::{App, AppContext as _, ClipboardItem, Entity, Subscription, Task};
use serde::{Deserialize, Serialize};

use crate::data::{DataTask, Listener};

/// A SKILL.md the scan found. `scope` is `project`, `user`, or `builtin`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredSkill {
    pub name: String,
    pub description: String,
    pub path: String,
    pub scope: String,
    pub source: String,
}

impl DiscoveredSkill {
    /// The scope chip: Personal, MonoCode, or Project.
    pub fn scope_label(&self) -> &'static str {
        match self.scope.as_str() {
            "user" => "Personal",
            "builtin" => "MonoCode",
            _ => "Project",
        }
    }
}

pub trait SkillsData: 'static {
    /// `listSkills(cwd)`: every SKILL.md for the project, hidden ones too.
    fn list_skills(&self, cwd: &str, cx: &mut App) -> DataTask<Vec<DiscoveredSkill>>;
    fn read_text_file(&self, path: &str, cx: &mut App) -> DataTask<String>;
    /// `loadDisabledSkillPaths`.
    fn disabled_paths(&self, cx: &App) -> Vec<String>;
    /// `saveDisabledSkillPaths`.
    fn save_disabled_paths(&self, paths: Vec<String>, cx: &mut App) -> Result<(), String>;
    /// `SKILLS_CHANGE_EVENT`.
    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription;
    /// `invalidateSkills()` and `SKILLS_CHANGE_EVENT`.
    fn invalidate(&self, cx: &mut App);
    /// `createBlankSkill`: a starter SKILL.md in the project or user folder.
    fn create_blank_skill(
        &self,
        cwd: &str,
        name: &str,
        project: bool,
        cx: &mut App,
    ) -> DataTask<()>;
    /// `revealItemInDir`.
    fn reveal(&self, path: &str, cx: &mut App) -> DataTask<()>;
    /// `copyText`.
    fn copy_text(&self, text: &str, cx: &mut App) -> DataTask<()> {
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        Task::ready(Ok(()))
    }
}

/// The state behind [`LocalSkills`].
#[derive(Default)]
pub struct LocalSkillsState {
    pub skills: Vec<DiscoveredSkill>,
    pub files: HashMap<String, String>,
    pub disabled: Vec<String>,
    /// Fails reads of these paths with the message.
    pub read_errors: HashMap<String, String>,
    pub lists: usize,
    pub revealed: Vec<String>,
    pub created: Vec<(String, String, bool)>,
}

/// In-memory skills for the gallery and tests. Calls answer at once.
#[derive(Clone)]
pub struct LocalSkills {
    state: Entity<LocalSkillsState>,
}

impl LocalSkills {
    pub fn new(skills: Vec<DiscoveredSkill>, cx: &mut App) -> Self {
        Self {
            state: cx.new(|_| LocalSkillsState {
                skills,
                ..Default::default()
            }),
        }
    }

    pub fn state(&self) -> &Entity<LocalSkillsState> {
        &self.state
    }

    pub fn set_file(&self, path: &str, text: &str, cx: &mut App) {
        self.state.update(cx, |state, _| {
            state.files.insert(path.to_string(), text.to_string());
        });
    }
}

impl SkillsData for LocalSkills {
    fn list_skills(&self, _cwd: &str, cx: &mut App) -> DataTask<Vec<DiscoveredSkill>> {
        let skills = self.state.update(cx, |state, _| {
            state.lists += 1;
            state.skills.clone()
        });
        Task::ready(Ok(skills))
    }

    fn read_text_file(&self, path: &str, cx: &mut App) -> DataTask<String> {
        let state = self.state.read(cx);
        if let Some(error) = state.read_errors.get(path) {
            return Task::ready(Err(error.clone()));
        }
        Task::ready(
            state
                .files
                .get(path)
                .cloned()
                .ok_or_else(|| format!("No such file: {path}")),
        )
    }

    fn disabled_paths(&self, cx: &App) -> Vec<String> {
        self.state.read(cx).disabled.clone()
    }

    fn save_disabled_paths(&self, paths: Vec<String>, cx: &mut App) -> Result<(), String> {
        self.state.update(cx, |state, cx| {
            state.disabled = paths;
            cx.notify();
        });
        Ok(())
    }

    fn subscribe(&self, listener: Listener, cx: &mut App) -> Subscription {
        cx.observe(&self.state, move |_, cx| listener(cx))
    }

    fn invalidate(&self, cx: &mut App) {
        self.state.update(cx, |_, cx| cx.notify());
    }

    fn create_blank_skill(
        &self,
        cwd: &str,
        name: &str,
        project: bool,
        cx: &mut App,
    ) -> DataTask<()> {
        self.state.update(cx, |state, _| {
            state
                .created
                .push((cwd.to_string(), name.to_string(), project));
            let path = if project {
                format!("{cwd}/.agents/skills/{name}/SKILL.md")
            } else {
                format!("~/.agents/skills/{name}/SKILL.md")
            };
            state.skills.push(DiscoveredSkill {
                name: name.to_string(),
                description: String::new(),
                path,
                scope: if project { "project" } else { "user" }.into(),
                source: "agents".into(),
            });
        });
        Task::ready(Ok(()))
    }

    fn reveal(&self, path: &str, cx: &mut App) -> DataTask<()> {
        self.state
            .update(cx, |state, _| state.revealed.push(path.to_string()));
        Task::ready(Ok(()))
    }
}
