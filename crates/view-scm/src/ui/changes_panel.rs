//! Port of src/features/source-control/ui/GitChangesPanel.tsx (and the
//! `SourceControl.tsx` wrapper, which only keyed the panel by folder):
//! the commit message box with generated messages, commit and its menu,
//! sync, publish, and pull request buttons, the staged and unstaged lists in
//! list or tree view, and the history graph below a resize sash.
//!
//! The diff index comes from the engine's `GitStatus` for the folder, which
//! polls it every 2 s while this panel watches it. Make a new panel for a
//! new folder, as the React wrapper did with `key={cwd}`.

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, EventEmitter,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement as _, Pixels, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, canvas, div,
    prelude::FluentBuilder as _,
};
use gpui_component::input::{Enter, InputEvent, TextareaState};
use monocode_core::HarnessId;
use monocode_core::appearance::ChangesView;
use monocode_engine::projects::git_status::same_index;
use monocode_engine::projects::{GitStatus, GitWatch, WatchKind};
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, file_type_icon, folder_type_icon, icon, u};

use crate::git::{GitChangedFile, GitDiffIndex, GitFileDiffKind, GitHistoryCommit, GitPr};
use crate::hooks::{CommitMessageRequest, PrContent, PrContentRequest};
use crate::model::changes::{
    AmendTarget, Busy, CONFIRM_AMEND_PUSHED, ChangeDir, ChangesFlags, FileAction, FlagInputs,
    amend_target_stale, build_change_tree, can_pull, confirm_default_message, create_pr_title,
    dirname, discard_all_prompt, discard_file_prompt, empty_list_label, is_active,
    pr_number_from_url, remote_pr_content, status_letter, status_tone, sync_buttons, sync_title,
    view_pr_label, view_pr_title,
};
use crate::paths::{MOD, basename, is_remote_project_path};
use crate::scm::Scm;
use crate::ui::common::{
    BoundsCell, PopoverPlacement, anchored_popover, contains, css_px, icon_action, palette,
    plain_textarea, spin_icon, status_color, track_bounds, with_alpha,
};
use crate::ui::history_graph::{
    GRAPH_PANEL_DEFAULT, GRAPH_PANEL_MIN, GitHistoryGraph, GraphEvent, clamp_graph_height,
};

/// What the panel asks its owner to open.
#[derive(Clone, Debug, PartialEq)]
pub enum ChangesPanelEvent {
    OpenFile {
        path: String,
        kind: GitFileDiffKind,
        pin: bool,
    },
    OpenAllChanges,
    OpenCommit {
        commit: GitHistoryCommit,
        pin: bool,
    },
}

/// How long the header's status line stays.
const STATUS_TIMEOUT: Duration = Duration::from_millis(4000);
/// `onMutated` invalidates watched files again after this delay.
const WATCH_RETRY: Duration = Duration::from_millis(150);

struct SashDrag {
    start_y: Pixels,
    start_height: f32,
}

pub struct GitChangesPanel {
    scm: Scm,
    cwd: String,
    enabled: bool,
    text_harness: Option<HarnessId>,
    selected_path: Option<String>,
    selected_kind: Option<GitFileDiffKind>,
    status: Option<Entity<GitStatus>>,
    watch: Option<GitWatch>,
    index: Option<GitDiffIndex>,
    branch_menu_open: bool,
    busy: Option<Busy>,
    status_text: Option<SharedString>,
    status_timer: Option<Task<()>>,
    graph: Entity<GitHistoryGraph>,
    graph_expanded: bool,
    graph_height: f32,
    sash: Option<SashDrag>,
    pane_height: Rc<Cell<f32>>,
    branch_toggle: BoundsCell,
    menu_toggle: BoundsCell,
    message: Entity<TextareaState>,
    message_editable: Option<bool>,
    placeholder_amend: Option<bool>,
    amend_target: Option<AmendTarget>,
    menu_open: bool,
    /// The file row under the pointer, whose actions show.
    hovered_row: Option<SharedString>,
    pr: Option<GitPr>,
    pr_task: Option<Task<()>>,
    generate: Option<Task<()>>,
    action: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ChangesPanelEvent> for GitChangesPanel {}

impl GitChangesPanel {
    pub fn new(
        scm: Scm,
        cwd: impl Into<String>,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let cwd = cwd.into();
        let valid = !cwd.is_empty() && cwd != "~";
        let (graph_open, graph_height) = {
            let state = scm.state.read(cx);
            (state.graph_open, state.graph_height)
        };
        let graph = cx.new(|cx| {
            GitHistoryGraph::new(scm.clone(), cwd.clone(), enabled, graph_open, window, cx)
        });
        let message = cx.new(|cx| TextareaState::new(window, cx).auto_grow(1, 8));
        let mut subscriptions = vec![
            cx.subscribe(&graph, |this, _, event: &GraphEvent, cx| match event {
                GraphEvent::ToggleExpanded => this.toggle_graph(cx),
                GraphEvent::OpenCommit { commit, pin } => cx.emit(ChangesPanelEvent::OpenCommit {
                    commit: commit.clone(),
                    pin: *pin,
                }),
            }),
            cx.subscribe(&message, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.load_pr(cx);
                }
            }),
        ];
        let status = valid.then(|| scm.status(&cwd, cx));
        if let Some(status) = &status {
            subscriptions.push(cx.observe_in(status, window, |this, _, window, cx| {
                this.sync_index(window, cx)
            }));
        }
        let pr = if valid {
            scm.state.read(cx).pr_by_cwd.get(&cwd).cloned().flatten()
        } else {
            None
        };
        let mut this = Self {
            scm,
            cwd,
            enabled,
            text_harness: None,
            selected_path: None,
            selected_kind: None,
            status,
            watch: None,
            index: None,
            branch_menu_open: false,
            busy: None,
            status_text: None,
            status_timer: None,
            graph,
            graph_expanded: graph_open,
            graph_height,
            sash: None,
            pane_height: Rc::new(Cell::new(0.)),
            branch_toggle: BoundsCell::default(),
            menu_toggle: BoundsCell::default(),
            message,
            message_editable: None,
            placeholder_amend: None,
            amend_target: None,
            menu_open: false,
            hovered_row: None,
            pr,
            pr_task: None,
            generate: None,
            action: None,
            _subscriptions: subscriptions,
        };
        this.update_watch(cx);
        this.sync_index(window, cx);
        this
    }

    // Props.

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        self.update_watch(cx);
        self.graph
            .update(cx, |graph, cx| graph.set_enabled(enabled, cx));
        cx.notify();
    }

    /// The harness that writes commit messages and PR text.
    pub fn set_text_harness(&mut self, harness: Option<HarnessId>) {
        self.text_harness = harness;
    }

    /// The file and commit open in the editor area, highlighted here.
    pub fn set_selection(
        &mut self,
        path: Option<String>,
        kind: Option<GitFileDiffKind>,
        sha: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.selected_path = path;
        self.selected_kind = kind;
        self.graph
            .update(cx, |graph, cx| graph.set_selected_sha(sha, cx));
        cx.notify();
    }

    // Reading, for owners and tests.

    pub fn index(&self) -> Option<&GitDiffIndex> {
        self.index.as_ref()
    }

    pub fn busy(&self) -> Option<&Busy> {
        self.busy.as_ref()
    }

    pub fn pr(&self) -> Option<&GitPr> {
        self.pr.as_ref()
    }

    pub fn status_text(&self) -> Option<&str> {
        self.status_text.as_deref()
    }

    pub fn message(&self, cx: &App) -> String {
        self.message.read(cx).value().to_string()
    }

    /// The commit message box's state.
    pub fn message_input(&self) -> &Entity<TextareaState> {
        &self.message
    }

    pub fn amending(&self) -> bool {
        self.amend_target.is_some()
    }

    pub fn branch_menu_open(&self) -> bool {
        self.branch_menu_open
    }

    pub fn graph(&self) -> &Entity<GitHistoryGraph> {
        &self.graph
    }

    pub fn flags(&self, cx: &App) -> ChangesFlags {
        let message = self.message(cx);
        ChangesFlags::compute(&FlagInputs {
            cwd: &self.cwd,
            index: self.index.as_ref(),
            pr: self.pr.as_ref(),
            busy: self.busy.is_some(),
            message: &message,
            amend: self.amend_target.is_some(),
        })
    }

    fn files(&self) -> &[GitChangedFile] {
        self.index
            .as_ref()
            .map(|i| i.files.as_slice())
            .unwrap_or(&[])
    }

    // The diff index.

    fn update_watch(&mut self, cx: &mut Context<Self>) {
        let want = self.enabled && self.status.is_some();
        if want && self.watch.is_none() {
            if let Some(status) = &self.status {
                self.watch =
                    Some(status.update(cx, |status, cx| status.watch(WatchKind::Index, cx)));
            }
        } else if !want {
            self.watch = None;
        }
    }

    /// The `useDiffIndex` result arrived through the engine's status.
    fn sync_index(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(status) = &self.status else {
            return;
        };
        let next = status.read(cx).index().cloned();
        let changed = match (&self.index, &next) {
            (None, None) => false,
            (Some(_), None) => true,
            (_, Some(next)) => !same_index(self.index.as_ref(), next),
        };
        if !changed {
            return;
        }
        let prev = std::mem::replace(&mut self.index, next);
        let branch_changed = prev.as_ref().and_then(|p| p.branch.clone())
            != self.index.as_ref().and_then(|i| i.branch.clone());
        if prev.is_some() && self.index.is_some() {
            // The history graph and diffs listen for this.
            self.scm.git_changed(cx);
        }
        if branch_changed || prev.is_none() {
            self.load_pr(cx);
        }
        if let Some(target) = &self.amend_target
            && amend_target_stale(target, self.index.as_ref())
        {
            self.amend_target = None;
            self.set_message(String::new(), window, cx);
        }
        cx.notify();
    }

    /// `reload`.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        if let Some(status) = &self.status {
            status.update(cx, |status, cx| status.reload_index(cx));
        }
    }

    // PR status (`usePrStatus`).

    fn load_pr(&mut self, cx: &mut Context<Self>) {
        let branch = self.index.as_ref().and_then(|i| i.branch.clone());
        if self.cwd.is_empty() || self.cwd == "~" || branch.is_none() {
            self.pr = None;
            self.pr_task = None;
            return;
        }
        let cwd = self.cwd.clone();
        let read = {
            let cwd = cwd.clone();
            self.scm.run(cx, move |git| git.git_pr_status(&cwd))
        };
        self.pr_task = Some(cx.spawn(async move |this, cx| {
            let result = read.await;
            let _ = this.update(cx, |this, cx| {
                let next = result.ok().flatten();
                this.scm.state.update(cx, |state, _| {
                    state.pr_by_cwd.insert(cwd.clone(), next.clone());
                });
                this.pr = next;
                cx.notify();
            });
        }));
    }

    fn record_pr_activity(&self, number: Option<i64>, cx: &mut App) {
        if let Some(number) = number.or(self.pr.as_ref().map(|pr| pr.number))
            && number != 0
        {
            self.scm.hooks.pr_activity(&self.cwd, number, cx);
        }
    }

    // The status line.

    fn set_status(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status_text = Some(text.into());
        self.status_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(STATUS_TIMEOUT).await;
            let _ = this.update(cx, |this, cx| {
                this.status_text = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    // The message box.

    fn set_message(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.message
            .update(cx, |state, cx| state.set_value(text, window, cx));
        cx.notify();
    }

    fn sync_message_state(
        &mut self,
        flags: &ChangesFlags,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editable = flags.can_edit_message;
        if self.message_editable != Some(editable) {
            self.message_editable = Some(editable);
            self.message
                .update(cx, |state, cx| state.set_disabled(!editable, cx));
        }
        let amend = self.amend_target.is_some();
        if self.placeholder_amend != Some(amend) {
            self.placeholder_amend = Some(amend);
            let placeholder = if amend {
                format!("Amend message ({MOD}↩ to amend)")
            } else {
                format!("Message ({MOD}↩ to commit)")
            };
            self.message.update(cx, |state, cx| {
                state.set_placeholder(placeholder, window, cx)
            });
        }
    }

    // Actions.

    pub fn toggle_branch_menu(&mut self, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        self.branch_menu_open = !self.branch_menu_open;
        cx.notify();
    }

    /// The branch menu's Pull.
    pub fn pull(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !can_pull(self.index.as_ref()) || self.busy.is_some() {
            return;
        }
        self.status_text = None;
        self.busy = Some(Busy::Pull);
        let cwd = self.cwd.clone();
        let call = self.scm.run(cx, move |git| git.git_pull(&cwd));
        self.action = Some(cx.spawn_in(window, async move |this, cx| {
            let result = call.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(()) => {
                        this.reload(cx);
                        this.scm.notify_git_changed(cx);
                        this.scm.hooks.files_changed(None, cx);
                        this.set_status("Pull complete", cx);
                    }
                    Err(error) => this.scm.hooks.alert(error, window, cx),
                }
                this.busy = None;
                this.branch_menu_open = false;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// `onMutated`.
    fn on_mutated(&mut self, paths: Option<Vec<String>>, cx: &mut Context<Self>) {
        self.reload(cx);
        self.scm.notify_git_changed(cx);
        self.scm.hooks.files_changed(paths.as_deref(), cx);
        let hooks = self.scm.hooks.clone();
        cx.spawn(async move |_, cx| {
            cx.background_executor().timer(WATCH_RETRY).await;
            cx.update(|cx| hooks.files_changed(paths.as_deref(), cx));
        })
        .detach();
    }

    /// `run`: stage, unstage, or discard one file.
    pub fn run_file(
        &mut self,
        file: GitChangedFile,
        action: FileAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() {
            return;
        }
        let confirm = (action == FileAction::Discard).then(|| {
            let (message, label) = discard_file_prompt(&file);
            self.scm.hooks.confirm(message, Some(label), window, cx)
        });
        let git = self.scm.git.clone();
        let cwd = self.cwd.clone();
        self.action = Some(cx.spawn_in(window, async move |this, cx| {
            if let Some(confirm) = confirm
                && !confirm.await
            {
                return;
            }
            let ok = this.update(cx, |this, cx| {
                if this.busy.is_some() {
                    return false;
                }
                this.busy = Some(Busy::File(file.relative.clone()));
                cx.notify();
                true
            });
            if !matches!(ok, Ok(true)) {
                return;
            }
            let relative = file.relative.clone();
            let result = cx
                .background_spawn(async move {
                    match action {
                        FileAction::Stage => git.git_stage_file(&cwd, &relative),
                        FileAction::Unstage => git.git_unstage_file(&cwd, &relative),
                        FileAction::Discard => git.git_discard_file(&cwd, &relative),
                    }
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(()) => this.on_mutated(Some(vec![file.path.clone()]), cx),
                    Err(error) => this.scm.hooks.alert(error, window, cx),
                }
                this.busy = None;
                cx.notify();
            });
        }));
    }

    /// `runAll`: stage, unstage, or discard every file.
    pub fn run_all(&mut self, action: FileAction, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        let unstaged: Vec<GitChangedFile> = self
            .files()
            .iter()
            .filter(|f| f.unstaged)
            .cloned()
            .collect();
        let confirm = if action == FileAction::Discard {
            let Some((message, label)) = discard_all_prompt(&unstaged) else {
                return;
            };
            Some(self.scm.hooks.confirm(message, Some(label), window, cx))
        } else {
            None
        };
        let git = self.scm.git.clone();
        let cwd = self.cwd.clone();
        self.action = Some(cx.spawn_in(window, async move |this, cx| {
            if let Some(confirm) = confirm
                && !confirm.await
            {
                return;
            }
            let ok = this.update(cx, |this, cx| {
                if this.busy.is_some() {
                    return false;
                }
                this.busy = Some(Busy::All(action));
                cx.notify();
                true
            });
            if !matches!(ok, Ok(true)) {
                return;
            }
            let result = cx
                .background_spawn(async move {
                    match action {
                        FileAction::Stage => git.git_stage_all(&cwd),
                        FileAction::Unstage => git.git_unstage_all(&cwd),
                        FileAction::Discard => git.git_discard_all(&cwd),
                    }
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(()) => {
                        let paths = (action == FileAction::Discard)
                            .then(|| unstaged.iter().map(|f| f.path.clone()).collect());
                        this.on_mutated(paths, cx);
                    }
                    Err(error) => this.scm.hooks.alert(error, window, cx),
                }
                this.busy = None;
                cx.notify();
            });
        }));
    }

    pub fn generating(&self) -> bool {
        self.generate.is_some()
    }

    /// Ask the agent for a commit message.
    pub fn generate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.flags(cx).can_generate || self.generate.is_some() {
            return;
        }
        let Some(generator) = self.scm.hooks.generate_commit_message.clone() else {
            return;
        };
        self.busy = Some(Busy::Generate);
        let task = generator(
            CommitMessageRequest {
                cwd: self.cwd.clone(),
                text_harness: self.text_harness,
            },
            cx,
        );
        self.generate = Some(cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.generate = None;
                this.busy = None;
                match result {
                    Ok(text) => this.set_message(text, window, cx),
                    Err(error) => this.scm.hooks.alert(error, window, cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Drop the generation task; a result it would have returned is ignored.
    pub fn cancel_generate(&mut self, cx: &mut Context<Self>) {
        self.generate = None;
        self.busy = None;
        cx.notify();
    }

    pub fn toggle_menu(&mut self, cx: &mut Context<Self>) {
        if !self.flags(cx).can_open_menu {
            return;
        }
        self.menu_open = !self.menu_open;
        cx.notify();
    }

    pub fn toggle_amend(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu_open = false;
        if self.amend_target.is_some() {
            self.amend_target = None;
            cx.notify();
            return;
        }
        let cwd = self.cwd.clone();
        let read = self.scm.run(cx, move |git| git.git_head_message(&cwd));
        cx.spawn_in(window, async move |this, cx| {
            let result = read.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(head_message) => {
                    if this.message(cx).trim().is_empty() {
                        this.set_message(head_message, window, cx);
                    }
                    this.amend_target = Some(AmendTarget {
                        branch: this.index.as_ref().and_then(|i| i.branch.clone()),
                        head: this.index.as_ref().and_then(|i| i.head.clone()),
                    });
                    cx.notify();
                }
                Err(error) => this.scm.hooks.alert(error, window, cx),
            });
        })
        .detach();
        cx.notify();
    }

    /// Commit, then push, then open a pull request, as far as asked.
    pub fn commit(
        &mut self,
        push: bool,
        create_pr: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let flags = self.flags(cx);
        if !flags.can_commit {
            return;
        }
        let default_question = if push || create_pr {
            confirm_default_message(self.index.as_ref(), flags.on_default, create_pr)
        } else {
            None
        };
        let amend = self.amend_target.is_some();
        let amend_pushed = amend && self.index.as_ref().is_some_and(|i| i.head_pushed);
        let message = self.message(cx);
        let git = self.scm.git.clone();
        let cwd = self.cwd.clone();
        self.action = Some(cx.spawn_in(window, async move |this, cx| {
            if let Some(question) = default_question {
                let Ok(answer) = this.update_in(cx, |this, window, cx| {
                    this.scm.hooks.confirm(question, None, window, cx)
                }) else {
                    return;
                };
                if !answer.await {
                    return;
                }
            }
            if amend_pushed {
                let Ok(answer) = this.update_in(cx, |this, window, cx| {
                    this.scm
                        .hooks
                        .confirm(CONFIRM_AMEND_PUSHED.into(), Some("Amend"), window, cx)
                }) else {
                    return;
                };
                if !answer.await {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.busy = Some(if create_pr { Busy::Pr } else { Busy::Commit });
                this.menu_open = false;
                cx.notify();
            });
            let steps = {
                let (git, cwd) = (git.clone(), cwd.clone());
                cx.background_spawn(async move {
                    git.git_commit(&cwd, &message, amend)?;
                    if push || create_pr {
                        git.git_push(&cwd)?;
                        return Ok(true);
                    }
                    Ok::<bool, String>(false)
                })
                .await
            };
            let result: Result<(), String> = match steps {
                Ok(pushed) => {
                    let _ = this.update_in(cx, |this, window, cx| {
                        if pushed {
                            this.record_pr_activity(None, cx);
                        }
                        this.set_message(String::new(), window, cx);
                        this.amend_target = None;
                        this.on_mutated(None, cx);
                    });
                    if create_pr {
                        let created = Self::open_created_pr(this.clone(), cx).await;
                        let _ = this.update(cx, |this, cx| this.load_pr(cx));
                        created
                    } else {
                        Ok(())
                    }
                }
                Err(error) => Err(error),
            };
            let _ = this.update_in(cx, |this, window, cx| {
                if let Err(error) = result {
                    this.scm.hooks.alert(error, window, cx);
                    this.on_mutated(None, cx);
                }
                this.busy = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// `openCreatedPr`: write the PR text, create it, and open it.
    async fn open_created_pr(
        this: gpui::WeakEntity<Self>,
        cx: &mut gpui::AsyncWindowContext,
    ) -> Result<(), String> {
        let (git, cwd, harness, generator) = this
            .update(cx, |this, _| {
                (
                    this.scm.git.clone(),
                    this.cwd.clone(),
                    this.text_harness,
                    this.scm.hooks.generate_pr_content.clone(),
                )
            })
            .map_err(|e| e.to_string())?;
        let content: Option<PrContent> = if is_remote_project_path(&cwd) {
            let cwd = cwd.clone();
            let git = git.clone();
            let range = cx
                .background_spawn(async move { git.git_range_context(&cwd) })
                .await?;
            Some(remote_pr_content(&range))
        } else {
            match generator {
                Some(generator) => {
                    let request = PrContentRequest {
                        cwd: cwd.clone(),
                        text_harness: harness,
                    };
                    let task = cx
                        .update(|_, cx| generator(request, cx))
                        .map_err(|e| e.to_string())?;
                    task.await?
                }
                None => None,
            }
        };
        let content = content.ok_or("Could not prepare pull request content")?;
        let url: String = {
            let cwd = cwd.clone();
            cx.background_spawn(async move {
                git.git_pr_create(
                    &cwd,
                    &content.title,
                    &content.body,
                    &content.base,
                    &content.head,
                )
            })
            .await?
        };
        let _ = this.update(cx, |this, cx| {
            if let Some(number) = pr_number_from_url(&url) {
                this.record_pr_activity(Some(number), cx);
            }
            this.scm.hooks.open_url(url.trim(), cx);
        });
        Ok(())
    }

    /// Pull and push, or publish a branch without an upstream.
    pub fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let flags = self.flags(cx);
        let Some(index) = &self.index else {
            return;
        };
        if !(flags.can_sync || flags.can_publish) {
            return;
        }
        let pushes_commits = index.ahead > 0;
        self.busy = Some(Busy::Sync);
        let cwd = self.cwd.clone();
        let call = self.scm.run(cx, move |git| git.git_sync(&cwd));
        self.action = Some(cx.spawn_in(window, async move |this, cx| {
            let result = call.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(()) => {
                        if pushes_commits {
                            this.record_pr_activity(None, cx);
                        }
                        this.on_mutated(None, cx);
                        this.load_pr(cx);
                    }
                    Err(error) => {
                        this.scm.hooks.alert(error, window, cx);
                        this.on_mutated(None, cx);
                    }
                }
                this.busy = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Push if needed, then open a pull request.
    pub fn create_pr(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let flags = self.flags(cx);
        if !flags.can_create_pr {
            return;
        }
        let question = confirm_default_message(self.index.as_ref(), flags.on_default, true);
        let ahead = self.index.as_ref().map_or(0, |i| i.ahead);
        let git = self.scm.git.clone();
        let cwd = self.cwd.clone();
        self.action = Some(cx.spawn_in(window, async move |this, cx| {
            if let Some(question) = question {
                let Ok(answer) = this.update_in(cx, |this, window, cx| {
                    this.scm.hooks.confirm(question, None, window, cx)
                }) else {
                    return;
                };
                if !answer.await {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.busy = Some(Busy::Pr);
                cx.notify();
            });
            let pushed = if ahead > 0 {
                let (git, cwd) = (git.clone(), cwd.clone());
                cx.background_spawn(async move { git.git_push(&cwd) }).await
            } else {
                Ok(())
            };
            let result = match pushed {
                Ok(()) => Self::open_created_pr(this.clone(), cx).await,
                Err(error) => Err(error),
            };
            let _ = this.update_in(cx, |this, window, cx| {
                if let Err(error) = &result {
                    this.scm.hooks.alert(error.clone(), window, cx);
                }
                this.on_mutated(None, cx);
                if result.is_ok() {
                    this.load_pr(cx);
                }
                this.busy = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn view_pr(&mut self, cx: &mut Context<Self>) {
        if let Some(url) = self.pr.as_ref().map(|pr| pr.url.clone())
            && !url.is_empty()
        {
            self.scm.hooks.open_url(&url, cx);
        }
    }

    pub fn toggle_view(&mut self, cx: &mut Context<Self>) {
        let next = match self.scm.state.read(cx).changes_view {
            ChangesView::Tree => ChangesView::List,
            ChangesView::List => ChangesView::Tree,
        };
        self.scm
            .state
            .update(cx, |state, _| state.changes_view = next);
        self.scm.hooks.save_changes_view(next, cx);
        cx.notify();
    }

    fn toggle_section(&mut self, staged: bool, cx: &mut Context<Self>) {
        self.scm.state.update(cx, |state, _| {
            if staged {
                state.staged_open = !state.staged_open;
            } else {
                state.changes_open = !state.changes_open;
            }
        });
        cx.notify();
    }

    fn toggle_dir(&mut self, key: String, cx: &mut Context<Self>) {
        self.scm.state.update(cx, |state, _| {
            if !state.collapsed_dirs.remove(&key) {
                state.collapsed_dirs.insert(key);
            }
        });
        cx.notify();
    }

    fn toggle_graph(&mut self, cx: &mut Context<Self>) {
        let open = !self.graph_expanded;
        self.graph_expanded = open;
        self.scm
            .state
            .update(cx, |state, _| state.graph_open = open);
        self.graph
            .update(cx, |graph, cx| graph.set_expanded(open, cx));
        cx.notify();
    }

    fn open_file(
        &mut self,
        file: &GitChangedFile,
        kind: GitFileDiffKind,
        pin: bool,
        cx: &mut Context<Self>,
    ) {
        if file.status == "deleted" {
            return;
        }
        cx.emit(ChangesPanelEvent::OpenFile {
            path: file.path.clone(),
            kind,
            pin,
        });
    }

    // The graph sash.

    fn max_graph_height(&self) -> f32 {
        let pane = self.pane_height.get();
        if pane <= 0. {
            return GRAPH_PANEL_DEFAULT * 2.;
        }
        (pane - 160.).max(GRAPH_PANEL_MIN)
    }

    fn set_graph_height(&mut self, height: f32, commit: bool, cx: &mut Context<Self>) {
        self.graph_height = height;
        if commit {
            self.scm
                .state
                .update(cx, |state, _| state.graph_height = height);
        }
        cx.notify();
    }

    fn begin_sash(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        if event.click_count >= 2 {
            let height = clamp_graph_height(GRAPH_PANEL_DEFAULT, self.max_graph_height());
            self.set_graph_height(height, true, cx);
            return;
        }
        self.sash = Some(SashDrag {
            start_y: event.position.y,
            start_height: self.graph_height,
        });
        cx.notify();
    }

    fn drag_sash(&mut self, y: Pixels, window: &Window, cx: &mut Context<Self>) {
        let Some(drag) = &self.sash else {
            return;
        };
        let delta = css_px(y - drag.start_y, window);
        let next = clamp_graph_height(drag.start_height - delta, self.max_graph_height());
        self.set_graph_height(next, false, cx);
    }

    fn end_sash(&mut self, cx: &mut Context<Self>) {
        if self.sash.take().is_some() {
            let height = clamp_graph_height(self.graph_height, self.max_graph_height());
            self.set_graph_height(height, true, cx);
        }
    }
}

// Rendering.

impl GitChangesPanel {
    fn render_header(
        &mut self,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut header = div()
            .flex()
            .flex_none()
            .h(u(36.))
            .items_center()
            .gap(u(8.))
            .border_b_1()
            .border_color(theme.colors.stroke)
            .px(u(12.))
            .child(
                div()
                    .text_px(12.)
                    .medium()
                    .text_color(theme.colors.content)
                    .child("Changes"),
            );
        if let Some(text) = &self.status_text {
            header = header.child(
                div()
                    .text_px(11.)
                    .text_color(theme.content(0.50))
                    .child(text.clone()),
            );
        }
        let Some(branch) = self.index.as_ref().and_then(|i| i.branch.clone()) else {
            return header.child(div().ml_auto());
        };
        let index = self.index.as_ref();
        let ahead = index.map_or(0, |i| i.ahead);
        let behind = index.map_or(0, |i| i.behind);
        let busy = self.busy.is_some();
        let pulling = self.busy == Some(Busy::Pull);
        let open = self.branch_menu_open;
        let mut info = div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(4.))
            .text_px(11.)
            .text_color(theme.content(0.50))
            .child(
                icon(IconName::GitBranch)
                    .size(u(12.))
                    .text_color(theme.content(0.50)),
            )
            .child(div().min_w_0().truncate().child(branch));
        if ahead > 0 {
            info = info.child(
                div()
                    .flex_none()
                    .tabular()
                    .text_color(theme.content(0.40))
                    .child(format!("↑{ahead}")),
            );
        }
        if behind > 0 {
            info = info.child(
                div()
                    .flex_none()
                    .tabular()
                    .text_color(theme.content(0.40))
                    .child(format!("↓{behind}")),
            );
        }
        let mut toggle = div()
            .id("branch-actions")
            .relative()
            .child(track_bounds(&self.branch_toggle))
            .flex()
            .flex_none()
            .size(u(20.))
            .items_center()
            .justify_center()
            .rounded(u(6.))
            .tooltip(tooltip("Branch actions"))
            .text_color(if open {
                theme.colors.content
            } else {
                theme.content(0.50)
            });
        if open {
            toggle = toggle.bg(theme.content(0.10));
        }
        if busy {
            toggle = toggle.opacity(0.4);
        } else {
            toggle = toggle
                .hover(|s| s.bg(theme.content(0.10)))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_branch_menu(cx)));
        }
        toggle = if pulling {
            toggle.child(spin_icon("pull-spin", 14., theme.content(0.50)))
        } else {
            toggle.child(
                icon(IconName::MoreHorizontal)
                    .size(u(16.))
                    .text_color(if open {
                        theme.colors.content
                    } else {
                        theme.content(0.50)
                    }),
            )
        };
        let mut group = div()
            .relative()
            .ml_auto()
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(4.))
            .child(info)
            .child(toggle);
        if open {
            let can = can_pull(self.index.as_ref());
            let disabled = busy || !can;
            let mut item = div()
                .id("pull")
                .flex()
                .h(u(28.))
                .w_full()
                .items_center()
                .gap(u(8.))
                .px(u(12.))
                .text_px(12.)
                .text_color(theme.colors.content);
            if !can {
                item = item.tooltip(tooltip(
                    "This branch needs a remote and upstream before it can pull",
                ));
            }
            if disabled {
                item = item.opacity(0.4);
            } else {
                item = item
                    .hover(|s| s.bg(theme.content(0.10)))
                    .on_click(cx.listener(|this, _, window, cx| this.pull(window, cx)));
            }
            item = if pulling {
                item.child(spin_icon("pull-item-spin", 14., theme.colors.content))
                    .child("Pulling…")
            } else {
                item.child(
                    icon(IconName::RefreshCw)
                        .size(u(14.))
                        .text_color(theme.colors.content),
                )
                .child("Pull")
            };
            let menu = plain_menu(theme, 144.)
                .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if !contains(&this.branch_toggle, event.position) {
                        this.branch_menu_open = false;
                        cx.notify();
                    }
                }))
                .child(item);
            group = group.child(anchored_popover(
                PopoverPlacement::BottomEnd,
                4.,
                theme.layer.popover,
                menu,
                window,
            ));
        }
        header.child(group)
    }

    fn render_commit_box(
        &mut self,
        flags: &ChangesFlags,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = theme.colors;
        let generating = self.busy == Some(Busy::Generate);
        let wand_disabled = !generating && !flags.can_generate;
        let wand_title = if generating {
            "Cancel commit message generation"
        } else {
            "Generate commit message"
        };
        let mut wand = div()
            .id("generate")
            .group("generate")
            .absolute()
            .top(u(4.))
            .right(u(4.))
            .flex()
            .size(u(20.))
            .items_center()
            .justify_center()
            .rounded(u(6.))
            .bg(theme.content(0.10))
            .tooltip(tooltip(wand_title));
        if wand_disabled {
            wand = wand.opacity(0.4);
        } else {
            wand = wand
                .hover(|s| s.bg(theme.content(0.20)))
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.busy == Some(Busy::Generate) {
                        this.cancel_generate(cx);
                    } else {
                        this.generate(window, cx);
                    }
                }));
        }
        wand = if generating {
            wand.child(
                div()
                    .group_hover("generate", |s| s.invisible())
                    .child(spin_icon("generate-spin", 14., c.content)),
            )
            .child(
                div()
                    .absolute()
                    .invisible()
                    .group_hover("generate", |s| s.visible())
                    .child(icon(IconName::X).size(u(14.)).text_color(c.content)),
            )
        } else {
            wand.child(
                icon(IconName::WandSparkles)
                    .size(u(12.))
                    .text_color(c.content),
            )
        };
        let message_box = div()
            .relative()
            .capture_action(cx.listener(|this, action: &Enter, window, cx| {
                if action.secondary && this.flags(cx).can_commit {
                    this.commit(false, false, window, cx);
                } else {
                    cx.propagate();
                }
            }))
            .child(
                div()
                    .w_full()
                    .rounded(u(6.))
                    .bg(theme.content(0.10))
                    .py(u(4.))
                    .pl(u(8.))
                    .pr(u(32.))
                    .when(!flags.can_edit_message, |el| el.opacity(0.4))
                    .text_px(13.)
                    .line_height(u(20.))
                    .child(plain_textarea(&self.message, cx)),
            )
            .child(wand);

        let can_commit = flags.can_commit;
        let (fill, ink) = if can_commit {
            (c.content, c.background_base)
        } else {
            (theme.content(0.40), c.background_base)
        };
        let amend = self.amend_target.is_some();
        let mut commit = div()
            .id("commit")
            .flex()
            .h(u(28.))
            .min_w_0()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(u(6.))
            .rounded_l(u(6.))
            .text_px(12.)
            .medium()
            .bg(fill)
            .text_color(ink)
            .child(icon(IconName::Check).size(u(14.)).text_color(ink))
            .child(if amend { "Amend Commit" } else { "Commit" });
        if can_commit {
            commit = commit
                .on_click(cx.listener(|this, _, window, cx| this.commit(false, false, window, cx)));
        }
        let menu_open = self.menu_open;
        let mut chevron = div()
            .id("commit-options")
            .relative()
            .child(track_bounds(&self.menu_toggle))
            .flex()
            .flex_none()
            .size(u(28.))
            .items_center()
            .justify_center()
            .rounded_r(u(6.))
            .border_l_1()
            .border_color(with_alpha(c.background_base, 0.10))
            .bg(if menu_open { c.content } else { fill })
            .tooltip(tooltip("Commit options"))
            .child(icon(IconName::ChevronDown).size(u(14.)).text_color(ink));
        if flags.can_open_menu {
            chevron = chevron
                .hover(move |s| {
                    s.bg(if can_commit {
                        theme.content(0.80)
                    } else {
                        c.content
                    })
                })
                .on_click(cx.listener(|this, _, _, cx| this.toggle_menu(cx)));
        }
        let mut row = div()
            .relative()
            .mt(u(6.))
            .flex()
            .child(commit)
            .child(chevron);
        if menu_open {
            let item = |id: &'static str, label: &'static str, enabled: bool| {
                let mut el = div()
                    .id(id)
                    .flex()
                    .h(u(28.))
                    .w_full()
                    .items_center()
                    .px(u(12.))
                    .text_px(12.)
                    .text_color(c.content)
                    .child(label);
                if enabled {
                    el = el.hover(|s| s.bg(theme.content(0.10)));
                } else {
                    el = el.opacity(0.4);
                }
                el
            };
            let commit_push = item("commit-push", "Commit & Push", flags.can_commit_push).when(
                flags.can_commit_push,
                |el| {
                    el.on_click(
                        cx.listener(|this, _, window, cx| this.commit(true, false, window, cx)),
                    )
                },
            );
            let commit_pr = item(
                "commit-pr",
                "Commit, Push & Create PR",
                flags.can_commit_push_pr,
            )
            .when(flags.can_commit_push_pr, |el| {
                el.on_click(cx.listener(|this, _, window, cx| this.commit(true, true, window, cx)))
            });
            let amend_item = item("amend", "Amend Last Commit", true)
                .justify_between()
                .gap(u(8.))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .size(u(14.))
                        .items_center()
                        .justify_center()
                        .when(amend, |el| {
                            el.child(icon(IconName::Check).size(u(14.)).text_color(c.content))
                        }),
                )
                .on_click(cx.listener(|this, _, window, cx| this.toggle_amend(window, cx)));
            let menu = plain_menu(theme, 192.)
                .on_mouse_down_out(cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if !contains(&this.menu_toggle, event.position) {
                        this.menu_open = false;
                        cx.notify();
                    }
                }))
                .child(commit_push)
                .child(commit_pr)
                .child(div().my(u(4.)).h(gpui::px(1.)).bg(theme.content(0.10)))
                .child(amend_item);
            row = row.child(anchored_popover(
                PopoverPlacement::BottomEnd,
                4.,
                theme.layer.popover,
                menu,
                window,
            ));
        }

        let mut area = div()
            .flex_none()
            .border_b_1()
            .border_color(c.stroke)
            .p(u(8.))
            .child(message_box)
            .child(row);
        if let Some(sync) = self.render_sync_actions(flags, theme, cx) {
            area = area.child(sync);
        }
        area
    }

    /// `GitSyncActions`.
    fn render_sync_actions(
        &mut self,
        flags: &ChangesFlags,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let index = self.index.clone()?;
        let buttons = sync_buttons(flags)?;
        let c = theme.colors;
        let busy = self.busy.is_some();
        let syncing = self.busy == Some(Busy::Sync);
        let title = sync_title(&index, syncing, flags.can_publish);
        let secondary = |id: &'static str, title: String, disabled: bool| {
            let mut el = div()
                .id(id)
                .flex()
                .h(u(28.))
                .w_full()
                .min_w_0()
                .items_center()
                .justify_center()
                .gap(u(6.))
                .rounded(u(6.))
                .px(u(8.))
                .text_px(12.)
                .medium()
                .bg(theme.content(0.10))
                .text_color(c.content)
                .tooltip(tooltip(title));
            if disabled {
                el = el.opacity(0.4);
            } else {
                el = el.hover(|s| s.bg(theme.content(0.15)));
            }
            el
        };
        let mut column = div().mt(u(6.)).flex().flex_col().gap(u(6.));
        if buttons.publish {
            let glyph = if syncing {
                spin_icon("publish-spin", 14., c.content).into_any_element()
            } else {
                icon(IconName::CloudUpload)
                    .size(u(14.))
                    .text_color(c.content)
                    .into_any_element()
            };
            column = column.child(
                secondary("publish", title.clone(), busy)
                    .child(glyph)
                    .child(div().min_w_0().truncate().child("Publish Branch"))
                    .when(!busy, |el| {
                        el.on_click(cx.listener(|this, _, window, cx| this.sync(window, cx)))
                    }),
            );
        } else if buttons.sync {
            let glyph = if syncing {
                spin_icon("sync-spin", 14., c.content).into_any_element()
            } else {
                icon(IconName::RefreshCw)
                    .size(u(14.))
                    .text_color(c.content)
                    .into_any_element()
            };
            let mut button = secondary("sync", title.clone(), busy)
                .child(glyph)
                .child(div().min_w_0().truncate().child("Sync Changes"));
            if index.behind > 0 {
                button = button.child(
                    div()
                        .flex_none()
                        .tabular()
                        .text_color(theme.content(0.55))
                        .child(format!("↓{}", index.behind)),
                );
            }
            if index.ahead > 0 {
                button = button.child(
                    div()
                        .flex_none()
                        .tabular()
                        .text_color(theme.content(0.55))
                        .child(format!("↑{}", index.ahead)),
                );
            }
            if !busy {
                button = button.on_click(cx.listener(|this, _, window, cx| this.sync(window, cx)));
            }
            column = column.child(button);
        }
        if buttons.create_pr {
            let disabled = !flags.can_create_pr || busy;
            let glyph = if self.busy == Some(Busy::Pr) {
                spin_icon("pr-spin", 14., c.content).into_any_element()
            } else {
                icon(IconName::GitPullRequest)
                    .size(u(14.))
                    .text_color(c.content)
                    .into_any_element()
            };
            column = column.child(
                secondary("create-pr", create_pr_title(&index), disabled)
                    .child(glyph)
                    .child("Create PR")
                    .when(!disabled, |el| {
                        el.on_click(cx.listener(|this, _, window, cx| this.create_pr(window, cx)))
                    }),
            );
        }
        if buttons.view_pr {
            let disabled = !flags.can_view_pr || busy;
            column = column.child(
                secondary("view-pr", view_pr_title(self.pr.as_ref()), disabled)
                    .child(
                        icon(IconName::ExternalLink)
                            .size(u(14.))
                            .text_color(c.content),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .child(view_pr_label(self.pr.as_ref())),
                    )
                    .when(!disabled, |el| {
                        el.on_click(cx.listener(|this, _, _, cx| this.view_pr(cx)))
                    }),
            );
        }
        Some(column.into_any_element())
    }

    fn render_list(&mut self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div()
            .id("changes-list")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .py(u(4.));
        let files = self.files().to_vec();
        if files.is_empty() {
            return list.child(
                div()
                    .px(u(12.))
                    .py(u(8.))
                    .text_px(12.)
                    .text_color(theme.content(0.45))
                    .child(empty_list_label(self.index.as_ref())),
            );
        }
        let state = self.scm.state.read(cx);
        let view = state.changes_view;
        let staged_open = state.staged_open;
        let changes_open = state.changes_open;
        let collapsed = state.collapsed_dirs.clone();
        let staged: Vec<GitChangedFile> = files.iter().filter(|f| f.staged).cloned().collect();
        let unstaged: Vec<GitChangedFile> = files.iter().filter(|f| f.unstaged).cloned().collect();
        if !staged.is_empty() {
            let actions = vec![
                section_action(
                    "staged-open-all",
                    IconName::FileDiff,
                    "Open All Changes",
                    cx.listener(|_, _, _, cx| cx.emit(ChangesPanelEvent::OpenAllChanges)),
                ),
                section_action(
                    "staged-unstage-all",
                    IconName::Minus,
                    "Unstage All Changes",
                    cx.listener(|this, _, window, cx| {
                        this.run_all(FileAction::Unstage, window, cx)
                    }),
                ),
            ];
            list = list.child(self.render_section(
                "staged",
                "STAGED CHANGES",
                staged.len(),
                staged_open,
                view,
                actions,
                theme,
                cx,
            ));
            if staged_open {
                list = list.children(self.render_files(
                    &staged,
                    view,
                    GitFileDiffKind::Staged,
                    &collapsed,
                    theme,
                    cx,
                ));
            }
        }
        if !unstaged.is_empty() {
            let actions = vec![
                section_action(
                    "changes-open-all",
                    IconName::FileDiff,
                    "Open All Changes",
                    cx.listener(|_, _, _, cx| cx.emit(ChangesPanelEvent::OpenAllChanges)),
                ),
                section_action(
                    "changes-discard-all",
                    IconName::Undo2,
                    "Discard All Changes",
                    cx.listener(|this, _, window, cx| {
                        this.run_all(FileAction::Discard, window, cx)
                    }),
                ),
                section_action(
                    "changes-stage-all",
                    IconName::Plus,
                    "Stage All Changes",
                    cx.listener(|this, _, window, cx| this.run_all(FileAction::Stage, window, cx)),
                ),
            ];
            list = list.child(self.render_section(
                "changes",
                "CHANGES",
                unstaged.len(),
                changes_open,
                view,
                actions,
                theme,
                cx,
            ));
            if changes_open {
                list = list.children(self.render_files(
                    &unstaged,
                    view,
                    GitFileDiffKind::Unstaged,
                    &collapsed,
                    theme,
                    cx,
                ));
            }
        }
        list
    }

    /// `FileSection`'s header.
    #[allow(clippy::too_many_arguments)]
    fn render_section(
        &mut self,
        id: &'static str,
        title: &'static str,
        count: usize,
        open: bool,
        view: ChangesView,
        actions: Vec<SectionAction>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let staged = id == "staged";
        let toggle = div()
            .id((id, 0usize))
            .flex()
            .min_w_0()
            .flex_1()
            .items_center()
            .gap(u(4.))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_section(staged, cx)))
            .child(
                icon(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(u(14.))
                .text_color(theme.content(0.50)),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_px(10.)
                    .semibold()
                    .text_color(theme.content(0.55))
                    .child(title),
            )
            .child(
                div()
                    .ml(u(4.))
                    .flex()
                    .flex_none()
                    .h(u(16.))
                    .min_w(u(16.))
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(theme.accent(0.80))
                    .px(u(4.))
                    .text_px(8.)
                    .text_color(palette::white())
                    .child(count.to_string()),
            );
        let view_toggle = icon_action(
            (id, 1usize),
            if view == ChangesView::Tree {
                IconName::ListBullet
            } else {
                IconName::FolderTree
            },
            if view == ChangesView::Tree {
                "View as List"
            } else {
                "View as Tree"
            },
        )
        .on_click(cx.listener(|this, _, _, cx| this.toggle_view(cx)));
        let mut header = div()
            .flex()
            .flex_none()
            .h(u(28.))
            .items_center()
            .gap(u(4.))
            .px(u(6.))
            .child(toggle)
            .child(view_toggle);
        for action in actions {
            header = header.child(
                icon_action(action.id, action.icon, action.title)
                    .on_click(move |event, window, cx| (action.handler)(event, window, cx)),
            );
        }
        header
    }

    #[allow(clippy::too_many_arguments)]
    fn render_files(
        &mut self,
        files: &[GitChangedFile],
        view: ChangesView,
        kind: GitFileDiffKind,
        collapsed: &HashSet<String>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut rows = Vec::new();
        if view == ChangesView::Tree {
            let tree = build_change_tree(files);
            self.render_dir_children(&tree, 0, kind, collapsed, theme, &mut rows, cx);
        } else {
            for file in files {
                rows.push(
                    self.render_row(file, kind, None, theme, cx)
                        .into_any_element(),
                );
            }
        }
        rows
    }

    #[allow(clippy::too_many_arguments)]
    fn render_dir_children(
        &mut self,
        dir: &ChangeDir,
        depth: usize,
        kind: GitFileDiffKind,
        collapsed: &HashSet<String>,
        theme: &Theme,
        rows: &mut Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) {
        for child in &dir.dirs {
            let key = format!("{}:{}", kind.as_str(), child.path);
            let open = !collapsed.contains(&key);
            rows.push(
                self.render_dir_row(child, depth, key, open, theme, cx)
                    .into_any_element(),
            );
            if open {
                self.render_dir_children(child, depth + 1, kind, collapsed, theme, rows, cx);
            }
        }
        for file in &dir.files {
            rows.push(
                self.render_row(file, kind, Some(depth), theme, cx)
                    .into_any_element(),
            );
        }
    }

    /// `ChangeDirRow`.
    fn render_dir_row(
        &mut self,
        dir: &ChangeDir,
        depth: usize,
        key: String,
        open: bool,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let dot = match &dir.status {
            Some(status) => status_color(status_tone(status), theme),
            None => theme.content(0.40),
        };
        let id: SharedString = format!("dir-{key}").into();
        div()
            .id(id)
            .flex()
            .flex_none()
            .h(u(28.))
            .w_full()
            .items_center()
            .gap(u(6.))
            .pl(u(8. + depth as f32 * 12.))
            .pr(u(8.))
            .text_color(theme.colors.content)
            .hover(|s| s.bg(theme.content(0.05)))
            .tooltip(tooltip(dir.path.clone()))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_dir(key.clone(), cx)))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .size(u(16.))
                    .items_center()
                    .justify_center()
                    .child(
                        icon(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size(u(14.))
                        .text_color(theme.content(0.50)),
                    ),
            )
            .child(folder_type_icon(dir.name.clone(), open, false).size(16.))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .text_px(13.)
                    .medium()
                    .child(dir.name.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .w(u(14.))
                    .items_center()
                    .justify_center()
                    .child(div().size(u(6.)).rounded_full().bg(dot)),
            )
    }

    /// `ChangeRow`. `depth` is set in tree view.
    fn render_row(
        &mut self,
        file: &GitChangedFile,
        kind: GitFileDiffKind,
        depth: Option<usize>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let c = theme.colors;
        let name = basename(&file.relative);
        let dir = if depth.is_some() {
            ""
        } else {
            dirname(&file.relative)
        };
        let active = is_active(
            file,
            self.selected_path.as_deref(),
            self.selected_kind,
            kind,
        );
        let busy = self.busy == Some(Busy::File(file.relative.clone()));
        let row_id: SharedString = format!("{}:{}", kind.as_str(), file.relative).into();
        let open_file = file.clone();
        let mut label = div()
            .id(SharedString::from(format!("open-{row_id}")))
            .flex()
            .min_w_0()
            .flex_1()
            .items_center()
            .gap(u(6.))
            .tooltip(tooltip(file.relative.clone()))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                this.open_file(&open_file, kind, event.click_count() >= 2, cx)
            }));
        if depth.is_some() {
            label = label.child(div().flex_none().size(u(16.)));
        }
        let mut text = div()
            .min_w_0()
            .flex_1()
            .truncate()
            .child(div().flex_none().text_px(13.).medium().child(name.clone()));
        if !dir.is_empty() {
            text = div()
                .flex()
                .min_w_0()
                .flex_1()
                .items_baseline()
                .overflow_hidden()
                .child(div().flex_none().text_px(13.).medium().child(name.clone()))
                .child(
                    div()
                        .ml(u(6.))
                        .min_w_0()
                        .truncate()
                        .text_px(11.)
                        .text_color(theme.content(0.40))
                        .child(dir.to_string()),
                );
        }
        label = label.child(file_type_icon(name).size(16.)).child(text);

        // Shown while active or hovered. The display comes from entity state,
        // not a hover style, so it cannot change between prepaint and paint.
        let shown = active || self.hovered_row.as_ref() == Some(&row_id);
        let mut actions = div().flex_none().items_center();
        actions = if shown {
            actions.flex()
        } else {
            actions.hidden()
        };
        if kind == GitFileDiffKind::Unstaged {
            let target = file.clone();
            actions = actions.child(
                icon_action(
                    SharedString::from(format!("discard-{row_id}")),
                    IconName::Undo2,
                    "Discard Changes",
                )
                .disabled(busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.run_file(target.clone(), FileAction::Discard, window, cx)
                })),
            );
        }
        let target = file.clone();
        actions = if kind == GitFileDiffKind::Staged {
            actions.child(
                icon_action(
                    SharedString::from(format!("unstage-{row_id}")),
                    IconName::Minus,
                    "Unstage Changes",
                )
                .disabled(busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.run_file(target.clone(), FileAction::Unstage, window, cx)
                })),
            )
        } else {
            actions.child(
                icon_action(
                    SharedString::from(format!("stage-{row_id}")),
                    IconName::Plus,
                    "Stage Changes",
                )
                .disabled(busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.run_file(target.clone(), FileAction::Stage, window, cx)
                })),
            )
        };
        let tone = status_color(status_tone(&file.status), theme);
        let hover_id = row_id.clone();
        let mut row = div()
            .id(row_id)
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                let next = if *hovered {
                    Some(hover_id.clone())
                } else if this.hovered_row.as_ref() == Some(&hover_id) {
                    None
                } else {
                    this.hovered_row.clone()
                };
                if next != this.hovered_row {
                    this.hovered_row = next;
                    cx.notify();
                }
            }))
            .flex()
            .flex_none()
            .h(u(28.))
            .w_full()
            .items_center()
            .gap(u(4.))
            .pr(u(8.))
            .text_color(c.content);
        row = match depth {
            Some(depth) => row.pl(u(8. + depth as f32 * 12.)),
            None => row.pl(u(8.)),
        };
        row = if active {
            row.bg(c.selection)
        } else {
            row.hover(|s| s.bg(theme.content(0.05)))
        };
        row.child(label).child(actions).child(
            div()
                .flex_none()
                .w(u(14.))
                .flex()
                .justify_end()
                .font_family(theme.fonts.mono.clone())
                .text_px(11.)
                .semibold()
                .text_color(tone)
                .child(status_letter(&file.status)),
        )
    }

    fn render_sash(&mut self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let dragging = self.sash.is_some();
        div()
            .id("graph-sash")
            .flex_none()
            .h(u(6.))
            .cursor_row_resize()
            .when(dragging, |el| el.bg(theme.content(0.15)))
            .when(!dragging, |el| el.hover(|s| s.bg(theme.content(0.10))))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.begin_sash(event, cx)
                }),
            )
    }
}

type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

struct SectionAction {
    id: &'static str,
    icon: IconName,
    title: &'static str,
    handler: ClickHandler,
}

fn section_action(
    id: &'static str,
    icon: IconName,
    title: &'static str,
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> SectionAction {
    SectionAction {
        id,
        icon,
        title,
        handler: Rc::new(handler),
    }
}

/// The small solid menus the panel opens (`rounded-md border-content/10
/// bg-background-base py-1 shadow-lg`).
fn plain_menu(theme: &Theme, min_width: f32) -> gpui::Stateful<gpui::Div> {
    div()
        .id("panel-menu")
        .occlude()
        .flex()
        .flex_col()
        .min_w(u(min_width))
        .rounded(u(6.))
        .border_1()
        .border_color(theme.content(0.10))
        .bg(theme.colors.background_base)
        .py(u(4.))
        .shadow_lg()
}

impl Render for GitChangesPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        if self.cwd.is_empty() || self.cwd == "~" {
            return div()
                .px(u(12.))
                .py(u(8.))
                .text_px(12.)
                .text_color(theme.content(0.50))
                .child("No project folder")
                .into_any_element();
        }
        let flags = self.flags(cx);
        self.sync_message_state(&flags, window, cx);
        // `useLayoutEffect`: keep the graph inside the pane.
        let pane = self.pane_height.get();
        if self.sash.is_none() && pane >= GRAPH_PANEL_MIN + 160. && self.graph_height > pane - 160.
        {
            let max = pane - 160.;
            self.graph_height = max;
            self.scm
                .state
                .update(cx, |state, _| state.graph_height = max);
        }
        let pane_height = self.pane_height.clone();
        let measure = canvas(
            move |bounds, window, _| pane_height.set(css_px(bounds.size.height, window)),
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();
        let header = self.render_header(&theme, window, cx).into_any_element();
        let commit_box = self
            .render_commit_box(&flags, &theme, window, cx)
            .into_any_element();
        let list = self.render_list(&theme, cx).into_any_element();
        let expanded = self.graph_expanded;
        let mut root = div()
            .id("git-changes-panel")
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .overflow_hidden()
            .child(measure)
            .child(header)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(commit_box)
                    .child(list),
            );
        if expanded {
            root = root.child(self.render_sash(&theme, cx));
        }
        root = root.child(
            div()
                .flex_none()
                .overflow_hidden()
                .border_t_1()
                .border_color(theme.colors.stroke)
                .when(expanded, |el| el.h(u(self.graph_height)))
                .when(!expanded, |el| el.h(u(28.)))
                .child(self.graph.clone()),
        );
        if self.sash.is_some() {
            let entity = cx.entity();
            root = root.child(
                canvas(
                    |_, _, _| {},
                    move |_, _, window, _| {
                        let moving = entity.clone();
                        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                            if phase == gpui::DispatchPhase::Bubble {
                                moving.update(cx, |this, cx| {
                                    this.drag_sash(event.position.y, window, cx)
                                });
                            }
                        });
                        let ending = entity.clone();
                        window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                            if phase == gpui::DispatchPhase::Bubble {
                                ending.update(cx, |this, cx| this.end_sash(cx));
                            }
                        });
                    },
                )
                .absolute()
                .size_0(),
            );
        }
        root.into_any_element()
    }
}
