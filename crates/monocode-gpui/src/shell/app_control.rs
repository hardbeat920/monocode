//! The `control.*` actions agents reach through the MonoCode MCP server: the
//! app's panels, any action the keymap can run, and other threads in the
//! caller's project. Shell actions go through the same handlers as a shortcut,
//! so an agent gets the same confirmation prompts a person would.
use super::*;
use crate::{right_panel::RightTab, settings::Section};
use app_views::AppView;
use monocode_core::{agent::AccessMode, app_control, files::path_key, mono::bridge::Call};
use serde_json::{Value, json};
use session_loading::Metadata;
use std::path::{Path, PathBuf};

mod settings;
#[cfg(test)]
mod tests;

const NOT_IN_PROJECT: &str = "Thread is not in this project.";
/// A conflict check reads whole transcripts, so it stays to the likeliest few.
const FILE_THREADS: usize = 10;
const FILES_PER_THREAD: usize = 40;
const RECENT_MS: u64 = 24 * 60 * 60 * 1000;

fn right_tab(name: &str) -> Option<RightTab> {
    Some(match name {
        "files" => RightTab::Files,
        "changes" => RightTab::Changes,
        "graph" => RightTab::Graph,
        "terminal" => RightTab::Terminal,
        _ => return None,
    })
}

fn settings_section(name: &str) -> Result<Section, String> {
    Ok(match name.to_lowercase().as_str() {
        "" | "general" => Section::General,
        "connections" => Section::Connections,
        "appearance" => Section::Appearance,
        "keybindings" => Section::Keybindings,
        "chat" => Section::Chat,
        "providers" => Section::Providers,
        "mcp" => Section::Mcp,
        "skills" => Section::Skills,
        "monos" => Section::Monos,
        "inbox" => Section::Inbox,
        "archive" => Section::Archive,
        "worktrees" => Section::Worktrees,
        _ => return Err(format!("Unknown settings section: {name}")),
    })
}

fn app_view(panel: &str) -> Option<AppView> {
    Some(match panel {
        "inbox" => AppView::Inbox,
        "notes" => AppView::Notes,
        "automations" => AppView::Automations,
        _ => return None,
    })
}

impl Shell {
    pub(super) fn handle_control_call(
        &mut self,
        call: Call,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = self.authorize_control_call(&call, cx) {
            let _ = call.reply.send(Err(error));
            return;
        }
        let result = match call.action.as_str() {
            "control.state" => Ok(self.control_state(cx)),
            "control.actions" => Ok(control_actions(&call.input, cx)),
            "control.threads" => Ok(self.control_threads(&call, cx)),
            "control.panel" => self
                .control_panel(&call.input, window, cx)
                .map(|()| self.control_state(cx)),
            "control.open" => self
                .control_thread_open(&call, window, cx)
                .map(|()| self.control_state(cx)),
            "control.close" => self
                .control_thread_close(&call, window, cx)
                .map(|()| self.control_state(cx)),
            "control.compose" | "control.composer" => self.control_compose(&call, cx),
            "control.settings" => Ok(self.control_settings()),
            "control.configure" => self.control_configure(&call.input["set"], cx),
            "control.set" => return self.control_thread_set(call, window, cx),
            "control.read" => return self.control_read(call, cx),
            "control.files" => return self.control_files(call, cx),
            "control.run" => return self.control_run(call, window, cx),
            _ => Err("Unknown app control action".into()),
        };
        let _ = call.reply.send(result);
    }

    fn authorize_control_call(&self, call: &Call, cx: &App) -> Result<(), String> {
        app_control::validate(&call.action, &call.input)?;
        let caller = self
            .sessions
            .get(&call.session)
            .map(|view| view.read(cx))
            .ok_or("This thread is not open in MonoCode.")?;
        let read_only = caller.app_read_only
            || caller.settings.plan
            || caller.settings.access == monocode_core::agent::AccessMode::ReadOnly;
        if read_only && !app_control::read_only(&call.action) {
            return Err("This thread's access policy allows only reading the app.".into());
        }
        Ok(())
    }

    fn control_state(&self, cx: &App) -> Value {
        let panel = self.right_panel.as_ref().map(|panel| panel.read(cx));
        json!({
            "project": self.workspace.project,
            "activeThread": self
                .workspace
                .active
                .map(|pane| self.workspace.session_for_pane(pane)),
            "panes": self
                .active_leaves()
                .iter()
                .map(|leaf| leaf.id)
                .filter(|id| self.session_summaries.contains_key(id))
                .collect::<Vec<_>>(),
            "sidebar": self.control_open("sidebar", cx),
            "right": self.workspace.right_panel_open,
            "rightTab": panel.map(|panel| format!("{:?}", panel.tab()).to_lowercase()),
            "terminal": self.control_open("terminal", cx),
            "settings": self.settings.is_some(),
            "view": self.active_app_view.map(AppView::label),
        })
    }

    fn control_open(&self, panel: &str, cx: &App) -> bool {
        match panel {
            "sidebar" if self.workspace.classic_layout => self.workspace.project_rail_open,
            "sidebar" => self.workspace.sidebar_visible,
            "right" => self.workspace.right_panel_open,
            "terminal" if self.terminal_in_panel(cx) => self.panel_terminal_shown(cx),
            "terminal" => self
                .workspace
                .docks
                .get(&self.workspace.project)
                .is_some_and(|dock| dock.open),
            "settings" => self.settings.is_some(),
            _ => app_view(panel).is_some_and(|view| self.active_app_view == Some(view)),
        }
    }

    fn control_panel(
        &mut self,
        input: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let panel = input["panel"].as_str().unwrap_or_default();
        let open = input["open"].as_bool().unwrap_or_default();
        let tab = input["tab"].as_str().and_then(right_tab);
        let section = input["section"].as_str().unwrap_or_default();
        if panel == "settings" && open {
            // Asking for a section moves settings that are already open.
            self.open_settings(settings_section(section)?, window, cx);
            return Ok(());
        }
        if let (true, "right", Some(tab)) = (open, panel, tab) {
            return self.control_right_tab(tab, window, cx);
        }
        if self.control_open(panel, cx) == open {
            return Ok(());
        }
        match (panel, app_view(panel)) {
            ("sidebar", _) => self.toggle_primary_sidebar(window, cx),
            ("right", _) => self.set_right_panel_open(open, window, cx),
            ("terminal", _) if self.terminal_in_title() => {
                return Err("Terminals open as title bar tabs here; use app_run.".into());
            }
            ("terminal", _) => self.toggle_terminal(window, cx),
            ("settings", _) => self.close_settings(window, cx),
            (_, Some(_)) if !open => self.close_app_view(window, cx),
            (_, Some(AppView::Inbox)) => self.open_inbox(window, cx),
            (_, Some(AppView::Notes)) => self.open_notes(window, cx),
            (_, Some(AppView::Automations)) => self.open_automations(window, cx),
            _ => return Err("Unknown panel".into()),
        }
        if self.control_open(panel, cx) == open {
            Ok(())
        } else {
            Err(format!(
                "The {panel} panel can't {} here.",
                if open { "open" } else { "close" }
            ))
        }
    }

    fn control_right_tab(
        &mut self,
        tab: RightTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.ensure_right_panel(window, cx);
        let panel = self.right_panel.clone().ok_or("No right panel here.")?;
        if !panel.read(cx).tabs().contains(&tab) {
            return Err("That tab is turned off in this project.".into());
        }
        match tab {
            RightTab::Terminal => self.show_panel_terminal(window, cx),
            _ => {
                panel.update(cx, |panel, cx| panel.select(tab, cx));
                if self.workspace.right_panel_open {
                    self.sync_right_panel(cx);
                } else {
                    self.set_right_panel_open(true, window, cx);
                }
            }
        }
        if self.workspace.right_panel_open {
            Ok(())
        } else {
            Err("The right panel can't open here.".into())
        }
    }

    /// Dispatches after the current update: a handler updates this shell, so
    /// running it here would re-enter the entity. The second defer lets an
    /// action dispatched through the window run before the state is read.
    fn control_run(&mut self, call: Call, window: &mut Window, cx: &mut Context<Self>) {
        let name = call.input["action"].as_str().unwrap_or_default().to_owned();
        let action = match cx.build_action(&name, None) {
            Ok(action) => action,
            Err(error) => {
                let _ = call.reply.send(Err(error.to_string()));
                return;
            }
        };
        let root = self.focus.clone();
        let owner = cx.entity().downgrade();
        let shell_level = name.starts_with("workspace::") || name.starts_with("application::");
        window.defer(cx, move |window, cx| {
            if shell_level {
                root.dispatch_action(action.as_ref(), window, cx);
            } else {
                window.dispatch_action(action, cx);
            }
            window.defer(cx, move |_, cx| {
                let state = owner
                    .update(cx, |shell, cx| shell.control_state(cx))
                    .map_err(|error| error.to_string());
                let _ = call.reply.send(state);
            });
        });
    }

    /// The threads an agent may read: its project's, minus Mono conversations.
    fn control_readable(&self, caller: i64, id: i64, cx: &App) -> bool {
        let project = |id: i64| {
            self.session_summaries
                .get(&id)
                .map(|summary| path_key(&summary.cwd))
        };
        project(id).is_some()
            && project(id) == project(caller)
            && !self
                .monos
                .as_ref()
                .is_some_and(|monos| monos.read(cx).hidden_session(id))
    }

    fn control_target(&self, call: &Call, cx: &App) -> Result<i64, String> {
        let id = call.input["threadId"].as_i64().unwrap_or_default();
        if self.control_readable(call.session, id, cx) {
            Ok(id)
        } else {
            Err(NOT_IN_PROJECT.into())
        }
    }

    /// Adds to the composer rather than replacing it, so an agent can't wipe
    /// what the person was writing.
    fn control_compose(&mut self, call: &Call, cx: &mut Context<Self>) -> Result<Value, String> {
        let id = self.control_target(call, cx)?;
        let view = self.ensure_session(id, cx).ok_or(NOT_IN_PROJECT)?;
        let current = view.read(cx).composer_text(cx);
        let Some(text) = call.input["text"].as_str() else {
            return Ok(json!({"text":current}));
        };
        let staged = if current.trim().is_empty() {
            text.to_owned()
        } else {
            format!("{current}\n\n{text}")
        };
        view.update(cx, |view, cx| view.set_composer_text(staged.clone(), cx));
        Ok(json!({"text":staged}))
    }

    /// Only the person can raise access, so that waits on their answer and
    /// nothing changes until they give it.
    fn control_thread_set(&mut self, call: Call, window: &mut Window, cx: &mut Context<Self>) {
        let checked = self.control_target(&call, cx).and_then(|id| {
            let view = self.ensure_session(id, cx).ok_or(NOT_IN_PROJECT)?;
            let text = |key: &str| call.input[key].as_str();
            view.read(cx)
                .check_agent_change(text("model"), text("effort"))?;
            Ok((id, view))
        });
        let (id, view) = match checked {
            Ok(checked) => checked,
            Err(error) => {
                let _ = call.reply.send(Err(error));
                return;
            }
        };
        let access = call.input["access"].as_str().map(AccessMode::from_id);
        if access.is_none_or(|access| access <= view.read(cx).settings.access) {
            let _ = call.reply.send(apply_thread_set(&view, &call.input, cx));
            return;
        }
        if self.access_prompted {
            let _ = call
                .reply
                .send(Err("Another access request is waiting for the user.".into()));
            return;
        }
        self.access_prompted = true;
        let title = self.session_metadata(id, cx).summary.title.clone();
        let asker = self
            .session_metadata(call.session, cx)
            .summary
            .title
            .clone();
        let label = access.unwrap_or_default().label();
        let answer = window.prompt(
            gpui::PromptLevel::Warning,
            &format!("Change \"{title}\" to {label}?"),
            Some(&format!("The agent in \"{asker}\" is asking for this.")),
            &["Allow", "Don't Allow"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let allowed = matches!(answer.await, Ok(0));
            let _ = this.update(cx, |this, cx| {
                this.access_prompted = false;
                let result = if allowed {
                    apply_thread_set(&view, &call.input, cx)
                } else {
                    Err("The user declined, so nothing changed.".into())
                };
                let _ = call.reply.send(result);
            });
        })
        .detach();
    }

    /// A subagent's pane belongs beside its lead, so it ignores placement.
    fn control_thread_open(
        &mut self,
        call: &Call,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let id = self.control_target(call, cx)?;
        let project = self.session_metadata(id, cx).summary.cwd.clone();
        let placement = self
            .app_placement(&call.input, call.session, &project, cx)?
            .filter(|_| self.subagent_lead(id).is_none());
        self.ensure_session(id, cx);
        self.app_place_launch(id, placement, true, window, cx);
        Ok(())
    }

    fn control_thread_close(
        &mut self,
        call: &Call,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let id = self.control_target(call, cx)?;
        if self.workspace.tab_of(id).is_some() {
            self.close_pane(id, window, cx);
        }
        if self.workspace.tab_of(id).is_some() {
            return Err("This is the app's only tab, so it stays open.".into());
        }
        Ok(())
    }

    /// The project's other threads, working ones first, then most recent.
    fn control_siblings<'a>(&'a self, caller: i64, cx: &'a App) -> Vec<Metadata<'a>> {
        let mut found: Vec<_> = self
            .session_summaries
            .keys()
            .copied()
            .filter(|id| *id != caller && self.control_readable(caller, *id, cx))
            .map(|id| self.session_metadata(id, cx))
            .collect();
        found.sort_by_key(|thread| {
            (
                !thread.busy,
                std::cmp::Reverse(thread.summary.updated_at),
                thread.summary.id,
            )
        });
        found
    }

    fn control_threads(&self, call: &Call, cx: &App) -> Value {
        let query = call.input["query"]
            .as_str()
            .unwrap_or_default()
            .to_lowercase();
        let archived = call.input["archived"].as_bool().unwrap_or_default();
        let limit = call.input["limit"]
            .as_u64()
            .unwrap_or(app_control::DEFAULT_THREADS) as usize;
        let found: Vec<_> = self
            .control_siblings(call.session, cx)
            .into_iter()
            .filter(|thread| {
                (archived || !thread.summary.archived)
                    && thread.summary.title.to_lowercase().contains(&query)
            })
            .collect();
        let total = found.len();
        json!({"total":total,"threads":found.iter().take(limit).map(|thread| json!({
            "threadId":thread.summary.id,
            "title":thread.summary.title,
            "harness":thread.summary.provider,
            "status":if thread.busy {"working"} else {"idle"},
            "archived":thread.summary.archived,
        })).collect::<Vec<_>>()})
    }

    fn control_read(&mut self, call: Call, cx: &mut Context<Self>) {
        let id = call.input["threadId"].as_i64().unwrap_or_default();
        if !self.control_readable(call.session, id, cx) {
            let _ = call.reply.send(Err(NOT_IN_PROJECT.into()));
            return;
        }
        let busy = self.session_metadata(id, cx).busy;
        let input = call.input.clone();
        let reader = call.session;
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            let store = locked(&store)?;
            store.note_thread_read(id, reader)?;
            store.app_conversation(id, &input, busy, false)
        });
        cx.spawn(async move |_, _| {
            let _ = call.reply.send(work.await);
        })
        .detach();
    }

    /// Which files the project's active threads changed, in the caller's terms:
    /// paths are relative to each thread's checkout, so worktrees line up.
    fn control_files(&mut self, call: Call, cx: &mut Context<Self>) {
        let own = PathBuf::from(self.session_working_copy(call.session, cx));
        let since = monocode_core::time::now_ms().saturating_sub(RECENT_MS);
        let active: Vec<_> = self
            .control_siblings(call.session, cx)
            .into_iter()
            .filter(|thread| {
                !thread.summary.archived && (thread.busy || thread.summary.updated_at >= since)
            })
            .collect();
        let unchecked = active.len().saturating_sub(FILE_THREADS);
        let threads: Vec<_> = active
            .into_iter()
            .take(FILE_THREADS)
            .map(|thread| {
                let id = thread.summary.id;
                let root = PathBuf::from(self.session_working_copy(id, cx));
                (id, thread.summary.title.clone(), thread.busy, root)
            })
            .collect();
        let wanted: Vec<_> = call.input["paths"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|path| checkout_path(path, &own))
            .collect();
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            let mut found = Vec::new();
            for (id, title, busy, root) in threads {
                let rows = locked(&store)?.load(id)?.rows;
                let edited = crate::runtime::edited_paths(&rows);
                let files = changed_files(&edited, &root, &wanted);
                if files.is_empty() {
                    continue;
                }
                found.push(json!({
                    "threadId":id,
                    "title":title,
                    "status":if busy {"working"} else {"idle"},
                    "sameCheckout":monocode_core::files::path_key(&root.to_string_lossy())
                        == monocode_core::files::path_key(&own.to_string_lossy()),
                    "moreFiles":files.len().saturating_sub(FILES_PER_THREAD),
                    "files":files.iter().take(FILES_PER_THREAD).collect::<Vec<_>>(),
                }));
            }
            Ok(json!({"threads":found,"unchecked":unchecked}))
        });
        cx.spawn(async move |_, _| {
            let _ = call.reply.send(work.await);
        })
        .detach();
    }
}

fn apply_thread_set(
    view: &Entity<Conversation>,
    input: &Value,
    cx: &mut Context<Shell>,
) -> Result<Value, String> {
    let text = |key: &str| input[key].as_str();
    let access = text("access").map(AccessMode::from_id);
    view.update(cx, |view, cx| {
        view.apply_agent_change(text("model"), text("effort"), access, cx)?;
        Ok(json!({
            "threadId":view.summary.id,
            "model":view.mobile_model(),
            "effort":view.settings.effort,
            "access":view.settings.access.id(),
        }))
    })
}

/// A path as it sits in its checkout; one outside it stays absolute.
pub(super) fn checkout_path(path: &str, root: &Path) -> PathBuf {
    let full = root.join(path);
    full.strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or(full)
}

/// A thread's edits, latest first, within `wanted` when that names any.
/// Asking about a directory covers the files under it.
fn changed_files(edited: &[String], root: &Path, wanted: &[PathBuf]) -> Vec<String> {
    edited
        .iter()
        .rev()
        .map(|path| checkout_path(path, root))
        .filter(|path| wanted.is_empty() || wanted.iter().any(|want| path.starts_with(want)))
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// Actions that carry data exist only for clicks to send, so app_run can't
/// build them.
fn control_actions(input: &Value, cx: &App) -> Value {
    let filter = input["filter"].as_str().unwrap_or_default().to_lowercase();
    let mut actions: Vec<_> = cx
        .all_action_names()
        .iter()
        .copied()
        .filter(|name| cx.build_action(name, None).is_ok())
        .filter(|name| {
            if filter.is_empty() {
                name.starts_with("workspace::") || name.starts_with("application::")
            } else {
                name.to_lowercase().contains(&filter)
            }
        })
        .collect();
    actions.sort_unstable();
    json!({"actions":actions})
}
