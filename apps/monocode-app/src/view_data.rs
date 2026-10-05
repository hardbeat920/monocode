//! What the shell regions draw, collected from the engine on each render:
//! the rail's projects, the sidebar's session cards, and the title bar's
//! tabs. These are plain structs so each region renders from one value, the
//! way the React shell rendered from props.

use std::collections::{HashMap, HashSet};

use gpui::{App, Entity};
use monocode_core::session::session_display_title;
use monocode_core::{HarnessId, Session};
use monocode_engine::attention::Attention;
use monocode_engine::runtime::Engine;
use monocode_engine::runtime::util::project_path::same_project_path;
use monocode_engine::workspace::Workspace;
use monocode_engine::workspace::title_tab::TitleTab;
use monocode_ui::ProviderLogo;

use monocode_app::boot::AppServices;
use monocode_app::history::SidebarHistory;
use monocode_app::projects::RailProject;

use crate::format::{self, NO_BRANCH_LABEL, format_git_label};

#[derive(Clone, Debug)]
pub struct Project {
    pub name: String,
    pub path: String,
    pub additions: i64,
    pub deletions: i64,
    pub busy: bool,
    /// The tint (`resolveTabGroupColor`), as `0xrrggbb`.
    pub color: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionStatus {
    Idle,
    Busy,
    Done,
    NeedsApproval,
    Draft,
}

/// One `SessionCard`.
#[derive(Clone, Debug)]
pub struct SessionCard {
    pub id: String,
    pub provider: ProviderLogo,
    pub model: String,
    pub title: String,
    pub git: String,
    pub additions: i64,
    pub deletions: i64,
    pub updated_at: i64,
    pub status: SessionStatus,
    pub pinned: bool,
}

/// A harness icon in a tab, with its turn state (`TabHarnesses`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarnessState {
    Idle,
    Busy,
    Done,
}

#[derive(Clone, Debug)]
pub enum TabLead {
    Harnesses(Vec<(ProviderLogo, HarnessState)>),
    File(String),
    Terminal,
}

/// One title bar tab, after `tabCopy`.
#[derive(Clone, Debug)]
pub struct TitleTabView {
    pub id: String,
    pub lead: TabLead,
    pub headline: String,
    pub meta: Option<String>,
    pub tooltip: String,
    pub dirty: bool,
    pub preview: bool,
}

/// A provider usage window for the footer.
#[derive(Clone, Debug)]
pub struct UsageWindow {
    pub used_percent: f32,
    pub label: String,
}

#[derive(Clone, Debug)]
pub struct UsageChip {
    pub provider: ProviderLogo,
    pub windows: Vec<UsageWindow>,
}

#[derive(Clone, Debug, Default)]
pub struct ShellData {
    pub projects: Vec<Project>,
    pub active_project: Option<usize>,
    pub sessions: Vec<SessionCard>,
    pub sessions_loading: bool,
    pub tabs: Vec<TitleTabView>,
    pub active_tab_id: String,
    pub active_session_id: Option<String>,
    pub usage: Vec<UsageChip>,
    pub inbox_unseen: bool,
}

fn parse_color(value: &str) -> u32 {
    u32::from_str_radix(value.trim_start_matches('#'), 16).unwrap_or(0x7dd3fc)
}

/// `sessionMeta` in TitleBar.tsx.
fn session_meta(tab: &TitleTab) -> String {
    if tab.more.len() == 1 {
        return tab.more[0].clone();
    }
    if tab.session_count > 1 {
        return format!("{} sessions", tab.session_count);
    }
    String::new()
}

/// `tabCopy` in TitleBar.tsx: the headline, meta line, and tooltip.
pub fn tab_copy(tab: &TitleTab) -> (String, String, String) {
    let project = match tab.project.trim() {
        "" => "~",
        project => project,
    };
    let conversation = tab.title.trim().to_string();
    let file = tab.files.first().cloned().unwrap_or_default();
    let sessions = session_meta(tab);
    let untitled = "New session".to_string();
    let mut meta: Vec<String> = Vec::new();
    let headline = if tab.multi_pane {
        if tab.file_focused && !file.is_empty() {
            if !conversation.is_empty() {
                meta.push(conversation.clone());
            } else if !sessions.is_empty() {
                meta.push(sessions.clone());
            }
            file.clone()
        } else if !conversation.is_empty() {
            if !file.is_empty() {
                meta.push(file.clone());
            } else if !sessions.is_empty() {
                meta.push(sessions.clone());
            }
            conversation.clone()
        } else if !file.is_empty() {
            if !sessions.is_empty() {
                meta.push(sessions.clone());
            }
            file.clone()
        } else {
            if !sessions.is_empty() {
                meta.push(sessions.clone());
            }
            untitled
        }
    } else {
        if !sessions.is_empty() {
            meta.push(sessions.clone());
        }
        [conversation.clone(), file.clone(), untitled]
            .into_iter()
            .find(|text| !text.is_empty())
            .unwrap_or_default()
    };
    let mut tooltip = vec![project.to_string()];
    if !conversation.is_empty() {
        tooltip.push(conversation);
    }
    tooltip.extend(tab.more.iter().cloned());
    if !tab.files.is_empty() {
        tooltip.push(tab.files.join(", "));
    }
    if tab.dirty {
        tooltip.push("Unsaved changes".into());
    }
    (headline, meta.join(" · "), tooltip.join(" · "))
}

fn title_tab_view(tab: &TitleTab) -> TitleTabView {
    let (headline, meta, tooltip) = tab_copy(tab);
    let lead = if !tab.harnesses.is_empty() {
        TabLead::Harnesses(
            tab.harnesses
                .iter()
                .take(3)
                .map(|harness| {
                    let state = if tab.busy_harnesses.contains(harness) {
                        HarnessState::Busy
                    } else if tab.done_harnesses.contains(harness) {
                        HarnessState::Done
                    } else {
                        HarnessState::Idle
                    };
                    (format::provider_logo(*harness), state)
                })
                .collect(),
        )
    } else if tab.terminal || tab.files.is_empty() {
        TabLead::Terminal
    } else {
        TabLead::File(tab.files[0].clone())
    };
    TitleTabView {
        id: tab.id.clone(),
        lead,
        headline,
        meta: (!meta.is_empty()).then_some(meta),
        tooltip,
        dirty: tab.dirty,
        preview: tab.preview_file_id.is_some(),
    }
}

/// The model's display name (`resolveModel(harness, model).name`).
fn model_name(harness: HarnessId, model: &str, cx: &App) -> String {
    AppServices::try_global(cx)
        .map(|services| {
            services
                .catalog
                .read()
                .resolve_model(harness, Some(model))
                .name
        })
        .unwrap_or_else(|| model.to_string())
}

impl ShellData {
    /// Read the engine for one frame of the shell.
    pub fn collect(
        workspace: Option<&Entity<Workspace>>,
        history: Option<&Entity<SidebarHistory>>,
        projects: &[RailProject],
        stats: &HashMap<String, (i64, i64)>,
        cx: &App,
    ) -> Self {
        let mut data = ShellData::default();
        let Some(engine) = Engine::try_global(cx) else {
            return data;
        };
        let sessions = engine.sessions.read(cx);
        let open: &[Session] = sessions.all();
        let busy: &HashSet<String> = sessions.busy_session_ids();
        let (approvals, unseen) = Attention::try_global(cx)
            .map(|attention| {
                (
                    attention.approvals.read(cx).approval_session_ids().clone(),
                    attention.notifier.read(cx).unseen_finished_ids().clone(),
                )
            })
            .unwrap_or_default();

        let sidebar_cwd = workspace.map(|workspace| workspace.read(cx).sidebar_cwd(cx));
        data.projects = projects
            .iter()
            .map(|project| {
                let (additions, deletions) = stats.get(&project.path).copied().unwrap_or_default();
                Project {
                    name: project.name.clone(),
                    path: project.path.clone(),
                    additions,
                    deletions,
                    busy: open.iter().any(|session| {
                        session.is_busy() && same_project_path(&session.cwd, &project.path)
                    }),
                    color: parse_color(&project.color),
                }
            })
            .collect();
        data.active_project = sidebar_cwd.as_deref().and_then(|cwd| {
            data.projects
                .iter()
                .position(|project| same_project_path(&project.path, cwd))
        });

        if let Some(workspace) = workspace {
            let workspace = workspace.read(cx);
            data.tabs = workspace
                .title_tabs(&unseen, cx)
                .iter()
                .map(title_tab_view)
                .collect();
            data.active_tab_id = workspace.active_tab_id().to_string();
            data.active_session_id = workspace.active_session(cx).map(|session| session.id);
        }

        if let Some(history) = history {
            let history = history.read(cx);
            data.sessions_loading = history.is_loading();
            data.sessions = history
                .visible_rows(cx)
                .into_iter()
                .map(|row| {
                    let status = if approvals.contains(&row.id) {
                        SessionStatus::NeedsApproval
                    } else if busy.contains(&row.id) {
                        SessionStatus::Busy
                    } else if unseen.contains(&row.id) {
                        SessionStatus::Done
                    } else if row.draft == Some(true) {
                        SessionStatus::Draft
                    } else {
                        SessionStatus::Idle
                    };
                    // An open session shows its live title and model.
                    let live = open.iter().find(|session| session.id == row.id);
                    let title = live.map(|s| s.title.as_str()).unwrap_or(&row.title);
                    let model = live.map(|s| s.model.as_str()).unwrap_or(&row.model);
                    SessionCard {
                        provider: format::provider_logo(row.harness),
                        model: model_name(row.harness, model, cx),
                        title: session_display_title(title, row.harness),
                        git: if row.worktree_removed == Some(true) {
                            NO_BRANCH_LABEL.to_string()
                        } else {
                            format_git_label(row.repo.as_deref(), row.branch.as_deref())
                        },
                        additions: row.additions.unwrap_or(0),
                        deletions: row.deletions.unwrap_or(0),
                        updated_at: row.updated_at,
                        status,
                        pinned: row.pinned == Some(true),
                        id: row.id,
                    }
                })
                .collect();
        }
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(title: &str, files: &[&str], multi: bool, file_focused: bool) -> TitleTab {
        TitleTab {
            id: "t".into(),
            project: "repo".into(),
            title: title.into(),
            files: files.iter().map(|file| file.to_string()).collect(),
            multi_pane: multi,
            file_focused,
            session_count: 1,
            ..Default::default()
        }
    }

    #[test]
    fn tab_copy_follows_the_title_bar() {
        assert_eq!(tab_copy(&tab("", &[], false, false)).0, "New session");
        assert_eq!(tab_copy(&tab("Fix it", &[], false, false)).0, "Fix it");
        let split = tab_copy(&tab("Fix it", &["a.rs"], true, true));
        assert_eq!((split.0.as_str(), split.1.as_str()), ("a.rs", "Fix it"));
        let split = tab_copy(&tab("Fix it", &["a.rs"], true, false));
        assert_eq!((split.0.as_str(), split.1.as_str()), ("Fix it", "a.rs"));
        assert_eq!(
            tab_copy(&tab("Fix it", &[], false, false)).2,
            "repo · Fix it"
        );
    }
}
