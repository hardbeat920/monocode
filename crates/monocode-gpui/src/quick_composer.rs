//! QuickComposer.tsx: the Spotlight-style panel opened by the global shortcut
//! and the menu bar. It floats over other apps without activating MonoCode,
//! keeps its draft across a dismiss, and opens its pickers below the toolbar
//! so the prompt never moves.
use crate::tooltip::Tip;
use crate::{
    git_picker::{GitPicker, Mode as GitMode, Picked as GitPicked},
    input::{self, Command, Input},
    pickers::{ModelList, ModelPicked, model_display_name, model_name},
    popover::{Anchor, AnchorSlot, Side, popover},
    theme::*,
};
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, ExternalPaths, FocusHandle, Focusable,
    MouseButton, Render, Role, SharedString, Subscription, Task, Window, WindowHandle, div,
    prelude::*, px,
};
use monocode_core::{
    agent::AccessMode, git::Snapshot, project_appearance::ProjectAppearance, provider::Kind,
};
use std::{collections::HashMap, path::PathBuf};
#[cfg(all(target_os = "macos", not(test)))]
mod hotkey;

pub(crate) const WIDTH: f32 = 680.;
pub(crate) const INITIAL_HEIGHT: f32 = 128.;
/// Down from the top of the screen's work area, like Spotlight.
pub(crate) const TOP_FRACTION: f32 = 0.22;
const MAX_HEIGHT: f32 = 520.;
/// Must match the window's corners so the blur and border share one outline.
const RADIUS: f32 = 16.;
/// Tallest the prompt grows before it scrolls.
const PROMPT_MAX_HEIGHT: f32 = 220.;
/// useQuickPickerMotion: the card's height tween, and the picker's fade-in.
const PICKER_RESIZE: f32 = 0.24;
const PICKER_FADE_DELAY: f32 = 0.04;
const PICKER_FADE: f32 = 0.18;
/// Commands the floating composer offers after a leading `/`.
const COMMANDS: [(Command, &str); 4] = [
    (
        Command::Plan,
        "Create a reviewable implementation plan before changing files.",
    ),
    (
        Command::Operator,
        "Give this thread access to MonoCode sessions, folders, and notes.",
    ),
    (Command::Orchestrator, "Plan and coordinate agent work."),
    (
        Command::Draft,
        "Save this message without starting the agent.",
    ),
];
/// RUNTIME_MODES, with their icons and hints.
const PERMISSIONS: [(AccessMode, &str, &str); 4] = [
    (
        AccessMode::Supervised,
        "lock",
        "Ask before commands and file changes.",
    ),
    (
        AccessMode::AutoAcceptEdits,
        "pencil",
        "Auto-approve edits, ask before other actions.",
    ),
    (
        AccessMode::Auto,
        "sparkles",
        "An AI reviewer can approve or deny actions.",
    ),
    (
        AccessMode::FullAccess,
        "shield",
        "Allow commands, edits, and supported MCP confirmations in non-plan turns without prompts.",
    ),
];

#[derive(Clone)]
pub(crate) struct Launch {
    pub cwd: String,
    pub kind: Kind,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub fast: bool,
    pub access: AccessMode,
    pub prompt: String,
    pub attachments: Vec<PathBuf>,
    pub staged_attachments: Vec<PathBuf>,
    pub new_worktree: bool,
    pub worktree_base: Option<String>,
    pub worktree_cwd: Option<String>,
    pub foreground: bool,
}
/// Pasted images wait here until the launch creates their conversation, which
/// then adopts them into its own folder.
pub(crate) fn paste_staging_dir() -> PathBuf {
    std::env::temp_dir().join("monocode-quick-composer")
}
impl Launch {
    pub(crate) fn choice(
        cwd: String,
        kind: Kind,
        model: Option<String>,
        access: AccessMode,
    ) -> Self {
        Self {
            cwd,
            kind,
            model,
            effort: None,
            fast: false,
            access,
            prompt: String::new(),
            attachments: Vec::new(),
            staged_attachments: Vec::new(),
            new_worktree: false,
            worktree_base: None,
            worktree_cwd: None,
            foreground: false,
        }
    }
}
#[derive(Default)]
pub(crate) struct Controller {
    pub window: Option<WindowHandle<QuickComposer>>,
    pub last: Option<Launch>,
    pub error: Option<String>,
    pub task: Option<Task<()>>,
    pub events: Option<Subscription>,
    config: String,
    #[cfg(all(target_os = "macos", not(test)))]
    registration: Option<hotkey::Registration>,
}
impl Controller {
    pub fn configure(
        &mut self,
        prefs: &crate::settings::Preferences,
    ) -> Option<async_channel::Receiver<()>> {
        let shortcut = match prefs.keybindings.get("App: Quick Composer") {
            Some(Some(v)) => v.clone(),
            Some(None) => String::new(),
            None => "cmd-shift-space".into(),
        };
        let config = format!("{}:{shortcut}", prefs.quick_composer);
        if self.config == config {
            return None;
        }
        self.config = config;
        self.error = None;
        self.task = None;
        #[cfg(all(target_os = "macos", not(test)))]
        {
            self.registration = None;
            if prefs.quick_composer && !shortcut.is_empty() {
                match hotkey::Registration::new(&shortcut) {
                    Ok((registration, rx)) => {
                        self.registration = Some(registration);
                        return Some(rx);
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Picker {
    Project,
    Model,
    Permissions,
    Attachments,
    Commands,
    Workspace,
    Branch,
}

/// quickWorkspace.ts: the working copy a launch starts in.
#[derive(Clone, Default)]
struct Workspace {
    new_worktree: bool,
    base: Option<String>,
    /// An existing linked worktree, rather than the project folder.
    tree: Option<String>,
}

pub(crate) struct QuickComposer {
    input: Entity<Input>,
    query: Entity<Input>,
    projects: Vec<String>,
    appearance: HashMap<String, ProjectAppearance>,
    cwd: String,
    kind: Kind,
    model: Option<String>,
    effort: Option<String>,
    fast: bool,
    access: AccessMode,
    workspace: Workspace,
    git: Option<Snapshot>,
    git_settled: bool,
    git_epoch: usize,
    attachments: Vec<PathBuf>,
    staged_attachments: Vec<PathBuf>,
    photos: HashMap<PathBuf, Entity<crate::transcript::attachments::Photo>>,
    attaching: bool,
    dragging: bool,
    picker: Option<Picker>,
    highlight: usize,
    models: Option<Entity<ModelList>>,
    git_picker: Option<Entity<GitPicker>>,
    permissions_focus: FocusHandle,
    error: Option<String>,
    busy: bool,
    shown: bool,
    card: AnchorSlot,
    /// useQuickPickerMotion: the card's painted height, and the picker and
    /// start of the tween that carries it to the newly measured height.
    height: f32,
    motion_picker: Option<Picker>,
    motion: Option<(std::time::Instant, f32)>,
    plus: AnchorSlot,
    effort_track: AnchorSlot,
    effort_drag: bool,
    _subscriptions: Vec<Subscription>,
    picker_events: Option<Subscription>,
}
impl EventEmitter<Launch> for QuickComposer {}
impl QuickComposer {
    pub fn new(
        projects: Vec<String>,
        appearance: HashMap<String, ProjectAppearance>,
        choice: Launch,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            let mut input = Input::new(cx);
            input.set_font_size(16.);
            input.set_line_height(24.);
            input.set_min_height(48.);
            input.set_max_height(PROMPT_MAX_HEIGHT);
            input.enable_inline_commands(cx);
            input
        });
        let query =
            cx.new(|cx| input::Input::field("Find a project", "Find a project", content(0.35), cx));
        let subscriptions = vec![
            cx.subscribe(&input, |this, _, _: &input::Submitted, cx| {
                this.launch(false, cx)
            }),
            cx.observe(&input, |this, _, cx| this.sync_commands(cx)),
            cx.subscribe(&query, |this, _, _: &input::Changed, cx| {
                this.highlight = 0;
                cx.notify();
            }),
        ];
        let mut this = Self {
            input,
            query,
            projects,
            appearance,
            cwd: String::new(),
            kind: choice.kind,
            model: None,
            effort: None,
            fast: false,
            access: choice.access,
            workspace: Workspace::default(),
            git: None,
            git_settled: false,
            git_epoch: 0,
            attachments: Vec::new(),
            staged_attachments: Vec::new(),
            photos: HashMap::new(),
            attaching: false,
            dragging: false,
            picker: None,
            highlight: 0,
            models: None,
            git_picker: None,
            permissions_focus: cx.focus_handle(),
            error: None,
            busy: false,
            shown: true,
            card: AnchorSlot::default(),
            height: 0.,
            motion_picker: None,
            motion: None,
            plus: AnchorSlot::default(),
            effort_track: AnchorSlot::default(),
            effort_drag: false,
            _subscriptions: subscriptions,
            picker_events: None,
        };
        this.apply_choice(choice, cx);
        this
    }
    /// Each show re-reads projects and defaults, which can change in the
    /// workspace between shows. The draft survives, like Spotlight's query.
    pub fn reopen(
        &mut self,
        projects: Vec<String>,
        appearance: HashMap<String, ProjectAppearance>,
        choice: Launch,
        cx: &mut Context<Self>,
    ) {
        self.projects = projects;
        self.appearance = appearance;
        let keep = self.projects.contains(&self.cwd);
        let cwd = self.cwd.clone();
        self.apply_choice(choice, cx);
        if keep && cwd != self.cwd {
            self.set_project(cwd, cx);
        }
        self.close_picker_state(cx);
        self.error = None;
        self.shown = true;
        cx.notify();
    }
    pub fn shown(&self) -> bool {
        self.shown
    }
    fn apply_choice(&mut self, choice: Launch, cx: &mut Context<Self>) {
        self.kind = choice.kind;
        self.model = choice.model;
        self.effort = choice
            .effort
            .or_else(|| crate::conversation::preferred_effort(self.kind.id(), cx));
        self.fast = choice.fast;
        if self.cwd != choice.cwd {
            self.cwd = choice.cwd;
            self.workspace = Workspace::default();
            self.load_git(cx);
        } else if self.git.is_none() {
            self.load_git(cx);
        }
        self.update_placeholder(cx);
    }
    fn update_placeholder(&mut self, cx: &mut Context<Self>) {
        let text = if self.cwd.is_empty() {
            "Open a project in MonoCode first".to_owned()
        } else {
            format!(
                "Start a {} session in {}…",
                self.kind.label(),
                crate::workspace::project_name(&self.cwd)
            )
        };
        self.input
            .update(cx, |input, cx| input.set_placeholder(text, cx));
    }
    fn set_project(&mut self, path: String, cx: &mut Context<Self>) {
        self.cwd = path;
        self.workspace = Workspace::default();
        let p = cx.global::<crate::settings::Preferences>();
        let kind = p
            .project_providers
            .get(&self.cwd)
            .and_then(|v| v.provider.as_deref())
            .or(p.default_provider.as_deref())
            .map(Kind::from_id)
            .unwrap_or(self.kind);
        let model = p
            .project_providers
            .get(&self.cwd)
            .and_then(|v| v.models.get(kind.id()))
            .or(p.default_models.get(kind.id()))
            .cloned();
        self.set_model(kind, model, cx);
        self.load_git(cx);
        self.update_placeholder(cx);
    }
    fn set_model(&mut self, kind: Kind, model: Option<String>, cx: &mut Context<Self>) {
        if kind != self.kind {
            self.effort = crate::conversation::preferred_effort(kind.id(), cx);
            self.fast = false;
        }
        self.kind = kind;
        self.model = model;
        if !self.attachments_supported() {
            self.error = None;
        }
        self.update_placeholder(cx);
    }
    fn load_git(&mut self, cx: &mut Context<Self>) {
        self.git_epoch += 1;
        let epoch = self.git_epoch;
        let cwd = self
            .workspace
            .tree
            .clone()
            .unwrap_or_else(|| self.cwd.clone());
        self.git = crate::git_cache::get(&cwd);
        self.git_settled = false;
        if cwd.is_empty() || cfg!(test) {
            self.git_settled = true;
            return;
        }
        let work = cx
            .background_executor()
            .spawn(async move { crate::git_cache::load(std::path::Path::new(&cwd)) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.git_epoch != epoch {
                    return;
                }
                this.git = result.ok();
                this.git_settled = true;
                cx.notify();
            });
        })
        .detach();
    }
    fn attachments_supported(&self) -> bool {
        self.kind != Kind::Fx
    }
    fn prompt(&self, cx: &App) -> String {
        self.input.read(cx).content.trim().to_owned()
    }
    fn leading(&self, cx: &App) -> Option<Command> {
        Command::leading(&self.input.read(cx).content)
    }
    /// The prompt without a leading mode command; Operator is part of it.
    fn message(&self, cx: &App) -> String {
        let prompt = self.prompt(cx);
        match self.leading(cx) {
            Some(Command::Operator) | None => prompt,
            Some(command) => prompt[command.name().len() + 1..].trim().to_owned(),
        }
    }
    fn can_submit(&self, cx: &App) -> bool {
        (!self.message(cx).is_empty() || !self.attachments.is_empty())
            && !self.cwd.is_empty()
            && !self.busy
            && !self.attaching
            && !matches!(self.picker, Some(Picker::Workspace | Picker::Branch))
            && (self.attachments_supported() || self.attachments.is_empty())
    }
    fn launch(&mut self, foreground: bool, cx: &mut Context<Self>) {
        if self.picker == Some(Picker::Commands) {
            self.choose_command(self.highlight, cx);
            return;
        }
        if self.input.read(cx).is_composing() || !self.can_submit(cx) {
            return;
        }
        let new_worktree = self.workspace.new_worktree;
        let base = self
            .workspace
            .base
            .clone()
            .or_else(|| self.git.as_ref().and_then(|g| g.current.clone()))
            .unwrap_or_else(|| "HEAD".into());
        cx.emit(Launch {
            cwd: self.cwd.clone(),
            kind: self.kind,
            model: self.model.clone(),
            effort: self.effort.clone(),
            fast: self.fast,
            access: self.access,
            prompt: self.prompt(cx),
            attachments: std::mem::take(&mut self.attachments),
            staged_attachments: std::mem::take(&mut self.staged_attachments),
            new_worktree,
            worktree_base: new_worktree.then_some(base),
            worktree_cwd: self
                .workspace
                .tree
                .clone()
                .filter(|_| !new_worktree)
                .filter(|tree| {
                    monocode_core::files::path_key(tree)
                        != monocode_core::files::path_key(&self.cwd)
                }),
            foreground,
        });
        self.input.update(cx, |input, cx| {
            input.take(cx);
        });
        self.close_picker_state(cx);
        self.error = None;
        cx.notify();
    }
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.input.focus_handle(cx), cx);
        window.activate_window();
    }
    pub fn dismiss(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_picker_state(cx);
        self.shown = false;
        #[cfg(all(target_os = "macos", not(test)))]
        crate::traffic_lights::hide(window);
        #[cfg(any(not(target_os = "macos"), test))]
        window.remove_window();
        cx.notify();
    }
    fn close_picker_state(&mut self, _cx: &mut Context<Self>) {
        self.picker = None;
        self.models = None;
        self.git_picker = None;
        self.picker_events = None;
        self.effort_drag = false;
    }
    fn close_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_picker_state(cx);
        window.focus(&self.input.focus_handle(cx), cx);
        cx.notify();
    }
    fn open(&mut self, picker: Picker, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker == Some(picker) {
            self.close_picker(window, cx);
            return;
        }
        self.close_picker_state(cx);
        self.picker = Some(picker);
        match picker {
            Picker::Project => {
                self.query.update(cx, |q, cx| {
                    q.take(cx);
                });
                self.highlight = self
                    .projects
                    .iter()
                    .position(|p| *p == self.cwd)
                    .unwrap_or(0);
                window.focus(&self.query.focus_handle(cx), cx);
            }
            Picker::Model => {
                let profile = crate::accounts::get(cx)
                    .map(|a| a.read(cx).selected(self.kind, &self.cwd))
                    .unwrap_or_default();
                let hidden = cx
                    .global::<crate::settings::Preferences>()
                    .hidden_for(&self.cwd);
                let list = cx.new(|cx| {
                    ModelList::new_for(self.kind, self.model.clone(), &self.cwd, profile, cx)
                        .hide_providers(hidden)
                        .quick(cx)
                });
                self.picker_events =
                    Some(
                        cx.subscribe_in(&list, window, |this, _, picked: &ModelPicked, _, cx| {
                            // As Tauri's selector: picking keeps it open, so the
                            // effort beneath can follow the new model.
                            this.set_model(picked.0, picked.1.clone(), cx);
                            if let Some(list) = this.models.clone() {
                                let (kind, model) = (this.kind, this.model.clone());
                                list.update(cx, |list, cx| list.set_current(kind, model, cx));
                            }
                            cx.notify();
                        }),
                    );
                window.focus(&list.read(cx).search_focus(cx), cx);
                self.models = Some(list);
            }
            Picker::Permissions => {
                self.highlight = PERMISSIONS
                    .iter()
                    .position(|(mode, ..)| *mode == self.access)
                    .unwrap_or(0);
                window.focus(&self.permissions_focus, cx);
            }
            Picker::Workspace | Picker::Branch => {
                let new = self.workspace.new_worktree;
                let (mode, selected) = match picker {
                    Picker::Workspace => (GitMode::Worktree, self.workspace.tree.clone()),
                    _ if new => (GitMode::Base, self.workspace.base.clone()),
                    _ => (GitMode::Branch, None),
                };
                let cwd = self
                    .workspace
                    .tree
                    .clone()
                    .unwrap_or_else(|| self.cwd.clone());
                let picker_view = cx.new(|cx| {
                    let picker = GitPicker::new(cwd.into(), mode, selected, cx);
                    if mode == GitMode::Worktree {
                        picker.existing_worktrees()
                    } else {
                        picker
                    }
                });
                self.picker_events = Some(cx.subscribe_in(
                    &picker_view,
                    window,
                    |this, _, picked: &GitPicked, window, cx| {
                        match picked {
                            GitPicked::Changed => this.load_git(cx),
                            GitPicked::Base(base) => this.workspace.base = Some(base.clone()),
                            GitPicked::Worktree(path) => {
                                this.workspace = Workspace {
                                    tree: Some(path.clone()),
                                    ..Default::default()
                                };
                                this.load_git(cx);
                            }
                            GitPicked::Dismissed => {}
                        }
                        this.close_picker(window, cx);
                    },
                ));
                window.focus(&picker_view.read(cx).focus(cx), cx);
                self.git_picker = Some(picker_view);
            }
            Picker::Attachments | Picker::Commands => {}
        }
        cx.notify();
    }
    /// A leading `/word` opens the command list; Operator only activates at
    /// the start of a prompt, so every mode does.
    fn sync_commands(&mut self, cx: &mut Context<Self>) {
        let text = self.input.read(cx).content.clone();
        let token = text
            .strip_prefix('/')
            .filter(|rest| !rest.contains(char::is_whitespace));
        let busy = matches!(self.picker, Some(Picker::Workspace | Picker::Branch));
        if token.is_some() && !self.busy && !busy {
            if self.picker != Some(Picker::Commands) {
                self.close_picker_state(cx);
                self.picker = Some(Picker::Commands);
                self.highlight = 0;
            }
            let count = self.command_options(cx).len();
            self.highlight = self.highlight.min(count.saturating_sub(1));
        } else if self.picker == Some(Picker::Commands) {
            self.picker = None;
        }
        cx.notify();
    }
    fn command_options(&self, cx: &App) -> Vec<(Command, &'static str)> {
        let content = self.input.read(cx).content.clone();
        let query = content.strip_prefix('/').unwrap_or("").to_lowercase();
        COMMANDS
            .into_iter()
            .filter(|(command, _)| command.name().contains(&query))
            .collect()
    }
    fn choose_command(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some((command, _)) = self.command_options(cx).get(ix).copied() else {
            return;
        };
        self.input.update(cx, |input, cx| {
            input.set_text(format!("/{} ", command.name()), cx)
        });
        self.picker = None;
        cx.notify();
    }
    fn project_options(&self, cx: &App) -> Vec<String> {
        let query = self.query.read(cx).content.trim().to_lowercase();
        self.projects
            .iter()
            .filter(|path| {
                query.is_empty()
                    || format!("{}\n{path}", self.project_label(path))
                        .to_lowercase()
                        .contains(&query)
            })
            .cloned()
            .collect()
    }
    fn project_label(&self, path: &str) -> String {
        self.appearance
            .get(path)
            .map_or_else(|| crate::workspace::project_name(path), |a| a.label(path))
    }
    fn step(&mut self, direction: isize, cx: &mut Context<Self>) {
        let count = match self.picker {
            Some(Picker::Commands) => self.command_options(cx).len(),
            Some(Picker::Project) => self.project_options(cx).len(),
            Some(Picker::Permissions) => PERMISSIONS.len(),
            _ => return,
        };
        if count > 0 {
            self.highlight =
                (self.highlight as isize + direction).rem_euclid(count as isize) as usize;
            cx.notify();
        }
    }
    fn choose_project(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.project_options(cx).into_iter().nth(ix) {
            self.set_project(path, cx);
        }
        self.close_picker(window, cx);
    }
    fn choose_permission(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((mode, ..)) = PERMISSIONS.get(ix) {
            self.access = *mode;
        }
        self.close_picker(window, cx);
    }
    fn choose_workspace(&mut self, new: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace = Workspace {
            new_worktree: new,
            base: None,
            tree: None,
        };
        self.load_git(cx);
        self.close_picker(window, cx);
    }

    // useQuickAttachments: files, pasted images and screenshots.
    fn add_attachments(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if !self.attachments_supported() {
            self.error = Some("This provider does not support attachments".into());
            cx.notify();
            return;
        }
        let mut all = self.attachments.clone();
        for path in paths {
            if !all.contains(&path) {
                all.push(path);
            }
        }
        match monocode_core::composer::validate_attachments(&all) {
            Ok(()) => {
                self.attachments = all;
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }
    fn paste(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(clipboard) = cx.read_from_clipboard() else {
            return false;
        };
        let mut paths = Vec::new();
        let mut images = Vec::new();
        for entry in clipboard.entries() {
            match entry {
                gpui::ClipboardEntry::ExternalPaths(p) => paths.extend(p.paths().to_vec()),
                gpui::ClipboardEntry::Image(image) => images.push(image.clone()),
                _ => {}
            }
        }
        if paths.is_empty() && images.is_empty() {
            return false;
        }
        if !self.attachments_supported() {
            return false;
        }
        for image in images {
            let dir = paste_staging_dir();
            let path = dir.join(format!(
                "pasted-{}.{}",
                monocode_core::mono::new_id(),
                image.format().extension()
            ));
            match std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, image.bytes())) {
                Ok(()) => {
                    self.staged_attachments.push(path.clone());
                    paths.push(path);
                }
                Err(error) => self.error = Some(format!("Could not paste the image: {error}")),
            }
        }
        self.add_attachments(paths, cx);
        self.staged_attachments
            .retain(|path| self.attachments.contains(path));
        true
    }
    fn choose_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_picker(window, cx);
        let prompt = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Attach".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = prompt.await {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.add_attachments(paths, cx);
                    this.focus(window, cx);
                });
            }
        })
        .detach();
    }
    /// User-initiated region or window capture. The panel steps aside while
    /// the system overlay is up; cancelling leaves the draft intact.
    fn take_screenshot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_picker_state(cx);
        self.attaching = true;
        #[cfg(all(target_os = "macos", not(test)))]
        crate::traffic_lights::hide(window);
        let path = std::env::temp_dir()
            .join("monocode-quick-composer")
            .join(format!("screenshot-{}.png", monocode_core::mono::new_id()));
        let work = cx.background_executor().spawn({
            let path = path.clone();
            async move {
                std::fs::create_dir_all(path.parent().unwrap_or(&path))
                    .map_err(|e| e.to_string())?;
                // Let WindowServer remove the panel before the overlay appears.
                std::thread::sleep(std::time::Duration::from_millis(150));
                let output = std::process::Command::new("/usr/sbin/screencapture")
                    .args(["-i", "-x", "-t", "png"])
                    .arg(&path)
                    .output()
                    .map_err(|e| format!("Could not take a screenshot: {e}"))?;
                if path.is_file() {
                    return Ok(Some(path));
                }
                let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                if !output.status.success() && !error.is_empty() {
                    return Err(format!("Could not take a screenshot: {error}"));
                }
                Ok(None)
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.attaching = false;
                match result {
                    Ok(Some(path)) => {
                        this.staged_attachments.push(path.clone());
                        this.add_attachments(vec![path], cx);
                        this.staged_attachments
                            .retain(|path| this.attachments.contains(path));
                    }
                    Ok(None) => {}
                    Err(error) => this.error = Some(error),
                }
                this.focus(window, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Returns the card's height this frame, the picker's fade, and whether
    /// the card is mid-tween and must be held to that height.
    fn picker_motion(&mut self, window: &mut Window, cx: &App) -> (f32, f32, bool) {
        // The probe sits inside the 1px border on each side.
        let natural = self.card.get().map(|b| f32::from(b.size.height) + 2.);
        if self.motion_picker != self.picker {
            self.motion_picker = self.picker;
            self.motion = (!cx.reduce_motion() && self.height > 0.)
                .then(|| (std::time::Instant::now(), self.height));
        }
        let (Some((start, from)), Some(to)) = (self.motion, natural) else {
            self.motion = None;
            self.height = natural.unwrap_or(0.);
            return (self.height, 1., false);
        };
        let elapsed = start.elapsed().as_secs_f32();
        if elapsed >= PICKER_RESIZE {
            self.motion = None;
            self.height = to;
            return (to, 1., false);
        }
        window.request_animation_frame();
        let grow = crate::reveal::bezier(elapsed / PICKER_RESIZE, 0.22, 1., 0.36, 1.);
        // `ease-out`, held at zero through the 40ms delay.
        let fade = crate::reveal::bezier(
            ((elapsed - PICKER_FADE_DELAY) / PICKER_FADE).clamp(0., 1.),
            0.,
            0.,
            0.58,
            1.,
        );
        self.height = from + (to - from) * grow;
        (self.height, fade, true)
    }

    fn fit(&mut self, height: f32, window: &mut Window, cx: &App) {
        if height <= 0. {
            return;
        }
        let height = height.clamp(1., MAX_HEIGHT);
        #[cfg(all(target_os = "macos", not(test)))]
        crate::traffic_lights::fit_height(window, cx, height as f64);
        #[cfg(any(not(target_os = "macos"), test))]
        let _ = cx;
        #[cfg(any(not(target_os = "macos"), test))]
        {
            let size = window.bounds().size;
            if (f32::from(size.height) - height).abs() > 0.5 {
                window.resize(gpui::size(size.width, px(height)));
            }
        }
    }
}
impl Focusable for QuickComposer {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

fn kbd(text: &'static str) -> impl IntoElement {
    div()
        .px_1()
        .rounded(px(4.))
        .border_1()
        .border_color(content(0.12))
        .text_size(px(10.))
        .text_color(content(0.55))
        .child(text)
}
/// The toolbar's and header's pill buttons.
fn pill(
    id: &'static str,
    label: impl Into<SharedString>,
    open: bool,
    ink: f32,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.into())
        .aria_expanded(open)
        .cursor_pointer()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(6.))
        .text_size(px(12.))
        .when(open, |d| d.bg(content(0.12)).text_color(content(1.)))
        .when(!open, |d| {
            d.text_color(content(ink))
                .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}
/// Tailwind's amber-400, which marks full access and fast mode.
fn amber(alpha: f32) -> gpui::Hsla {
    gpui::Hsla::from(gpui::rgb(0xfbbf24)).opacity(alpha)
}
fn effort_name(value: &str) -> String {
    crate::conversation::effort_name(value)
}
impl QuickComposer {
    fn header(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let new = self.workspace.new_worktree;
        let linked = self.workspace.tree.is_some();
        let enabled = !self.busy
            && !matches!(self.picker, Some(p) if !matches!(p, Picker::Workspace | Picker::Branch))
            && self.git.as_ref().is_some_and(|g| g.current.is_some());
        let label = if new {
            "New worktree"
        } else {
            "Current checkout"
        };
        let current = self.git.as_ref().and_then(|g| g.current.clone());
        let base = self
            .workspace
            .base
            .clone()
            .or(current.clone())
            .unwrap_or_else(|| "HEAD".into());
        let branch = if new {
            format!("From {base}")
        } else {
            current.unwrap_or_else(|| {
                if self.git_settled {
                    "No repo".into()
                } else {
                    "Loading…".into()
                }
            })
        };
        let loading = !self.git_settled && self.git.is_none();
        let tree = linked
            && self.git.as_ref().is_some_and(|g| {
                g.worktrees
                    .iter()
                    .any(|w| !w.main && Some(w.path.as_str()) == self.workspace.tree.as_deref())
            });
        let project = if self.cwd.is_empty() {
            "No project".to_owned()
        } else {
            self.project_label(&self.cwd)
        };
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_2()
            .px(px(20.))
            .pt_3()
            .pr_8()
            .child(
                pill(
                    "quick-workspace",
                    format!("Workspace {label}"),
                    self.picker == Some(Picker::Workspace),
                    0.55,
                )
                .h(px(24.))
                .ml(px(-6.))
                .px(px(6.))
                .max_w(px(192.))
                .when(!enabled, |d| d.opacity(0.4))
                .child(
                    icon(
                        if new || linked {
                            "folder-tree"
                        } else {
                            "folder"
                        },
                        14.,
                    )
                    .text_color(content(0.55)),
                )
                .child(div().truncate().child(label))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if enabled {
                        this.open(Picker::Workspace, window, cx)
                    }
                })),
            )
            .child(
                pill(
                    "quick-branch",
                    if new {
                        format!("Create worktree from {base}")
                    } else {
                        "Choose branch".into()
                    },
                    self.picker == Some(Picker::Branch),
                    0.55,
                )
                .h(px(24.))
                .ml(px(-6.))
                .px(px(6.))
                .max_w(px(256.))
                .when(!enabled, |d| d.opacity(0.4))
                .child(
                    icon(if tree { "folder-tree" } else { "git-branch" }, 14.)
                        .text_color(content(0.55)),
                )
                .child(div().relative().min_w_0().truncate().map(|d| {
                    if loading {
                        // Reserve the line box while the current branch loads.
                        d.child(div().invisible().child("main")).child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .top(px(5.))
                                .h(px(6.))
                                .rounded_full()
                                .bg(content(0.3)),
                        )
                    } else {
                        d.child(branch)
                    }
                }))
                .when(tree, |d| {
                    d.child(
                        div()
                            .flex_shrink_0()
                            .px_1()
                            .rounded(px(4.))
                            .bg(content(0.08))
                            .text_size(px(10.))
                            .text_color(content(0.45))
                            .child("Worktree"),
                    )
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    if enabled {
                        this.open(Picker::Branch, window, cx)
                    }
                })),
            )
            .child(
                pill(
                    "quick-project",
                    "Project (⌘P)",
                    self.picker == Some(Picker::Project),
                    0.55,
                )
                .ml_auto()
                .max_w(px(WIDTH * 0.4))
                .h(px(24.))
                .px(px(6.))
                .when(self.projects.is_empty(), |d| d.opacity(0.5))
                .when(!self.cwd.is_empty(), |d| {
                    d.child(crate::project_settings::project_icon(
                        &self.appearance.get(&self.cwd).cloned().unwrap_or_default(),
                        &self.cwd,
                        12.,
                    ))
                })
                .child(div().truncate().child(project))
                .child(icon("chevron-down", 12.).text_color(content(0.4)))
                .on_click(cx.listener(|this, _, window, cx| {
                    if !this.projects.is_empty() {
                        this.open(Picker::Project, window, cx)
                    }
                })),
            )
    }
    fn attachment_chips(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.attachments.is_empty() {
            return None;
        }
        let attachments = &self.attachments;
        self.photos.retain(|path, _| attachments.contains(path));
        for path in &self.attachments {
            if let Some(mime) = monocode_core::composer::image_mime(path)
                && !self.photos.contains_key(path)
            {
                let origin = crate::transcript::attachments::Origin::Path(path.clone());
                let photo = cx
                    .new(|cx| crate::transcript::attachments::Photo::new(origin, mime.into(), cx));
                self.photos.insert(path.clone(), photo);
            }
        }
        let busy = self.busy;
        Some(
            div()
                .id("quick-attachments")
                .role(Role::Group)
                .aria_label("Attachments")
                .flex_shrink_0()
                .max_h(px(112.))
                .overflow_y_scroll()
                .flex()
                .flex_wrap()
                .gap(px(6.))
                .px(px(20.))
                .pt_4()
                .pb_1()
                .children(self.attachments.iter().enumerate().map(|(ix, path)| {
                    let label = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let photo = self.photos.get(path).cloned();
                    let remove = div()
                        .id(("quick-remove-attachment", ix))
                        .role(Role::Button)
                        .aria_label(format!("Remove {label}"))
                        .tip(format!("Remove {label}"))
                        .cursor_pointer()
                        .flex_shrink_0()
                        .size(px(16.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(content(0.6))
                        .hover(|s| s.bg(content(0.15)).text_color(content(1.)))
                        .child(icon("close", 10.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if ix < this.attachments.len() {
                                let path = this.attachments.remove(ix);
                                this.staged_attachments.retain(|staged| staged != &path);
                                cx.notify();
                            }
                        }));
                    div()
                        .id(("quick-attachment", ix))
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .pl_1()
                        .pr_1()
                        .rounded(px(6.))
                        .bg(content(0.06))
                        .text_size(px(11.))
                        .child(match photo {
                            Some(photo) => div()
                                .size(px(20.))
                                .rounded(px(4.))
                                .overflow_hidden()
                                .child(photo)
                                .into_any_element(),
                            None => crate::file_icons::file_type_icon(&label, false, false, 14.),
                        })
                        .child(div().max_w(px(190.)).truncate().child(label))
                        .when(!busy, |d| d.child(remove))
                }))
                .into_any_element(),
        )
    }
    fn toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let supported = self.attachments_supported();
        let name = model_display_name(
            self.kind,
            self.model.as_deref().unwrap_or(""),
            &model_name(self.kind, self.model.as_deref()),
        );
        let can_submit = self.can_submit(cx);
        let draft = self.leading(cx) == Some(Command::Draft);
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .border_t_1()
            .border_color(content(0.08))
            .px_3()
            .py_2()
            .child(
                pill(
                    "quick-attach",
                    "Add attachment",
                    self.picker == Some(Picker::Attachments),
                    0.7,
                )
                .relative()
                .size(px(26.))
                .flex_shrink_0()
                .justify_center()
                .when(!supported || self.attaching || self.busy, |d| {
                    d.opacity(0.4)
                })
                .child(self.plus.probe())
                .child(icon("plus", 14.).text_color(content(0.7)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    if supported && !this.attaching && !this.busy {
                        this.open(Picker::Attachments, window, cx)
                    }
                })),
            )
            .child(
                pill(
                    "quick-model",
                    "Model (⌘.)",
                    self.picker == Some(Picker::Model),
                    0.7,
                )
                .max_w(px(WIDTH * 0.4))
                .px_2()
                .py_1()
                .child(provider_icon(self.kind.id(), 14.))
                .child(div().truncate().child(name))
                .child(icon("chevron-down", 12.).text_color(content(0.4)))
                .on_click(cx.listener(|this, _, window, cx| this.open(Picker::Model, window, cx))),
            )
            .when(self.kind != Kind::Fx, |d| {
                let permission = PERMISSIONS
                    .iter()
                    .find(|(mode, ..)| *mode == self.access)
                    .map_or("lock", |(_, icon, _)| *icon);
                d.child(
                    pill(
                        "quick-permissions",
                        "Permissions",
                        self.picker == Some(Picker::Permissions),
                        0.7,
                    )
                    .px_2()
                    .py_1()
                    .child(icon(permission, 14.).text_color(
                        if self.access == AccessMode::FullAccess {
                            amber(0.9)
                        } else {
                            content(0.7)
                        },
                    ))
                    .child(div().truncate().child(self.access.label()))
                    .child(icon("chevron-down", 12.).text_color(content(0.4)))
                    .on_click(
                        cx.listener(|this, _, window, cx| {
                            this.open(Picker::Permissions, window, cx)
                        }),
                    ),
                )
            })
            .child(
                div()
                    .ml_auto()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .text_size(px(11.))
                    .text_color(content(0.45))
                    .map(|d| {
                        if self.attaching {
                            d.child(
                                div()
                                    .id("quick-attaching")
                                    .role(Role::Status)
                                    .child("Adding attachment…"),
                            )
                        } else if let Some(error) = self.error.clone() {
                            d.child(
                                div()
                                    .max_w(px(288.))
                                    .truncate()
                                    .text_color(gpui::rgb(0xf87171))
                                    .child(error),
                            )
                        } else {
                            d.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(kbd("↵"))
                                    .child("start"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(kbd("⌘↵"))
                                    .child("start and open"),
                            )
                        }
                    })
                    .child(
                        div()
                            .id("quick-send")
                            .role(Role::Button)
                            .aria_label(if draft { "Save draft" } else { "Start" })
                            .px(px(10.))
                            .py_1()
                            .rounded(px(6.))
                            .bg(accent_color())
                            .text_size(px(12.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(white(1.))
                            .when(!can_submit, |d| d.opacity(0.4))
                            .when(can_submit, |d| d.cursor_pointer())
                            .child(if draft { "Save draft" } else { "Start" })
                            .on_click(cx.listener(|this, _, _, cx| this.launch(false, cx))),
                    ),
            )
    }
    fn option_row(
        &self,
        id: (&'static str, usize),
        active: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let ix = id.1;
        div()
            .id(id)
            .role(Role::ListBoxOption)
            .aria_selected(active)
            .cursor_pointer()
            .w_full()
            .rounded(px(8.))
            .px_2()
            .py(px(6.))
            .text_size(px(13.))
            .when(active, |d| d.bg(content(0.12)).text_color(content(1.)))
            .when(!active, |d| d.text_color(content(0.75)))
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.highlight = ix;
                    cx.notify();
                }
            }))
    }
    fn commands(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let options = self.command_options(cx);
        div()
            .id("quick-composer-commands")
            .role(Role::ListBox)
            .aria_label("Commands")
            .flex_shrink_0()
            .border_t_1()
            .border_color(content(0.08))
            .p_2()
            .when(options.is_empty(), |d| {
                d.child(
                    div()
                        .px_2()
                        .py_2()
                        .text_size(px(12.))
                        .text_color(content(0.45))
                        .child("No matching commands"),
                )
            })
            .children(
                options
                    .into_iter()
                    .enumerate()
                    .map(|(ix, (command, description))| {
                        let name = command.name();
                        let label = format!("{}{}", name[..1].to_uppercase(), &name[1..]);
                        self.option_row(("quick-command", ix), ix == self.highlight, cx)
                            .aria_label(label.clone())
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap(px(2.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(icon(command.icon(), 14.).text_color(command.color()))
                                    .child(label),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .line_height(px(16.))
                                    .text_color(content(0.5))
                                    .child(description),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.choose_command(ix, cx);
                                window.focus(&this.input.focus_handle(cx), cx);
                            }))
                    }),
            )
            .into_any_element()
    }
    fn permissions(&mut self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("quick-permissions-list")
            .role(Role::ListBox)
            .aria_label("Permissions")
            .track_focus(&self.permissions_focus)
            .flex_shrink_0()
            .border_t_1()
            .border_color(content(0.08))
            .p_2()
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "down" => this.step(1, cx),
                    "up" => this.step(-1, cx),
                    "enter" | "space" => this.choose_permission(this.highlight, window, cx),
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .children(
                PERMISSIONS
                    .into_iter()
                    .enumerate()
                    .map(|(ix, (mode, glyph, hint))| {
                        let selected = mode == self.access;
                        self.option_row(("quick-permission", ix), ix == self.highlight, cx)
                            .aria_label(mode.label())
                            .aria_selected(selected)
                            .px_3()
                            .py_2()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(
                                icon(glyph, 16.).text_color(if mode == AccessMode::FullAccess {
                                    amber(0.9)
                                } else {
                                    content(0.75)
                                }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .child(mode.label()),
                                    )
                                    .child(
                                        div()
                                            .mt(px(2.))
                                            .text_size(px(11.))
                                            .text_color(content(0.45))
                                            .child(hint),
                                    ),
                            )
                            .when(selected, |d| {
                                d.child(icon("check-2", 14.).text_color(accent_color()))
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.choose_permission(ix, window, cx)
                            }))
                    }),
            )
            .into_any_element()
    }
    fn project_list(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let options = self.project_options(cx);
        let focused = self.query.focus_handle(cx).is_focused(window);
        div()
            .id("quick-projects")
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(content(0.08))
            .capture_action(cx.listener(|this, _: &input::Up, _, cx| {
                this.step(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Down, _, cx| {
                this.step(1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Submit, window, cx| {
                this.choose_project(this.highlight, window, cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .py_2()
                    .text_color(content(1.))
                    .child(icon("search", 14.).text_color(content(if focused {
                        0.6
                    } else {
                        0.45
                    })))
                    .child(div().flex_1().min_w_0().child(self.query.clone())),
            )
            .child(
                div()
                    .id("quick-project-list")
                    .role(Role::ListBox)
                    .aria_label("Projects")
                    .max_h(px(256.))
                    .overflow_y_scroll()
                    .px_2()
                    .pb_2()
                    .when(options.is_empty(), |d| {
                        d.child(
                            div()
                                .px_2()
                                .py_2()
                                .text_size(px(12.))
                                .text_color(content(0.45))
                                .child("No matches"),
                        )
                    })
                    .children(options.into_iter().enumerate().map(|(ix, path)| {
                        self.option_row(("quick-project-option", ix), ix == self.highlight, cx)
                            .aria_label(self.project_label(&path))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(crate::project_settings::project_icon(
                                &self.appearance.get(&path).cloned().unwrap_or_default(),
                                &path,
                                12.,
                            ))
                            .child(div().min_w_0().truncate().child(self.project_label(&path)))
                            .child(
                                div()
                                    .ml_auto()
                                    .min_w_0()
                                    .pl_3()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(content(0.4))
                                    .child(crate::pickers::pretty_parent(&path)),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.choose_project(ix, window, cx)
                            }))
                    })),
            )
            .into_any_element()
    }
    fn workspace_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let new = self.workspace.new_worktree;
        let current = !new && self.workspace.tree.is_none();
        let row = |id: &'static str, glyph: &'static str, label: &'static str, checked: bool| {
            div()
                .id(id)
                .role(Role::MenuItemRadio)
                .aria_label(label)
                .aria_selected(checked)
                .cursor_pointer()
                .h(px(32.))
                .px_2()
                .rounded(px(8.))
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(13.))
                .text_color(content(0.85))
                .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                .child(icon(glyph, 14.).text_color(content(0.6)))
                .child(div().flex_1().child(label))
                .when(checked, |d| {
                    d.child(icon("check-2", 14.).text_color(accent_color()))
                })
        };
        div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(content(0.08))
            .child(
                div()
                    .p_2()
                    .child(
                        row(
                            "quick-workspace-current",
                            "folder",
                            "Current checkout",
                            current,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.choose_workspace(false, window, cx)
                        })),
                    )
                    .child(
                        row("quick-workspace-new", "folder-tree", "New worktree", new).on_click(
                            cx.listener(|this, _, window, cx| {
                                this.choose_workspace(true, window, cx)
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .px_4()
                    .pt_1()
                    .pb_1()
                    .text_size(px(11.))
                    .text_color(content(0.45))
                    .child("Existing worktrees"),
            )
            .children(
                self.git_picker
                    .clone()
                    .map(|picker| div().h(px(200.)).flex().flex_col().child(picker)),
            )
            .into_any_element()
    }
    /// QuickModelSelector.tsx's footer: fast mode, the effort, and a reset.
    fn effort(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let model = self
            .models
            .as_ref()?
            .read(cx)
            .catalog_model(self.kind, self.model.as_deref())?
            .clone();
        if model.efforts.is_empty() {
            return None;
        }
        let count = model.efforts.len();
        let selected = self
            .effort
            .clone()
            .filter(|e| model.efforts.contains(e))
            .or(model.default_effort.clone())
            .unwrap_or_else(|| model.efforts[0].clone());
        let index = model
            .efforts
            .iter()
            .position(|e| *e == selected)
            .unwrap_or(0);
        let fraction = index as f32 / (count.max(2) - 1) as f32;
        let fast = self.fast;
        let efforts = model.efforts.clone();
        let track = self.effort_track.clone();
        let pick = move |this: &mut Self, x: gpui::Pixels, cx: &mut Context<Self>| {
            let Some(bounds) = track.get() else {
                return;
            };
            let inset = px(10.);
            let width = (bounds.size.width - inset * 2.).max(px(1.));
            let ratio = ((x - bounds.origin.x - inset) / width).clamp(0., 1.);
            let ix = (ratio * (efforts.len() - 1) as f32).round() as usize;
            if let Some(effort) = efforts.get(ix) {
                this.effort = Some(effort.clone());
                cx.notify();
            }
        };
        let pick_down = pick.clone();
        let effort_label: SharedString = effort_name(&selected).into();
        Some(
            div()
                .flex_shrink_0()
                .border_t_1()
                .border_color(content(0.08))
                .px_4()
                .pt_2()
                .pb_3()
                .child(
                    div()
                        .mb_1()
                        .flex()
                        .items_center()
                        .child(div().size(px(28.)).map(|d| {
                            if !model.fast {
                                return d;
                            }
                            d.child(
                                div()
                                    .id("quick-fast")
                                    .role(Role::Button)
                                    .aria_label("Fast mode")
                                    .aria_toggled(if fast {
                                        gpui::Toggled::True
                                    } else {
                                        gpui::Toggled::False
                                    })
                                    .cursor_pointer()
                                    .size(px(28.))
                                    .rounded(px(6.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(fast, |d| d.bg(amber(0.2)).hover(|s| s.bg(amber(0.3))))
                                    .when(!fast, |d| d.hover(|s| s.bg(content(0.08))))
                                    .child(icon("zap", 16.).text_color(if fast {
                                        amber(1.)
                                    } else {
                                        content(0.4)
                                    }))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.fast = !this.fast;
                                        cx.notify();
                                    })),
                            )
                        }))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .justify_center()
                                .text_size(px(13.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(accent_color())
                                .child(effort_label.clone()),
                        )
                        .child(
                            div()
                                .id("quick-effort-reset")
                                .role(Role::Button)
                                .aria_label("Reset to saved defaults")
                                .tip("Reset to saved defaults")
                                .group("quick-effort-reset")
                                .cursor_pointer()
                                .size(px(28.))
                                .rounded(px(6.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|s| s.bg(content(0.08)))
                                .child(
                                    icon("rotate-ccw", 16.)
                                        .text_color(content(0.4))
                                        .group_hover("quick-effort-reset", |s| {
                                            s.text_color(content(1.))
                                        }),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.effort =
                                        crate::conversation::preferred_effort(this.kind.id(), cx);
                                    this.fast = false;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .id("quick-effort")
                        .role(Role::Slider)
                        .aria_label(if self.kind == Kind::Codex {
                            "Reasoning"
                        } else {
                            "Effort"
                        })
                        .aria_value(effort_label)
                        .relative()
                        .h(px(24.))
                        .cursor_pointer()
                        .child(self.effort_track.probe())
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .top(px(6.))
                                .h(px(12.))
                                .rounded_full()
                                .overflow_hidden()
                                .bg(content(0.15))
                                .child(
                                    div()
                                        .h_full()
                                        .w(gpui::relative(fraction))
                                        .bg(accent_color()),
                                ),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(10.))
                                .right(px(10.))
                                .top(px(12.))
                                .children((0..count).map(|ix| {
                                    div()
                                        .absolute()
                                        .left(gpui::relative(ix as f32 / (count.max(2) - 1) as f32))
                                        .ml(px(-2.))
                                        .top(px(-2.))
                                        .size(px(4.))
                                        .rounded_full()
                                        .bg(if ix <= index {
                                            white(0.45)
                                        } else {
                                            content(0.25)
                                        })
                                })),
                        )
                        .child(
                            div()
                                .absolute()
                                .left(px(10.))
                                .right(px(10.))
                                .top(px(2.))
                                .child(
                                    div()
                                        .absolute()
                                        .left(gpui::relative(fraction))
                                        .ml(px(-10.))
                                        .size(px(20.))
                                        .rounded_full()
                                        .border_1()
                                        .border_color(black(0.05))
                                        .bg(white(1.))
                                        .shadow(vec![gpui::BoxShadow {
                                            color: black(0.2),
                                            offset: gpui::point(px(0.), px(1.)),
                                            blur_radius: px(4.),
                                            spread_radius: px(0.),
                                            inset: false,
                                        }]),
                                ),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                                this.effort_drag = true;
                                pick_down(this, event.position.x, cx);
                            }),
                        ),
                )
                .on_mouse_move(
                    cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
                        if this.effort_drag {
                            if event.pressed_button == Some(MouseButton::Left) {
                                pick(this, event.position.x, cx);
                            } else {
                                this.effort_drag = false;
                            }
                        }
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, _| this.effort_drag = false),
                )
                .into_any_element(),
        )
    }
}
impl Render for QuickComposer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (height, fade, held) = self.picker_motion(window, cx);
        self.fit(height, window, cx);
        if !cx.has_active_drag() {
            self.dragging = false;
        }
        let picker = self.picker;
        let warning = !self.attachments.is_empty() && !self.attachments_supported();
        let fill = if is_light() {
            floating_fill().opacity(0.8)
        } else {
            gpui::rgba(0x1717178c).into()
        };
        let mut root = div()
            .id("quick-composer")
            .key_context("QuickComposer")
            .w_full()
            .text_color(content(1.))
            .font_family(crate::fonts::UI)
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                let m = &event.keystroke.modifiers;
                if key == "escape" {
                    if this.picker.is_some() {
                        this.close_picker(window, cx);
                    } else {
                        this.dismiss(window, cx);
                    }
                    cx.stop_propagation();
                } else if key == "enter" && m.platform {
                    this.launch(true, cx);
                    cx.stop_propagation();
                } else if key == "p" && m.platform {
                    this.open(Picker::Project, window, cx);
                    cx.stop_propagation();
                } else if key == "." && m.platform {
                    this.open(Picker::Model, window, cx);
                    cx.stop_propagation();
                } else if key == "tab" && this.picker == Some(Picker::Commands) {
                    this.choose_command(this.highlight, cx);
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &input::Paste, _, cx| {
                if this.paste(cx) {
                    cx.stop_propagation();
                }
            }))
            .on_drag_move::<ExternalPaths>(cx.listener(
                |this, event: &gpui::DragMoveEvent<ExternalPaths>, _, cx| {
                    let over = event.bounds.contains(&event.event.position);
                    if this.dragging != over {
                        this.dragging = over;
                        cx.notify();
                    }
                },
            ))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.dragging = false;
                this.add_attachments(paths.paths().to_vec(), cx);
                this.focus(window, cx);
            }))
            .child(
                // The window is exactly this card, so the native shadow and
                // blur follow its rounded outline.
                div()
                    .relative()
                    .w_full()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .rounded(px(RADIUS))
                    .border_1()
                    .border_color(content(0.1))
                    .bg(fill)
                    // Mid-tween the card clips its full-height contents, so the
                    // lists are revealed rather than squeezed.
                    .when(held, |d| d.h(px(height)))
                    .child(
                div()
                    .relative()
                    .w_full()
                    .flex()
                    .flex_col()
                    .flex_shrink_0()
                    .child(self.card.layout_probe())
                    .when(picker == Some(Picker::Commands), |d| {
                        d.capture_action(cx.listener(|this, _: &input::Up, _, cx| {
                            this.step(-1, cx);
                            cx.stop_propagation();
                        }))
                        .capture_action(cx.listener(|this, _: &input::Down, _, cx| {
                            this.step(1, cx);
                            cx.stop_propagation();
                        }))
                    })
                    .child(
                        div()
                            .id("quick-drag")
                            .group("quick-drag")
                            .absolute()
                            .top_0()
                            .left_0()
                            .right_0()
                            .h(px(12.))
                            .flex()
                            .justify_center()
                            .pt_1()
                            .cursor_grab()
                            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                                cx.stop_propagation();
                                window.start_window_move();
                            })
                            .child(
                                div()
                                    .w(px(24.))
                                    .h(px(2.))
                                    .rounded_full()
                                    .bg(content(0.15))
                                    .group_hover("quick-drag", |s| s.bg(content(0.35))),
                            ),
                    )
                    .child(
                        div()
                            .id("quick-close")
                            .role(Role::Button)
                            .aria_label("Close composer")
                            .tip("Close composer")
                            .group("quick-close")
                            .cursor_pointer()
                            .absolute()
                            .right_2()
                            .top(px(14.))
                            .size(px(20.))
                            .rounded(px(4.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .hover(|s| s.bg(content(0.08)))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(
                                icon("close", 12.)
                                    .text_color(content(0.35))
                                    .group_hover("quick-close", |s| s.text_color(content(1.))),
                            )
                            .on_click(cx.listener(|this, _, window, cx| this.dismiss(window, cx))),
                    )
                    .child(self.header(cx))
                    .children(self.attachment_chips(cx))
                    .child(
                        div()
                            .flex_shrink_0()
                            .pl(px(20.))
                            .pr(px(36.))
                            .pt_4()
                            .pb_2()
                            .child(self.input.clone()),
                    )
                    .when(warning, |d| {
                        d.child(
                            div()
                                .id("quick-attachment-warning")
                                .role(Role::Alert)
                                .px(px(20.))
                                .pb_2()
                                .text_size(px(12.))
                                .text_color(amber(1.))
                                .child(
                                    "Choose a provider that supports attachments, or remove the attached files.",
                                ),
                        )
                    })
                    .child(self.toolbar(cx))
                    .child(div().flex().flex_col().flex_shrink_0().opacity(fade).map(|d| match picker {
                        Some(Picker::Commands) => d.child(self.commands(cx)),
                        Some(Picker::Permissions) => d.child(self.permissions(cx)),
                        Some(Picker::Project) => d.child(self.project_list(window, cx)),
                        Some(Picker::Workspace) => d.child(self.workspace_list(cx)),
                        Some(Picker::Branch) => d.children(self.git_picker.clone().map(|p| {
                            div()
                                .flex_shrink_0()
                                .h(px(280.))
                                .flex()
                                .flex_col()
                                .border_t_1()
                                .border_color(content(0.08))
                                .child(p)
                        })),
                        Some(Picker::Model) => {
                            let effort = self.effort(cx);
                            d.children(self.models.clone().map(|list| {
                                div()
                                    .flex_shrink_0()
                                    .border_t_1()
                                    .border_color(content(0.08))
                                    .child(list)
                            }))
                            .children(effort)
                        }
                        _ => d,
                    })),
                    )
                    .when(self.dragging, |d| {
                        d.child(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded(px(RADIUS))
                                .border_1()
                                .border_dashed()
                                .border_color(accent_color().opacity(0.6))
                                .bg(floating_fill().opacity(0.9))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(14.))
                                .text_color(accent_color())
                                .child("Drop to attach"),
                        )
                    }),
            );
        if picker == Some(Picker::Attachments) {
            let weak = cx.weak_entity();
            root = root.child(
                popover("quick-attach-options", Anchor::Slot(self.plus.clone()))
                    .side(Side::Top)
                    .width(220.)
                    .on_dismiss(move |window, cx| {
                        let _ = weak.update(cx, |this, cx| this.close_picker(window, cx));
                    })
                    .child(
                        div()
                            .p_1()
                            .child(
                                div()
                                    .id("quick-choose-files")
                                    .role(Role::MenuItem)
                                    .aria_label("Choose files…")
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_2()
                                    .py(px(6.))
                                    .rounded(px(6.))
                                    .text_size(px(12.))
                                    .hover(|s| s.bg(content(0.08)))
                                    .child(icon("image-add", 14.).text_color(content(0.75)))
                                    .child("Choose files…")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.choose_files(window, cx)
                                    })),
                            )
                            .child(
                                div()
                                    .id("quick-screenshot")
                                    .role(Role::MenuItem)
                                    .aria_label("Take screenshot…")
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .px_2()
                                    .py(px(6.))
                                    .rounded(px(6.))
                                    .text_size(px(12.))
                                    .hover(|s| s.bg(content(0.08)))
                                    .child(icon("maximize-2", 14.).text_color(content(0.75)))
                                    .child("Take screenshot…")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.take_screenshot(window, cx)
                                    })),
                            ),
                    ),
            );
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn open(cx: &mut TestAppContext) -> WindowHandle<QuickComposer> {
        cx.update(|cx| cx.set_global(crate::settings::Preferences::default()));
        let window = cx.add_window(|_, cx| {
            QuickComposer::new(
                vec!["/work/app".into(), "/work/site".into()],
                HashMap::new(),
                Launch::choice(
                    "/work/app".into(),
                    Kind::Codex,
                    None,
                    AccessMode::Supervised,
                ),
                cx,
            )
        });
        cx.run_until_parked();
        window
    }

    #[gpui::test]
    fn every_picker_opens_below_the_toolbar_and_escape_closes_it(cx: &mut TestAppContext) {
        let window = open(cx);
        for picker in [
            Picker::Project,
            Picker::Model,
            Picker::Permissions,
            Picker::Attachments,
            Picker::Workspace,
            Picker::Branch,
        ] {
            window
                .update(cx, |view, window, cx| {
                    view.open(picker, window, cx);
                    assert!(view.picker == Some(picker));
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |view, window, cx| {
                    // Opening the same picker again closes it, as a toggle.
                    view.open(picker, window, cx);
                    assert!(view.picker.is_none());
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn a_leading_slash_lists_modes_and_picking_one_keeps_it_in_the_prompt(cx: &mut TestAppContext) {
        let window = open(cx);
        window
            .update(cx, |view, _, cx| {
                view.input.update(cx, |i, cx| i.set_text("/pl".into(), cx));
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                assert!(view.picker == Some(Picker::Commands));
                assert_eq!(view.command_options(cx).len(), 1);
                view.choose_command(0, cx);
                assert_eq!(view.input.read(cx).content.as_ref(), "/plan ");
                assert!(view.picker.is_none());
                // A bare mode is not a message yet.
                assert!(!view.can_submit(cx));
            })
            .unwrap();
    }

    #[gpui::test]
    fn launch_carries_the_workspace_choice_and_clears_the_draft(cx: &mut TestAppContext) {
        let window = open(cx);
        let launched = std::rc::Rc::new(std::cell::RefCell::new(None));
        window
            .update(cx, |view, window, cx| {
                let sink = launched.clone();
                cx.subscribe_in(&cx.entity(), window, move |_, _, launch: &Launch, _, _| {
                    *sink.borrow_mut() = Some(launch.clone());
                })
                .detach();
                view.workspace.new_worktree = true;
                view.workspace.base = Some("main".into());
                view.access = AccessMode::FullAccess;
                view.input
                    .update(cx, |i, cx| i.set_text("/draft fix the build".into(), cx));
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                assert_eq!(view.message(cx), "fix the build");
                view.launch(true, cx);
                assert!(view.input.read(cx).content.is_empty());
            })
            .unwrap();
        let launch = launched.borrow_mut().take().expect("launched");
        assert_eq!(launch.prompt, "/draft fix the build");
        assert!(launch.new_worktree);
        assert_eq!(launch.worktree_base.as_deref(), Some("main"));
        assert_eq!(launch.worktree_cwd, None);
        assert!(launch.foreground);
        assert_eq!(launch.access, AccessMode::FullAccess);
    }
}
