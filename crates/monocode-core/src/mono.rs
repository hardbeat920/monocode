//! Persistent Mono identities and app-owned context, shared by native views,
//! scheduled runs and the local app CLI. React's monos/model is the reference.
pub mod api;
pub mod bridge;
pub mod cards;
mod desktop;
pub mod files;
pub mod habits;
pub mod memory;
pub mod work;
pub(crate) use desktop::codex_home;
#[cfg(test)]
mod tests;

use crate::{
    project_appearance::{Background, COLORS, MASCOTS},
    store::Store,
    transcript::Row,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

pub const COLORS_MONO: [u32; 9] = [
    COLORS[1], COLORS[2], COLORS[3], COLORS[4], COLORS[5], COLORS[6], COLORS[7], COLORS[8],
    0x8b63e9,
];
pub const DEFAULT_SOUL: &str =
    "# Soul\n\n## Standing instructions\n- (Add anything it should always keep in mind.)\n";
pub fn new_id() -> String {
    format!("{:032x}", rand::random::<u128>())
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Mono {
    pub id: String,
    pub session_id: Option<i64>,
    /// Keep a missing desktop association visible instead of silently starting
    /// a replacement conversation when its saved UUID cannot be resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_session: Option<String>,
    pub name: String,
    pub mascot: String,
    pub color: u32,
    pub projects: Vec<String>,
    pub show_started_sessions_in_sidebar: bool,
    pub use_sidebar_folders: bool,
    pub background: Option<Background>,
    pub launches: Vec<Launch>,
    pub documents: Vec<Document>,
    /// References last projected from desktop blocks, to reconcile deletions.
    #[serde(default)]
    pub desktop_document_ids: Vec<String>,
    #[serde(default)]
    pub desktop_launch_ids: Vec<i64>,
    pub outbox: Vec<Message>,
    pub pending_reports: Vec<String>,
    pub habit_sessions: Vec<i64>,
    pub seen_rows: usize,
}
impl Default for Mono {
    fn default() -> Self {
        Self {
            id: new_id(),
            session_id: None,
            unavailable_session: None,
            name: String::new(),
            mascot: MASCOTS[0].into(),
            color: COLORS_MONO[0],
            projects: Vec::new(),
            show_started_sessions_in_sidebar: false,
            use_sidebar_folders: false,
            background: None,
            launches: Vec::new(),
            documents: Vec::new(),
            desktop_document_ids: Vec::new(),
            desktop_launch_ids: Vec::new(),
            outbox: Vec::new(),
            pending_reports: Vec::new(),
            habit_sessions: Vec::new(),
            seen_rows: 0,
        }
    }
}
impl Mono {
    pub fn label(&self) -> String {
        if !self.name.trim().is_empty() {
            return self.name.clone();
        }
        let name = if self.mascot == "mushroom" {
            "Shroom".into()
        } else {
            let mut chars = self.mascot.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        };
        format!("Mono{name}")
    }
    pub fn works_on(&self, project: &str) -> bool {
        self.projects
            .iter()
            .any(|p| crate::files::path_key(p) == crate::files::path_key(project))
    }
    pub fn project(&self, requested: Option<&str>) -> Result<String, String> {
        if let Some(requested) = requested {
            if let Some(path) = self
                .projects
                .iter()
                .find(|path| crate::files::path_key(path) == crate::files::path_key(requested))
            {
                return Ok(path.clone());
            }
            let named: Vec<_> = self
                .projects
                .iter()
                .filter(|p| {
                    Path::new(p)
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(requested))
                })
                .collect();
            if named.len() == 1 {
                return Ok(named[0].clone());
            }
            return Err("Choose one of this Mono's assigned projects by full path".into());
        }
        match self.projects.as_slice() {
            [project] => Ok(project.clone()),
            [] => Err("Add a project in this Mono's settings first".into()),
            _ => Err("This Mono has several projects; include project with its path".into()),
        }
    }
    pub fn normalize(&mut self) {
        self.name = self.name.trim().chars().take(40).collect();
        if !MASCOTS.contains(&self.mascot.as_str()) {
            self.mascot = MASCOTS[0].into();
        }
        if self.color > 0xffffff {
            self.color = COLORS_MONO[0];
        }
        let mut seen = HashSet::new();
        self.projects
            .retain(|p| !p.trim().is_empty() && seen.insert(crate::files::path_key(p)));
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Launch {
    pub session_id: i64,
    pub cwd: String,
    pub title: String,
    pub harness: String,
    pub model: Option<String>,
    pub origin_turn: u64,
    pub notify_on_complete: bool,
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub dismissed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub summary: String,
    pub body: String,
    pub created_at: u64,
    pub updated_at: u64,
}
pub type Work = (Vec<Document>, Vec<Launch>);
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub sent_at: Option<u64>,
    #[serde(default)]
    pub internal: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub attachments: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Roster {
    pub monos: Vec<Mono>,
    pub hidden_sessions: Vec<i64>,
    /// Last observed desktop identities, for reconciling later desktop edits
    /// without discarding deliberate native edits or resurrecting deletions.
    #[serde(skip_serializing_if = "Option::is_none")]
    desktop_source: Option<Vec<Mono>>,
}
impl Roster {
    pub fn load(store: &Store) -> Result<Self, String> {
        let mut roster: Self = store
            .setting("mono-roster")?
            .map(|text| {
                serde_json::from_str(&text).map_err(|e| format!("Cannot read Mono roster: {e}"))
            })
            .transpose()?
            .unwrap_or_default();
        if store.uses_monocode_schema() {
            if let Some(preferences) = store.desktop_preferences() {
                roster.reconcile_desktop(store, preferences)?;
            }
        }
        let mut ids = HashSet::new();
        roster.monos.retain(|m| {
            !m.id.is_empty()
                && m.id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
                && ids.insert(m.id.clone())
        });
        for mono in &mut roster.monos {
            mono.normalize();
        }
        Ok(roster)
    }
    pub fn save(&self, store: &Store) -> Result<(), String> {
        store.save_setting(
            "mono-roster",
            &serde_json::to_string(self).map_err(|e| e.to_string())?,
        )
    }
    pub fn create(&mut self, projects: Vec<String>) -> &Mono {
        let mut mono = Mono {
            mascot: MASCOTS
                .iter()
                .find(|name| !self.monos.iter().any(|m| &m.mascot == *name))
                .unwrap_or(&MASCOTS[self.monos.len() % MASCOTS.len()])
                .to_string(),
            color: *COLORS_MONO
                .iter()
                .find(|color| !self.monos.iter().any(|m| m.color == **color))
                .unwrap_or(&COLORS_MONO[self.monos.len() % COLORS_MONO.len()]),
            projects,
            ..Default::default()
        };
        mono.normalize();
        self.monos.push(mono);
        self.monos.last().unwrap()
    }
    pub fn get(&self, id: &str) -> Option<&Mono> {
        self.monos.iter().find(|m| m.id == id)
    }
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Mono> {
        self.monos.iter_mut().find(|m| m.id == id)
    }
    pub fn of_session(&self, id: i64) -> Option<&Mono> {
        self.monos.iter().find(|m| m.session_id == Some(id))
    }
    pub fn reorder(&mut self, ids: &[String]) {
        let mut seen = HashSet::new();
        let ordered: Vec<_> = ids
            .iter()
            .filter(|id| seen.insert(id.as_str()))
            .filter_map(|id| self.get(id).cloned())
            .collect();
        let mut next = ordered.iter();
        for mono in &mut self.monos {
            if ordered.iter().any(|m| m.id == mono.id) {
                *mono = next.next().unwrap().clone();
            }
        }
    }
}

pub fn root(store: &Store) -> PathBuf {
    store
        .data_dir()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("monocode-test-{}", new_id())))
        .join("monos")
}
pub fn load_files(root: &Path, mono: &Mono) -> Result<files::AgentFiles, String> {
    let mut files = files::load(root, &mono.id, None)?;
    if files.soul.is_none() {
        let soul = DEFAULT_SOUL;
        match files::save(
            root,
            &mono.id,
            None,
            "SOUL.md",
            soul,
            Some(&files.soul_hash),
        ) {
            Ok(hash) => {
                files.soul = Some(soul.into());
                files.soul_hash = hash;
            }
            Err(error) if error == files::CONFLICT => files = files::load(root, &mono.id, None)?,
            Err(error) => return Err(error),
        }
    }
    Ok(files)
}

pub fn context(mono: &Mono, files: &files::AgentFiles, executable: &Path) -> String {
    let cli = format!(
        "{} app",
        shlex::try_quote(&executable.to_string_lossy()).unwrap_or_default()
    );
    let projects = if mono.projects.is_empty() {
        "No projects assigned yet. Ask the user to add projects in your settings before doing project work.".into()
    } else {
        mono.projects
            .iter()
            .map(|p| format!("- {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "<monocode_context>\nMonoCode adds this context, not the user. Act on it without quoting or describing it.\n\nYou are {}, a persistent Mono in MonoCode. Work only on the projects assigned to you:\n{projects}\n\nThe app CLI is `{cli} ACTION --json JSON`. Its credentials are already in your environment; never print them. Run `{cli} help` for the exact actions and inputs. Use sessions.*, worktrees.* and folders.* with project set to an assigned path. Use sessions.start for substantial independent work, inherit your model unless another is appropriate, and avoid overlapping edits. Once delegation is accepted, tell the user and end your turn. You will receive one completion report after every session in that turn's group stops; then inspect and integrate the results. sessions.send supports notifyOnComplete:true. Never poll delegated work.\n\n<soul>\n{}\n</soul>\nStanding instructions rank below the user's current request. Change your soul only when asked, through soul.read and soul.update with expectedHash; never edit its file directly.\n\n<memory>\n{}\nTopic notes: {}\n</memory>\nKeep useful facts through memory.add/replace/remove, use memory.search before answering about earlier facts, and never save credentials. Topic notes are read with memory.read.\n\nHabits are scheduled work in separate sessions. Suggest them when useful, but add one with habits.add only after agreement. State whether its instructions allow changes. Quiet runs should return NOTHING_TO_REPORT.\n\nSave substantial written reports with artifacts.write, kind document, title, summary and complete Markdown body. Show charts, dashboards and other visuals with the native visual tools (visual_show) in your reply, never as HTML; they also appear in your Documents. Cards appear in the chat and overview. Use notes.write only for the user's Notes.\n\nFor acknowledgements requiring no words, your whole reply may be <reaction>👍</reaction> with one fitting emoji. Questions and requests still get a written answer.\n</monocode_context>",
        mono.label(),
        files.soul.as_deref().unwrap_or_default(),
        memory::within_budget(&files.memory),
        files.topics.join(", ")
    )
}

/// A fresh provider session gets a bounded brief; the saved chat stays whole.
pub fn rotation_brief(rows: &[Row]) -> String {
    let mut exchanges: Vec<(String, String)> = Vec::new();
    for row in rows {
        if row.is_prompt() {
            if row.first || exchanges.is_empty() {
                exchanges.push((row.text.clone(), String::new()));
            } else if let Some(last) = exchanges.last_mut() {
                last.0.push_str(&row.text);
            }
        } else if row.role == "assistant" {
            if let Some(last) = exchanges.last_mut() {
                if row.first {
                    last.1 = row.text.clone();
                } else {
                    last.1.push_str(&row.text);
                }
            }
        }
    }
    let split = exchanges.len().saturating_sub(2);
    let clip = |text: &str, n: usize| text.chars().take(n).collect::<String>();
    let earlier = exchanges[..split]
        .iter()
        .rev()
        .take(10)
        .map(|(u, a)| {
            clip(
                &format!(
                    "User: {} → {}",
                    u.split_whitespace().collect::<Vec<_>>().join(" "),
                    a.split_whitespace().collect::<Vec<_>>().join(" ")
                ),
                280,
            )
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    let recent = exchanges[split..]
        .iter()
        .map(|(u, a)| format!("User: {}\nYou: {}", clip(u, 2000), clip(a, 3000)))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "Your provider context was rotated while preserving the full chat. Earlier exchanges:\n{earlier}\n\nRecent exchanges:\n{recent}"
    )
}
