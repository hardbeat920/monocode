//! The `Composer` entity: a port of the `Composer` component in
//! src/features/sessions/ui/Composer.tsx. React state becomes fields, props
//! become [`ComposerProps`] (applied with [`Composer::set_props`], which
//! runs the effects that watched them), and callbacks become
//! [`ComposerHost`] calls or [`ComposerEvent`]s.

mod attachments;
mod chips;
mod colors;
mod context_meter;
mod decorations;
mod keys;
mod message_queue;
mod render;
pub(crate) mod runner;
mod submit;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    SharedString, Subscription, Task, Window,
};
use monocode_core::block::ModelSettings;
use monocode_core::context_usage::ContextUsage;
use monocode_core::handoff::HandoffComposerCard;
use monocode_core::harness::harness_supports_attachments;
use monocode_core::inbox::InboxComposerCard;
use monocode_core::notes::NoteComposerCard;
use monocode_core::session::{MessageQueueStatus, QueuedMessage};
use monocode_core::{Attachment, HarnessId, RuntimeMode};

use super::host::{ComposerHost, McpServers, ResendTicket, SessionFolder, SkillContext};
use super::model::chat_context::{ChatContextItem, compose_chat_context, split_chat_context};
use super::model::commands::{self, consume_session_folder_command};
use super::model::mcp::{McpTag, tagged_mcp_servers};
use super::model::mentions::{
    MentionIndex, MentionToken, RankedFile, build_mention_index, mention_token_at,
};
use super::model::mode_commands::{Mode, ModeCommandToken, leading_mode_command};
use super::model::paths::looks_like_project;
use super::model::quote_draft::{ComposerInsertRequest, consume_composer_insert};
use super::model::skills::{MAX_PICKER, Skill, SlashToken, rank_skills, slash_token_at};
use super::prompt_input::{PromptInput, PromptInputEvent};

pub use message_queue::MessageQueueState;

/// `COMPOSER_MAX_HEIGHT` in composerResize.ts: the prompt grows with its
/// text up to this height, then scrolls.
pub const COMPOSER_MAX_HEIGHT: f32 = 160.0;

/// `remoteFeatures`: what an older remote host can do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RemoteFeatures {
    pub attachments: bool,
    pub plan: bool,
    pub draft: bool,
}

/// `LastTurnRecall`: the last user turn, for Up-arrow editing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LastTurnRecall {
    pub text: String,
    pub attachments: Vec<Attachment>,
}

/// The props of Composer.tsx that are data. Defaults match the React
/// defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct ComposerProps {
    pub enabled: bool,
    /// The composer should hold focus when nothing else claims it.
    pub focused: bool,
    /// Bump to refocus even when `focused` was already true.
    pub focus_token: u64,
    pub shell: bool,
    pub compact: bool,
    pub placeholder: Option<SharedString>,
    pub disabled: bool,
    pub harness: HarnessId,
    pub model: String,
    pub model_settings: ModelSettings,
    pub allowed_model_harnesses: Option<Vec<HarnessId>>,
    pub runtime_mode: RuntimeMode,
    pub cwd: String,
    pub execution_cwd: String,
    pub session_id: Option<String>,
    pub branch: Option<String>,
    pub hide_project_picker: bool,
    pub hide_branch_picker: bool,
    pub hide_top_bar: bool,
    /// Keeps local file mentions, skills, and app modes off for host
    /// sessions.
    pub remote_session: bool,
    pub remote_features: Option<RemoteFeatures>,
    pub context: Option<ContextUsage>,
    pub compact_supported: bool,
    pub busy: bool,
    /// Typed text replaces Stop with Send while a turn runs.
    pub allow_busy_submit: bool,
    pub edit_last_turn_supported: bool,
    pub last_turn_recall: Option<LastTurnRecall>,
    pub queued_messages: Vec<QueuedMessage>,
    pub queue_status: Option<MessageQueueStatus>,
    pub inbox_card: Option<InboxComposerCard>,
    pub note_card: Option<NoteComposerCard>,
    pub handoff_card: Option<HandoffComposerCard>,
    pub can_save_draft: bool,
    /// `onBtwCommand` is wired.
    pub btw_enabled: bool,
    /// `onPlaceInFolder` is wired.
    pub folders_enabled: bool,
    pub worktree_removed: bool,
    /// The composer runner setting (`loadComposerRunner`).
    pub runner_enabled: bool,
    /// Notes join `@` mentions (`loadNotesEnabled`).
    pub notes_enabled: bool,
    /// `modelControls === "beside"`: effort and speed show as pills.
    pub model_controls_beside: bool,
    pub hotkeys: bool,
    /// Bump to clear the draft (`draftResetToken`).
    pub draft_reset_token: Option<u64>,
    /// Text or a context chip to add (`insertRequest`).
    pub insert_request: Option<ComposerInsertRequest>,
    /// `prefers-reduced-motion`.
    pub reduced_motion: bool,
    /// Turn animations off, for screenshots.
    pub animate: bool,
    /// The project's mascot (`resolveTabGroupMascot`); `None` picks one by
    /// the project name.
    pub runner_mascot: Option<String>,
    /// The project's color (`resolveTabGroupColor`); `None` uses the text
    /// color.
    pub runner_color: Option<gpui::Hsla>,
}

impl Default for ComposerProps {
    fn default() -> Self {
        Self {
            enabled: true,
            focused: false,
            focus_token: 0,
            shell: false,
            compact: false,
            placeholder: None,
            disabled: false,
            harness: HarnessId::Claude,
            model: String::new(),
            model_settings: ModelSettings::new(),
            allowed_model_harnesses: None,
            runtime_mode: RuntimeMode::default(),
            cwd: "~".into(),
            execution_cwd: "~".into(),
            session_id: None,
            branch: None,
            hide_project_picker: false,
            hide_branch_picker: false,
            hide_top_bar: false,
            remote_session: false,
            remote_features: None,
            context: None,
            compact_supported: false,
            busy: false,
            allow_busy_submit: true,
            edit_last_turn_supported: false,
            last_turn_recall: None,
            queued_messages: Vec::new(),
            queue_status: None,
            inbox_card: None,
            note_card: None,
            handoff_card: None,
            can_save_draft: false,
            btw_enabled: false,
            folders_enabled: false,
            worktree_removed: false,
            runner_enabled: true,
            notes_enabled: true,
            model_controls_beside: false,
            hotkeys: false,
            draft_reset_token: None,
            insert_request: None,
            reduced_motion: false,
            animate: true,
            runner_mascot: None,
            runner_color: None,
        }
    }
}

/// Notifications for the owner: the callback props of Composer.tsx that
/// return nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum ComposerEvent {
    /// `onFocus`: the composer was clicked or focused.
    Focus,
    /// `onModelChange`.
    ModelChange { harness: HarnessId, model: String },
    /// `onModelSettingsChange`.
    ModelSettingsChange(ModelSettings),
    /// `onRuntimeModeChange`.
    RuntimeModeChange(RuntimeMode),
    /// The model picker's favorites changed.
    FavoritesChange(Vec<String>),
    /// `onInsertRequestConsumed`.
    InsertRequestConsumed(i64),
    /// `onOpenFile`, from a context chip or a created skill.
    OpenFile { path: String, line: Option<i64> },
    /// The MCP picker's Manage row (`monocode:open-mcp-settings`).
    OpenMcpSettings,
    /// `onDeleteQueuedMessage`.
    DeleteQueuedMessage(String),
    /// `onEditQueuedMessage`.
    EditQueuedMessage { id: String, text: String },
    /// `onQueuedMessageEditingChange`.
    QueuedMessageEditing(Option<String>),
    /// `onSteerQueuedMessage`.
    SteerQueuedMessage(String),
    /// `onResumeQueue`.
    ResumeQueue,
    /// `onEditingLastTurnChange`.
    EditingLastTurnChange(bool),
}

/// The mode toggles in the + menu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ModeSelection {
    pub plan: bool,
    pub operator: bool,
    pub orchestration: bool,
    pub draft: bool,
}

/// A resend kept so a late rejection can restore it (`onResendRejected`).
#[derive(Clone, Debug)]
pub(crate) struct PendingResend {
    pub ticket: ResendTicket,
    pub revision: u64,
    pub text: String,
    pub files: Vec<Attachment>,
    pub borrowed: HashSet<String>,
    pub mcp: Vec<McpTag>,
}

/// The session composer.
pub struct Composer {
    pub(crate) host: Rc<dyn ComposerHost>,
    pub(crate) props: ComposerProps,
    pub(crate) prompt: Entity<PromptInput>,

    /// `draft`: the typed text, without context chips.
    pub(crate) draft: String,
    pub(crate) context_items: Vec<ChatContextItem>,
    pub(crate) has_value: bool,
    pub(crate) attachments: Vec<Attachment>,
    pub(crate) attachment_preview: Option<chips::AttachmentPreview>,
    pub(crate) attachment_images: HashMap<String, chips::AttachmentImage>,
    pub(crate) borrowed_attachment_ids: HashSet<String>,
    pub(crate) paste_error: Option<String>,
    pub(crate) file_drag: bool,
    pub(crate) plus_open: bool,
    pub(crate) modes: ModeSelection,
    pub(crate) slash: Option<SlashToken>,
    pub(crate) skill_active: usize,
    pub(crate) creating_skill: bool,
    pub(crate) create_error: Option<String>,
    pub(crate) create_busy: bool,
    pub(crate) session_folder_open: bool,
    pub(crate) session_folders: Vec<SessionFolder>,
    pub(crate) session_folder_selected: bool,
    pub(crate) mcp_picker_open: bool,
    pub(crate) mcp_servers: McpServers,
    pub(crate) mcp_insert_at: Option<usize>,
    pub(crate) selected_mcp: Vec<McpTag>,
    pub(crate) mention: Option<MentionToken>,
    pub(crate) mention_active: usize,
    pub(crate) resend_edited: bool,
    pub(crate) runner_live: bool,

    pub(crate) skills: Vec<Skill>,
    pub(crate) mention_index: MentionIndex,
    pub(crate) ranked_files: Vec<RankedFile>,

    /// Bumped by every edit, so a late resend rejection cannot restore over
    /// newer text.
    pub(crate) draft_revision: u64,
    /// Bumped when the draft is cleared, so a late paste cannot land on the
    /// next one.
    pub(crate) paste_generation: u64,
    /// Pastes still reading when Send is pressed.
    pub(crate) pastes_in_flight: usize,
    /// A Send waiting for those pastes, tagged with the paste generation.
    pub(crate) submit_waiting: Option<u64>,
    pub(crate) consumed_insert_id: Option<i64>,
    pub(crate) next_resend: u64,
    pub(crate) pending_resend: Option<PendingResend>,

    pub(crate) queue: MessageQueueState,
    pub(crate) meter: context_meter::MeterState,
    pub(crate) runner: Option<Entity<runner::ComposerRunner>>,
    pub(crate) bar: render::BottomBar,
    pub(crate) chip_preview: chips::ChipPreview,
    pub(crate) runner_geometry: runner::RunnerGeometry,

    /// Views the owner puts above the composer box: the question form,
    /// `children`, the usage limit notice.
    pub(crate) header_views: Vec<AnyView>,
    /// Views inside the box above the prompt: inbox, note, handoff cards.
    pub(crate) card_views: Vec<AnyView>,
    /// The project and branch pickers. Without them the top bar shows the
    /// working directory and branch as labels.
    pub(crate) top_bar_views: Vec<AnyView>,

    pub(crate) _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ComposerEvent> for Composer {}

impl Focusable for Composer {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.prompt.read(cx).focus_handle(cx)
    }
}

impl Composer {
    /// A composer for `props`. `initial_draft` may end in a context block,
    /// which becomes chips (`initialDraft`).
    pub fn new(
        host: Rc<dyn ComposerHost>,
        props: ComposerProps,
        initial_draft: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let prompt = cx.new(|cx| PromptInput::new(window, cx));
        let subscriptions = vec![cx.subscribe_in(&prompt, window, Self::on_prompt_event)];
        let initial = initial_draft.as_deref().map(split_chat_context);
        let draft = initial
            .as_ref()
            .map(|message| message.text.clone())
            .unwrap_or_default();
        let context_items = initial.map(|message| message.items).unwrap_or_default();
        prompt.update(cx, |prompt, cx| prompt.reset_text(draft.clone(), cx));
        let selected_mcp = props
            .session_id
            .as_deref()
            .map(|id| host.load_mcp_tags(id, cx))
            .unwrap_or_default();
        let has_value = initial_draft
            .as_deref()
            .is_some_and(|text| !monocode_core::js::trim(text).is_empty())
            || props.inbox_card.is_some()
            || props.note_card.is_some()
            || props.handoff_card.is_some();
        let runner_live = props.busy && props.runner_enabled;
        let bar = render::BottomBar::new(&host, &props, window, cx);
        let mut this = Self {
            host,
            props,
            prompt,
            draft,
            context_items,
            has_value,
            attachments: Vec::new(),
            borrowed_attachment_ids: HashSet::new(),
            paste_error: None,
            file_drag: false,
            plus_open: false,
            modes: ModeSelection::default(),
            slash: None,
            skill_active: 0,
            creating_skill: false,
            create_error: None,
            create_busy: false,
            session_folder_open: false,
            session_folders: Vec::new(),
            session_folder_selected: false,
            mcp_picker_open: false,
            mcp_servers: McpServers::default(),
            mcp_insert_at: None,
            selected_mcp,
            mention: None,
            mention_active: 0,
            resend_edited: false,
            runner_live,
            skills: Vec::new(),
            mention_index: MentionIndex::default(),
            ranked_files: Vec::new(),
            draft_revision: 0,
            paste_generation: 0,
            pastes_in_flight: 0,
            submit_waiting: None,
            consumed_insert_id: None,
            next_resend: 0,
            pending_resend: None,
            queue: MessageQueueState::default(),
            meter: context_meter::MeterState::default(),
            runner: None,
            bar,
            chip_preview: chips::ChipPreview::default(),
            attachment_preview: None,
            attachment_images: HashMap::new(),
            runner_geometry: runner::RunnerGeometry::default(),
            header_views: Vec::new(),
            card_views: Vec::new(),
            top_bar_views: Vec::new(),
            _tasks: Vec::new(),
            _subscriptions: subscriptions,
        };
        this.install_decorator(cx);
        this.apply_prompt_style(cx);
        this.refresh_suggestions(cx);
        this.sync_runner(window, cx);
        if let Some(request) = this.props.insert_request.clone() {
            this.consume_insert(request, window, cx);
        }
        this
    }

    // Owner API.

    pub fn props(&self) -> &ComposerProps {
        &self.props
    }

    /// The composer box bounds from the last frame, in window coordinates.
    pub fn bounds(&self) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.runner_geometry.r#box.get()
    }

    pub fn prompt(&self) -> &Entity<PromptInput> {
        &self.prompt
    }

    /// The typed text.
    pub fn draft(&self) -> &str {
        &self.draft
    }

    pub fn context_items(&self) -> &[ChatContextItem] {
        &self.context_items
    }

    pub fn attachments(&self) -> &[Attachment] {
        &self.attachments
    }

    pub fn selected_mcp(&self) -> &[McpTag] {
        &self.selected_mcp
    }

    /// `hasValue`: something would be sent.
    pub fn has_value(&self) -> bool {
        self.has_value
    }

    /// Editing the last turn (`resendEdited`).
    pub fn is_editing_last_turn(&self) -> bool {
        self.resend_edited
    }

    pub fn paste_error(&self) -> Option<&str> {
        self.paste_error.as_deref()
    }

    pub fn set_header_views(&mut self, views: Vec<AnyView>, cx: &mut Context<Self>) {
        self.header_views = views;
        cx.notify();
    }

    pub fn set_card_views(&mut self, views: Vec<AnyView>, cx: &mut Context<Self>) {
        self.card_views = views;
        cx.notify();
    }

    pub fn set_top_bar_views(&mut self, views: Vec<AnyView>, cx: &mut Context<Self>) {
        self.top_bar_views = views;
        cx.notify();
    }

    pub fn open_model_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.props.hotkeys || !self.props.enabled || self.prompt.read(cx).is_composing() {
            return;
        }
        if let Some(picker) = &self.bar.model_picker {
            picker.update(cx, |picker, cx| picker.toggle_from_hotkey(window, cx));
        }
    }

    /// Replaces the prompt text as typing would, caret at the end. The
    /// usual input handling runs: tokens, pickers, MCP tag pruning.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = text.to_string();
        let end = text.len();
        self.prompt
            .update(cx, |prompt, cx| prompt.set_text(text, end, cx));
    }

    /// Adds context chips, as an "Add to chat" request would.
    pub fn set_context_items(&mut self, items: Vec<ChatContextItem>, cx: &mut Context<Self>) {
        self.context_items = items;
        self.sync_has_value();
        self.report_draft(cx);
        cx.notify();
    }

    /// Selects MCP servers for the draft, for a restored session.
    pub fn set_selected_mcp(&mut self, tags: Vec<McpTag>, cx: &mut Context<Self>) {
        self.selected_mcp = tags;
        self.prompt
            .update(cx, |prompt, cx| prompt.invalidate_decorations(cx));
        cx.notify();
    }

    /// Opens or closes the + menu.
    pub fn set_plus_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.plus_open = open;
        cx.notify();
    }

    /// Shows the context meter's details, as hovering it would.
    pub fn set_context_details_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.meter.hovered = open;
        cx.notify();
    }

    /// The runner's view of the transcript's jump-to-latest chevron, which
    /// the mascot hops.
    pub fn set_runner_obstacle(&mut self, bounds: Option<gpui::Bounds<gpui::Pixels>>) {
        self.runner_geometry.obstacle.set(bounds);
    }

    /// Focuses the prompt.
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        if !self.props.disabled {
            let handle = self.prompt.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        }
    }

    /// Re-reads the skill catalog and mention files, for when the host's
    /// caches changed (`subscribeSkills`, `subscribeProjectFiles`).
    pub fn refresh_suggestions(&mut self, cx: &mut Context<Self>) {
        let context = self.skill_context();
        self.skills = if self.props.remote_session {
            Vec::new()
        } else {
            self.host.skills(&context, cx)
        };
        let local = self.local_cwd();
        let files = if local.is_empty() {
            Vec::new()
        } else {
            self.host.mention_files(&local, cx)
        };
        self.mention_index = build_mention_index(&files);
        self.clamp_skill_active();
        self.refresh_ranked_files(cx);
        self.prompt
            .update(cx, |prompt, cx| prompt.invalidate_decorations(cx));
        cx.notify();
    }

    /// Applies new props and runs the React effects that watched them.
    pub fn set_props(&mut self, props: ComposerProps, window: &mut Window, cx: &mut Context<Self>) {
        let old = std::mem::replace(&mut self.props, props);
        if !self.props.enabled || self.props.disabled || old.session_id != self.props.session_id {
            self.attachment_preview = None;
        }
        let new = &self.props;
        let suggestions = old.harness != new.harness
            || old.execution_cwd != new.execution_cwd
            || old.session_id != new.session_id
            || old.remote_session != new.remote_session;
        if old.execution_cwd != new.execution_cwd
            || old.harness != new.harness
            || old.session_id != new.session_id
        {
            self.mcp_picker_open = false;
        }
        if old.cwd != new.cwd {
            self.skill_active = 0;
            self.mention_active = 0;
            self.session_folder_open = false;
            self.session_folder_selected = false;
        }
        if old.session_id != new.session_id {
            self.draft_revision += 1;
            self.set_resend_edited(false, cx);
        }
        if !self.props.edit_last_turn_supported && self.resend_edited {
            self.set_resend_edited(false, cx);
        }
        if !harness_supports_attachments(self.props.harness) && !self.attachments.is_empty() {
            for file in std::mem::take(&mut self.attachments) {
                if !self.borrowed_attachment_ids.remove(&file.id) {
                    self.host.revoke_attachment(&file, cx);
                }
            }
        }
        if old.inbox_card != self.props.inbox_card
            || old.note_card != self.props.note_card
            || old.handoff_card != self.props.handoff_card
        {
            self.sync_has_value();
        }
        if self.props.draft_reset_token.is_some()
            && old.draft_reset_token != self.props.draft_reset_token
        {
            self.reset_draft(window, cx);
        }
        if let Some(request) = self.props.insert_request.clone()
            && old.insert_request.as_ref().map(|r| r.id) != Some(request.id)
        {
            self.consume_insert(request, window, cx);
        }
        let refocus = self.props.focused
            && !self.props.disabled
            && (old.focused != self.props.focused
                || old.focus_token != self.props.focus_token
                || old.busy != self.props.busy
                || old.disabled != self.props.disabled);
        self.bar.sync(&self.props, cx);
        self.apply_prompt_style(cx);
        if suggestions {
            self.refresh_suggestions(cx);
        }
        self.sync_runner(window, cx);
        if refocus && !self.any_picker_open(cx) {
            self.focus(window, cx);
        }
        cx.notify();
    }

    // Derived values.

    pub(crate) fn remote(&self) -> bool {
        self.props.remote_session
    }

    /// Local indexes (files, skills) never read a remote session's path.
    pub(crate) fn local_cwd(&self) -> String {
        if self.remote() {
            String::new()
        } else {
            self.props.execution_cwd.clone()
        }
    }

    pub(crate) fn skill_context(&self) -> SkillContext {
        SkillContext {
            harness: self.props.harness,
            cwd: self.local_cwd(),
            session_id: self.props.session_id.clone(),
        }
    }

    pub(crate) fn has_native_commands(&self) -> bool {
        self.host.has_native_commands(self.props.harness)
    }

    pub(crate) fn mention_open(&self) -> bool {
        !self.remote()
            && self.mention.is_some()
            && (looks_like_project(&self.props.cwd) || self.props.notes_enabled)
    }

    /// `navigationEmpty`: nothing typed, attached, or carded.
    pub fn navigation_empty(&self) -> bool {
        self.draft.is_empty()
            && self.attachments.is_empty()
            && self.context_items.is_empty()
            && self.props.inbox_card.is_none()
            && self.props.note_card.is_none()
            && self.props.handoff_card.is_none()
    }

    pub(crate) fn skill_picker_open(&self) -> bool {
        self.creating_skill || self.slash.is_some()
    }

    pub(crate) fn picker_open(&self) -> bool {
        self.skill_picker_open() || self.session_folder_open || self.mcp_picker_open
    }

    /// True while a popup that should keep focus is open (the focus effect
    /// skips stealing focus then).
    pub fn any_picker_open(&self, cx: &App) -> bool {
        self.picker_open()
            || self.mention_open()
            || self.plus_open
            || self.bar.any_open(cx)
            || self.attachment_preview.is_some()
    }

    /// `slashItems`: MonoCode's commands, then the catalog without the names
    /// those commands own.
    pub(crate) fn slash_items(&self) -> Vec<Skill> {
        if self.remote() {
            let mut items = Vec::new();
            if self.props.remote_features.is_some_and(|f| f.plan) {
                items.push(commands::plan_command());
            }
            items.push(commands::compact_command());
            return items;
        }
        let mut items = vec![
            commands::session_folder_command(),
            commands::mcp_command(),
            commands::operator_command(),
        ];
        if !self.props.hide_top_bar {
            items.push(commands::orchestrator_command());
        }
        items.push(commands::plan_command());
        if self.props.can_save_draft {
            items.push(commands::draft_command());
        }
        items.push(commands::compact_command());
        if monocode_core::btw::supports_btw_harness(Some(self.props.harness)) {
            items.push(commands::btw_command());
        }
        let reserved = [
            commands::PLAN,
            commands::COMPACT,
            commands::SESSION_FOLDER,
            commands::MCP,
            commands::ORCHESTRATOR,
            commands::DRAFT,
            commands::BTW,
        ];
        items.extend(
            self.skills
                .iter()
                .filter(|skill| {
                    ![commands::OPERATOR, "mono", "monocode"].contains(&skill.name.as_str())
                        && (skill.kind == super::model::skills::SkillKind::Native
                            || !reserved.contains(&skill.name.as_str()))
                })
                .cloned(),
        );
        items
    }

    pub(crate) fn ranked_skills(&self) -> Vec<Skill> {
        let limit = if self.has_native_commands() {
            usize::MAX
        } else {
            MAX_PICKER
        };
        let query = self.slash.as_ref().map(|t| t.query.as_str()).unwrap_or("");
        rank_skills(&self.slash_items(), query, limit)
    }

    /// `skillNames`: every invocation the picker lists.
    pub(crate) fn skill_names(&self) -> HashSet<String> {
        self.slash_items()
            .into_iter()
            .map(|skill| skill.invocation)
            .collect()
    }

    pub(crate) fn attachments_supported(&self) -> bool {
        (!self.remote() || self.props.remote_features.is_some_and(|f| f.attachments))
            && harness_supports_attachments(self.props.harness)
    }

    pub(crate) fn leading_mode(&self) -> Option<ModeCommandToken> {
        leading_mode_command(&self.draft, &self.skill_names())
    }

    fn leading_is(&self, mode: Mode) -> bool {
        self.leading_mode().is_some_and(|token| token.mode == mode)
    }

    pub(crate) fn operator_active(&self) -> bool {
        self.modes.operator || self.leading_is(Mode::Operator)
    }

    pub(crate) fn orchestration_active(&self) -> bool {
        self.modes.orchestration || self.leading_is(Mode::Orchestrator)
    }

    pub(crate) fn draft_active(&self) -> bool {
        self.modes.draft || self.leading_is(Mode::Draft)
    }

    pub(crate) fn plan_active(&self) -> bool {
        self.modes.plan || self.leading_is(Mode::Plan)
    }

    pub(crate) fn mode_active(&self, mode: Mode) -> bool {
        match mode {
            Mode::Plan => self.plan_active(),
            Mode::Operator => self.operator_active(),
            Mode::Orchestrator => self.orchestration_active(),
            Mode::Draft => self.draft_active(),
            Mode::Btw => false,
        }
    }

    /// `syncHasValue`.
    pub(crate) fn sync_has_value(&mut self) {
        self.attachment_images
            .retain(|id, _| self.attachments.iter().any(|file| &file.id == id));
        self.has_value = !monocode_core::js::trim(&self.draft).is_empty()
            || !self.attachments.is_empty()
            || !self.context_items.is_empty()
            || self.props.inbox_card.is_some()
            || self.props.note_card.is_some()
            || self.props.handoff_card.is_some();
    }

    pub(crate) fn clamp_skill_active(&mut self) {
        let len = self.ranked_skills().len();
        self.skill_active = if len == 0 {
            0
        } else {
            self.skill_active.min(len - 1)
        };
    }

    pub(crate) fn refresh_ranked_files(&mut self, cx: &mut Context<Self>) {
        if !self.mention_open() {
            self.ranked_files.clear();
            self.mention_active = 0;
            return;
        }
        let query = self
            .mention
            .as_ref()
            .map(|t| t.query.clone())
            .unwrap_or_default();
        let cwd = self.props.execution_cwd.clone();
        self.ranked_files = self.host.rank_mentions(&cwd, &query, cx);
        self.mention_active = if self.ranked_files.is_empty() {
            0
        } else {
            self.mention_active.min(self.ranked_files.len() - 1)
        };
    }

    pub(crate) fn set_resend_edited(&mut self, editing: bool, cx: &mut Context<Self>) {
        self.resend_edited = editing;
        cx.emit(ComposerEvent::EditingLastTurnChange(editing));
    }

    /// The composed draft `onDraftChange` reported.
    pub(crate) fn report_draft(&self, cx: &mut Context<Self>) {
        let text = compose_chat_context(&self.draft, &self.context_items);
        self.host.draft_changed(&text, cx);
    }

    pub(crate) fn report_draft_text(&self, text: &str, cx: &mut Context<Self>) {
        self.host.draft_changed(text, cx);
    }

    pub(crate) fn save_mcp_tags(&self, cx: &mut Context<Self>) {
        if let Some(session_id) = self.props.session_id.as_deref() {
            self.host.save_mcp_tags(session_id, &self.selected_mcp, cx);
        }
    }

    pub(crate) fn prompt_text(&self, cx: &App) -> String {
        self.prompt.read(cx).text().to_string()
    }

    /// `el.value = next` plus a caret move, keeping `draft` in step.
    pub(crate) fn set_prompt(&mut self, text: String, cursor: usize, cx: &mut Context<Self>) {
        self.draft = text.clone();
        self.prompt
            .update(cx, |prompt, cx| prompt.set_text(text, cursor, cx));
    }

    // Prompt events.

    fn on_prompt_event(
        &mut self,
        _: &Entity<PromptInput>,
        event: &PromptInputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            PromptInputEvent::Changed => self.on_input(window, cx),
            PromptInputEvent::SelectionChanged => {
                self.sync_tokens(cx);
                cx.notify();
            }
            PromptInputEvent::Focused => cx.emit(ComposerEvent::Focus),
            PromptInputEvent::Blurred => cx.notify(),
        }
    }

    /// The textarea's `onInput`.
    fn on_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.prompt_text(cx);
        if value == self.draft {
            // A programmatic set the composer already applied.
            self.sync_tokens(cx);
            cx.notify();
            return;
        }
        if self.enter_btw_from_prefix(&value, window, cx) {
            return;
        }
        self.draft_revision += 1;
        self.draft = value.clone();
        let retained: Vec<McpTag> = self
            .selected_mcp
            .iter()
            .filter(|tag| !tagged_mcp_servers(&value, std::slice::from_ref(tag)).is_empty())
            .cloned()
            .collect();
        if retained.len() != self.selected_mcp.len() {
            self.selected_mcp = retained;
            self.save_mcp_tags(cx);
            self.prompt
                .update(cx, |prompt, cx| prompt.invalidate_decorations(cx));
        }
        self.paste_error = None;
        if self.session_folder_selected && !consume_session_folder_command(&value).matched {
            self.session_folder_selected = false;
        }
        self.sync_has_value();
        self.sync_tokens(cx);
        self.report_draft(cx);
        cx.notify();
    }

    /// `syncTokensFromTextarea`: open the slash or `@` picker for the token
    /// under the caret.
    pub(crate) fn sync_tokens(&mut self, cx: &mut Context<Self>) {
        if self.creating_skill {
            return;
        }
        let (text, cursor) = {
            let prompt = self.prompt.read(cx);
            (prompt.text().to_string(), prompt.selection_start())
        };
        let token = slash_token_at(&text, cursor, self.has_native_commands());
        let mention = if token.is_some() {
            None
        } else {
            mention_token_at(&text, cursor)
        };
        if token.as_ref().map(|t| &t.query) != self.slash.as_ref().map(|t| &t.query) {
            self.skill_active = 0;
        }
        let picker_opened = token.is_some() && self.slash.is_none();
        self.slash = token;
        if mention.as_ref().map(|t| &t.query) != self.mention.as_ref().map(|t| &t.query) {
            self.mention_active = 0;
        }
        self.mention = mention;
        if picker_opened && !self.remote() {
            let context = self.skill_context();
            let refresh =
                super::model::skills_context::picker_skill_refresh(self.has_native_commands());
            self.host.reload_skills(&context, refresh, cx);
        }
        self.clamp_skill_active();
        self.refresh_ranked_files(cx);
    }

    /// The draft-reset effect (`draftResetToken`).
    pub(crate) fn reset_draft(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.paste_generation += 1;
        self.draft_revision += 1;
        self.draft.clear();
        self.prompt
            .update(cx, |prompt, cx| prompt.reset_text(String::new(), cx));
        self.context_items.clear();
        self.report_draft_text("", cx);
        self.modes.draft = false;
        self.modes.plan = false;
        self.modes.orchestration = false;
        self.session_folder_selected = false;
        self.session_folder_open = false;
        self.mcp_picker_open = false;
        self.selected_mcp.clear();
        self.save_mcp_tags(cx);
        self.plus_open = false;
        self.slash = None;
        self.mention = None;
        self.creating_skill = false;
        self.create_error = None;
        self.sync_has_value();
        cx.notify();
    }

    /// The insert-request effect (`consumeComposerInsert`).
    pub(crate) fn consume_insert(
        &mut self,
        request: ComposerInsertRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fresh = Some(request.id) != self.consumed_insert_id;
        let value = self.prompt_text(cx);
        let result = consume_composer_insert(
            &value,
            &self.context_items,
            self.consumed_insert_id,
            Some(&request),
        );
        self.consumed_insert_id = result.consumed_id;
        if result.changed {
            let end = result.draft.len();
            self.set_prompt(result.draft, end, cx);
            self.context_items = result.context;
            self.sync_has_value();
            self.slash = None;
            self.mention = None;
            self.creating_skill = false;
            self.create_error = None;
            self.report_draft(cx);
        }
        // Adding the same chip twice changes nothing, but the user still
        // expects to land in the composer.
        if fresh {
            self.focus(window, cx);
        }
        cx.emit(ComposerEvent::InsertRequestConsumed(request.id));
        cx.notify();
    }

    /// `removeContextItem`.
    pub(crate) fn remove_context_item(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.context_items
            .retain(|item| super::model::chat_context::chat_context_key(item) != key);
        self.draft_revision += 1;
        self.sync_has_value();
        self.report_draft(cx);
        self.focus(window, cx);
        cx.notify();
    }

    fn apply_prompt_style(&mut self, cx: &mut Context<Self>) {
        let theme = monocode_ui::Theme::of(cx).clone();
        let props = self.props.clone();
        let placeholder = self.placeholder_text();
        self.prompt.update(cx, |prompt, cx| {
            let y = if props.shell { 16. } else { 12. };
            prompt.set_padding([y, 12., y, 12.], cx);
            prompt.set_max_height(Some(COMPOSER_MAX_HEIGHT), cx);
            prompt.set_placeholder(placeholder, cx);
            prompt.set_disabled(props.disabled, cx);
            prompt.set_colors(
                super::prompt_input::PromptColors {
                    selection: monocode_ui::color::with_alpha(theme.user_accent_or_accent(), 0.30),
                    placeholder: theme.content(0.40),
                    caret: theme.colors.content,
                },
                cx,
            );
        });
    }

    /// The textarea placeholder, by what the composer holds.
    pub(crate) fn placeholder_text(&self) -> SharedString {
        if self.props.worktree_removed {
            "Select a branch or worktree to continue…".into()
        } else if self.props.inbox_card.is_some() {
            "Add a note, or send to start…".into()
        } else if self.props.note_card.is_some() || !self.context_items.is_empty() {
            "Add a message, or send…".into()
        } else if self.props.handoff_card.is_some() {
            "Add context, or send to continue…".into()
        } else {
            self.props
                .placeholder
                .clone()
                .unwrap_or_else(|| "Ask, build, / for commands, @ for references... ".into())
        }
    }

    fn sync_runner(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.props.runner_enabled {
            self.runner_live = false;
        } else if self.props.busy {
            self.runner_live = true;
        }
        let show = self.runner_live && self.props.runner_enabled && !self.remote();
        match (&self.runner, show) {
            (None, true) => {
                let composer = cx.entity().downgrade();
                let props = self.props.clone();
                let geometry = self.runner_geometry.clone();
                self.runner =
                    Some(cx.new(|cx| {
                        runner::ComposerRunner::new(composer, geometry, &props, window, cx)
                    }));
            }
            (Some(runner), true) => {
                let props = self.props.clone();
                runner.update(cx, |runner, cx| runner.set_props(&props, cx));
            }
            (Some(_), false) => self.runner = None,
            (None, false) => {}
        }
    }

    /// `onExited`: the runner finished its exit hop.
    pub(crate) fn runner_exited(&mut self, cx: &mut Context<Self>) {
        self.runner_live = false;
        self.runner = None;
        cx.notify();
    }
}
