//! The floating quick composer. Port of
//! src/features/quick-composer/ui/QuickComposer.tsx and the hooks it uses
//! (useQuickAttachments.ts, useQuickPickerMotion.ts) and of
//! QuickWorkspaceControls.tsx.
//!
//! The composer draws the card and handles input. Data and side effects go
//! through [`QuickComposerHost`]; window work (hide, resize, the git popup)
//! goes out as [`QuickComposerEvent`]s, which [`crate::QuickPanels`] or the
//! app turns into native calls.

mod attachments;
mod render;
#[cfg(test)]
mod tests;
mod workspace;

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyBinding,
    ScrollHandle, Subscription, Task, Window, actions,
};
use monocode_core::block::ModelSettings;
use monocode_core::harness::{DEFAULT_RUNTIME_MODE, harness_supports_attachments};
use monocode_core::models::{AgentModel, LastModelChoice, ModelCatalog, ModelPrefs};
use monocode_core::{HarnessId, RuntimeMode, js};
use monocode_layout::paths::project_name;
use monocode_ui::{Theme, icon, u};
use monocode_view_composer::composer::model::commands::{DRAFT, ORCHESTRATOR, PLAN};
use monocode_view_composer::composer::model::mode_commands::{
    ModeCommandToken, leading_mode_command,
};
use monocode_view_composer::composer::model::skills::{
    MAX_PICKER, Skill, SlashToken, rank_skills, replace_slash_token, slash_token_at,
};
use monocode_view_composer::composer::prompt_input::{
    Decorator, GlyphOverlay, OverlayPlacement, PromptDecorations, PromptInput, PromptInputEvent,
};

use crate::colors;
use crate::field::{field_colors, search_field};
use crate::host::{QuickComposerHost, QuickSnapshot};
use crate::model::appearance::ProjectAppearance;
use crate::model::git_controls::GitControls;
use crate::model::launch::{
    GitBranches, QuickGitRequest, QuickIntent, QuickLaunchRequest, QuickWorkspace,
    needs_worktree_check, quick_launch_attachments, quick_workspace_fields, workspace_for_project,
};
use crate::model::motion::{Picker, PickerMotion};
use crate::model::prompt::{
    MODE_INDENT, PROMPT_MAX_HEIGHT, mode_commands, mode_names, quick_prompt_mode,
};
use crate::model::selector::{filter_quick_projects, loading_model, resolve_quick_model};
use crate::permissions::{QuickPermissions, QuickPermissionsEvent};
use crate::selector::{QuickModelSelector, QuickModelSelectorEvent, SelectorProps};

pub use attachments::QuickAttachments;

actions!(quick_composer, [OpenProjects, OpenModels]);

/// The prompt's key context.
pub const PROMPT_CONTEXT: &str = "QuickPrompt";

/// Binds Command+P (projects) and Command+. (models) in the prompt. Call
/// once at startup, after `monocode_view_composer::composer::init`.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-p", OpenProjects, Some(PROMPT_CONTEXT)),
        KeyBinding::new("cmd-.", OpenModels, Some(PROMPT_CONTEXT)),
    ]);
}

/// Window work the composer asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum QuickComposerEvent {
    /// `quick_composer_dismiss`: hide the panel and the git popup. The
    /// draft stays, like Spotlight's query.
    Dismiss,
    /// `quick_composer_fit`: the card is this tall (CSS px, rounded up).
    /// Keep the panel's top edge and change its height.
    Fit(u32),
    /// `quick_git_open`: show the git popup for this request. Report a
    /// failure with [`QuickComposer::git_open_failed`] and the result with
    /// [`QuickComposer::apply_git_result`].
    OpenGit(Box<QuickGitRequest>),
    /// `quick_git_complete(id, null, restoreFocus: false)`: the composer
    /// dropped this popup request; hide the popup.
    CancelGit(String),
}

/// `useProjectBranchesState` for the working copy controls.
#[derive(Default)]
pub(crate) struct BranchState {
    pub cwd: String,
    pub branches: Option<GitBranches>,
    pub settled: bool,
    pub task: Option<Task<()>>,
}

pub struct QuickComposer {
    pub(crate) host: Rc<dyn QuickComposerHost>,
    focus_handle: FocusHandle,
    pub(crate) prompt: Entity<PromptInput>,
    pub(crate) query: Entity<PromptInput>,

    pub(crate) projects: Vec<String>,
    pub(crate) appearance: ProjectAppearance,
    pub(crate) cwd: Option<String>,
    pub(crate) choice: LastModelChoice,
    pub(crate) catalog: ModelCatalog,
    pub(crate) prefs: ModelPrefs,
    pub(crate) available: Option<Vec<HarnessId>>,
    pub(crate) model_settings: ModelSettings,
    pub(crate) runtime_mode: RuntimeMode,

    pub(crate) workspace_choice: QuickWorkspace,
    pub(crate) git_open: bool,
    pub(crate) git: GitControls,
    pub(crate) branches: BranchState,
    /// The workspace and branch triggers' last bounds, for the popup anchor.
    pub(crate) trigger_bounds: [Option<gpui::Bounds<gpui::Pixels>>; 2],

    pub(crate) picker: Option<Picker>,
    pub(crate) slash: Option<SlashToken>,
    pub(crate) highlight: usize,
    pub(crate) error: Option<String>,
    pub(crate) busy: bool,
    pub(crate) attachments: QuickAttachments,
    pub(crate) selector: Option<(Entity<QuickModelSelector>, Subscription)>,
    pub(crate) permissions: Option<(Entity<QuickPermissions>, Subscription)>,

    pub(crate) motion: PickerMotion,
    /// The card's natural height, measured each frame.
    pub(crate) natural_height: Rc<Cell<f32>>,
    /// The picker that the motion has not caught up with yet.
    pub(crate) motion_pending: bool,
    pub(crate) list_scroll: ScrollHandle,
    pub(crate) submit_task: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<QuickComposerEvent> for QuickComposer {}

impl Focusable for QuickComposer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl QuickComposer {
    pub fn new(
        host: Rc<dyn QuickComposerHost>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let snapshot = host.snapshot(cx);
        let prompt = cx.new(|cx| {
            let mut input = PromptInput::new(window, cx);
            input.set_padding([16., 36., 8., 20.], cx);
            input.set_max_height(Some(PROMPT_MAX_HEIGHT), cx);
            input
        });
        let colors = field_colors(cx, 0.40);
        prompt.update(cx, |prompt, cx| prompt.set_colors(colors, cx));
        let query = search_field("Find a project", window, cx);
        let subscriptions = vec![
            cx.subscribe_in(&prompt, window, |this, _, event, window, cx| match event {
                PromptInputEvent::Changed | PromptInputEvent::SelectionChanged => {
                    this.sync_prompt_command(window, cx)
                }
                _ => {}
            }),
            cx.subscribe(&query, |this, query, event: &PromptInputEvent, cx| {
                if *event == PromptInputEvent::Changed {
                    let _ = query;
                    this.highlight = 0;
                    cx.notify();
                }
            }),
            cx.subscribe_in(&query, window, |this, _, event, window, cx| {
                if *event == PromptInputEvent::Blurred {
                    this.query_blurred(window, cx);
                }
            }),
        ];
        cx.on_release(|this: &mut Self, cx| this.attachments.release(&this.host, cx))
            .detach();
        let cwd = snapshot.initial_project.clone();
        let mut this = Self {
            host,
            focus_handle: cx.focus_handle(),
            prompt,
            query,
            projects: snapshot.projects,
            appearance: snapshot.appearance,
            workspace_choice: QuickWorkspace::current(cwd.as_deref()),
            cwd,
            choice: snapshot.choice,
            catalog: snapshot.catalog,
            prefs: snapshot.prefs,
            available: snapshot.available,
            model_settings: snapshot.model_settings,
            runtime_mode: DEFAULT_RUNTIME_MODE,
            git_open: false,
            git: GitControls::default(),
            branches: BranchState::default(),
            trigger_bounds: [None, None],
            picker: None,
            slash: None,
            highlight: 0,
            error: None,
            busy: false,
            attachments: QuickAttachments::default(),
            selector: None,
            permissions: None,
            motion: PickerMotion::default(),
            natural_height: Rc::new(Cell::new(0.)),
            motion_pending: false,
            list_scroll: ScrollHandle::new(),
            submit_task: None,
            _subscriptions: subscriptions,
        };
        this.install_decorator(cx);
        this.sync_prompt_props(cx);
        this.refresh_branches(cx);
        let harness = this.choice.harness;
        this.host.request_catalog(harness, cx);
        this.focus_prompt(window, cx);
        this
    }

    // Reading.

    /// The prompt text.
    pub fn text(&self, cx: &App) -> String {
        self.prompt.read(cx).text().to_string()
    }

    pub fn prompt(&self) -> &Entity<PromptInput> {
        &self.prompt
    }

    pub fn query_input(&self) -> &Entity<PromptInput> {
        &self.query
    }

    pub fn picker(&self) -> Option<Picker> {
        self.picker
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn attachments(&self) -> &QuickAttachments {
        &self.attachments
    }

    pub fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    pub fn workspace(&self) -> QuickWorkspace {
        workspace_for_project(&self.workspace_choice, self.cwd.as_deref())
    }

    pub fn runtime_mode(&self) -> RuntimeMode {
        self.runtime_mode
    }

    pub fn highlight(&self) -> usize {
        self.highlight
    }

    pub fn git_open(&self) -> bool {
        self.git_open
    }

    pub fn git_controls(&self) -> &GitControls {
        &self.git
    }

    pub fn selector(&self) -> Option<&Entity<QuickModelSelector>> {
        self.selector.as_ref().map(|(selector, _)| selector)
    }

    pub fn permissions(&self) -> Option<&Entity<QuickPermissions>> {
        self.permissions
            .as_ref()
            .map(|(permissions, _)| permissions)
    }

    /// `resolveQuickModel(choice)`.
    pub fn resolved_model(&self) -> Option<AgentModel> {
        resolve_quick_model(&self.catalog, &self.choice)
    }

    /// The model, or a placeholder while its catalog loads.
    pub fn model(&self) -> AgentModel {
        self.resolved_model()
            .unwrap_or_else(|| loading_model(&self.choice))
    }

    /// `mergeModelSettings(model, modelSettings)`.
    pub fn settings(&self) -> ModelSettings {
        self.catalog
            .merge_model_settings(&self.model(), Some(&self.model_settings))
    }

    pub fn attachments_supported(&self) -> bool {
        harness_supports_attachments(self.choice.harness)
    }

    /// The leading mode command, as the prompt colors it.
    pub fn leading_mode(&self, cx: &App) -> Option<ModeCommandToken> {
        leading_mode_command(self.prompt.read(cx).text(), &mode_names())
    }

    /// `commandOptions`.
    pub fn command_options(&self) -> Vec<Skill> {
        rank_skills(
            &mode_commands(),
            self.slash.as_ref().map_or("", |slash| slash.query.as_str()),
            MAX_PICKER,
        )
    }

    /// `projectOptions`.
    pub fn project_options(&self, cx: &App) -> Vec<String> {
        let query = if self.picker == Some(Picker::Project) {
            self.query.read(cx).text().to_string()
        } else {
            String::new()
        };
        filter_quick_projects(&self.projects, &query)
    }

    /// `optionCount`.
    fn option_count(&self, cx: &App) -> usize {
        if self.picker == Some(Picker::Commands) {
            self.command_options().len()
        } else {
            self.project_options(cx).len()
        }
    }

    /// `canSubmit`.
    pub fn can_submit(&self, cx: &App) -> bool {
        let launch = quick_prompt_mode(js::trim(self.prompt.read(cx).text()));
        let files = !self.attachments.files.is_empty();
        (!js::trim(&launch.prompt).is_empty() || files)
            && self.cwd.is_some()
            && !self.busy
            && !self.attachments.loading
            && !self.git_open
            && self.resolved_model().is_some()
            && (self.attachments_supported() || !files)
    }

    // Data from the app.

    /// `QUICK_COMPOSER_SHOWN_EVENT`: projects and defaults can change in a
    /// workspace between shows, so each show reads them again. The draft
    /// and its attachments survive a dismiss.
    pub fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let snapshot = self.host.snapshot(cx);
        self.apply_snapshot(snapshot, cx);
        self.close_lists();
        self.picker = None;
        self.slash = None;
        self.error = None;
        let harness = self.choice.harness;
        self.host.request_catalog(harness, cx);
        self.refresh_branches(cx);
        self.focus_prompt(window, cx);
        cx.notify();
    }

    fn apply_snapshot(&mut self, snapshot: QuickSnapshot, cx: &mut Context<Self>) {
        let keep = self
            .cwd
            .as_ref()
            .filter(|current| snapshot.projects.contains(current))
            .cloned();
        let cwd = keep.or(snapshot.initial_project);
        self.projects = snapshot.projects;
        self.appearance = snapshot.appearance;
        self.set_cwd(cwd, cx);
        self.choice = snapshot.choice;
        self.model_settings = snapshot.model_settings;
        self.catalog = snapshot.catalog;
        self.prefs = snapshot.prefs;
        self.available = snapshot.available;
        self.sync_prompt_props(cx);
    }

    /// A workspace window's live catalog landed (`QUICK_COMPOSER_CATALOG_EVENT`).
    pub fn set_catalog(
        &mut self,
        catalog: ModelCatalog,
        available: Option<Vec<HarnessId>>,
        cx: &mut Context<Self>,
    ) {
        self.catalog = catalog;
        if available.is_some() {
            self.available = available;
        }
        self.sync_prompt_props(cx);
        self.sync_selector(cx);
        cx.notify();
    }

    // Focus.

    /// `focusPrompt`: focus the prompt with the caret at the end.
    pub fn focus_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt.update(cx, |prompt, cx| {
            let end = prompt.text().len();
            prompt.move_to(end, cx);
            prompt.focus(window, cx);
        });
    }

    // The prompt.

    fn install_decorator(&mut self, cx: &mut Context<Self>) {
        let decorator: Decorator = Rc::new(prompt_decorations);
        self.prompt
            .update(cx, |prompt, cx| prompt.set_decorator(Some(decorator), cx));
    }

    /// The placeholder and the disabled state follow the project and model.
    fn sync_prompt_props(&mut self, cx: &mut Context<Self>) {
        let placeholder = match &self.cwd {
            Some(cwd) => format!(
                "Start a {} session in {}\u{2026}",
                self.model().harness.title(),
                project_name(cwd)
            ),
            None => "Open a project in MonoCode first".to_string(),
        };
        let disabled = self.cwd.is_none();
        self.prompt.update(cx, |prompt, cx| {
            prompt.set_placeholder(placeholder, cx);
            prompt.set_disabled(disabled, cx);
        });
    }

    /// `syncPromptCommand`: a leading `/` opens the command list.
    fn sync_prompt_command(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let (text, selection) = {
            let prompt = self.prompt.read(cx);
            (prompt.text().to_string(), prompt.selection())
        };
        let token = selection
            .is_empty()
            .then(|| slash_token_at(&text, selection.start, false))
            .flatten();
        // Commands work only at the start of a prompt.
        let leading = token.filter(|token| js::trim(&text[..token.start]).is_empty());
        let changed = self.slash != leading;
        self.slash = leading.clone();
        if leading.is_some() && !self.busy && !self.git_open {
            if self.picker != Some(Picker::Commands) || changed {
                self.highlight = 0;
            }
            self.set_picker(Some(Picker::Commands), cx);
        } else if self.picker == Some(Picker::Commands) {
            self.set_picker(None, cx);
        }
        cx.notify();
    }

    // Pickers.

    pub(crate) fn set_picker(&mut self, picker: Option<Picker>, cx: &mut Context<Self>) {
        if self.picker == picker {
            return;
        }
        if matches!(self.picker, Some(Picker::Model | Picker::Permissions)) {
            self.close_lists();
        }
        self.picker = picker;
        self.motion_pending = true;
        if picker.is_some() {
            // `enabled={!busy && !picker}` turns the git controls off.
            self.cancel_git(cx);
        }
        cx.notify();
    }

    /// Drops the model selector and the permissions list, which hold their
    /// own state while open.
    fn close_lists(&mut self) {
        self.selector = None;
        self.permissions = None;
    }

    /// `openPicker`: a second press on the same control closes it.
    pub fn open_picker(&mut self, kind: Picker, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker == Some(kind) {
            self.close_picker(window, cx);
            return;
        }
        self.slash = None;
        self.set_picker(Some(kind), cx);
        self.query.update(cx, |query, cx| query.reset_text("", cx));
        self.highlight = self
            .cwd
            .as_ref()
            .and_then(|cwd| self.projects.iter().position(|path| path == cwd))
            .unwrap_or(0);
        self.list_scroll.scroll_to_item(self.highlight);
        match kind {
            Picker::Project => {
                let query = self.query.clone();
                query.update(cx, |query, cx| query.focus(window, cx));
            }
            Picker::Model => self.open_selector(window, cx),
            Picker::Permissions => self.open_permissions(window, cx),
            _ => {}
        }
        cx.notify();
    }

    /// `closePicker`.
    pub fn close_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let was = self.picker;
        self.set_picker(None, cx);
        self.slash = None;
        self.query.update(cx, |query, cx| query.reset_text("", cx));
        if was == Some(Picker::Commands) {
            self.prompt
                .update(cx, |prompt, cx| prompt.focus(window, cx));
        } else {
            self.focus_prompt(window, cx);
        }
        cx.notify();
    }

    /// `chooseAt`.
    pub fn choose_at(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        match self.picker {
            Some(Picker::Commands) => {
                let options = self.command_options();
                let (Some(command), Some(slash)) = (options.get(index), self.slash.clone()) else {
                    return;
                };
                // The command stays in the prompt, where it shows its icon.
                let text = self.prompt.read(cx).text().to_string();
                let next = replace_slash_token(&text, &slash, &command.invocation);
                let mut cursor = slash.start + command.invocation.len() + 1;
                if next.as_bytes().get(cursor) == Some(&b' ') {
                    cursor += 1;
                }
                self.set_picker(None, cx);
                self.slash = None;
                self.prompt.update(cx, |prompt, cx| {
                    prompt.set_text(next, cursor, cx);
                    prompt.focus(window, cx);
                });
                // `set_text` reopened the list through the change event.
                self.slash = None;
                self.set_picker(None, cx);
            }
            Some(Picker::Project) => {
                let Some(path) = self.project_options(cx).get(index).cloned() else {
                    return;
                };
                self.set_cwd(Some(path), cx);
                self.close_picker(window, cx);
            }
            _ => self.close_picker(window, cx),
        }
        cx.notify();
    }

    /// Arrow keys over the open list.
    pub(crate) fn step_highlight(&mut self, down: bool, cx: &mut Context<Self>) {
        let count = self.option_count(cx);
        if count == 0 {
            return;
        }
        self.highlight = if down {
            (self.highlight + 1) % count
        } else {
            (self.highlight + count - 1) % count
        };
        self.list_scroll.scroll_to_item(self.highlight);
        cx.notify();
    }

    fn query_blurred(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Focus that moved inside the card keeps the list open.
        let inside = self.focus_handle.contains_focused(window, cx)
            || self.prompt.read(cx).is_focused(window);
        if self.picker == Some(Picker::Project) && !inside {
            self.set_picker(None, cx);
            self.slash = None;
            cx.notify();
        }
    }

    pub(crate) fn set_cwd(&mut self, cwd: Option<String>, cx: &mut Context<Self>) {
        if self.cwd == cwd {
            return;
        }
        // `key={cwd}` remounted the working copy controls, which dropped
        // their popup request.
        self.cancel_git(cx);
        self.workspace_choice = QuickWorkspace::current(cwd.as_deref());
        self.cwd = cwd;
        self.sync_prompt_props(cx);
        self.refresh_branches(cx);
    }

    // The model selector.

    fn selector_props(&self) -> SelectorProps {
        SelectorProps {
            model: self.model(),
            values: self.settings(),
            available: self.available.clone(),
            catalog: self.catalog.clone(),
            prefs: self.prefs.clone(),
        }
    }

    fn open_selector(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let props = self.selector_props();
        let selector = cx.new(|cx| QuickModelSelector::new(props, window, cx));
        let subscription = cx.subscribe_in(&selector, window, Self::on_selector_event);
        self.selector = Some((selector, subscription));
    }

    fn sync_selector(&mut self, cx: &mut Context<Self>) {
        if let Some((selector, _)) = &self.selector {
            let props = self.selector_props();
            selector.update(cx, |selector, cx| selector.set_props(props, cx));
        }
    }

    fn on_selector_event(
        &mut self,
        _: &Entity<QuickModelSelector>,
        event: &QuickModelSelectorEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            QuickModelSelectorEvent::Change(model) => {
                self.choice = LastModelChoice {
                    harness: model.harness,
                    model: model.id.clone(),
                };
                self.model_settings = self
                    .catalog
                    .merge_model_settings(model, Some(&self.model_settings));
                self.sync_prompt_props(cx);
            }
            QuickModelSelectorEvent::Settings(values) => self.model_settings = values.clone(),
            QuickModelSelectorEvent::Close => {
                self.close_picker(window, cx);
                return;
            }
            QuickModelSelectorEvent::Favorites(favorites) => {
                self.prefs.favorite_models = favorites.clone();
                self.host.save_favorites(favorites, cx);
            }
            QuickModelSelectorEvent::RequestCatalog(harness) => {
                self.host.request_catalog(*harness, cx);
                return;
            }
        }
        self.sync_selector(cx);
        cx.notify();
    }

    // The permissions list.

    fn open_permissions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.runtime_mode;
        let permissions = cx.new(|cx| QuickPermissions::new(value, window, cx));
        let subscription = cx.subscribe_in(&permissions, window, Self::on_permissions_event);
        self.permissions = Some((permissions, subscription));
    }

    fn on_permissions_event(
        &mut self,
        _: &Entity<QuickPermissions>,
        event: &QuickPermissionsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            QuickPermissionsEvent::Change(mode) => {
                self.runtime_mode = *mode;
                cx.notify();
            }
            QuickPermissionsEvent::Close => self.close_picker(window, cx),
        }
    }

    // Dismiss and submit.

    /// `dismiss`: hide the panel. The draft stays.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        cx.emit(QuickComposerEvent::Dismiss);
    }

    /// Escape: close the open list first, then the panel.
    pub(crate) fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.is_some() {
            self.close_picker(window, cx);
        } else {
            self.dismiss(cx);
        }
    }

    /// `submit`: queue a session for a workspace window. `reveal` brings the
    /// window forward (Command+Return).
    pub fn submit(&mut self, reveal: bool, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.prompt.read(cx).text().to_string();
        let launch = quick_prompt_mode(js::trim(&raw));
        let text = js::trim(&launch.prompt).to_string();
        let files = self.attachments.files.clone();
        let Some(cwd) = self.cwd.clone() else {
            return;
        };
        if (text.is_empty() && files.is_empty())
            || self.busy
            || self.attachments.loading
            || self.git_open
            || (!files.is_empty() && !self.attachments_supported())
        {
            return;
        }
        let Some(model) = self.resolved_model() else {
            return;
        };
        self.busy = true;
        self.error = None;
        cx.notify();
        let settings = self.settings();
        let workspace = self.workspace();
        let mut request = QuickLaunchRequest {
            prompt: text,
            draft: (launch.mode == Some(DRAFT)).then_some(true),
            intent: match launch.mode {
                Some(PLAN) => Some(QuickIntent::Plan),
                Some(ORCHESTRATOR) => Some(QuickIntent::Orchestrate),
                _ => None,
            },
            cwd: cwd.clone(),
            harness: model.harness,
            model: Some(model.id.clone()),
            model_settings: Some(settings),
            runtime_mode: Some(self.runtime_mode),
            attachments: None,
            workspace_mode: None,
            worktree_base: None,
            worktree_cwd: None,
            reveal,
        };
        let attachments = quick_launch_attachments(&files);
        let listing = needs_worktree_check(&workspace).then(|| self.host.worktrees(&cwd, cx));
        let host = self.host.clone();
        self.submit_task = Some(cx.spawn_in(window, async move |this, cx| {
            let prepared: Result<QuickLaunchRequest, String> = async {
                request.attachments = Some(attachments?);
                let listed = match listing {
                    Some(task) => Some(task.await?),
                    None => None,
                };
                quick_workspace_fields(&workspace, listed.as_deref())?.apply(&mut request);
                Ok(request)
            }
            .await;
            let result = match prepared {
                Ok(request) => {
                    let task = this.update(cx, |_, cx| host.submit(request.clone(), cx));
                    match task {
                        Ok(task) => task.await.map(|()| request),
                        Err(err) => Err(err.to_string()),
                    }
                }
                Err(err) => Err(err),
            };
            this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(request) => {
                        this.host.remember(&request, cx);
                        this.prompt
                            .update(cx, |prompt, cx| prompt.set_text("", 0, cx));
                        this.set_picker(None, cx);
                        this.slash = None;
                        this.attachments.clear(&this.host, cx);
                        cx.emit(QuickComposerEvent::Dismiss);
                    }
                    Err(err) => this.error = Some(err),
                }
                this.busy = false;
                this.submit_task = None;
                let _ = window;
                cx.notify();
            })
            .ok();
        }));
    }
}

/// The prompt's colors: a leading mode command takes its color, and its `/`
/// gives way to the mode icon in a 15px first-line indent.
fn prompt_decorations(text: &str, cx: &App) -> PromptDecorations {
    let Some(token) = leading_mode_command(text, &mode_names()) else {
        return PromptDecorations::default();
    };
    let theme = Theme::of(cx);
    let color = colors::mode_text(token.mode, theme);
    let mode = token.mode;
    PromptDecorations {
        spans: vec![(0..token.end, color)],
        hidden: std::iter::once(0..1).collect(),
        overlays: vec![GlyphOverlay {
            range: 0..1,
            placement: OverlayPlacement::Before(MODE_INDENT),
            size: 16.,
            render: Rc::new(move |_, _| {
                use gpui::{IntoElement as _, Styled as _};
                icon(mode.icon())
                    .size(u(16.))
                    .text_color(color)
                    .into_any_element()
            }),
        }],
        first_line_indent: MODE_INDENT,
    }
}
