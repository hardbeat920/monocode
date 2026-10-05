//! Port of src/features/sessions/ui/BtwSheet.tsx: the side conversation over
//! the transcript. Tabs for every side thread in the session, the active
//! thread drawn through the main [`TranscriptView`], and a [`Composer`] for
//! it. The sheet morphs out of the session composer's box and folds back
//! into it on close; opening also sets off a [`BtwQuestionBurst`].
//!
//! GPUI has no clip-path, so the morph clips the sheet with a moving
//! `overflow_hidden` box over the same keyframes and timings. The session
//! composer is the owner's; it reads [`BtwSheet::home_opacity`] to fade in
//! step, the way the React sheet animated `[data-session-composer]`.

mod conversation;
mod host;
#[cfg(test)]
mod tests;

use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Entity, EventEmitter, FocusHandle,
    Focusable as _, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton,
    ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Task, WeakEntity, Window, div, px,
};
use monocode_core::block::{Block, BtwThreadStatus, ModelSettings};
use monocode_core::harness::DEFAULT_RUNTIME_MODE;
use monocode_core::{Attachment, HarnessId, ModelPrefs, ProjectProviders, Session};
use monocode_ui::color::with_alpha;
use monocode_ui::styled::glass_backdrop;
use monocode_ui::theme::CubicBezier;
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, icon, u};
use monocode_view_composer::composer::model::clipboard::ClipboardFile;
use monocode_view_composer::composer::model::mcp::McpTag;
use monocode_view_composer::composer::model::mentions::{ProjectFile, RankedFile};
use monocode_view_composer::composer::model::skills::Skill;
use monocode_view_composer::composer::prompt_input::Escape;
use monocode_view_composer::composer::{
    Composer, ComposerEvent, ComposerHost, ComposerProps, ComposerSubmission, McpServers,
    NewSkillScope, SkillContext,
};
use monocode_view_composer::pickers::ModelSource;
use monocode_view_composer::pickers::anchor::BoundsCell;

pub use conversation::{BtwConversation, BtwConversationProps, BtwTab, Seed, compact_question};
pub use host::{BtwHost, BtwRequest, BtwThreadBlocksInput};

use super::btw_burst::{BtwQuestionBurst, BtwQuestionBurstEvent, BurstRect};
use super::style::{amber_300, btw_error_red, emerald_300, red_50, red_100, red_200, red_300};
use crate::transcript::{TranscriptConfig, TranscriptEvent, TranscriptView};

/// The session composer's box, which the sheet grows out of. The owner puts
/// `probe()` inside its composer box.
pub type BoxProbe = BoundsCell;

/// `max-w-4xl`.
const SHEET_MAX_WIDTH: f32 = 896.;
const OPEN_MS: f32 = 360.;
const CLOSE_MS: f32 = 260.;
const OPEN_EASE: CubicBezier = CubicBezier(0.22, 1., 0.36, 1.);
const CLOSE_EASE: CubicBezier = CubicBezier(0.4, 0., 0.2, 1.);
const EASE_IN: CubicBezier = CubicBezier(0.42, 0., 1., 1.);

/// What the sheet shows and how.
#[derive(Clone)]
pub struct BtwSheetProps {
    pub conversation: BtwConversationProps,
    pub cwd: Option<String>,
    /// False while another tab is in front.
    pub visible: bool,
    /// `prefers-reduced-motion`: no morph and no burst.
    pub reduced_motion: bool,
    /// The app's composer settings (runner, notes, model controls,
    /// animation). The sheet sets the side-question fields itself.
    pub composer: ComposerProps,
    /// The transcript settings (layout, catalog, notes). The sheet sets
    /// `visible` and turns approvals off.
    pub transcript: TranscriptConfig,
}

impl Default for BtwSheetProps {
    fn default() -> Self {
        Self {
            conversation: BtwConversationProps::default(),
            cwd: None,
            visible: true,
            reduced_motion: false,
            composer: ComposerProps::default(),
            transcript: TranscriptConfig::default(),
        }
    }
}

/// What the sheet reports.
#[derive(Clone, Debug, PartialEq)]
pub enum BtwSheetEvent {
    /// The side thread's transcript: open a file or diff, save a note.
    Transcript(TranscriptEvent),
    /// The sheet opened or started closing.
    OpenChanged(bool),
}

/// A box in overlay coordinates, in px.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn lerp(self, to: Rect, t: f32) -> Rect {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Rect {
            x: mix(self.x, to.x),
            y: mix(self.y, to.y),
            w: mix(self.w, to.w),
            h: mix(self.h, to.h),
        }
    }

    fn from_bounds(bounds: Bounds<Pixels>, origin: gpui::Point<Pixels>) -> Rect {
        Rect {
            x: f32::from(bounds.origin.x - origin.x),
            y: f32::from(bounds.origin.y - origin.y),
            w: f32::from(bounds.size.width),
            h: f32::from(bounds.size.height),
        }
    }
}

/// One run of the morph: opening or closing, from where the last one left
/// off.
#[derive(Clone, Copy, Debug)]
struct Morph {
    open: bool,
    /// `None` until the sheet has painted once, so the morph plays from its
    /// start instead of mid-way.
    started: Option<Instant>,
    from_clip: Option<Rect>,
    from_opacity: Option<f32>,
    from_frame: Option<Rect>,
    /// Draws one fixed moment, for screenshots.
    frozen_ms: Option<f32>,
}

/// The morph's state at one moment.
#[derive(Clone, Copy, Debug)]
struct Pose {
    clip: Rect,
    opacity: f32,
    frame: Option<(Rect, f32)>,
    composer_opacity: f32,
    home_opacity: f32,
    done: bool,
}

fn keyframes(frames: &[(f32, f32)], t: f32) -> f32 {
    let mut previous = frames[0];
    for &frame in frames {
        if t <= frame.0 {
            let span = frame.0 - previous.0;
            if span <= 0. {
                return frame.1;
            }
            return previous.1 + (frame.1 - previous.1) * (t - previous.0) / span;
        }
        previous = frame;
    }
    previous.1
}

impl Morph {
    fn pose(&self, full: Rect, home: Rect, away: Option<Rect>) -> Pose {
        let (duration, ease) = if self.open {
            (OPEN_MS, OPEN_EASE)
        } else {
            (CLOSE_MS, CLOSE_EASE)
        };
        let raw = self.started.map_or(0., |started| {
            let elapsed = self
                .frozen_ms
                .unwrap_or_else(|| started.elapsed().as_secs_f32() * 1000.);
            (elapsed / duration).clamp(0., 1.)
        });
        let p = ease.ease(raw);
        let (clip, opacity) = if self.open {
            (
                self.from_clip.unwrap_or(home).lerp(full, p),
                keyframes(
                    &[(0., self.from_opacity.unwrap_or(0.)), (0.3, 1.), (1., 1.)],
                    p,
                ),
            )
        } else {
            (
                self.from_clip.unwrap_or(full).lerp(home, p),
                keyframes(
                    &[(0., self.from_opacity.unwrap_or(1.)), (0.2, 1.), (1., 0.)],
                    p,
                ),
            )
        };
        let frame = away.map(|away| {
            let from = self
                .from_frame
                .unwrap_or(if self.open { home } else { away });
            let to = if self.open { away } else { home };
            (
                from.lerp(to, p),
                keyframes(&[(0., 0.), (0.15, 1.), (0.8, 1.), (1., 0.)], p),
            )
        });
        let composer_opacity = if self.open {
            keyframes(&[(0., 0.), (0.45, 0.), (1., 1.)], p)
        } else {
            1.
        };
        let home_opacity = if self.open {
            keyframes(&[(0., 1.), (0.3, 0.), (1., 0.)], p)
        } else {
            keyframes(&[(0., 0.), (0.55, 0.), (1., 1.)], p)
        };
        Pose {
            clip,
            opacity,
            frame,
            composer_opacity,
            home_opacity,
            done: self.started.is_some() && raw >= 1.,
        }
    }
}

/// The side composer's host: the session composer's host, except that the
/// sheet answers submit, stop, and draft changes.
struct SheetComposerHost {
    inner: Rc<dyn ComposerHost>,
    sheet: WeakEntity<BtwSheet>,
}

impl ComposerHost for SheetComposerHost {
    fn submit(&self, submission: ComposerSubmission, _: &mut Window, cx: &mut App) -> bool {
        self.sheet
            .update(cx, |sheet, cx| sheet.submit(&submission.text, cx))
            .unwrap_or(false)
    }

    fn stop(&self, _: &mut Window, cx: &mut App) {
        let sheet = self.sheet.clone();
        cx.defer(move |cx| {
            sheet.update(cx, |sheet, cx| sheet.stop(cx)).ok();
        });
    }

    fn draft_changed(&self, text: &str, cx: &mut App) {
        let sheet = self.sheet.clone();
        let text = text.to_string();
        cx.defer(move |cx| {
            sheet
                .update(cx, |sheet, _| sheet.conversation.change_draft(&text))
                .ok();
        });
    }

    fn load_mcp_tags(&self, session_id: &str, cx: &mut App) -> Vec<McpTag> {
        self.inner.load_mcp_tags(session_id, cx)
    }

    fn save_mcp_tags(&self, session_id: &str, tags: &[McpTag], cx: &mut App) {
        self.inner.save_mcp_tags(session_id, tags, cx)
    }

    fn attachments_from_paths(&self, paths: Vec<String>, cx: &mut App) -> Task<Vec<Attachment>> {
        self.inner.attachments_from_paths(paths, cx)
    }

    fn attachments_from_files(
        &self,
        files: Vec<ClipboardFile>,
        cx: &mut App,
    ) -> Task<Vec<Attachment>> {
        self.inner.attachments_from_files(files, cx)
    }

    fn pick_attachments(&self, window: &mut Window, cx: &mut App) -> Task<Vec<Attachment>> {
        self.inner.pick_attachments(window, cx)
    }

    fn revoke_attachment(&self, attachment: &Attachment, cx: &mut App) {
        self.inner.revoke_attachment(attachment, cx)
    }

    fn skills(&self, context: &SkillContext, cx: &mut App) -> Vec<Skill> {
        self.inner.skills(context, cx)
    }

    fn reload_skills(&self, context: &SkillContext, refresh: bool, cx: &mut App) {
        self.inner.reload_skills(context, refresh, cx)
    }

    fn has_native_commands(&self, harness: HarnessId) -> bool {
        self.inner.has_native_commands(harness)
    }

    fn raw_slash_commands(&self, harness: HarnessId) -> bool {
        self.inner.raw_slash_commands(harness)
    }

    fn create_skill(
        &self,
        cwd: &str,
        name: &str,
        scope: NewSkillScope,
        cx: &mut App,
    ) -> Task<Result<String, String>> {
        self.inner.create_skill(cwd, name, scope, cx)
    }

    fn mention_files(&self, cwd: &str, cx: &mut App) -> Vec<ProjectFile> {
        self.inner.mention_files(cwd, cx)
    }

    fn rank_mentions(&self, cwd: &str, query: &str, cx: &mut App) -> Vec<RankedFile> {
        self.inner.rank_mentions(cwd, query, cx)
    }

    fn mentions_loading(&self, cwd: &str, cx: &mut App) -> bool {
        self.inner.mentions_loading(cwd, cx)
    }

    fn model_source(&self, cx: &mut App) -> Option<Rc<dyn ModelSource>> {
        self.inner.model_source(cx)
    }

    fn model_prefs(&self, cx: &mut App) -> ModelPrefs {
        self.inner.model_prefs(cx)
    }

    fn project_providers(&self, cx: &mut App) -> ProjectProviders {
        self.inner.project_providers(cx)
    }

    fn mcp_servers(&self, cwd: &str, harness: HarnessId, cx: &mut App) -> McpServers {
        self.inner.mcp_servers(cwd, harness, cx)
    }
}

/// The sheet.
pub struct BtwSheet {
    host: Rc<dyn BtwHost>,
    conversation: BtwConversation,
    props: BtwSheetProps,
    origin: Option<BoxProbe>,
    focus: FocusHandle,
    composer_host: Rc<dyn ComposerHost>,
    transcript: Option<(String, Entity<TranscriptView>, Subscription)>,
    composer: Option<(String, u64, Entity<Composer>, Subscription)>,
    morph: Option<Morph>,
    burst: Option<(Entity<BtwQuestionBurst>, Subscription)>,
    overlay_bounds: BoundsCell,
    composer_bounds: BoundsCell,
    last_open: bool,
    opened_at: Option<Instant>,
    closed_at: Option<Instant>,
    focus_composer: bool,
    finish_timer: Option<Task<()>>,
    morph_clock: Option<f32>,
}

impl EventEmitter<BtwSheetEvent> for BtwSheet {}

impl BtwSheet {
    pub fn new(host: Rc<dyn BtwHost>, cx: &mut Context<Self>) -> Self {
        let composer_host: Rc<dyn ComposerHost> = Rc::new(SheetComposerHost {
            inner: host.composer_host(),
            sheet: cx.entity().downgrade(),
        });
        Self {
            host,
            conversation: BtwConversation::new(),
            props: BtwSheetProps::default(),
            origin: None,
            focus: cx.focus_handle(),
            composer_host,
            transcript: None,
            composer: None,
            morph: None,
            burst: None,
            overlay_bounds: BoundsCell::default(),
            composer_bounds: BoundsCell::default(),
            last_open: false,
            opened_at: None,
            closed_at: None,
            focus_composer: false,
            finish_timer: None,
            morph_clock: None,
        }
    }

    pub fn set_props(&mut self, props: BtwSheetProps, cx: &mut Context<Self>) {
        self.conversation
            .set_props(props.conversation.clone(), self.host.as_ref());
        self.props = props;
        self.after_change(cx);
    }

    /// The session composer's box the sheet grows out of. Without one the
    /// sheet opens and closes in place.
    pub fn set_origin(&mut self, origin: Option<BoxProbe>) {
        self.origin = origin;
    }

    pub fn conversation(&self) -> &BtwConversation {
        &self.conversation
    }

    pub fn is_open(&self) -> bool {
        self.conversation.open()
    }

    /// The sheet is on screen, open or closing.
    pub fn is_rendered(&self) -> bool {
        self.conversation.rendered()
    }

    pub fn tabs(&self) -> Vec<BtwTab> {
        self.conversation.tabs()
    }

    pub fn active_tab_id(&self) -> Option<String> {
        self.conversation.active_tab_id()
    }

    pub fn running(&self) -> bool {
        self.conversation.running()
    }

    pub fn can_start_draft(&self) -> bool {
        self.conversation.can_start_draft(self.host.as_ref())
    }

    /// The side composer, once the sheet has drawn.
    pub fn composer(&self) -> Option<Entity<Composer>> {
        self.composer
            .as_ref()
            .map(|(_, _, composer, _)| composer.clone())
    }

    /// The side thread's transcript.
    pub fn transcript(&self) -> Option<Entity<TranscriptView>> {
        self.transcript
            .as_ref()
            .map(|(_, transcript, _)| transcript.clone())
    }

    /// The burst, while it plays.
    pub fn burst(&self) -> Option<Entity<BtwQuestionBurst>> {
        self.burst.as_ref().map(|(burst, _)| burst.clone())
    }

    /// The active thread as transcript blocks.
    pub fn thread_blocks(&self) -> Vec<Block> {
        let host = self.host.as_ref();
        let conversation = &self.conversation;
        let messages = conversation.messages();
        let pending = conversation.pending_blocks();
        let model = conversation.model();
        host.thread_blocks(BtwThreadBlocksInput {
            messages: &messages,
            pending_blocks: Some(&pending),
            running: conversation.running(),
            updated_at: conversation.persisted().map(|thread| thread.updated_at),
            harness: Some(conversation.harness(host)),
            model: Some(&model),
        })
    }

    /// The session composer's opacity: it steps aside while the sheet sits
    /// over it, in step with the morph.
    pub fn home_opacity(&self) -> f32 {
        if let (Some(morph), Some((full, home, away))) = (self.morph, self.geometry()) {
            return morph.pose(full, home, away).home_opacity;
        }
        if self.conversation.open() { 0. } else { 1. }
    }

    /// The side composer's props.
    pub fn composer_props(&self) -> ComposerProps {
        let host = self.host.as_ref();
        let conversation = &self.conversation;
        let open = conversation.open();
        let visible = self.props.visible;
        let running = conversation.running();
        let harness = conversation.harness(host);
        let cwd = self.props.cwd.clone().unwrap_or_else(|| "~".into());
        ComposerProps {
            compact: true,
            enabled: visible && open,
            disabled: !conversation.can_ask(host),
            focused: visible && open && !running,
            harness,
            model: conversation.model(),
            model_settings: conversation.model_settings(host),
            runtime_mode: DEFAULT_RUNTIME_MODE,
            cwd: cwd.clone(),
            execution_cwd: cwd,
            session_id: conversation.active_tab_id(),
            hide_project_picker: true,
            hide_branch_picker: true,
            hide_top_bar: true,
            placeholder: Some("Ask a side question…".into()),
            allowed_model_harnesses: Some(vec![harness]),
            busy: running,
            allow_busy_submit: false,
            btw_enabled: false,
            can_save_draft: false,
            folders_enabled: false,
            ..self.props.composer.clone()
        }
    }

    // The hook's actions.

    /// Opens a new tab. `text` is sent right away, or left unsent in the side
    /// composer with `draft`. False when the sheet cannot open or the app
    /// rejected the question.
    pub fn open_with(&mut self, text: &str, draft: bool, cx: &mut Context<Self>) -> bool {
        let host = self.host.clone();
        let opened = self.conversation.open_with(text, draft, host.as_ref(), cx);
        self.after_change(cx);
        opened
    }

    pub fn start_draft(&mut self, cx: &mut Context<Self>) {
        let host = self.host.clone();
        self.conversation.start_draft(host.as_ref());
        self.after_change(cx);
    }

    pub fn select_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        self.conversation.select_tab(id);
        self.after_change(cx);
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.conversation.close();
        self.after_change(cx);
    }

    /// Escape: collapse the sheet from anywhere in the pane. The owner calls
    /// this for an Escape that reached it with nothing focused.
    pub fn escape(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.conversation.open() {
            return false;
        }
        self.close(cx);
        true
    }

    pub fn close_tab(&mut self, id: &str, cx: &mut Context<Self>) {
        let host = self.host.clone();
        self.conversation.close_tab(id, host.as_ref(), cx);
        self.after_change(cx);
    }

    /// Asks the active tab. False keeps the text in the composer.
    pub fn submit(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let host = self.host.clone();
        let accepted = self.conversation.submit(text, host.as_ref(), cx);
        self.after_change(cx);
        accepted
    }

    pub fn stop(&mut self, cx: &mut Context<Self>) {
        let host = self.host.clone();
        self.conversation.stop(host.as_ref(), cx);
        self.after_change(cx);
    }

    pub fn retry(&mut self, cx: &mut Context<Self>) {
        let host = self.host.clone();
        self.conversation.retry(host.as_ref(), cx);
        self.after_change(cx);
    }

    pub fn change_model(&mut self, harness: HarnessId, model: &str, cx: &mut Context<Self>) {
        let host = self.host.clone();
        self.conversation
            .change_model(harness, model, host.as_ref(), cx);
        self.after_change(cx);
    }

    pub fn change_model_settings(&mut self, settings: ModelSettings, cx: &mut Context<Self>) {
        let host = self.host.clone();
        self.conversation
            .change_model_settings(settings, host.as_ref(), cx);
        self.after_change(cx);
    }

    // Open, close, and the morph.

    /// Draws the morph (and the burst) at one fixed moment, `ms` into the
    /// next open or close, for screenshots.
    pub fn freeze_morph_at(&mut self, ms: Option<f32>) {
        self.morph_clock = ms;
    }

    /// The overlay's full sheet box, the origin box, and the side
    /// composer's box, in overlay coordinates.
    fn geometry(&self) -> Option<(Rect, Rect, Option<Rect>)> {
        let overlay = self.overlay_bounds.get()?;
        let home = self.origin.as_ref()?.get()?;
        if f32::from(home.size.width) <= 0. || f32::from(home.size.height) <= 0. {
            return None;
        }
        let full = sheet_rect(overlay, px(16.));
        let away = self
            .composer_bounds
            .get()
            .map(|bounds| Rect::from_bounds(bounds, overlay.origin));
        Some((full, Rect::from_bounds(home, overlay.origin), away))
    }

    fn can_morph(&self) -> bool {
        !self.props.reduced_motion
            && self
                .origin
                .as_ref()
                .is_some_and(|origin| origin.get().is_some())
    }

    /// The `[open, rendered]` layout effect: start the morph, or finish a
    /// close right away when there is nothing to morph from.
    fn after_change(&mut self, cx: &mut Context<Self>) {
        let open = self.conversation.open();
        if open != self.last_open {
            self.last_open = open;
            cx.emit(BtwSheetEvent::OpenChanged(open));
            let current = self.current_pose();
            if self.can_morph() {
                let running = self.morph.is_some_and(|morph| morph.started.is_some());
                self.morph = Some(Morph {
                    open,
                    started: if open { None } else { Some(Instant::now()) },
                    from_clip: running.then(|| current.map(|pose| pose.clip)).flatten(),
                    from_opacity: running.then(|| current.map(|pose| pose.opacity)).flatten(),
                    from_frame: running
                        .then(|| current.and_then(|pose| pose.frame.map(|frame| frame.0)))
                        .flatten(),
                    frozen_ms: self.morph_clock,
                });
                if !open {
                    self.schedule_finish(cx);
                }
            } else {
                self.morph = None;
                if !open {
                    self.conversation.finish_close();
                }
            }
            if open {
                self.opened_at = Some(Instant::now());
                self.closed_at = None;
                self.focus_composer = true;
            } else {
                self.closed_at = Some(Instant::now());
            }
        } else if !open && self.conversation.rendered() && self.morph.is_none() {
            self.conversation.finish_close();
        }
        self.sync_transcript(cx);
        cx.notify();
    }

    fn current_pose(&self) -> Option<Pose> {
        let morph = self.morph?;
        let (full, home, away) = self.geometry()?;
        Some(morph.pose(full, home, away))
    }

    /// Ends a close once its morph has played.
    fn schedule_finish(&mut self, cx: &mut Context<Self>) {
        self.finish_timer = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(CLOSE_MS as u64 + 16))
                .await;
            this.update(cx, |this, cx| {
                if !this.conversation.open() {
                    this.morph = None;
                    this.conversation.finish_close();
                    this.sync_transcript(cx);
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// Starts a pending morph once the sheet has painted, and sets off the
    /// burst on a fresh open.
    fn tick_morph(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut morph) = self.morph else {
            return;
        };
        let Some((full, home, away)) = self.geometry() else {
            if morph.started.is_none() {
                window.request_animation_frame();
            }
            return;
        };
        if morph.started.is_none() {
            morph.started = Some(Instant::now());
            self.morph = Some(morph);
            if morph.open
                && morph.from_clip.is_none()
                && let Some(away) = away
            {
                self.start_burst(away, cx);
            }
        }
        let pose = morph.pose(full, home, away);
        if pose.done {
            self.morph = None;
            if !morph.open {
                self.conversation.finish_close();
                self.sync_transcript(cx);
            }
        }
        if morph.frozen_ms.is_none() {
            window.request_animation_frame();
        }
    }

    fn start_burst(&mut self, rect: Rect, cx: &mut Context<Self>) {
        let frozen = self.morph_clock;
        let burst = cx.new(|cx| {
            let mut burst = BtwQuestionBurst::new(
                BurstRect {
                    left: rect.x,
                    top: rect.y,
                    width: rect.w,
                    height: rect.h,
                },
                cx,
            );
            if let Some(ms) = frozen {
                burst.freeze_at(ms, cx);
            }
            burst
        });
        let subscription = cx.subscribe(&burst, |this, _, _: &BtwQuestionBurstEvent, cx| {
            this.burst = None;
            cx.notify();
        });
        self.burst = Some((burst, subscription));
    }

    // Children.

    fn transcript_session(&self) -> Session {
        let host = self.host.as_ref();
        let conversation = &self.conversation;
        let mut session = Session::blank(
            conversation.active_tab_id().unwrap_or_default(),
            conversation.harness(host),
            conversation.model(),
            self.props.cwd.clone().unwrap_or_else(|| "~".into()),
        );
        session.blocks = self.thread_blocks();
        session.busy = Some(conversation.running());
        session.model_settings = conversation.model_settings(host);
        session
    }

    /// The side thread renders through the main transcript, so it anchors,
    /// follows, and folds its work exactly like the conversation behind it.
    /// A new tab gets a new transcript.
    fn sync_transcript(&mut self, cx: &mut Context<Self>) {
        if !self.conversation.rendered() {
            self.transcript = None;
            return;
        }
        let key = self.conversation.active_tab_id().unwrap_or_default();
        if self.transcript.as_ref().is_none_or(|(id, _, _)| *id != key) {
            let transcript = cx.new(TranscriptView::new);
            let subscription = cx.subscribe(&transcript, |_, _, event: &TranscriptEvent, cx| {
                cx.emit(BtwSheetEvent::Transcript(event.clone()));
            });
            self.transcript = Some((key, transcript, subscription));
        }
        let session = self.transcript_session();
        let config = TranscriptConfig {
            visible: self.props.visible && self.conversation.open(),
            approvals: false,
            ..self.props.transcript.clone()
        };
        let Some((_, transcript, _)) = &self.transcript else {
            return;
        };
        transcript.update(cx, |view, cx| {
            let changed = view.session().is_none_or(|current| {
                current.blocks != session.blocks
                    || current.busy != session.busy
                    || current.model != session.model
                    || current.harness != session.harness
            });
            if view.config().visible != config.visible {
                view.set_config(config, cx);
            }
            if changed {
                view.set_session(Arc::new(session), cx);
            }
        });
    }

    /// A composer per tab and seed, as `key={activeTabId:seed.key}` did.
    fn sync_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab_id) = self.conversation.active_tab_id() else {
            self.composer = None;
            return;
        };
        let seed = self.conversation.seed().clone();
        let props = self.composer_props();
        let current = self
            .composer
            .as_ref()
            .filter(|(id, key, _, _)| *id == tab_id && *key == seed.key)
            .map(|(_, _, composer, _)| composer.clone());
        match current {
            Some(composer) => {
                if composer.read(cx).props() != &props {
                    composer.update(cx, |composer, cx| composer.set_props(props, window, cx));
                }
            }
            None => {
                let host = self.composer_host.clone();
                let initial = Some(seed.text.clone());
                let composer = cx.new(|cx| Composer::new(host, props, initial, window, cx));
                let subscription =
                    cx.subscribe(
                        &composer,
                        |this, _, event: &ComposerEvent, cx| match event {
                            ComposerEvent::ModelChange { harness, model } => {
                                this.change_model(*harness, model, cx)
                            }
                            ComposerEvent::ModelSettingsChange(settings) => {
                                this.change_model_settings(settings.clone(), cx)
                            }
                            _ => {}
                        },
                    );
                self.composer = Some((tab_id, seed.key, composer, subscription));
                if self.conversation.open() {
                    self.focus_composer = true;
                }
            }
        }
        if self.focus_composer && self.conversation.open() && self.props.visible {
            self.focus_composer = false;
            if let Some((_, _, composer, _)) = &self.composer {
                let composer = composer.clone();
                window.defer(cx, move |window, cx| {
                    let handle = composer.read(cx).focus_handle(cx);
                    window.focus(&handle, cx);
                });
            }
        }
    }

    // Drawing.

    fn render_tabs(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let active = self.conversation.active_tab_id();
        let mut list = div()
            .id("btw-tabs")
            .flex()
            .min_w_0()
            .flex_1()
            .items_center()
            .gap(u(4.))
            .overflow_x_scroll();
        for tab in self.conversation.tabs() {
            let selected = active.as_deref() == Some(tab.id.as_str());
            let label = compact_question(tab.question.as_deref());
            let hover = theme.content(0.05);
            let hover_ink = theme.content(0.80);
            let status = tab.status.map(|status| match status {
                BtwThreadStatus::Running => amber_300(),
                BtwThreadStatus::Error => red_300(),
                BtwThreadStatus::Ready => emerald_300(),
            });
            let select_id = tab.id.clone();
            let close_id = tab.id.clone();
            let close_ink = theme.colors.content;
            let close_hover = theme.content(0.10);
            let saved = tab.thread.is_some();
            list = list.child(
                div()
                    .id(SharedString::from(format!("btw-tab:{}", tab.id)))
                    .group("btw-tab")
                    .flex()
                    .flex_none()
                    .h(u(28.))
                    .max_w(u(240.))
                    .items_center()
                    .rounded(u(theme.radius.md))
                    .when(selected, |el| {
                        el.bg(theme.content(0.10)).text_color(theme.colors.content)
                    })
                    .when(!selected, |el| {
                        el.text_color(theme.content(0.50))
                            .hover(move |style| style.bg(hover).text_color(hover_ink))
                    })
                    .child(
                        div()
                            .id(SharedString::from(format!("btw-tab-select:{}", tab.id)))
                            .flex()
                            .h_full()
                            .min_w_0()
                            .items_center()
                            .gap(u(6.))
                            .rounded(u(theme.radius.md))
                            .pr(u(4.))
                            .pl(u(10.))
                            .text_px(12.)
                            .when_some(tab.question.clone(), |el, question| {
                                el.tooltip(tooltip(question))
                            })
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.select_tab(&select_id, cx)),
                            )
                            .children(status.map(|color| {
                                div().flex_none().size(u(6.)).rounded_full().bg(color)
                            }))
                            .child(div().min_w_0().truncate().child(label)),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("btw-tab-close:{}", tab.id)))
                            .mr(u(4.))
                            .flex()
                            .flex_none()
                            .size(u(20.))
                            .items_center()
                            .justify_center()
                            .rounded(u(theme.radius.sm))
                            .when(!selected, |el| {
                                el.opacity(0.)
                                    .group_hover("btw-tab", |style| style.opacity(1.))
                            })
                            .hover(move |style| style.bg(close_hover))
                            .tooltip(tooltip(if saved { "Delete" } else { "Discard" }))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.close_tab(&close_id, cx)),
                            )
                            .child(
                                icon(IconName::X)
                                    .size(u(12.))
                                    .text_color(theme.content(0.40))
                                    .group_hover("btw-tab", move |style| {
                                        style.text_color(close_ink)
                                    }),
                            ),
                    ),
            );
        }
        let square = |id: &'static str, glyph: IconName, size: f32| {
            let hover = theme.content(0.08);
            div()
                .id(id)
                .flex()
                .flex_none()
                .size(u(28.))
                .items_center()
                .justify_center()
                .rounded(u(theme.radius.md))
                .hover(move |style| style.bg(hover))
                .child(icon(glyph).size(u(size)).text_color(theme.content(0.45)))
        };
        div()
            .flex()
            .h(u(44.))
            .flex_none()
            .items_center()
            .gap(u(4.))
            .border_b_1()
            .border_color(theme.content(0.08))
            .pr(u(8.))
            .pl(u(10.))
            .child(list)
            .when(self.can_start_draft(), |el| {
                el.child(
                    square("btw-new", IconName::Plus, 14.)
                        .tooltip(tooltip("New side question"))
                        .on_click(cx.listener(|this, _, _, cx| this.start_draft(cx))),
                )
            })
            .child(
                square("btw-back", IconName::ChevronDown, 16.)
                    .tooltip(tooltip("Back to the conversation (Esc)"))
                    .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
            )
            .into_any_element()
    }

    fn render_error(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let persisted = self.conversation.persisted()?;
        if persisted.status != BtwThreadStatus::Error {
            return None;
        }
        let harness = self.conversation.harness(self.host.as_ref());
        let message = persisted
            .error
            .filter(|error| !error.is_empty())
            .unwrap_or_else(|| format!("{} could not answer this side question.", harness.title()));
        let hover = with_alpha(red_200(), 0.10);
        let hover_ink = red_50();
        Some(
            div()
                .flex_none()
                .px(u(20.))
                .pb(u(12.))
                .child(
                    div()
                        .flex()
                        .items_start()
                        .justify_between()
                        .gap(u(12.))
                        .px(u(11.))
                        .py(u(10.))
                        .border_1()
                        .border_color(with_alpha(btw_error_red(), 0.24))
                        .rounded(u(10.))
                        .bg(with_alpha(btw_error_red(), 0.07))
                        .child(
                            div()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_px(10.)
                                        .medium()
                                        .text_color(with_alpha(red_200(), 0.70))
                                        .child("COULDN’T FINISH"),
                                )
                                .child(
                                    div()
                                        .mt(u(4.))
                                        .text_px(12.)
                                        .line_height(u(18.))
                                        .text_color(with_alpha(red_100(), 0.75))
                                        .child(message),
                                ),
                        )
                        .child(
                            div()
                                .id("btw-retry")
                                .group("btw-retry")
                                .flex()
                                .flex_none()
                                .h(u(24.))
                                .items_center()
                                .gap(u(4.))
                                .rounded(u(theme.radius.md))
                                .px(u(8.))
                                .text_px(11.)
                                .medium()
                                .text_color(with_alpha(red_100(), 0.80))
                                .hover(move |style| style.bg(hover).text_color(hover_ink))
                                .on_click(cx.listener(|this, _, _, cx| this.retry(cx)))
                                .child(
                                    icon(IconName::RefreshCw)
                                        .size(u(12.))
                                        .text_color(with_alpha(red_100(), 0.80))
                                        .group_hover("btw-retry", move |style| {
                                            style.text_color(hover_ink)
                                        }),
                                )
                                .child("Retry"),
                        ),
                )
                .into_any_element(),
        )
    }

    /// `btw-body-in` on open and `btw-fade-out` on close.
    fn body_motion(&self) -> (f32, f32) {
        if self.props.reduced_motion {
            return (1., 0.);
        }
        if !self.conversation.open() {
            let t = self.closed_at.map_or(1., |at| {
                (at.elapsed().as_secs_f32() * 1000. / 120.).clamp(0., 1.)
            });
            return (1. - EASE_IN.ease(t), 0.);
        }
        let Some(opened) = self.opened_at else {
            return (1., 0.);
        };
        let t = ((opened.elapsed().as_secs_f32() * 1000. - 100.) / 240.).clamp(0., 1.);
        let eased = OPEN_EASE.ease(t);
        (eased, 10. * (1. - eased))
    }

    fn render_sheet(
        &mut self,
        full: Option<Rect>,
        composer_opacity: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let (body_opacity, body_lift) = self.body_motion();
        let animating_body = body_opacity < 1. || (!self.conversation.open() && body_opacity > 0.);
        if animating_body && !self.props.reduced_motion {
            window.request_animation_frame();
        }
        let closing_fade = if self.conversation.open() {
            1.
        } else {
            body_opacity
        };
        let body = |el: gpui::Div| el.opacity(body_opacity).relative().top(px(body_lift));
        let tabs = self.render_tabs(&theme, cx);
        let error = self.render_error(&theme, cx);
        let transcript = self.transcript();
        let composer = self.composer();
        let radius = theme.radius.lg;
        let mut content = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .font_family(theme.fonts.sans.clone())
            .text_px(14.)
            .text_color(theme.colors.content)
            .child(body(div()).flex_none().child(tabs))
            .children(transcript.map(|transcript| {
                body(div())
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(transcript)
            }))
            .children(error.map(|error| body(div()).flex_none().child(error)));
        // Same inset as the docked composer, so the morph starts on its box.
        content = content.child(
            div()
                .flex_none()
                .p(u(6.))
                .pt_0()
                .opacity(composer_opacity * closing_fade)
                .child(
                    div()
                        .relative()
                        .child(self.composer_bounds.probe())
                        .children(composer),
                ),
        );
        let glass = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .rounded_t(u(radius))
            .border_t_1()
            .border_l_1()
            .border_r_1()
            .border_color(theme.content(0.10))
            .shadow_xl()
            .child(glass_backdrop(radius, 24., theme.colors.popover_backdrop));
        let mut sheet = div()
            .id("btw-sheet")
            .relative()
            .size_full()
            .track_focus(&self.focus)
            .child(glass)
            .child(content);
        if let Some(full) = full {
            sheet = sheet.w(px(full.w)).h(px(full.h));
        }
        sheet.into_any_element()
    }
}

/// `.btw-sheet`: `inset-x-0 bottom-0 mx-auto max-w-4xl` with
/// `top: clamp(2.5rem, 18%, 12rem)`, in overlay coordinates.
fn sheet_rect(overlay: Bounds<Pixels>, rem: Pixels) -> Rect {
    let rem = f32::from(rem);
    let width = f32::from(overlay.size.width);
    let height = f32::from(overlay.size.height);
    let w = width.min(SHEET_MAX_WIDTH * rem / 16.);
    let top = (height * 0.18).clamp(2.5 * rem, 12. * rem);
    Rect {
        x: (width - w) / 2.,
        y: top,
        w,
        h: (height - top).max(0.),
    }
}

impl Render for BtwSheet {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.conversation.rendered() {
            self.composer = None;
            self.overlay_bounds.clear();
            self.composer_bounds.clear();
            return div().into_any_element();
        }
        self.sync_transcript(cx);
        self.sync_composer(window, cx);
        self.tick_morph(window, cx);
        if !self.conversation.rendered() {
            return div().into_any_element();
        }
        let theme = Theme::of(cx).clone();
        let open = self.conversation.open();
        let rem = window.rem_size();
        let full = self
            .overlay_bounds
            .get()
            .map(|overlay| sheet_rect(overlay, rem));
        let pose = self.current_pose();
        let composer_opacity = pose.map_or(1., |pose| pose.composer_opacity);
        let sheet = self.render_sheet(full, composer_opacity, window, cx);

        let mut overlay = div()
            .id("btw-overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .on_action(cx.listener(|this, _: &Escape, _, cx| {
                this.escape(cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.escape(cx) {
                    cx.stop_propagation();
                }
            }))
            .child(self.overlay_bounds.probe());
        if open {
            overlay = overlay.child(div().absolute().top_0().left_0().size_full().on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close(cx)),
            ));
        }
        overlay = match (full, pose) {
            (Some(full), Some(pose)) => {
                // The clip box moves between the composer box and the sheet;
                // the sheet stays put inside it, as `clip-path: inset()` did.
                let clip = pose.clip;
                overlay.child(
                    div()
                        .absolute()
                        .left(px(clip.x))
                        .top(px(clip.y))
                        .w(px(clip.w.max(0.)))
                        .h(px(clip.h.max(0.)))
                        .overflow_hidden()
                        .opacity(pose.opacity)
                        .child(
                            div()
                                .absolute()
                                .left(px(full.x - clip.x))
                                .top(px(full.y - clip.y))
                                .child(sheet),
                        ),
                )
            }
            (Some(full), None) => overlay.child(
                div()
                    .absolute()
                    .left(px(full.x))
                    .top(px(full.y))
                    .child(sheet),
            ),
            // The first frame, before the overlay is measured. A morph
            // waiting to start keeps the sheet hidden until then.
            (None, _) => overlay.child(
                div()
                    .absolute()
                    .left_0()
                    .bottom_0()
                    .size_full()
                    .when(self.morph.is_some(), |el| el.opacity(0.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_end()
                    .child(
                        div()
                            .w_full()
                            .max_w(u(SHEET_MAX_WIDTH))
                            .h(gpui::relative(0.82))
                            .child(sheet),
                    ),
            ),
        };
        if let Some(pose) = pose
            && let Some((frame, opacity)) = pose.frame
        {
            // `.btw-morph-frame`: the composer's outline, carried between
            // the two boxes while the composers cross-fade inside it.
            let light = !theme.is_dark();
            overlay = overlay.child(
                div()
                    .absolute()
                    .left(px(frame.x))
                    .top(px(frame.y))
                    .w(px(frame.w.max(0.)))
                    .h(px(frame.h.max(0.)))
                    .opacity(opacity)
                    .rounded(u(theme.radius.lg))
                    .border_1()
                    .border_color(theme.content(0.12))
                    .when(light, |el| el.bg(theme.colors.background_base).shadow_lg())
                    .when(!light, |el| el.bg(theme.content(0.03))),
            );
        }
        if let Some((burst, _)) = &self.burst {
            overlay = overlay.child(burst.clone());
        }
        overlay.into_any_element()
    }
}
