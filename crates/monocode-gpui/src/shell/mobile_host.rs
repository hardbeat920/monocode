//! A paired phone browses and drives this workspace. The core host pairs and
//! serves devices; the shell answers their requests from its own state, through
//! the same paths as the sidebar, composer and Changes panel.
use super::*;
use crate::{
    accounts::Tone,
    mobile::{catalog as mobile_catalog, project, transcript as turns},
    notifications::delivery,
    pickers,
};
use gpui::{EntityId, Task};
use monocode_core::{
    agent::{AccessMode, Settings, TurnInfo},
    mobile::{Event, Invite, MobileHost, Push, Request, Status, WatchStart, notify, watch},
    transcript::Row,
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

/// How often the workspace a phone polls is refreshed while access is on.
const PUBLISH_INTERVAL: Duration = Duration::from_secs(3);
/// At most this often while a reply streams: about 20 frames a second.
const PUSH_INTERVAL: Duration = Duration::from_millis(50);
/// Turns a conversation opens on when the phone does not say.
const DEFAULT_TAIL: usize = 3;
const MAX_PAGE: usize = 100;
const READ_TURNS: usize = 30;
/// Under the host's 25 second wait; the phone asks again with the same ID.
const GIT_ACTION_WAIT: Duration = Duration::from_secs(20);
const MAX_RECEIPTS: usize = 1000;
const MAX_GIT_JOBS: usize = 100;
const MAX_ATTACHMENTS: usize = 10;
const MAX_MESSAGE: usize = 100_000;

#[derive(Default)]
pub(super) struct Mobile {
    host: Option<MobileHost>,
    /// The visible pairing code, until it expires, is used or is closed.
    pub(super) invite: Option<Invite>,
    /// Commands by device-scoped ID, so a phone's retry never runs one twice.
    receipts: HashMap<String, Receipt>,
    receipt_order: VecDeque<String>,
    /// Git actions can outlast a request; the phone asks again until done.
    git_jobs: HashMap<String, GitJob>,
    git_order: VecDeque<String>,
    watches: HashMap<String, Watch>,
    push_scheduled: bool,
    publishing: bool,
    /// Persisted session IDs, which stay stable for paired phones.
    ids: HashMap<i64, String>,
    /// Sessions whose current turn a phone started: their approvals,
    /// questions and finish are sent to it.
    started: HashSet<i64>,
    /// The last message of each Mono conversation not open here, by its
    /// session, with the update time it was read at: read again only once
    /// the conversation has changed.
    mono_previews: HashMap<i64, MonoPreview>,
}

#[derive(Clone)]
struct MonoPreview {
    updated_at: u64,
    text: Value,
}

struct CatalogEntry {
    id: i64,
    project: String,
    summary: Session,
    live: Option<mobile_catalog::LiveSession>,
}

enum Receipt {
    Pending(String),
    Done(String, Result<Value, String>),
}
struct GitJob {
    signature: String,
    result: Arc<Mutex<Option<Result<Value, String>>>>,
}

#[derive(Clone, PartialEq)]
enum Target {
    Session(i64),
    Mono(String),
}

/// A conversation read from disk, reused until the session opens on desktop.
struct Saved {
    rows: Vec<Row>,
    turns: HashMap<usize, TurnInfo>,
    harness: String,
    model: Option<String>,
    cwd: String,
    /// The store cursor for turns above `rows`: a long Mono loads only its
    /// newest page.
    history: Option<i64>,
    /// The Mono's name, so its saved turns are drawn as Mono turns, as they
    /// are once its chat opens, and keep their heights when it does.
    mono: Option<String>,
    /// The cards of the sessions the Mono started, as they stood on reading.
    launches: HashMap<i64, Value>,
}

/// How an older page's turns are drawn, as the conversation's own are.
struct PageLook {
    mono: Option<String>,
    launches: HashMap<i64, Value>,
    cwd: String,
    harness: String,
    model: Option<String>,
}

/// A read's answer, or the store page it still needs.
enum Read {
    Done(Value),
    /// The turns before the loaded rows, from the page at this cursor.
    Page(i64),
}

struct Watch {
    target: Target,
    tail: usize,
    /// The first turn the watch carries; the window grows from here.
    from: Option<String>,
    incremental: bool,
    baseline: Option<watch::Baseline>,
    saved: Option<Arc<Saved>>,
    loading: bool,
    observed: Option<EntityId>,
    _observers: Vec<Subscription>,
}

/// What a session's turns are built from, live or saved.
struct Snapshot<'a> {
    rows: &'a [Row],
    turns: &'a HashMap<usize, TurnInfo>,
    cwd: &'a str,
    harness: &'a str,
    model: Option<String>,
    busy: bool,
    waiting: Option<&'static str>,
    mono: Option<String>,
    pending: Vec<turns::Pending>,
    /// A Mono's launched sessions, as the phone's cards show them.
    launches: &'a HashMap<i64, Value>,
    /// The question the agent waits on, as the phone's card shows it; null
    /// for none, as always for a saved conversation.
    question: Value,
    /// The store cursor for turns above `rows`, which the phone pages in
    /// through `conversation.older`.
    history: Option<i64>,
}
impl Snapshot<'_> {
    fn source(&self) -> turns::Source<'_> {
        turns::Source {
            rows: self.rows,
            turns: self.turns,
            cwd: self.cwd,
            harness: self.harness,
            model_name: self.model.as_deref().map(|model| {
                pickers::cached_model_name(Kind::from_id(self.harness), Some(model)).to_string()
            }),
            busy: self.busy,
            waiting: self.waiting,
            mono: self.mono.as_deref(),
            pending: &self.pending,
            launches: self.launches,
        }
    }
}

/// `encodeURIComponent`, as paired phones already know projects by it.
/// A visual document's component tree, as its first render draws it.
fn visual_tree(body: &str) -> Result<Value, String> {
    use monocode_core::visual::{VisualDocument, VisualRuntime};
    let visual: VisualDocument = serde_json::from_str(body).map_err(|e| e.to_string())?;
    visual.validate()?;
    let tree = VisualRuntime::new(&visual.source)?.render()?;
    serde_json::to_value(tree).map_err(|e| e.to_string())
}

fn project_id(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
fn hex(color: u32) -> String {
    format!("#{:06x}", color & 0xff_ffff)
}
fn string(params: &Value, key: &str) -> Option<String> {
    params[key].as_str().map(str::to_owned)
}

impl Shell {
    pub(super) fn init_mobile_host(&mut self, cx: &mut Context<Self>) {
        // GPUI's deterministic executor cannot be woken by the link's threads.
        if cx.is_test() {
            return;
        }
        let Some(path) = locked(&self.store)
            .ok()
            .and_then(|store| store.data_dir().map(|dir| dir.join("mobile-access.json")))
        else {
            return;
        };
        let (host, events) = match MobileHost::open(path) {
            Ok(opened) => opened,
            Err(error) => {
                self.error = Some(format!("Mobile access: {error}"));
                return;
            }
        };
        self.mobile.host = Some(host);
        // Close the link and release the router mapping with the app.
        cx.on_app_quit(|this, _| {
            this.shutdown_mobile_host();
            async {}
        })
        .detach();
        cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                if this
                    .update_in(cx, |this, window, cx| {
                        this.handle_mobile_event(event, window, cx)
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                if this
                    .update(cx, |this, cx| this.publish_mobile_catalog(cx))
                    .is_err()
                {
                    break;
                }
                cx.background_executor().timer(PUBLISH_INTERVAL).await;
            }
        })
        .detach();
    }

    pub(super) fn shutdown_mobile_host(&mut self) {
        if let Some(host) = &self.mobile.host {
            host.shutdown();
        }
    }

    pub(super) fn mobile_status(&self) -> Option<Status> {
        self.mobile.host.as_ref().map(MobileHost::status)
    }

    pub(super) fn mobile_configure(
        &mut self,
        enabled: bool,
        open_router: bool,
        remote_address: String,
        cx: &mut Context<Self>,
    ) {
        let Some(host) = self.mobile.host.clone() else {
            return;
        };
        if !enabled {
            self.mobile.invite = None;
            self.mobile.watches.clear();
        }
        // Binding the port and reading the keyring can block.
        let work = cx
            .background_executor()
            .spawn(async move { host.configure(enabled, open_router, &remote_address) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.error = Some(format!("Mobile access: {error}"));
                }
                this.publish_mobile_catalog(cx);
                this.sync_settings(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn mobile_invite(&mut self, cx: &mut Context<Self>) {
        let Some(host) = self.mobile.host.clone() else {
            return;
        };
        let work = cx.background_executor().spawn(async move { host.invite() });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(invite) => this.mobile.invite = Some(invite),
                    Err(error) => this.error = Some(format!("Mobile access: {error}")),
                }
                this.sync_settings(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn mobile_cancel_invite(&mut self, cx: &mut Context<Self>) {
        if let Some(host) = &self.mobile.host {
            host.cancel_invite();
        }
        self.mobile.invite = None;
        self.sync_settings(cx);
    }

    pub(super) fn mobile_revoke(&mut self, device_id: &str, cx: &mut Context<Self>) {
        if let Some(host) = &self.mobile.host
            && let Err(error) = host.revoke(device_id)
        {
            self.error = Some(format!("Mobile access: {error}"));
        }
        self.sync_settings(cx);
    }

    fn handle_mobile_event(&mut self, event: Event, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            Event::Request(request) => self.handle_mobile_request(request, window, cx),
            Event::Watch(start) => self.start_mobile_watch(start, cx),
            Event::WatchStop(id) => {
                self.mobile.watches.remove(&id);
            }
        }
    }

    // MARK: catalog

    /// Projects on the rail and those Monos work in.
    fn mobile_projects(&self, cx: &App) -> Vec<String> {
        let mut paths: Vec<String> = Vec::new();
        let monos = self
            .monos
            .as_ref()
            .map(|m| {
                m.read(cx)
                    .roster
                    .monos
                    .iter()
                    .flat_map(|mono| mono.projects.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for path in self.workspace.projects.iter().cloned().chain(monos) {
            if !path.is_empty()
                && path != "~"
                && !paths.iter().any(|p| {
                    monocode_core::files::path_key(p) == monocode_core::files::path_key(&path)
                })
            {
                paths.push(path);
            }
        }
        paths
    }

    fn mobile_project_path(&self, id: &str, cx: &App) -> Result<String, String> {
        self.mobile_projects(cx)
            .into_iter()
            .find(|path| project_id(path) == id)
            .ok_or_else(|| "Project is no longer available".into())
    }

    /// What a new session in `cwd` starts with, as a new desktop tab would.
    fn mobile_defaults(&self, cwd: &str, cx: &App) -> (Kind, Option<String>, AccessMode) {
        let prefs = &self.workspace.preferences;
        let project = prefs.project_providers.get(cwd);
        let kind = project
            .and_then(|p| p.provider.as_deref())
            .or(prefs.default_provider.as_deref())
            .and_then(Kind::parse)
            .unwrap_or(Kind::Codex);
        let model = project
            .and_then(|p| p.models.get(kind.id()))
            .or(prefs.default_models.get(kind.id()))
            .and_then(|model| monocode_core::store::native_model(model, kind.id()));
        (kind, model, crate::conversation::default_access(cx))
    }

    fn mobile_catalog_sessions(&self, paths: &[String], cx: &App) -> Vec<CatalogEntry> {
        self.order
            .iter()
            .filter(|id| !self.sidebar_hidden_sessions.contains(id))
            .filter_map(|&id| {
                let metadata = self.session_metadata(id, cx);
                let summary = metadata.summary;
                if summary.archived
                    || !summary.has_messages
                    || self.orchestration_worker(id)
                    || self
                        .monos
                        .as_ref()
                        .is_some_and(|m| m.read(cx).hidden_session(id))
                {
                    return None;
                }
                let key = monocode_core::files::path_key(&summary.cwd);
                let project = paths
                    .iter()
                    .find(|path| monocode_core::files::path_key(path) == key)?;
                let live = self.sessions.get(&id).map(|view| {
                    let view = view.read(cx);
                    mobile_catalog::LiveSession {
                        loaded: view.history_loaded(),
                        metadata: monocode_core::store::MobileSessionMetadata {
                            model: view.mobile_model().map(str::to_owned),
                            branch: view.branch_name().map(str::to_owned),
                            worktree_cwd: view.settings.worktree_cwd.clone(),
                            worktree_removed: view.settings.worktree_removed,
                            access: view.settings.access,
                        },
                        busy: view.busy,
                        waiting: match view.mobile_waiting(cx) {
                            Some(crate::conversation::MobileWaiting::Approval) => Some("approval"),
                            Some(crate::conversation::MobileWaiting::Question) => Some("question"),
                            None => None,
                        },
                    }
                });
                Some(CatalogEntry {
                    id,
                    project: project_id(project),
                    summary: summary.clone(),
                    live,
                })
            })
            .collect()
    }

    fn publish_mobile_catalog(&mut self, cx: &mut Context<Self>) {
        let Some(host) = self.mobile.host.clone() else {
            return;
        };
        if self.mobile.publishing || !host.enabled() {
            return;
        }
        let paths = self.mobile_projects(cx);
        let projects: Vec<Value> = paths
            .iter()
            .map(|path| {
                let appearance = self.appearance(path);
                let (kind, model, access) = self.mobile_defaults(path, cx);
                let mut project = json!({
                    "id": project_id(path),
                    "path": path,
                    "name": appearance.label(path),
                    "machine": "Desktop",
                    "branch": "",
                    "color": hex(appearance.color(path)),
                    "additions": 0,
                    "deletions": 0,
                    "defaultHarness": kind.id(),
                    "defaultModelName": pickers::cached_model_name(kind, model.as_deref()).to_string(),
                    "defaultRuntimeMode": access.id(),
                });
                if let Some(model) = model {
                    project["defaultModel"] = json!(mobile_catalog::model_id(kind, Some(&model)));
                }
                if let Some(mascot) = appearance.mascot {
                    project["mascot"] = json!(mascot);
                }
                project
            })
            .collect();
        let entries = self.mobile_catalog_sessions(&paths, cx);
        let monos: Vec<(monocode_core::mono::Mono, Option<Value>)> = self
            .monos
            .as_ref()
            .map(|m| {
                let manager = m.read(cx);
                manager
                    .roster
                    .monos
                    .iter()
                    .map(|mono| {
                        let live = manager.open_chat(&mono.id).map(|chat| {
                            let chat = chat.read(cx);
                            let rows = chat.transcript.read(cx).source_rows();
                            let last = turns::messages(rows).pop();
                            json!({
                                "status": chat.mono_status(),
                                "busy": chat.busy,
                                "harness": chat.summary.provider,
                                "model": mobile_catalog::model_id(Kind::from_id(&chat.summary.provider), chat.mobile_model()),
                                "lastMessage": last.map(|m| m["text"].clone()).unwrap_or(json!("")),
                                "lastActiveAt": chat.summary.updated_at,
                            })
                        });
                        (mono.clone(), live)
                    })
                    .collect()
            })
            .unwrap_or_default();
        // A Mono's conversation not open here still shows its last message
        // on the phone, read from its saved history.
        let saved_monos: Vec<(i64, Option<u64>)> = monos
            .iter()
            .filter(|(_, live)| live.is_none())
            .filter_map(|(mono, _)| mono.session_id)
            .map(|id| (id, self.session_summaries.get(&id).map(|s| s.updated_at)))
            .collect();
        let mut previews = self.mobile.mono_previews.clone();
        let store = self.store.clone();
        let mut ids = self.mobile.ids.clone();
        self.mobile.publishing = true;
        let work = cx.background_executor().spawn(async move {
            let saved = {
                let store = locked(&store)?;
                for id in entries.iter().map(|entry| entry.id).chain(monos.iter().filter_map(|(mono, _)| mono.session_id)) {
                    if let std::collections::hash_map::Entry::Vacant(entry) = ids.entry(id)
                        && let Ok(value) = store.persisted_session_id(id)
                    {
                        entry.insert(value);
                    }
                }
                for (id, updated_at) in &saved_monos {
                    let fresh = previews
                        .get(id)
                        .is_some_and(|p| updated_at.is_some_and(|at| at == p.updated_at));
                    if fresh {
                        continue;
                    }
                    let Ok(history) = store.load(*id) else {
                        continue;
                    };
                    let text = turns::messages(&history.rows)
                        .pop()
                        .map(|m| m["text"].clone())
                        .unwrap_or(json!(""));
                    previews.insert(*id, MonoPreview { updated_at: updated_at.unwrap_or(0), text });
                }
                previews.retain(|id, _| saved_monos.iter().any(|(saved, _)| saved == id));
                store.mobile_session_metadata()?
            };
            let mut branches = HashMap::new();
            let projects: Vec<_> = projects.into_iter().map(|mut project| {
                let cwd = project["path"].as_str().unwrap_or_default().to_owned();
                project["branch"] = json!(mobile_catalog::checkout_branch(&cwd, &mut branches));
                project
            }).collect();
            let mut sessions = Vec::new();
            for entry in entries {
                let Some(id) = ids.get(&entry.id) else {
                    continue;
                };
                let metadata = mobile_catalog::metadata(saved.get(&entry.id).unwrap_or(&Default::default()), entry.live.as_ref());
                let branch = if !metadata.worktree_removed && metadata.branch.as_ref().is_none_or(|branch| branch.is_empty()) {
                    mobile_catalog::checkout_branch(metadata.worktree_cwd.as_deref().unwrap_or(&entry.summary.cwd), &mut branches)
                } else { String::new() };
                sessions.push(mobile_catalog::session(&entry.summary, id, &entry.project, &metadata, entry.live.as_ref(), &branch));
            }
            sessions.sort_by_key(|s| std::cmp::Reverse(s["updatedAt"].as_u64()));
            let monos: Vec<Value> = monos
                .into_iter()
                .map(|(mono, live)| {
                    let mut value = json!({
                        "id": mono.id,
                        "name": mono.label(),
                        "mascot": mono.mascot,
                        "color": hex(mono.color),
                        "projectIds": mono.projects.iter().map(|p| project_id(p)).collect::<Vec<_>>(),
                        "unread": 0,
                        "status": "idle",
                        "busy": false,
                        "live": live.is_some(),
                        "lastMessage": "",
                        "lastActiveAt": 0,
                    });
                    if let Some(session) = mono.session_id.and_then(|id| ids.get(&id)) {
                        value["sessionId"] = json!(session);
                    }
                    if let Some(preview) = mono.session_id.and_then(|id| previews.get(&id)) {
                        value["lastMessage"] = preview.text.clone();
                        value["lastActiveAt"] = json!(preview.updated_at);
                    }
                    if let Some(live) = live {
                        for (key, field) in live.as_object().into_iter().flatten() {
                            if !field.is_null() {
                                value[key] = field.clone();
                            }
                        }
                    }
                    value
                })
                .collect();
            Ok::<_, String>((
                // `monoTools`: this desktop answers the phone's Mono profile,
                // editing, reset, delete and create; the older app doesn't,
                // and its phones hide them. `push`: it notifies phones through
                // Expo, so they no longer post their own alerts. `monoFiles`:
                // it reads and saves a Mono's soul and memory for the phone.
                json!({"projects": projects, "monos": monos, "sessions": sessions, "monoTools": true, "push": true, "monoFiles": true}),
                ids,
                previews,
            ))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, _| {
                this.mobile.publishing = false;
                if let Ok((catalog, ids, previews)) = result {
                    this.mobile.ids = ids;
                    this.mobile.mono_previews = previews;
                    let _ = host.publish(catalog);
                }
            });
        })
        .detach();
    }

    // MARK: requests

    fn mobile_target(&self, id: &str, cx: &App) -> Result<Target, String> {
        if let Some(mono) = id.strip_prefix("mono:") {
            return self
                .monos
                .as_ref()
                .is_some_and(|m| m.read(cx).roster.get(mono).is_some())
                .then(|| Target::Mono(mono.to_owned()))
                .ok_or_else(|| "Mono is no longer available".into());
        }
        let session = self
            .mobile
            .ids
            .iter()
            .find(|(_, persisted)| *persisted == id)
            .map(|(id, _)| *id)
            .or_else(|| locked(&self.store).ok()?.resolve_existing_session_id(id))
            .filter(|id| self.session_summaries.contains_key(id))
            .ok_or("Session is no longer available")?;
        Ok(Target::Session(session))
    }

    /// The sessions a Mono started, as the cards in its chat show them on the
    /// phone: the phone's IDs for each and the project it runs in, its title
    /// and how it stands. None for a session's own conversation.
    fn mobile_launches(&self, target: &Target, cx: &App) -> HashMap<i64, Value> {
        let Target::Mono(mono) = target else {
            return HashMap::new();
        };
        let Some(mono) = self
            .monos
            .as_ref()
            .and_then(|m| m.read(cx).roster.get(mono).cloned())
        else {
            return HashMap::new();
        };
        let projects = self.mobile_projects(cx);
        crate::monos::cards::launch_views(&mono, self, cx)
            .into_iter()
            .map(|(id, view)| {
                // A worktree's session belongs to the project it was made from.
                let cwd = view.cwd.as_str();
                let project = projects
                    .iter()
                    .find(|path| {
                        monocode_core::files::path_key(path) == monocode_core::files::path_key(cwd)
                    })
                    .or_else(|| {
                        mono.projects.iter().find(|path| {
                            projects.contains(path) && cwd.starts_with(path.as_str())
                        })
                    });
                let name = match project {
                    Some(path) => self.appearance(path).label(path),
                    None => std::path::Path::new(cwd)
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                };
                let phone_id = self.mobile.ids.get(&id).cloned().unwrap_or_else(|| {
                    locked(&self.store)
                        .ok()
                        .and_then(|store| store.persisted_session_id(id).ok())
                        .unwrap_or_else(|| id.to_string())
                });
                let card = json!({
                    "id": phone_id,
                    "projectId": project_id(project.map_or(cwd, String::as_str)),
                    "project": name,
                    "title": monocode_core::session_title::display_title(&view.title, &view.harness),
                    "harness": view.harness,
                    // One the desktop can't find any more reads as done.
                    "state": if view.state == "unavailable" { "done" } else { view.state },
                });
                (id, card)
            })
            .collect()
    }

    fn mobile_session_id(&mut self, id: i64) -> String {
        if let Some(value) = self.mobile.ids.get(&id) {
            return value.clone();
        }
        let value = locked(&self.store)
            .ok()
            .and_then(|store| store.persisted_session_id(id).ok())
            .unwrap_or_else(|| id.to_string());
        self.mobile.ids.insert(id, value.clone());
        value
    }

    fn handle_mobile_request(
        &mut self,
        request: Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let method = request.method.clone();
        match method.as_str() {
            "conversation.read" | "conversation.older" => self.mobile_read(request, cx),
            "models.list" => self.mobile_models(request, cx),
            "accounts.usage" => self.mobile_usage(request, cx),
            "mono.document" => self.mobile_document(request, cx),
            "mono.profile" | "mono.update" | "mono.reset" | "mono.delete" | "mono.create" => {
                self.mobile_mono(request, cx)
            }
            "mono.file" | "mono.saveFile" => self.mobile_mono_file(request, cx),
            "project.fileHighlight" => {
                let job = string(&request.params, "job").unwrap_or_default();
                let from = request.params["from"].as_u64().unwrap_or(0) as usize;
                request.reply(Ok(project::file_highlight(&job, from)));
            }
            "project.gitAction" => self.mobile_git_action(request, cx),
            method if method.starts_with("project.") => self.mobile_project(request, cx),
            _ => self.mobile_command(request, window, cx),
        }
    }

    /// Project reads run on the background executor against a checked root.
    fn mobile_project(&mut self, request: Request, cx: &mut Context<Self>) {
        let params = request.params.clone();
        let cwd =
            match self.mobile_project_path(&string(&params, "projectId").unwrap_or_default(), cx) {
                Ok(cwd) => cwd,
                Err(error) => return request.reply(Err(error)),
            };
        let method = request.method.clone();
        let work = cx.background_executor().spawn(async move {
            let cwd = PathBuf::from(cwd);
            let worktree = params["worktreePath"].as_str();
            let path = string(&params, "path").unwrap_or_default();
            match method.as_str() {
                "project.worktrees" => project::worktrees(&cwd),
                "project.changes" => {
                    project::changes(&project::resolve_root(&cwd, worktree)?, worktree)
                }
                "project.files" => project::files(&project::resolve_root(&cwd, worktree)?, &path),
                "project.file" => project::file(&project::resolve_root(&cwd, worktree)?, &path),
                "project.search" => project::search(
                    &project::resolve_root(&cwd, worktree)?,
                    params["query"].as_str().unwrap_or_default(),
                ),
                "project.git" => project::git(&project::resolve_root(&cwd, worktree)?),
                _ => Err("Unsupported mobile operation".into()),
            }
        });
        cx.spawn(async move |_, _| request.reply(work.await))
            .detach();
    }

    /// Staging, committing, pushing and pull requests, as the Changes panel
    /// runs them. A slow action keeps running; the phone asks again with the
    /// same command ID until it is done, and asking again never reruns it.
    fn mobile_git_action(&mut self, request: Request, cx: &mut Context<Self>) {
        let signature = request.params.to_string();
        let result = match self.mobile.git_jobs.get(&request.command_id) {
            Some(job) if job.signature != signature => {
                return request.reply(Err(
                    "Command ID was already used with different input".into()
                ));
            }
            Some(job) => job.result.clone(),
            None => {
                let cwd = match self.mobile_project_path(
                    &string(&request.params, "projectId").unwrap_or_default(),
                    cx,
                ) {
                    Ok(cwd) => cwd,
                    Err(error) => return request.reply(Err(error)),
                };
                let result = Arc::new(Mutex::new(None));
                let slot = result.clone();
                let params = request.params.clone();
                let chosen = cx
                    .try_global::<crate::settings::Preferences>()
                    .and_then(|p| p.background_text_model());
                cx.background_executor()
                    .spawn(async move {
                        let cwd = PathBuf::from(cwd);
                        let outcome = project::resolve_root(&cwd, params["worktreePath"].as_str())
                            .and_then(|root| project::git_action(&root, &params, chosen));
                        if let Ok(mut slot) = slot.lock() {
                            *slot = Some(outcome);
                        }
                    })
                    .detach();
                if self.mobile.git_jobs.len() >= MAX_GIT_JOBS
                    && let Some(oldest) = self.mobile.git_order.pop_front()
                {
                    self.mobile.git_jobs.remove(&oldest);
                }
                self.mobile.git_order.push_back(request.command_id.clone());
                self.mobile.git_jobs.insert(
                    request.command_id.clone(),
                    GitJob {
                        signature,
                        result: result.clone(),
                    },
                );
                result
            }
        };
        cx.spawn(async move |this, cx| {
            let started = std::time::Instant::now();
            loop {
                let done = result.lock().ok().and_then(|slot| slot.clone());
                if let Some(done) = done {
                    // The Changes panel and badges read the new state too.
                    let _ = this.update(cx, |this, cx| this.sync_git(cx));
                    return request.reply(done);
                }
                if started.elapsed() >= GIT_ACTION_WAIT {
                    return request.reply(Ok(json!({"pending": true})));
                }
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
            }
        })
        .detach();
    }

    /// A conversation as the phone shows it: its latest turns, or a page of
    /// older ones before a turn it already has.
    fn mobile_read(&mut self, request: Request, cx: &mut Context<Self>) {
        let conversation = string(&request.params, "conversationId").unwrap_or_default();
        let target = match self.mobile_target(&conversation, cx) {
            Ok(target) => target,
            Err(error) => return request.reply(Err(error)),
        };
        let older = request.method == "conversation.older";
        let before = string(&request.params, "before").unwrap_or_default();
        let limit = (request.params["limit"]
            .as_u64()
            .unwrap_or(DEFAULT_TAIL as u64) as usize)
            .clamp(1, MAX_PAGE);
        // Before a turn of an older page the phone already holds: the store
        // has the rest.
        if older && let Some((cursor, at)) = page_turn(&before) {
            return self.mobile_older_page(request, &target, cursor, Some(at), limit, cx);
        }
        let page = move |snapshot: &Snapshot| -> Result<Read, String> {
            let source = snapshot.source();
            let ids = turns::turn_ids(&source);
            if older {
                let (start, end, more) = watch::before(&ids, &before, limit)?;
                // At the top of the loaded rows; a long Mono keeps the rest in
                // the store.
                if start == end
                    && let Some(cursor) = snapshot.history
                {
                    return Ok(Read::Page(cursor));
                }
                return Ok(Read::Done(json!({
                    "turns": turns::build(&source, start..end),
                    "older": more || snapshot.history.is_some(),
                })));
            }
            let start = ids.len().saturating_sub(READ_TURNS);
            Ok(Read::Done(json!({
                "id": conversation,
                "messages": turns::messages(snapshot.rows),
                "turns": turns::build(&source, start..ids.len()),
                "busy": snapshot.busy,
                "older": start > 0 || snapshot.history.is_some(),
                "question": snapshot.question,
            })))
        };
        match self.with_live_snapshot(&target, cx, &page) {
            Some(Ok(Read::Done(value))) => request.reply(Ok(value)),
            Some(Ok(Read::Page(cursor))) => {
                self.mobile_older_page(request, &target, cursor, None, limit, cx)
            }
            Some(Err(error)) => request.reply(Err(error)),
            None => {
                let load = self.load_saved(&target, cx);
                cx.spawn(async move |this, cx| {
                    match load.await.and_then(|saved| page(&saved_snapshot(&saved))) {
                        Ok(Read::Done(value)) => request.reply(Ok(value)),
                        Ok(Read::Page(cursor)) => {
                            let _ = this.update(cx, |this, cx| {
                                this.mobile_older_page(request, &target, cursor, None, limit, cx)
                            });
                        }
                        Err(error) => request.reply(Err(error)),
                    }
                })
                .detach();
            }
        }
    }

    /// Turns from the store page at `cursor`, above the loaded rows: before
    /// its turn starting at row `before`, or with none its newest. Their IDs
    /// name the page, so the turns the phone has keep theirs.
    fn mobile_older_page(
        &mut self,
        request: Request,
        target: &Target,
        cursor: i64,
        before: Option<usize>,
        limit: usize,
        cx: &mut Context<Self>,
    ) {
        let (session, name) = match target {
            Target::Session(id) => (Some(*id), None),
            Target::Mono(mono) => {
                let found = self.monos.as_ref().and_then(|m| {
                    let mono = m.read(cx).roster.get(mono)?;
                    Some((mono.session_id, mono.label()))
                });
                (found.as_ref().and_then(|f| f.0), found.map(|f| f.1))
            }
        };
        let Some(session) = session else {
            return request.reply(Ok(json!({"turns": [], "older": false})));
        };
        // Drawn as the loaded turns are, from the open chat or the summary.
        let look = self
            .with_live_snapshot(target, cx, |s| PageLook {
                mono: s.mono.clone(),
                launches: s.launches.clone(),
                cwd: s.cwd.to_owned(),
                harness: s.harness.to_owned(),
                model: s.model.clone(),
            })
            .unwrap_or_else(|| {
                let summary = self.session_summaries.get(&session);
                PageLook {
                    mono: name,
                    launches: self.mobile_launches(target, cx),
                    cwd: summary.map(|s| s.cwd.clone()).unwrap_or_default(),
                    harness: summary.map(|s| s.provider.clone()).unwrap_or_default(),
                    model: None,
                }
            });
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            let store = locked(&store)?;
            older_page(&store, session, cursor, before, limit, look)
        });
        cx.spawn(async move |_, _| request.reply(work.await))
            .detach();
    }

    /// Runs `f` on the open chat's transcript when its history is in memory.
    fn with_live_snapshot<T>(
        &self,
        target: &Target,
        cx: &App,
        f: impl FnOnce(&Snapshot) -> T,
    ) -> Option<T> {
        let (view, mono) = match target {
            Target::Session(id) => (self.sessions.get(id)?.clone(), None),
            Target::Mono(id) => {
                let manager = self.monos.as_ref()?.read(cx);
                let name = manager.roster.get(id).map(|m| m.label());
                (manager.open_chat(id)?, name)
            }
        };
        let chat = view.read(cx);
        if !chat.history_loaded() {
            return None;
        }
        // Built for every streamed frame, so only once there's history to show.
        let launches = self.mobile_launches(target, cx);
        let waiting = match chat.mobile_waiting(cx) {
            Some(crate::conversation::MobileWaiting::Approval) => Some("Waiting for approval"),
            Some(crate::conversation::MobileWaiting::Question) => Some("Waiting for answers"),
            None => None,
        };
        let snapshot = Snapshot {
            rows: chat.transcript.read(cx).source_rows(),
            turns: &chat.settings.turns,
            cwd: &chat.summary.cwd,
            harness: &chat.summary.provider,
            model: chat.mobile_model().map(str::to_owned),
            busy: chat.busy,
            waiting,
            mono,
            pending: chat.mobile_pending(cx),
            launches: &launches,
            question: chat.mobile_question(cx),
            history: chat.transcript.read(cx).older_history_cursor(),
        };
        Some(f(&snapshot))
    }

    fn load_saved(
        &self,
        target: &Target,
        cx: &mut Context<Self>,
    ) -> Task<Result<Arc<Saved>, String>> {
        let (id, mono) = match target {
            Target::Session(id) => (Some(*id), None),
            Target::Mono(mono) => {
                let manager = self.monos.as_ref().map(|m| m.read(cx));
                let found = manager.and_then(|m| m.roster.get(mono));
                (found.and_then(|m| m.session_id), found.map(|m| m.label()))
            }
        };
        let summary = id.and_then(|id| self.session_summaries.get(&id).cloned());
        let launches = self.mobile_launches(target, cx);
        let store = self.store.clone();
        cx.background_executor().spawn(async move {
            let Some(id) = id else {
                // A Mono that has not talked yet.
                return Ok(Arc::new(Saved {
                    rows: Vec::new(),
                    turns: HashMap::new(),
                    harness: String::new(),
                    model: None,
                    cwd: String::new(),
                    history: None,
                    mono,
                    launches,
                }));
            };
            let store = locked(&store)?;
            let rows = store.load(id)?.rows;
            let history = store.history_before(id)?;
            let settings = store.agent_settings(id)?.unwrap_or_default();
            let (harness, cwd) = match summary {
                Some(summary) => (summary.provider, summary.cwd),
                None => store
                    .sessions()?
                    .into_iter()
                    .find(|s| s.id == id)
                    .map(|s| (s.provider, s.cwd))
                    .ok_or("Session is no longer available")?,
            };
            Ok(Arc::new(Saved {
                rows,
                turns: settings.turns,
                harness,
                model: settings.model,
                cwd,
                history,
                mono,
                launches,
            }))
        })
    }

    // MARK: watches

    fn start_mobile_watch(&mut self, start: WatchStart, cx: &mut Context<Self>) {
        let target = match self.mobile_target(&start.conversation_id, cx) {
            Ok(target) => target,
            Err(error) => return start.accept(Err(error)),
        };
        let id = start.id.clone();
        self.mobile.watches.insert(
            id,
            Watch {
                target,
                tail: (start.tail.unwrap_or(DEFAULT_TAIL as u64) as usize).clamp(1, MAX_PAGE),
                from: start.from.clone(),
                incremental: start.incremental,
                baseline: None,
                saved: None,
                loading: false,
                observed: None,
                _observers: Vec::new(),
            },
        );
        start.accept(Ok(()));
        // The phone waits for its first frame; send it straight away.
        self.push_mobile_watches(cx);
    }

    fn schedule_mobile_push(&mut self, cx: &mut Context<Self>) {
        if self.mobile.push_scheduled || self.mobile.watches.is_empty() {
            return;
        }
        self.mobile.push_scheduled = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PUSH_INTERVAL).await;
            let _ = this.update(cx, |this, cx| {
                this.mobile.push_scheduled = false;
                this.push_mobile_watches(cx);
            });
        })
        .detach();
    }

    /// Follows the chat a watch shows, so each change becomes a frame.
    fn observe_mobile_watch(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(watch) = self.mobile.watches.get(id) else {
            return;
        };
        let view = match &watch.target {
            Target::Session(session) => self.sessions.get(session).cloned(),
            Target::Mono(mono) => self.monos.as_ref().and_then(|m| m.read(cx).open_chat(mono)),
        };
        let Some(view) = view else {
            return;
        };
        if watch.observed == Some(view.entity_id()) {
            return;
        }
        let transcript = view.read(cx).transcript.clone();
        let observers = vec![
            cx.observe(&view, |this, _, cx| this.schedule_mobile_push(cx)),
            cx.observe(&transcript, |this, _, cx| this.schedule_mobile_push(cx)),
        ];
        if let Some(watch) = self.mobile.watches.get_mut(id) {
            watch.observed = Some(view.entity_id());
            watch._observers = observers;
        }
    }

    fn push_mobile_watches(&mut self, cx: &mut Context<Self>) {
        let Some(host) = self.mobile.host.clone() else {
            return;
        };
        let ids: Vec<String> = self.mobile.watches.keys().cloned().collect();
        let mut again = false;
        for id in ids {
            self.observe_mobile_watch(&id, cx);
            let Some(watch) = self.mobile.watches.get(&id) else {
                continue;
            };
            let target = watch.target.clone();
            let (tail, from, incremental) = (watch.tail, watch.from.clone(), watch.incremental);
            let frame = |snapshot: &Snapshot| {
                let source = snapshot.source();
                let ids = turns::turn_ids(&source);
                let (start, from, older) = watch::window(&ids, from.as_deref(), tail);
                // Turns above the loaded rows are older history too.
                let older = older || snapshot.history.is_some();
                let built = turns::build(&source, start..ids.len());
                (from, older, built, snapshot.busy, snapshot.question.clone())
            };
            let built = match self.with_live_snapshot(&target, cx, frame) {
                Some(built) => built,
                None => match watch.saved.clone() {
                    Some(saved) => frame(&saved_snapshot(&saved)),
                    None => {
                        self.load_mobile_watch(&id, cx);
                        continue;
                    }
                },
            };
            let Some(watch) = self.mobile.watches.get_mut(&id) else {
                continue;
            };
            let (from, older, built, busy, question) = built;
            watch.from = from;
            let Some((update, baseline)) = watch::update(
                watch.baseline.as_ref(),
                busy,
                older,
                &question,
                &built,
                incremental,
            ) else {
                continue;
            };
            match host.push(&id, update) {
                Ok(()) => watch.baseline = Some(baseline),
                // The phone is behind; the next push sends from the old baseline.
                Err(Push::Busy) => again = true,
                Err(Push::Gone) => {
                    self.mobile.watches.remove(&id);
                }
            }
        }
        if again {
            self.schedule_mobile_push(cx);
        }
    }

    /// A watched session that is not open is read from disk once.
    fn load_mobile_watch(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(watch) = self.mobile.watches.get_mut(id) else {
            return;
        };
        if watch.loading {
            return;
        }
        watch.loading = true;
        let target = watch.target.clone();
        let load = self.load_saved(&target, cx);
        let id = id.to_owned();
        cx.spawn(async move |this, cx| {
            let result = load.await;
            let _ = this.update(cx, |this, cx| {
                let Some(watch) = this.mobile.watches.get_mut(&id) else {
                    return;
                };
                watch.loading = false;
                match result {
                    Ok(saved) => {
                        watch.saved = Some(saved);
                        this.push_mobile_watches(cx);
                    }
                    Err(error) => {
                        if let Some(host) = &this.mobile.host {
                            let _ = host.push(&id, json!({"error": error}));
                        }
                        this.mobile.watches.remove(&id);
                    }
                }
            });
        })
        .detach();
    }

    // MARK: commands

    fn mobile_command(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        let signature = json!([request.method, request.params]).to_string();
        match self.mobile.receipts.get(&request.command_id) {
            Some(Receipt::Pending(seen) | Receipt::Done(seen, _)) if *seen != signature => {
                return request.reply(Err(
                    "Command ID was already used with different input".into()
                ));
            }
            Some(Receipt::Done(_, result)) => return request.reply(result.clone()),
            Some(Receipt::Pending(_)) => {
                return request.reply(Err("Desktop is still working on this; try again".into()));
            }
            None => {}
        }
        if self.mobile.receipts.len() >= MAX_RECEIPTS
            && let Some(oldest) = self.mobile.receipt_order.pop_front()
        {
            self.mobile.receipts.remove(&oldest);
        }
        self.mobile
            .receipt_order
            .push_back(request.command_id.clone());
        self.mobile.receipts.insert(
            request.command_id.clone(),
            Receipt::Pending(signature.clone()),
        );
        let command = request.command_id.clone();
        let finish = move |this: &mut Shell, request: Request, result: Result<Value, String>| {
            // A failed command may be retried with the same ID.
            match &result {
                Ok(_) => {
                    this.mobile.receipts.insert(
                        command.clone(),
                        Receipt::Done(signature.clone(), result.clone()),
                    );
                }
                Err(_) => {
                    this.mobile.receipts.remove(&command);
                }
            }
            request.reply(result);
        };
        match request.method.as_str() {
            "session.start" => self.mobile_start(request, finish, cx),
            "conversation.send" => self.mobile_send(request, finish, cx),
            _ => {
                let result = self.mobile_session_command(&request, window, cx);
                finish(self, request, result);
            }
        }
    }

    fn mobile_session_command(
        &mut self,
        request: &Request,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let params = &request.params;
        let conversation = string(params, "conversationId").unwrap_or_default();
        let target = self.mobile_target(&conversation, cx)?;
        let reply = json!({"sessionId": conversation});
        match (request.method.as_str(), &target) {
            ("session.cancel", Target::Session(id)) => {
                if let Some(view) = self.sessions.get(id) {
                    view.update(cx, |v, cx| v.stop_if_busy(cx));
                }
                Ok(reply)
            }
            ("session.cancel", Target::Mono(mono)) => {
                if let Some(chat) = self.monos.as_ref().and_then(|m| m.read(cx).open_chat(mono)) {
                    chat.update(cx, |v, cx| v.stop_if_busy(cx));
                }
                Ok(reply)
            }
            ("session.configure", target) => {
                let view = match target {
                    Target::Session(id) => self.ensure_session(*id, cx),
                    Target::Mono(mono) => self
                        .monos
                        .clone()
                        .and_then(|m| m.update(cx, |m, cx| m.mobile_chat(mono, cx))),
                }
                .ok_or("This conversation is still opening on desktop; try again")?;
                let model = match (
                    string(params, "harness").as_deref().and_then(Kind::parse),
                    string(params, "model"),
                ) {
                    (Some(kind), Some(model)) if model.len() <= 256 => {
                        Some((kind, monocode_core::store::native_model(&model, kind.id())))
                    }
                    _ => None,
                };
                let settings = model_settings(params);
                let access = params["runtimeMode"]
                    .as_str()
                    .filter(|mode| {
                        crate::conversation::mobile_access_modes().any(|(id, ..)| id == *mode)
                    })
                    .map(AccessMode::from_id);
                view.update(cx, |v, cx| {
                    v.load(cx);
                    v.mobile_configure(model, &settings, access, cx);
                });
                Ok(reply)
            }
            ("session.rename", Target::Session(id)) => {
                let title = params["title"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                if title.is_empty() || title.chars().count() > 200 {
                    return Err("Enter a title under 200 characters".into());
                }
                let id = *id;
                if let Some(summary) = self.session_summaries.get_mut(&id) {
                    summary.title = title.clone();
                }
                if let Some(view) = self.sessions.get(&id) {
                    view.update(cx, |v, cx| v.rename(title.clone(), cx));
                }
                self.store_write("rename the session", cx, move |s| s.title(id, &title));
                self.invalidate_chrome(cx);
                cx.notify();
                Ok(reply)
            }
            ("session.archive", Target::Session(id)) => {
                if !self.session_metadata(*id, cx).summary.archived {
                    self.toggle_archive(*id, cx);
                }
                Ok(reply)
            }
            ("session.delete", Target::Session(id)) => {
                self.delete_session(*id, window, cx);
                Ok(reply)
            }
            ("session.answerQuestion" | "session.keepQuestionOpen", target) => {
                let view = match target {
                    Target::Session(id) => self.sessions.get(id).cloned(),
                    Target::Mono(mono) => {
                        self.monos.as_ref().and_then(|m| m.read(cx).open_chat(mono))
                    }
                }
                .ok_or("This question was already answered")?;
                let request_id = params["requestId"]
                    .as_u64()
                    .ok_or("This question was already answered")?;
                // Keeping it open sends no reply, only holds a timed question.
                let answer = request.method == "session.answerQuestion";
                let answered = answer.then(|| &params["reply"]);
                view.update(cx, |v, cx| {
                    v.mobile_answer_question(request_id, answered, cx)
                })?;
                Ok(reply)
            }
            ("session.rename", Target::Mono(_)) => {
                Err("A Mono's conversation can't be renamed".into())
            }
            ("session.archive" | "session.delete", Target::Mono(_)) => {
                Err("This session can't be changed from the phone".into())
            }
            _ => Err("Unsupported mobile command".into()),
        }
    }

    fn mobile_send(
        &mut self,
        request: Request,
        finish: impl FnOnce(&mut Shell, Request, Result<Value, String>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let conversation = string(&request.params, "conversationId").unwrap_or_default();
        let staged = (|| {
            let target = self.mobile_target(&conversation, cx)?;
            let text = message_text(&request.params)?;
            Ok::<_, String>((target, text))
        })();
        let (target, text) = match staged {
            Ok(staged) => staged,
            Err(error) => return finish(self, request, Err(error)),
        };
        let stage = self.stage_mobile_attachments(&request.params, cx);
        cx.spawn(async move |this, cx| {
            let attachments = stage.await;
            let _ = this.update(cx, |this, cx| {
                let result = attachments.and_then(|attachments| match &target {
                    Target::Session(id) => {
                        let view = this
                            .ensure_session(*id, cx)
                            .ok_or("Session is no longer available")?;
                        view.update(cx, |v, cx| v.mobile_submit(text, attachments, cx));
                        this.mobile.started.insert(*id);
                        Ok(())
                    }
                    Target::Mono(mono) => this
                        .monos
                        .clone()
                        .ok_or("Mono is no longer available")?
                        .update(cx, |m, cx| {
                            m.mobile_send(
                                mono,
                                text,
                                attachments
                                    .iter()
                                    .map(|p| p.to_string_lossy().into_owned())
                                    .collect(),
                                cx,
                            )
                        }),
                });
                let result = result.map(|()| json!({"sessionId": conversation}));
                this.schedule_mobile_push(cx);
                finish(this, request, result);
            });
        })
        .detach();
    }

    /// A new session from the phone, set up as a new desktop tab would be and
    /// to the phone's model sheet choice, then sent its first message.
    fn mobile_start(
        &mut self,
        request: Request,
        finish: impl FnOnce(&mut Shell, Request, Result<Value, String>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let params = request.params.clone();
        let prepared = (|| {
            let text = message_text(&params)?;
            let cwd =
                self.mobile_project_path(&string(&params, "projectId").unwrap_or_default(), cx)?;
            let (default_kind, default_model, default_access) = self.mobile_defaults(&cwd, cx);
            let chosen = string(&params, "harness").as_deref().and_then(Kind::parse);
            let kind = chosen.unwrap_or(default_kind);
            let model = match (chosen, string(&params, "model")) {
                (Some(kind), Some(model)) if model.len() <= 256 => {
                    monocode_core::store::native_model(&model, kind.id())
                }
                (Some(kind), _) if kind != default_kind => None,
                _ => default_model,
            };
            let access = params["runtimeMode"]
                .as_str()
                .filter(|mode| {
                    crate::conversation::mobile_access_modes().any(|(id, ..)| id == *mode)
                })
                .map_or(default_access, AccessMode::from_id);
            let account = self.mobile_account(kind, &cwd, &params, cx)?;
            Ok::<_, String>((text, cwd, kind, model, access, account))
        })();
        let (text, cwd, kind, model, access, account) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return finish(self, request, Err(error)),
        };
        let effort = crate::conversation::preferred_effort(kind.id(), cx);
        let mut chosen_settings = model_settings(&params);
        let stage = self.stage_mobile_attachments(&params, cx);
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            let attachments = stage.await?;
            let root = PathBuf::from(&cwd);
            let worktree = match params["worktreePath"].as_str() {
                Some(path) => Some(
                    project::resolve_root(&root, Some(path))?
                        .to_string_lossy()
                        .into_owned(),
                ),
                None => None,
            };
            let store = locked(&store)?;
            let summary = store.create(kind.id(), &cwd)?;
            let mut settings = session_loading::new_session_settings(
                store
                    .desktop_preferences()
                    .and_then(|preferences| preferences.session_defaults(kind.id(), &cwd)),
                model,
            );
            settings.access = access;
            settings.account = account;
            if let Some(effort) = chosen_settings.remove("effort").or(effort) {
                settings.effort = Some(effort);
            }
            if let Some(fast) = chosen_settings.remove("fast") {
                settings.fast = fast == "true";
            }
            if let Some(thinking) = chosen_settings.remove("thinking") {
                settings.thinking = Some(thinking == "true");
            }
            settings.model_settings.extend(chosen_settings);
            settings.worktree_cwd = worktree;
            // A new worktree off HEAD is made on the first send, as automations do.
            settings.new_worktree = params["workspaceMode"] == "worktree";
            store.save_agent_settings(summary.id, &settings)?;
            Ok::<_, String>((summary, settings, attachments))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                let result = result.map(|(summary, settings, attachments)| {
                    let id = summary.id;
                    this.mobile.started.insert(id);
                    this.add(summary, cx);
                    if let Some(view) = this.sessions.get(&id) {
                        view.update(cx, |v, cx| {
                            v.install_transfer(settings, None, cx);
                            v.mobile_submit(text, attachments, cx);
                        });
                    }
                    this.order.retain(|s| *s != id);
                    this.order.insert(0, id);
                    this.invalidate_chrome(cx);
                    this.persist(cx);
                    cx.notify();
                    json!({"sessionId": this.mobile_session_id(id)})
                });
                this.publish_mobile_catalog(cx);
                finish(this, request, result);
            });
        })
        .detach();
    }

    // MARK: notifications

    /// Sends a session's notice to paired phones: always a Mono's, and
    /// another's while a phone started its current turn, which its finish
    /// ends. Not while the desktop shows that session in front of the user.
    pub(super) fn notify_mobile(
        &mut self,
        summary: &Session,
        category: &str,
        body: &str,
        cx: &mut Context<Self>,
    ) {
        let id = summary.id;
        let from_phone = if category == "agentFinished" {
            self.mobile.started.remove(&id)
        } else {
            self.mobile.started.contains(&id)
        };
        // A Mono's own chat, or one of its habit runs.
        let mono = self.monos.as_ref().and_then(|monos| {
            let mono = monos.read(cx).notification_mono(id)?;
            Some((mono.id.clone(), mono.label(), mono.session_id != Some(id)))
        });
        if self.mobile.host.is_none()
            || (mono.is_none() && !from_phone)
            || (self.window_active && self.notice_session_visible(id, cx))
        {
            return;
        }
        let body = delivery::clip_body(body);
        let notice = match mono {
            Some((mono, name, habit)) => notify::Notice {
                title: name,
                subtitle: None,
                body: if body.is_empty() {
                    "Sent you a message".into()
                } else {
                    body
                },
                // A habit run's question is in its own session, not the
                // Mono's chat: the phone opens it as it does a Mono's
                // sessions, in any project.
                data: if habit && self.session_summaries.contains_key(&id) {
                    let mut data = self.mobile_notice_session(summary, cx);
                    data["monoId"] = json!(mono);
                    data
                } else {
                    json!({"monoId": mono})
                },
            },
            None => {
                let (project, _) = self.mobile_notice_project(summary, cx);
                notify::Notice {
                    title: monocode_core::session_title::display_title(
                        &summary.title,
                        &summary.provider,
                    )
                    .to_owned(),
                    subtitle: Some(project),
                    body: if body.is_empty() {
                        "Finished".into()
                    } else {
                        body
                    },
                    data: self.mobile_notice_session(summary, cx),
                }
            }
        };
        // A Mono's own messages are their own category; its questions and
        // approvals are like any agent's.
        let category = if self
            .monos
            .as_ref()
            .is_some_and(|monos| monos.read(cx).notification_mono(id).is_some())
            && category != "agentInput"
        {
            "monoMessages"
        } else {
            category
        };
        self.send_mobile_notice(category, notice, cx);
    }

    /// The project a session's notice is filed under, by name and path.
    fn mobile_notice_project(&self, summary: &Session, cx: &App) -> (String, String) {
        let key = monocode_core::files::path_key(&summary.cwd);
        let path = self
            .mobile_projects(cx)
            .into_iter()
            .find(|path| monocode_core::files::path_key(path) == key)
            .unwrap_or_else(|| summary.cwd.clone());
        (self.appearance(&path).label(&path), path)
    }

    /// Where a tap on a session's notice opens it on the phone.
    fn mobile_notice_session(&mut self, summary: &Session, cx: &App) -> Value {
        let (_, path) = self.mobile_notice_project(summary, cx);
        json!({
            "projectId": project_id(&path),
            "sessionId": self.mobile_session_id(summary.id),
        })
    }

    /// A turn the phone started ended with no finish to tell it of, as when
    /// it was stopped: the session's next turns are the desktop's again.
    pub(super) fn mobile_turn_ended(&mut self, id: i64) {
        self.mobile.started.remove(&id);
    }

    /// A Mono's report, posted to its chat on its own.
    pub(super) fn notify_mobile_post(&mut self, session: i64, body: &str, cx: &mut Context<Self>) {
        if let Some(view) = self.notification_chat(session, cx) {
            let summary = view.read(cx).summary.clone();
            self.notify_mobile(&summary, "monoMessages", body, cx);
        }
    }

    fn send_mobile_notice(&self, category: &str, notice: notify::Notice, cx: &App) {
        let Some(host) = self.mobile.host.clone() else {
            return;
        };
        let tokens = host.push_tokens(category);
        if tokens.is_empty() {
            return;
        }
        cx.background_executor()
            .spawn(async move {
                match notify::send(&tokens, &notice) {
                    Ok(gone) if !gone.is_empty() => {
                        let _ = host.forget_push_tokens(&gone);
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("Mobile notification not sent: {error}"),
                }
            })
            .detach();
    }

    /// The account a new session uses: the phone's pick, checked against the
    /// final provider, else the project's selection.
    fn mobile_account(
        &self,
        kind: Kind,
        cwd: &str,
        params: &Value,
        cx: &App,
    ) -> Result<monocode_core::accounts::Profile, String> {
        let Some(center) = crate::accounts::get(cx) else {
            return Ok(Default::default());
        };
        let center = center.read(cx);
        let Some(id) = params.get("providerAccountId").filter(|v| !v.is_null()) else {
            return Ok(center.selected(kind, cwd));
        };
        if !monocode_core::accounts::supports_profiles(kind) {
            return Err("This provider does not support account switching".into());
        }
        match id.as_str() {
            Some(id) if center.book.exists(kind, id) => Ok(center.profile(id)),
            _ => Err(
                "Account is no longer available. Choose another account in the model picker."
                    .into(),
            ),
        }
    }

    /// Files a phone sent with a message, as `{ name, mimeType, data }` in
    /// base64, written where pasted files go and checked as attachments are.
    fn stage_mobile_attachments(
        &self,
        params: &Value,
        cx: &mut Context<Self>,
    ) -> Task<Result<Vec<PathBuf>, String>> {
        let value = params["attachments"].clone();
        let store = self.store.clone();
        cx.background_executor().spawn(async move {
            if value.is_null() {
                return Ok(Vec::new());
            }
            let items = value
                .as_array()
                .filter(|items| items.len() <= MAX_ATTACHMENTS)
                .ok_or(format!("Attach up to {MAX_ATTACHMENTS} files"))?;
            if items.is_empty() {
                return Ok(Vec::new());
            }
            let root = locked(&store)?
                .data_dir()
                .map(Path::to_path_buf)
                .unwrap_or_else(std::env::temp_dir)
                .join("attachments");
            std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
            let mut paths = Vec::new();
            for item in items {
                let name = item["name"]
                    .as_str()
                    .map(str::trim)
                    .filter(|name| !name.is_empty() && name.len() <= 255)
                    .ok_or("An attachment is missing its name")?;
                let data = item["data"]
                    .as_str()
                    .ok_or_else(|| format!("{name} has no contents"))?;
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(|_| format!("{name} could not be read"))?;
                // Only the file name survives; a phone never picks the folder.
                let safe: String = Path::new(name)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "attachment".into())
                    .chars()
                    .map(|c| {
                        if c.is_control() || c == '/' || c == '\\' {
                            '_'
                        } else {
                            c
                        }
                    })
                    .collect();
                let path = root.join(format!("mobile-{}-{safe}", monocode_core::mono::new_id()));
                std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
                paths.push(path);
            }
            monocode_core::composer::validate_attachments(&paths)?;
            Ok(paths)
        })
    }

    // MARK: models and accounts

    /// What the phone's model sheet offers: each provider the desktop picker
    /// shows, their models and settings, the permission modes, the accounts,
    /// and what the conversation or a new session in the project runs with.
    fn mobile_models(&mut self, request: Request, cx: &mut Context<Self>) {
        let params = request.params.clone();
        let current = match string(&params, "conversationId") {
            Some(conversation) => self.mobile_target(&conversation, cx).and_then(|target| {
                let view = match &target {
                    Target::Session(id) => self.sessions.get(id).cloned(),
                    Target::Mono(mono) => {
                        self.monos.as_ref().and_then(|m| m.read(cx).open_chat(mono))
                    }
                };
                match view {
                    Some(view) => {
                        let v = view.read(cx);
                        Ok((
                            v.summary.cwd.clone(),
                            Kind::from_id(&v.summary.provider),
                            v.mobile_model().map(str::to_owned),
                            v.mobile_model_settings(),
                            v.settings.access,
                            Some(v.settings.account.id.clone()),
                        ))
                    }
                    None => match target {
                        Target::Session(id) => {
                            let summary = self
                                .session_summaries
                                .get(&id)
                                .ok_or("Session is no longer available")?;
                            let settings =
                                locked(&self.store)?.agent_settings(id)?.unwrap_or_default();
                            Ok((
                                summary.cwd.clone(),
                                Kind::from_id(&summary.provider),
                                settings.model.clone(),
                                mobile_settings_of(&settings),
                                settings.access,
                                Some(settings.account.id.clone()),
                            ))
                        }
                        // Not open here: what its saved conversation runs with.
                        Target::Mono(mono) => {
                            let id = self
                                .monos
                                .as_ref()
                                .and_then(|m| m.read(cx).roster.get(&mono)?.session_id)
                                .ok_or(
                                    "This conversation is still opening on desktop; try again",
                                )?;
                            let store = locked(&self.store)?;
                            let (cwd, provider) = match self.session_summaries.get(&id) {
                                Some(summary) => (summary.cwd.clone(), summary.provider.clone()),
                                None => store
                                    .sessions()?
                                    .into_iter()
                                    .find(|s| s.id == id)
                                    .map(|s| (s.cwd, s.provider))
                                    .ok_or("Session is no longer available")?,
                            };
                            let settings = store.agent_settings(id)?.unwrap_or_default();
                            Ok((
                                cwd,
                                Kind::from_id(&provider),
                                settings.model.clone(),
                                mobile_settings_of(&settings),
                                settings.access,
                                Some(settings.account.id.clone()),
                            ))
                        }
                    },
                }
            }),
            None => self
                .mobile_project_path(&string(&params, "projectId").unwrap_or_default(), cx)
                .map(|cwd| {
                    let (kind, model, access) = self.mobile_defaults(&cwd, cx);
                    let mut settings = HashMap::new();
                    if let Some(effort) = crate::conversation::preferred_effort(kind.id(), cx) {
                        settings.insert("effort".into(), effort);
                    }
                    (cwd, kind, model, settings, access, None)
                }),
        };
        let (cwd, kind, model, settings, access, account) = match current {
            Ok(current) => current,
            Err(error) => return request.reply(Err(error)),
        };
        let hidden = self.workspace.preferences.hidden_providers.clone();
        let center = crate::accounts::get(cx).map(|c| c.read(cx));
        let account_kinds = [Kind::Claude, Kind::Codex];
        let accounts: Vec<Value> = center
            .map(|center| {
                account_kinds
                    .iter()
                    .flat_map(|kind| center.book.list(*kind))
                    .map(|a| {
                        let mut value =
                            json!({"id": a.id, "provider": a.provider, "label": a.label});
                        if a.id == monocode_core::accounts::DEFAULT {
                            value["isDefault"] = json!(true);
                        }
                        value
                    })
                    .collect()
            })
            .unwrap_or_default();
        let selected: serde_json::Map<String, Value> = account_kinds
            .iter()
            .map(|k| {
                (
                    k.id().to_owned(),
                    json!(
                        center
                            .map(|c| c.book.selected(*k, &cwd))
                            .unwrap_or_default()
                    ),
                )
            })
            .collect();
        let profiles: Vec<(Kind, monocode_core::accounts::Profile)> = Kind::ALL
            .into_iter()
            .map(|k| {
                let profile = center.map(|c| c.selected(k, &cwd)).unwrap_or_default();
                (k, profile)
            })
            .collect();
        let account = account.filter(|_| monocode_core::accounts::supports_profiles(kind));
        let work = cx.background_executor().spawn(async move {
            let harnesses: Vec<Kind> = profiles
                .iter()
                .map(|(k, _)| *k)
                .filter(|k| !hidden.iter().any(|h| h == k.id()))
                .filter(|k| monocode_core::provider::resolve(*k).is_some())
                .collect();
            let mut current_model = mobile_catalog::model_id(kind, model.as_deref());
            let models: Vec<Value> = profiles
                .iter()
                .filter(|(k, _)| harnesses.contains(k))
                .flat_map(|(k, profile)| {
                    let rows = monocode_core::catalog::models_for(*k, profile).unwrap_or_default();
                    if *k == kind {
                        current_model = mobile_catalog::current_model_id(kind, model.as_deref(), &rows);
                    }
                    rows
                        .into_iter()
                        .map(move |m| catalog_model(*k, m))
                })
                .collect();
            let mut current = json!({
                "harness": kind.id(),
                "model": current_model,
                "modelSettings": settings,
                "runtimeMode": access.id(),
            });
            if let Some(account) = account {
                current["providerAccountId"] = json!(account);
            }
            json!({
                "harnesses": harnesses.iter().map(|k| json!({"id": k.id(), "label": k.label()})).collect::<Vec<_>>(),
                "models": models,
                "runtimeModes": crate::conversation::mobile_access_modes()
                    .map(|(id, label, hint)| json!({"id": id, "label": label, "hint": hint}))
                    .collect::<Vec<_>>(),
                "accounts": accounts,
                "selectedAccounts": selected,
                "current": current,
            })
        });
        cx.spawn(async move |_, _| request.reply(Ok(work.await)))
            .detach();
    }

    /// The desktop footer's usage windows for a provider's accounts, from the
    /// same cache; only an explicit refresh probes again.
    /// A document a Mono wrote, for the phone's reader: only one a Mono's
    /// chat lists, read from the store when the roster has no body for it.
    fn mobile_document(&mut self, request: Request, cx: &mut Context<Self>) {
        let id = string(&request.params, "documentId").unwrap_or_default();
        let listed = self.monos.as_ref().and_then(|monos| {
            monos.read(cx).roster.monos.iter().find_map(|mono| {
                let document = mono.documents.iter().find(|d| d.id == id);
                (document.is_some() || mono.desktop_document_ids.contains(&id))
                    .then(|| document.cloned())
            })
        });
        let Some(document) = listed else {
            return request.reply(Err("This document is no longer available".into()));
        };
        // A visual runs here and goes as its rendered tree, which the phone
        // draws with the same dither; its source stays on the desktop.
        let reply = |document: monocode_core::mono::Document| {
            let mut value =
                json!({"title": document.title, "kind": document.kind, "body": document.body});
            if document.kind == "visual" {
                match visual_tree(&document.body) {
                    Ok(tree) => value["tree"] = tree,
                    Err(error) => value["treeError"] = json!(error),
                }
            }
            value
        };
        if let Some(document) = document.filter(|d| !d.body.is_empty()) {
            return request.reply(Ok(reply(document)));
        }
        let store = self.store.clone();
        let work = cx
            .background_executor()
            .spawn(async move { locked(&store)?.mono_document(&id) });
        cx.spawn(async move |_, _| {
            request.reply(work.await.and_then(|document| {
                document
                    .map(reply)
                    .ok_or_else(|| "This document is no longer available".into())
            }))
        })
        .detach();
    }

    /// A Mono's details for the phone's profile, or a change to its look.
    fn mobile_mono(&mut self, request: Request, cx: &mut Context<Self>) {
        let id = string(&request.params, "monoId").unwrap_or_default();
        let Some(monos) = self.monos.clone() else {
            return request.reply(Err("Mono is no longer available".into()));
        };
        let result = if request.method == "mono.create" {
            let result = monos.update(cx, |monos, cx| monos.mobile_create(cx));
            if result.is_ok() {
                self.publish_mobile_catalog(cx);
            }
            result
        } else if matches!(request.method.as_str(), "mono.reset" | "mono.delete") {
            let reset = request.method == "mono.reset";
            let result = monos.update(cx, |monos, cx| {
                if reset {
                    monos.mobile_reset(&id, cx)
                } else {
                    monos.mobile_delete(&id, cx)
                }
            });
            if result.is_ok() {
                self.publish_mobile_catalog(cx);
                // A reset chat is a new one: phones watching it follow it now.
                self.push_mobile_watches(cx);
            }
            result
        } else if request.method == "mono.update" {
            // Projects come as the phone's IDs; the roster keeps paths.
            let mut params = request.params.clone();
            if let Some(add) = string(&request.params, "addProject") {
                match self.mobile_project_path(&add, cx) {
                    Ok(path) => params["addPath"] = json!(path),
                    Err(error) => return request.reply(Err(error)),
                }
            }
            if let Some(remove) = string(&request.params, "removeProject") {
                let paths: Vec<String> = monos
                    .read(cx)
                    .roster
                    .get(&id)
                    .map(|mono| {
                        mono.projects
                            .iter()
                            .filter(|p| project_id(p) == remove)
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                params["removePaths"] = json!(paths);
            }
            let result = monos.update(cx, |monos, cx| monos.mobile_update(&id, &params, cx));
            // The phone's drawer and header show the new look at once.
            if result.is_ok() {
                self.publish_mobile_catalog(cx);
            }
            result
        } else {
            let views = self.mono_session_views(cx);
            monos.update(cx, |monos, cx| monos.mobile_profile(&id, &views, cx))
        };
        request.reply(result);
    }

    /// A Mono's soul or memory for the phone to read, or its edit to save.
    fn mobile_mono_file(&mut self, request: Request, cx: &mut Context<Self>) {
        let id = string(&request.params, "monoId").unwrap_or_default();
        let Some(monos) = self.monos.clone() else {
            return request.reply(Err("Mono is no longer available".into()));
        };
        let work = monos.update(cx, |monos, cx| monos.mobile_file(&id, &request.params, cx));
        cx.spawn(async move |_, _| request.reply(work.await))
            .detach();
    }

    fn mobile_usage(&mut self, request: Request, cx: &mut Context<Self>) {
        let kind = match request.params["provider"].as_str() {
            Some("claude") => Kind::Claude,
            Some("codex") => Kind::Codex,
            _ => {
                return request.reply(Err(
                    "Usage is only available for Claude Code and Codex accounts".into(),
                ));
            }
        };
        let Some(center) = crate::accounts::get(cx) else {
            return request.reply(Ok(json!({"accounts": {}})));
        };
        let refresh = request.params["refresh"] == true;
        let accounts: Vec<String> = center
            .read(cx)
            .book
            .list(kind)
            .into_iter()
            .map(|a| a.id)
            .collect();
        center.update(cx, |center, cx| {
            for id in &accounts {
                let key = crate::accounts::Key::new(kind, center.profile(id), None);
                center.refresh(key, refresh, cx);
            }
        });
        let show_remaining = self.workspace.preferences.show_remaining_usage;
        cx.spawn(async move |_, cx| {
            // Wait briefly for probes in flight, as the footer would.
            for _ in 0..50 {
                let fetching = cx
                    .update(|cx| {
                        let center = center.read(cx);
                        accounts.iter().any(|id| {
                            let key = crate::accounts::Key::new(kind, center.profile(id), None);
                            center.usage.get(&key).is_some_and(|state| state.fetching)
                        })
                    });
                if !fetching {
                    break;
                }
                cx.background_executor().timer(Duration::from_millis(200)).await;
            }
            let result = cx.update(|cx| {
                let center = center.read(cx);
                let now = monocode_core::usage::now_ms();
                let snapshots: serde_json::Map<String, Value> = accounts
                    .iter()
                    .map(|id| {
                        let key = crate::accounts::Key::new(kind, center.profile(id), None);
                        let state = center.usage.get(&key);
                        let status = center.status(kind, id);
                        let snapshot = state.and_then(|s| s.snapshot.as_ref());
                        let meters: Vec<Value> = snapshot
                            .map(|s| s.windows.as_slice())
                            .unwrap_or_default()
                            .iter()
                            .filter_map(|window| {
                                let title = match window.minutes {
                                    300 => "5h",
                                    10_080 => "Weekly",
                                    43_200 => "Monthly",
                                    _ => return None,
                                };
                                let used = window.used_percent.clamp(0., 100.);
                                let percent = if show_remaining { 100. - used } else { used };
                                let reset = match window.resets_at {
                                    None => window.label(),
                                    Some(at) if at <= now => "reset due".into(),
                                    Some(at) => format!("Resets in {}", monocode_core::usage::duration(at - now)),
                                };
                                Some(json!({
                                    "title": title,
                                    "usedPercent": used,
                                    "percent": percent,
                                    "label": format!("{percent:.0}% {}", if show_remaining { "left" } else { "used" }),
                                    "reset": reset,
                                }))
                            })
                            .collect();
                        (
                            id.clone(),
                            json!({
                                "status": {
                                    "tone": tone_id(status.tone),
                                    "label": status.label,
                                    "detail": status.detail,
                                },
                                "updatedAt": snapshot.map_or(0, |s| s.updated_at),
                                "stale": state.is_none_or(|s| s.fetching || s.error.is_some()),
                                "meters": meters,
                            }),
                        )
                    })
                    .collect();
                json!({"accounts": snapshots})
            });
            request.reply(Ok(result));
        })
        .detach();
    }
}

fn tone_id(tone: Tone) -> &'static str {
    match tone {
        Tone::Ready => "ready",
        Tone::Low => "low",
        Tone::Exhausted => "exhausted",
        Tone::Checking => "checking",
        _ => "unknown",
    }
}

/// An older page's turn ID, `p{cursor}t{row}`: its page's store cursor and
/// the row its turn starts at.
fn page_turn(id: &str) -> Option<(i64, usize)> {
    let (cursor, row) = id.strip_prefix('p')?.split_once('t')?;
    Some((cursor.parse().ok()?, row.parse().ok()?))
}

/// See `Shell::mobile_older_page`. Before a page's first turn it reads the
/// page before that one.
fn older_page(
    store: &monocode_core::store::Store,
    session: i64,
    mut cursor: i64,
    before: Option<usize>,
    limit: usize,
    mut look: PageLook,
) -> Result<Value, String> {
    if look.harness.is_empty()
        && let Some(saved) = store.sessions()?.into_iter().find(|s| s.id == session)
    {
        look.harness = saved.provider;
        look.cwd = saved.cwd;
    }
    let mut page = store.load_before(session, cursor, false)?;
    let mut before = before.map(|row| format!("t{row}"));
    loop {
        // The page's turns, or None when the turn asked for is its first.
        let turns = {
            let snapshot = Snapshot {
                rows: &page.rows,
                turns: &page.turns,
                cwd: &look.cwd,
                harness: &look.harness,
                model: look.model.clone(),
                busy: false,
                waiting: None,
                mono: look.mono.clone(),
                pending: Vec::new(),
                launches: &look.launches,
                question: Value::Null,
                history: page.before,
            };
            let source = snapshot.source();
            let ids = turns::turn_ids(&source);
            let end = match &before {
                Some(id) => ids
                    .iter()
                    .position(|turn| turn == id)
                    .ok_or("This part of the conversation is no longer available")?,
                None => ids.len(),
            };
            let start = end.saturating_sub(limit);
            (end > 0).then(|| (start, turns::build(&source, start..end)))
        };
        let Some((start, mut built)) = turns else {
            // Before this page's first turn: the page above it, if any.
            let Some(above) = page.before else {
                return Ok(json!({"turns": [], "older": false}));
            };
            cursor = above;
            page = store.load_before(session, cursor, false)?;
            before = None;
            continue;
        };
        for turn in &mut built {
            let id = turn["id"].as_str().map(|id| format!("p{cursor}{id}"));
            if let Some(id) = id {
                turn["id"] = json!(id);
            }
        }
        return Ok(json!({"turns": built, "older": start > 0 || page.before.is_some()}));
    }
}

fn saved_snapshot(saved: &Saved) -> Snapshot<'_> {
    Snapshot {
        rows: &saved.rows,
        turns: &saved.turns,
        cwd: &saved.cwd,
        harness: &saved.harness,
        model: saved.model.clone(),
        busy: false,
        waiting: None,
        mono: saved.mono.clone(),
        pending: Vec::new(),
        launches: &saved.launches,
        question: Value::Null,
        history: saved.history,
    }
}

fn message_text(params: &Value) -> Result<String, String> {
    let text = params["text"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_owned();
    let attached = params["attachments"]
        .as_array()
        .is_some_and(|a| !a.is_empty());
    if (text.is_empty() && !attached) || text.len() > MAX_MESSAGE {
        return Err("Enter a message under 100,000 characters".into());
    }
    Ok(text)
}

/// A model sheet's settings, keeping only well-formed ones.
fn model_settings(params: &Value) -> HashMap<String, String> {
    params["modelSettings"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| {
            let value = value.as_str()?;
            (key.len() <= 64 && value.len() <= 64).then(|| (key.clone(), value.to_owned()))
        })
        .collect()
}

fn mobile_settings_of(settings: &Settings) -> HashMap<String, String> {
    let mut values = settings.model_settings.clone();
    if let Some(effort) = &settings.effort {
        values.insert("effort".into(), effort.clone());
    }
    values.insert("fast".into(), settings.fast.to_string());
    if let Some(thinking) = settings.thinking {
        values.insert("thinking".into(), thinking.to_string());
    }
    values
}

fn title_case(value: &str) -> String {
    let mut chars = value.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// A catalog model as the phone's model sheet lists it, with its settings.
fn catalog_model(kind: Kind, model: monocode_core::catalog::Model) -> Value {
    let toggle = |id: &str, label: &str| {
        json!({
            "id": id, "label": label, "kind": "toggle", "value": "false",
            "options": [{"value": "true", "label": "On"}, {"value": "false", "label": "Off"}],
        })
    };
    let mut settings = Vec::new();
    if !model.efforts.is_empty() {
        settings.push(json!({
            "id": "effort",
            "label": "Reasoning",
            "kind": "select",
            "value": model.default_effort.clone().or_else(|| model.efforts.first().cloned()).unwrap_or_default(),
            "options": model.efforts.iter().map(|e| json!({"value": e, "label": title_case(e)})).collect::<Vec<_>>(),
        }));
    }
    if model.fast {
        settings.push(toggle("fast", "Fast"));
    }
    if model.thinking {
        settings.push(toggle("thinking", "Thinking"));
    }
    for control in &model.controls {
        let is_toggle = control.choices.len() == 2
            && control
                .choices
                .iter()
                .all(|(value, _)| value == "true" || value == "false");
        settings.push(json!({
            "id": control.id,
            "label": control.name,
            "kind": if is_toggle { "toggle" } else { "select" },
            "value": control.current,
            "options": control.choices.iter().map(|(value, label)| json!({"value": value, "label": label})).collect::<Vec<_>>(),
        }));
    }
    json!({
        "id": mobile_catalog::model_id(kind, Some(&model.id)),
        "harness": kind.id(),
        "name": pickers::model_display_name(kind, &model.id, &model.name).to_string(),
        "settings": settings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn mobile_catalog_matches_sidebar_visibility_for_saved_and_open_sessions(
        cx: &mut gpui::TestAppContext,
    ) {
        let (window, saved, blank) = super::super::tests::window(cx);
        window
            .update(cx, |shell, _, cx| {
                let paths = shell.mobile_projects(cx);
                let ids = |shell: &Shell, cx: &App| {
                    shell
                        .mobile_catalog_sessions(&paths, cx)
                        .into_iter()
                        .map(|entry| entry.id)
                        .collect::<Vec<_>>()
                };
                assert_eq!(
                    ids(shell, cx),
                    vec![saved],
                    "empty tabs must stay out of the phone's history"
                );
                let view = shell.sessions.remove(&saved).unwrap();
                let entries = shell.mobile_catalog_sessions(&paths, cx);
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].id, saved);
                assert!(
                    entries[0].live.is_none(),
                    "history must not need an open desktop view"
                );
                shell.sessions.insert(saved, view.clone());
                shell.sidebar_hidden_sessions.insert(saved);
                assert!(ids(shell, cx).is_empty());
                shell.sidebar_hidden_sessions.remove(&saved);
                view.update(cx, |chat, _| chat.summary.archived = true);
                assert!(ids(shell, cx).is_empty());
                shell.sessions[&blank].update(cx, |chat, _| chat.summary.has_messages = true);
                assert_eq!(
                    ids(shell, cx),
                    vec![blank],
                    "a real conversation with a default title is still history"
                );
            })
            .unwrap();
    }

    #[test]
    fn model_choices_use_the_same_ids_as_session_cards_and_normalize_back_for_the_cli() {
        let model = monocode_core::catalog::Model {
            alias_target: None,
            id: "claude-opus-5-5".into(),
            name: "Claude Opus 5.5".into(),
            efforts: vec!["high".into()],
            default_effort: Some("high".into()),
            fast: true,
            thinking: false,
            default: true,
            tiers: vec![],
            controls: vec![],
        };
        assert_eq!(
            mobile_catalog::current_model_id(Kind::Claude, None, std::slice::from_ref(&model)),
            "claude:opus-5-5"
        );
        assert_eq!(
            mobile_catalog::current_model_id(
                Kind::Claude,
                Some("opus"),
                std::slice::from_ref(&model)
            ),
            "claude:opus"
        );
        let choice = catalog_model(Kind::Claude, model);
        assert_eq!(choice["id"], "claude:opus-5-5");
        assert_eq!(choice["name"], "Claude Opus 5.5");
        assert_eq!(choice["settings"][0]["value"], "high");
        assert_eq!(
            monocode_core::store::native_model(choice["id"].as_str().unwrap(), "claude").as_deref(),
            Some("claude-opus-5-5")
        );
    }

    #[test]
    fn project_ids_match_the_earlier_desktop_encoding() {
        assert_eq!(project_id("/Users/me/My App"), "%2FUsers%2Fme%2FMy%20App");
        assert_eq!(project_id("C:\\code\\é"), "C%3A%5Ccode%5C%C3%A9");
        assert_eq!(project_id("a-b_c.d!~*'()"), "a-b_c.d!~*'()");
    }

    #[test]
    fn older_page_turns_name_their_page_and_window_turns_do_not() {
        assert_eq!(page_turn("p42t7"), Some((42, 7)));
        assert_eq!(page_turn("p-3t0"), Some((-3, 0)));
        // The loaded window's own turns, and anything malformed, are not pages.
        assert_eq!(page_turn("t7"), None);
        assert_eq!(page_turn("p42"), None);
        assert_eq!(page_turn("pxtn"), None);
    }

    #[test]
    fn messages_need_text_or_files_and_settings_drop_malformed_values() {
        assert!(message_text(&json!({"text": "  "})).is_err());
        assert_eq!(message_text(&json!({"text": " hi "})).unwrap(), "hi");
        assert!(message_text(&json!({"text": "", "attachments": [{}]})).is_ok());
        let settings = model_settings(&json!({"modelSettings": {"effort": "high", "bad": 3}}));
        assert_eq!(settings.len(), 1);
        assert_eq!(settings["effort"], "high");
    }
}
