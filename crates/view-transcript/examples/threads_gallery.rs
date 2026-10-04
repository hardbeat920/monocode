//! Shows the side thread and orchestration views with synthetic data.
//!
//! ```sh
//! cargo run -p monocode-view-transcript --example threads_gallery -- --scene btw
//! cargo run -p monocode-view-transcript --example threads_gallery -- \
//!     --scene orchestration --screenshot target/threads-shots/orchestration.png
//! ```
//!
//! Scenes: btw, btw-error, btw-morph, burst, second-opinion, orchestration,
//! orchestration-picker, sidebar-agents, live-agents, constellation, cards.
//! `--theme light` switches the scheme. `--screenshot` draws the window
//! offscreen with `Window::render_to_image` and exits. A screenshot window is
//! never shown or focused, so it cannot take the keyboard from the user.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AnyElement, App, AppContext as _, AsyncApp, Bounds, Context, Entity, IntoElement,
    ParentElement as _, Render, Styled as _, Subscription, Task, Window, WindowBounds,
    WindowOptions, div, px, size,
};
use gpui_component::Root;
use monocode_core::block::{
    Block, BlockRole, BtwMessage, BtwMessageRole, BtwThread, BtwThreadStatus, ModelSettings,
    SecondOpinionKind, SecondOpinionMeta, TurnModel,
};
use monocode_core::btw::{BtwSessionThread, session_has_btw_threads, supports_btw_harness};
use monocode_core::models::{
    AgentModel, HarnessAvailability, ModelCatalog, ModelPrefs, ModelSetting, ModelSettingChoice,
    ModelSettingKind,
};
use monocode_core::orchestration::{
    OrchestrationChoice, OrchestrationProposal, OrchestrationProposalStatus, OrchestrationSettings,
    ProposedTask,
};
use monocode_core::transcript::BlockRef;
use monocode_core::transcript::activity::group_turns;
use monocode_core::{Attachment, Extra, HarnessId, Session};
use monocode_layout::tab_groups::JsRecord;
use monocode_ui::{AppearanceSettings, Theme, ThemePreference, UiStyled as _, u};
use monocode_view_composer::composer::model::clipboard::ClipboardFile;
use monocode_view_composer::composer::model::mentions::{ProjectFile, RankedFile};
use monocode_view_composer::composer::model::skills::Skill;
use monocode_view_composer::composer::{
    ComposerHost, ComposerProps, ComposerSubmission, SkillContext,
};
use monocode_view_transcript::threads::actions::{
    OrchestrationActions, OrchestrationRunStatus, OrchestrationRunView, OrchestrationRuns,
    OrchestrationSummary, OrchestrationSummaryTask, OrchestrationTaskStatus, OrchestrationTaskView,
    OrchestrationWorkerDetail, OrchestrationWorkers, ResumeBlocker,
};
use monocode_view_transcript::threads::btw_burst::make_marks;
use monocode_view_transcript::threads::{
    BoxProbe, BtwConversationProps, BtwHost, BtwQuestionBurst, BtwRequest, BtwSheet, BtwSheetProps,
    BtwThreadBlocksInput, BurstRect, CatalogMenuSource, HandoffCard, LiveAgent, LiveAgentsPreview,
    OrchestrationPreview, OrchestrationSidebarAgents, OrchestratorConstellation, ProjectAppearance,
    SecondOpinionButton, SecondOpinionProps, handoff_mini_card, second_opinion_card,
};
use monocode_view_transcript::transcript::{TranscriptConfig, TranscriptView};

const USAGE: &str = "\
usage: threads_gallery [--scene <name>] [--theme dark|light] [--size WxH] [--screenshot <out.png>]";

const SCENES: [&str; 11] = [
    "btw",
    "btw-error",
    "btw-morph",
    "burst",
    "second-opinion",
    "orchestration",
    "orchestration-picker",
    "sidebar-agents",
    "live-agents",
    "constellation",
    "cards",
];

#[derive(Clone, Debug)]
struct Args {
    scene: String,
    light: bool,
    size: Option<(f32, f32)>,
    screenshot: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        scene: "btw".into(),
        light: false,
        size: None,
        screenshot: None,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| iter.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--scene" => args.scene = value("--scene")?,
            "--theme" => args.light = value("--theme")? == "light",
            "--size" => {
                let raw = value("--size")?;
                let (w, h) = raw.split_once('x').ok_or("--size takes WxH")?;
                args.size = Some((
                    w.parse().map_err(|_| "--size width")?,
                    h.parse().map_err(|_| "--size height")?,
                ));
            }
            "--screenshot" => args.screenshot = Some(PathBuf::from(value("--screenshot")?)),
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown flag {other}\n{USAGE}")),
        }
    }
    if !SCENES.contains(&args.scene.as_str()) {
        return Err(format!(
            "unknown scene {}; one of {}",
            args.scene,
            SCENES.join(", ")
        ));
    }
    Ok(args)
}

fn default_size(scene: &str) -> (f32, f32) {
    match scene {
        "btw" | "btw-error" | "btw-morph" => (900., 760.),
        "burst" => (640., 360.),
        "second-opinion" => (1100., 520.),
        "orchestration" => (760., 980.),
        "orchestration-picker" => (760., 560.),
        "sidebar-agents" => (340., 600.),
        "live-agents" => (320., 520.),
        "constellation" => (640., 300.),
        _ => (560., 360.),
    }
}

// Synthetic data.

fn effort_setting(id: &str) -> ModelSetting {
    ModelSetting {
        id: id.into(),
        label: "Reasoning".into(),
        kind: ModelSettingKind::Select,
        value: "high".into(),
        options: [
            ("xhigh", "Extra High"),
            ("high", "High"),
            ("medium", "Medium"),
            ("low", "Low"),
        ]
        .iter()
        .map(|(value, label)| ModelSettingChoice {
            value: (*value).into(),
            label: (*label).into(),
        })
        .collect(),
        description: None,
    }
}

fn catalog() -> ModelCatalog {
    let mut catalog = ModelCatalog::new();
    let mut gpt = AgentModel::new("codex:gpt-5.5", HarnessId::Codex, "GPT-5.5");
    gpt.settings = Some(vec![effort_setting("reasoningEffort")]);
    let mut mini = AgentModel::new("codex:gpt-5.5-mini", HarnessId::Codex, "GPT-5.5 Mini");
    mini.settings = Some(vec![effort_setting("reasoningEffort")]);
    catalog.set_harness_models(HarnessId::Codex, vec![gpt, mini]);
    let mut opus = AgentModel::new("claude:opus-5", HarnessId::Claude, "Claude Opus 5");
    opus.settings = Some(vec![effort_setting("effort")]);
    let sonnet = AgentModel::new("claude:sonnet-5", HarnessId::Claude, "Claude Sonnet 5");
    catalog.set_harness_models(HarnessId::Claude, vec![opus, sonnet]);
    catalog
}

fn message(id: &str, role: BtwMessageRole, text: &str, at: i64) -> BtwMessage {
    BtwMessage {
        id: id.into(),
        role,
        text: text.into(),
        created_at: at,
        blocks: None,
        extra: Extra::new(),
    }
}

fn btw_thread(id: &str, status: BtwThreadStatus, messages: Vec<BtwMessage>, at: i64) -> BtwThread {
    BtwThread {
        id: id.into(),
        source_end_block_id: "a1".into(),
        created_at: at,
        updated_at: at + 9_000,
        status,
        messages,
        harness: Some(HarnessId::Claude),
        model: None,
        model_settings: None,
        provider_thread_id: None,
        error: None,
        pending_blocks: None,
        extra: Extra::new(),
    }
}

fn btw_session(error: bool) -> Vec<Block> {
    let at = 1_700_000_000_000;
    let ready = btw_thread(
        "t1",
        BtwThreadStatus::Ready,
        vec![
            message(
                "t1-q",
                BtwMessageRole::User,
                "Why does the attract loop restart after every game?",
                at,
            ),
            message(
                "t1-a",
                BtwMessageRole::Assistant,
                "The loop in `gridArcade.ts` picks the next game when the **result screen** times out. \
                 It waits `resultMs` (1.8s) after a WIN or LOSE, then calls `startRandomGame()`.\n\n\
                 - Snake and pong end on a collision.\n- Invaders and breakout end when the board clears.",
                at + 8_000,
            ),
        ],
        at,
    );
    let mut running = btw_thread(
        "t2",
        BtwThreadStatus::Running,
        vec![message(
            "t2-q",
            BtwMessageRole::User,
            "Which file owns the frame timer?",
            at + 60_000,
        )],
        at + 60_000,
    );
    running.pending_blocks = Some(vec![Block::new(
        "t2-live",
        BlockRole::Assistant,
        "Looking at `speechBubble.ts` and the scene list…",
    )]);
    let mut failed = btw_thread(
        "t3",
        BtwThreadStatus::Error,
        vec![message(
            "t3-q",
            BtwMessageRole::User,
            "Can pong speed up each round?",
            at + 120_000,
        )],
        at + 120_000,
    );
    failed.error = Some("Claude Code exited before it answered (code 1).".into());
    let mut threads = vec![ready, running];
    if error {
        threads.push(failed);
    }
    let mut user = Block::new(
        "u1",
        BlockRole::User,
        "Make each arcade game finish with a WIN or LOSE screen.",
    );
    user.started_at = Some(at - 200_000);
    user.duration_ms = Some(159_000);
    user.turn_model = Some(TurnModel {
        harness: HarnessId::Claude,
        id: "claude:opus-5".into(),
        name: "Claude Opus 5".into(),
        extra: Extra::new(),
    });
    user.btw_threads = Some(threads);
    vec![
        user,
        Block::new(
            "a1",
            BlockRole::Assistant,
            "Each game now plays all the way through. It ends in a **WIN** or **LOSE**, that word shows in the \
             center, then a different game starts at random.",
        ),
    ]
}

fn turns_of(blocks: &[Block]) -> Vec<Vec<Block>> {
    let refs: Vec<BlockRef> = blocks.iter().cloned().map(Arc::new).collect();
    group_turns(&refs, false)
        .into_iter()
        .map(|turn| turn.into_iter().map(Arc::unwrap_or_clone).collect())
        .collect()
}

/// The btw.ts rules for these synthetic sessions. The app wires the
/// engine's `side_threads::btw` functions instead.
struct GalleryBtwHost;

impl BtwHost for GalleryBtwHost {
    fn session_threads(&self, blocks: &[Block], _: bool) -> Vec<BtwSessionThread> {
        if !session_has_btw_threads(blocks) {
            return Vec::new();
        }
        let mut entries = Vec::new();
        for turn in turns_of(blocks) {
            let threads = turn
                .iter()
                .find(|block| block.role == BlockRole::User)
                .and_then(|block| block.btw_threads.clone())
                .unwrap_or_default();
            for thread in threads {
                entries.push(BtwSessionThread {
                    thread,
                    turn: turn.clone(),
                });
            }
        }
        entries.sort_by_key(|entry| entry.thread.created_at);
        entries
    }

    fn open_target_turn_id(
        &self,
        turns: &[Vec<Block>],
        _: &[Block],
        harness: HarnessId,
        _: bool,
    ) -> Option<String> {
        turns.iter().rev().find_map(|turn| {
            let user = turn
                .iter()
                .rev()
                .find(|block| block.role == BlockRole::User)?;
            (user.duration_ms.is_some() && supports_btw_harness(Some(harness)))
                .then(|| turn[0].id.clone())
        })
    }

    fn surface_harness(
        &self,
        _: &[Block],
        _: &[Block],
        harness: HarnessId,
        _: Option<&[BtwThread]>,
    ) -> Option<HarnessId> {
        supports_btw_harness(Some(harness)).then_some(harness)
    }

    fn thread_blocks(&self, input: BtwThreadBlocksInput<'_>) -> Vec<Block> {
        let mut blocks = Vec::new();
        for (index, message) in input.messages.iter().enumerate() {
            if message.role == BtwMessageRole::Assistant {
                blocks.push(Block::new(
                    message.id.clone(),
                    BlockRole::Assistant,
                    message.text.clone(),
                ));
                continue;
            }
            let last = index + 1 == input.messages.len();
            let mut question =
                Block::new(message.id.clone(), BlockRole::User, message.text.clone());
            question.started_at = Some(message.created_at);
            question.duration_ms = match input.messages.get(index + 1) {
                Some(answer) => Some(answer.created_at - message.created_at),
                None if !input.running => Some(0),
                None => None,
            };
            blocks.push(question);
            if last && input.running {
                blocks.extend(input.pending_blocks.unwrap_or_default().iter().cloned());
            }
        }
        blocks
    }

    fn preferred_model_settings(
        &self,
        _: HarnessId,
        _: &str,
        current: &ModelSettings,
    ) -> ModelSettings {
        current.clone()
    }

    fn submit(&self, request: BtwRequest<'_>, _: &mut App) -> bool {
        eprintln!("btw submit: {}", request.text);
        true
    }

    fn retry(&self, _: &[Block], thread_id: &str, _: &mut App) {
        eprintln!("btw retry: {thread_id}");
    }

    fn composer_host(&self) -> Rc<dyn ComposerHost> {
        Rc::new(GalleryComposerHost)
    }
}

struct GalleryComposerHost;

impl ComposerHost for GalleryComposerHost {
    fn submit(&self, _: ComposerSubmission, _: &mut Window, _: &mut App) -> bool {
        false
    }
    fn attachments_from_paths(&self, _: Vec<String>, _: &mut App) -> Task<Vec<Attachment>> {
        Task::ready(Vec::new())
    }
    fn attachments_from_files(&self, _: Vec<ClipboardFile>, _: &mut App) -> Task<Vec<Attachment>> {
        Task::ready(Vec::new())
    }
    fn pick_attachments(&self, _: &mut Window, _: &mut App) -> Task<Vec<Attachment>> {
        Task::ready(Vec::new())
    }
    fn skills(&self, _: &SkillContext, _: &mut App) -> Vec<Skill> {
        Vec::new()
    }
    fn mention_files(&self, _: &str, _: &mut App) -> Vec<ProjectFile> {
        Vec::new()
    }
    fn rank_mentions(&self, _: &str, _: &str, _: &mut App) -> Vec<RankedFile> {
        Vec::new()
    }
}

/// An orchestrator with fixed runs.
#[derive(Default)]
struct GalleryRuns {
    runs: RefCell<Vec<OrchestrationRunView>>,
    blocker: Option<ResumeBlocker>,
}

impl OrchestrationRuns for GalleryRuns {
    fn runs(&self, _: &App) -> Vec<OrchestrationRunView> {
        self.runs.borrow().clone()
    }
    fn observe(&self, _: Box<dyn Fn(&mut App)>, _: &mut App) -> Subscription {
        Subscription::new(|| {})
    }
    fn resume_blocker(&self, _: &str, _: &App) -> Option<ResumeBlocker> {
        self.blocker.clone()
    }
    fn cancel_task(&self, _: &str, _: &str, _: &mut App) -> Task<Result<(), String>> {
        Task::ready(Ok(()))
    }
    fn start(&self, _: &str, _: &[HarnessId], _: i64, _: &mut App) -> Task<Result<(), String>> {
        Task::ready(Ok(()))
    }
}

impl OrchestrationActions for GalleryRuns {
    fn update(&self, _: &str, _: &str, _: OrchestrationProposal, _: &mut App) {}
    fn confirm(&self, _: &str, _: &str, _: &mut App) -> Task<Result<(), String>> {
        Task::ready(Ok(()))
    }
    fn retry(&self, _: &str, _: &str, _: &mut App) {}
    fn open(&self, _: &str, _: &mut App) {}
    fn can_open_agents(&self) -> bool {
        true
    }
}

impl OrchestrationWorkers for GalleryRuns {
    fn selected_id(&self, _: &App) -> Option<String> {
        None
    }
    fn inspect(&self, _: Option<&str>, _: &mut App) {}
    fn can_open_details(&self) -> bool {
        true
    }
    fn open_details(&self, _: OrchestrationWorkerDetail, _: &mut App) {}
}

fn choice(harness: HarnessId, model: &str, name: &str) -> OrchestrationChoice {
    OrchestrationChoice {
        harness,
        model: model.into(),
        name: name.into(),
        extra: Extra::new(),
    }
}

fn proposed(
    id: &str,
    title: &str,
    prompt: &str,
    harness: HarnessId,
    model: &str,
    after: &[&str],
) -> ProposedTask {
    ProposedTask {
        id: id.into(),
        title: title.into(),
        prompt: prompt.into(),
        harness,
        model: model.into(),
        model_settings: None,
        files: vec!["src".into()],
        depends_on: after.iter().map(|id| id.to_string()).collect(),
        extra: Extra::new(),
    }
}

fn proposal(status: OrchestrationProposalStatus, title: &str) -> OrchestrationProposal {
    let choices = vec![
        choice(HarnessId::Codex, "codex:gpt-5.5", "GPT-5.5"),
        choice(HarnessId::Codex, "codex:gpt-5.5-mini", "GPT-5.5 Mini"),
        choice(HarnessId::Claude, "claude:opus-5", "Claude Opus 5"),
        choice(HarnessId::Claude, "claude:sonnet-5", "Claude Sonnet 5"),
    ];
    let mut tasks = vec![
        proposed(
            "ui",
            "Settings form and keyboard navigation",
            "Build the settings form in src/features/settings/ui. Every control must be reachable with Tab, and Escape closes the dialog.",
            HarnessId::Codex,
            "codex:gpt-5.5",
            &[],
        ),
        proposed(
            "store",
            "Persist settings in monocode.db",
            "Add a settings table and the load and save calls.",
            HarnessId::Claude,
            "claude:opus-5",
            &[],
        ),
        proposed(
            "wire",
            "Wire the form to the store",
            "Load on open, save on change, and show a toast on failure.",
            HarnessId::Claude,
            "claude:sonnet-5",
            &["ui", "store"],
        ),
        proposed(
            "tests",
            "Tests for the settings round trip",
            "Cover load, save, and a failed save.",
            HarnessId::Codex,
            "codex:gpt-5.5-mini",
            &["wire"],
        ),
    ];
    tasks[0].model_settings = Some(
        [("reasoningEffort".to_string(), "xhigh".to_string())]
            .into_iter()
            .collect(),
    );
    OrchestrationProposal {
        version: 1,
        lead_id: "lead".into(),
        cwd: "/Users/dev/arcade".into(),
        checkout_cwd: None,
        request: "Build settings".into(),
        author: choices[2].clone(),
        settings: OrchestrationSettings {
            choices,
            max_workers: 2,
            extra: Extra::new(),
        },
        status,
        title: title.into(),
        summary: "Split UI and persistence".into(),
        tasks,
        error: None,
        response: None,
        extra: Extra::new(),
    }
}

fn plan_block(id: &str, proposal: OrchestrationProposal) -> Block {
    Block {
        orchestration: Some(proposal),
        ..Block::new(id, BlockRole::Plan, "")
    }
}

fn summary_task(
    id: &str,
    title: &str,
    harness: HarnessId,
    model: &str,
    status: &str,
    needs_input: bool,
) -> OrchestrationSummaryTask {
    OrchestrationSummaryTask {
        session_id: id.into(),
        title: title.into(),
        harness,
        model: model.into(),
        status: status.into(),
        needs_input: needs_input.then_some(true),
        extra: Extra::new(),
    }
}

// The gallery view.

type Build = Rc<dyn Fn(&Theme) -> AnyElement>;

/// One thing on the page: a view, or an element rebuilt each frame.
enum Item {
    View(gpui::AnyView),
    /// A small view at its own width, indented.
    Row(gpui::AnyView),
    Build(Build),
}

struct Gallery {
    items: Vec<Item>,
    /// Lay items out by absolute position instead of in a padded column.
    canvas: bool,
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx).clone();
        let mut root = div()
            .size_full()
            .relative()
            .bg(theme.colors.background_base)
            .text_color(theme.colors.content)
            .font_family(theme.fonts.sans.clone());
        if !self.canvas {
            root = root.p(u(24.)).flex().flex_col().gap(u(16.));
        }
        for item in &self.items {
            root = match item {
                Item::View(view) => root.child(view.clone()),
                Item::Row(view) => root.child(div().flex().pl(u(240.)).child(view.clone())),
                Item::Build(build) => root.child(build(&theme)),
            };
        }
        root
    }
}

fn build(f: impl Fn(&Theme) -> AnyElement + 'static) -> Item {
    Item::Build(Rc::new(f))
}

/// A pane with a transcript, a docked composer box, and the btw sheet over
/// them, as the session pane lays them out.
struct BtwPane {
    transcript: Entity<TranscriptView>,
    sheet: Entity<BtwSheet>,
    origin: BoxProbe,
    /// Opens the sheet once the composer box has painted, so it can morph.
    open_after_paint: bool,
}

impl Render for BtwPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.open_after_paint && self.origin.get().is_some() {
            self.open_after_paint = false;
            self.sheet.update(cx, |sheet, cx| {
                sheet.open_with("", false, cx);
                if let Some(tab) = sheet.tabs().first() {
                    let id = tab.id.clone();
                    sheet.select_tab(&id, cx);
                }
            });
        }
        if self.open_after_paint {
            window.request_animation_frame();
        }
        let theme = Theme::of(cx);
        let home = self.sheet.read(cx).home_opacity();
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme.colors.background_base)
            .child(div().flex_1().min_h_0().child(self.transcript.clone()))
            .child(
                div().p(u(6.)).pt_0().opacity(home).child(
                    div()
                        .relative()
                        .h(u(96.))
                        .rounded(u(theme.radius.lg))
                        .border_1()
                        .border_color(theme.content(0.12))
                        .bg(theme.content(0.03))
                        .child(self.origin.probe()),
                ),
            )
            .child(self.sheet.clone())
    }
}

fn build_scene(scene: &str, window: &mut Window, cx: &mut App) -> gpui::AnyView {
    let runs = Rc::new(GalleryRuns::default());
    match scene {
        "btw" | "btw-error" | "btw-morph" => {
            let morph = scene == "btw-morph";
            let blocks = Arc::new(btw_session(scene == "btw-error"));
            let mut session = Session::blank(
                "lead",
                HarnessId::Claude,
                "claude:opus-5",
                "/Users/dev/arcade",
            );
            session.blocks = (*blocks).clone();
            let transcript = cx.new(|cx| {
                let mut view = TranscriptView::new(cx);
                view.set_session(Arc::new(session), cx);
                view
            });
            let origin = BoxProbe::default();
            let sheet = cx.new(|cx| {
                let mut sheet = BtwSheet::new(Rc::new(GalleryBtwHost), cx);
                sheet.set_origin(Some(origin.clone()));
                sheet.set_props(
                    BtwSheetProps {
                        conversation: BtwConversationProps {
                            available: true,
                            blocks,
                            harness: HarnessId::Claude,
                            model: "claude:opus-5".into(),
                            ..Default::default()
                        },
                        cwd: Some("/Users/dev/arcade".into()),
                        reduced_motion: !morph,
                        composer: ComposerProps {
                            animate: false,
                            runner_enabled: false,
                            ..ComposerProps::default()
                        },
                        transcript: TranscriptConfig {
                            catalog: Arc::new(catalog()),
                            ..TranscriptConfig::default()
                        },
                        ..BtwSheetProps::default()
                    },
                    cx,
                );
                if morph {
                    // Halfway through the open, with the burst going.
                    sheet.freeze_morph_at(Some(170.));
                    return sheet;
                }
                sheet.open_with("", false, cx);
                let tabs = sheet.tabs();
                let pick = if scene == "btw-error" { 2 } else { 0 };
                if let Some(tab) = tabs.get(pick) {
                    let id = tab.id.clone();
                    sheet.select_tab(&id, cx);
                }
                sheet
            });
            cx.new(|_| BtwPane {
                transcript,
                sheet,
                origin,
                open_after_paint: morph,
            })
            .into()
        }
        "burst" => {
            let mut random = 7u64;
            let marks = make_marks(&mut || {
                random = random
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (random >> 40) as f32 / (1u64 << 24) as f32
            });
            let burst = cx.new(|cx| {
                let mut burst = BtwQuestionBurst::with_marks(
                    BurstRect {
                        left: 40.,
                        top: 220.,
                        width: 560.,
                        height: 96.,
                    },
                    marks,
                    cx,
                );
                burst.freeze_at(420., cx);
                burst
            });
            cx.new(|_| Gallery {
                items: vec![
                    build(|theme| {
                        div()
                            .absolute()
                            .left(px(40.))
                            .top(px(220.))
                            .w(px(560.))
                            .h(px(96.))
                            .rounded(u(theme.radius.lg))
                            .border_1()
                            .border_color(theme.content(0.12))
                            .bg(theme.content(0.03))
                            .into_any_element()
                    }),
                    Item::View(burst.into()),
                ],
                canvas: true,
            })
            .into()
        }
        "second-opinion" => {
            let source = Rc::new(CatalogMenuSource::new(
                catalog(),
                ModelPrefs::default(),
                HarnessAvailability {
                    installed: [
                        HarnessId::Claude,
                        HarnessId::Codex,
                        HarnessId::Grok,
                        HarnessId::Cursor,
                    ]
                    .into_iter()
                    .collect(),
                    probed: true,
                },
            ));
            let button = cx.new(|cx| {
                let mut button = SecondOpinionButton::new(
                    SecondOpinionProps::second_opinion(HarnessId::Claude),
                    source,
                    cx,
                );
                button.set_animate(false);
                button
            });
            button.update(cx, |button, cx| {
                button.toggle(window, cx);
                button.hover_provider(0, cx);
                button.hover_model(0, cx);
            });
            cx.new(|_| Gallery {
                items: vec![
                    build(|_| div().h(u(400.)).into_any_element()),
                    Item::Row(button.into()),
                ],
                canvas: false,
            })
            .into()
        }
        "orchestration" | "orchestration-picker" => {
            let catalog = Arc::new(catalog());
            let ready = plan_block(
                "card-ready",
                proposal(OrchestrationProposalStatus::Ready, "Build settings"),
            );
            let ready_card = cx.new(|cx| {
                let mut card =
                    OrchestrationPreview::new(&ready, runs.clone(), Some(runs.clone()), window, cx);
                card.set_catalog(catalog.clone(), cx);
                card.set_animate(false);
                if scene == "orchestration" {
                    card.toggle_details("ui", window, cx);
                } else {
                    card.toggle_assignment("ui", window, cx);
                    card.hover_assignment(0, cx);
                }
                card
            });
            let mut entities: Vec<Item> = vec![Item::View(ready_card.into())];
            if scene == "orchestration" {
                let mut planning = proposal(OrchestrationProposalStatus::Planning, "");
                planning.tasks.clear();
                let planning = plan_block("card-planning", planning);
                entities.push(Item::View(
                    cx.new(|cx| {
                        OrchestrationPreview::new(
                            &planning,
                            runs.clone(),
                            Some(runs.clone()),
                            window,
                            cx,
                        )
                    })
                    .into(),
                ));
                runs.runs.borrow_mut().push(OrchestrationRunView {
                    lead_id: "lead".into(),
                    proposal_id: Some("card-approved".into()),
                    status: OrchestrationRunStatus::Active,
                    allowed_harnesses: vec![HarnessId::Codex, HarnessId::Claude],
                    max_workers: 2,
                    error: None,
                    tasks: Vec::new(),
                });
                let approved = plan_block(
                    "card-approved",
                    proposal(
                        OrchestrationProposalStatus::Approved,
                        "Review the orchestration branch",
                    ),
                );
                entities.push(Item::View(
                    cx.new(|cx| {
                        let mut card = OrchestrationPreview::new(
                            &approved,
                            runs.clone(),
                            Some(runs.clone()),
                            window,
                            cx,
                        );
                        card.set_catalog(catalog.clone(), cx);
                        card
                    })
                    .into(),
                ));
            }
            cx.new(|_| Gallery {
                items: entities,
                canvas: false,
            })
            .into()
        }
        "sidebar-agents" => {
            let runs = Rc::new(GalleryRuns {
                runs: RefCell::new(vec![OrchestrationRunView {
                    lead_id: "lead".into(),
                    proposal_id: None,
                    status: OrchestrationRunStatus::Paused,
                    allowed_harnesses: vec![HarnessId::Codex],
                    max_workers: 2,
                    error: None,
                    tasks: vec![OrchestrationTaskView {
                        id: "t-ui".into(),
                        session_id: "ui".into(),
                        title: "Settings form".into(),
                        harness: HarnessId::Codex,
                        status: OrchestrationTaskStatus::Queued,
                        error: None,
                    }],
                }]),
                blocker: Some(ResumeBlocker {
                    id: "investigation".into(),
                    title: "Investigating the failure".into(),
                }),
            });
            let summary = OrchestrationSummary {
                status: "active".into(),
                live: Some(true),
                tasks: vec![
                    summary_task(
                        "ui",
                        "Settings form and keyboard navigation",
                        HarnessId::Codex,
                        "codex:gpt-5.5",
                        "running",
                        false,
                    ),
                    summary_task(
                        "store",
                        "Persist settings in monocode.db",
                        HarnessId::Claude,
                        "claude:opus-5",
                        "running",
                        true,
                    ),
                    summary_task(
                        "wire",
                        "Wire the form to the store",
                        HarnessId::Claude,
                        "claude:sonnet-5",
                        "completed",
                        false,
                    ),
                    summary_task(
                        "tests",
                        "Tests for the settings round trip",
                        HarnessId::Codex,
                        "codex:gpt-5.5-mini",
                        "failed",
                        false,
                    ),
                    summary_task(
                        "docs",
                        "Document the settings file",
                        HarnessId::Codex,
                        "codex:gpt-5.5-mini",
                        "queued",
                        false,
                    ),
                ],
                extra: Extra::new(),
            };
            let card = cx.new(|cx| {
                let mut card = OrchestrationSidebarAgents::new(
                    "lead",
                    summary,
                    runs.clone(),
                    runs.clone(),
                    cx,
                );
                card.set_catalog(Arc::new(catalog()), cx);
                card.set_actions(Some(runs.clone()), cx);
                card.toggle("ui", cx);
                card
            });
            cx.new(|_| Gallery {
                items: vec![Item::View(card.into())],
                canvas: false,
            })
            .into()
        }
        "live-agents" => {
            let now = 1_700_000_100_000;
            let agent =
                |id: &str, cwd: &str, title: &str, harness, activity: &str, started: i64| {
                    LiveAgent {
                        id: id.into(),
                        cwd: cwd.into(),
                        title: title.into(),
                        harness,
                        activity: activity.into(),
                        started_at: Some(now - started),
                        duration_ms: None,
                        needs_approval: false,
                        done: false,
                    }
                };
            let mut waiting = agent(
                "b",
                "/Users/dev/site",
                "Fix the pricing table on mobile",
                HarnessId::Codex,
                "Edit pricing.css",
                95_000,
            );
            waiting.needs_approval = true;
            let mut done = agent(
                "c",
                "/Users/dev/arcade",
                "Add a high score board",
                HarnessId::Claude,
                "Done",
                0,
            );
            done.done = true;
            done.started_at = None;
            done.duration_ms = Some(312_000);
            let agents = vec![
                agent(
                    "a",
                    "/Users/dev/arcade",
                    "Make each game finish with a result screen",
                    HarnessId::Claude,
                    "Read gridArcade.ts",
                    42_000,
                ),
                waiting,
                done,
                agent(
                    "d",
                    "/Users/dev/notes",
                    "Summarize this week's inbox",
                    HarnessId::Grok,
                    "Search inbox",
                    4_000_000,
                ),
                agent(
                    "e",
                    "/Users/dev/site",
                    "Update the footer links",
                    HarnessId::Cursor,
                    "Thinking",
                    8_000,
                ),
            ];
            let mut labels = JsRecord::new();
            labels.insert("/Users/dev/site", "Website".to_string());
            let preview = cx.new(|cx| {
                let mut preview = LiveAgentsPreview::new(cx);
                preview.set_now(Some(now), cx);
                preview.set_appearance(
                    ProjectAppearance {
                        labels,
                        ..Default::default()
                    },
                    cx,
                );
                preview.set_agents(agents, cx);
                preview.set_active_session_id(Some("a".into()), cx);
                preview
            });
            cx.new(|_| Gallery {
                items: vec![Item::View(preview.into())],
                canvas: false,
            })
            .into()
        }
        "constellation" => {
            let moments = [500., 1100., 2000.];
            let mut items = Vec::new();
            for (index, ms) in moments.into_iter().enumerate() {
                let constellation = cx.new(|_| OrchestratorConstellation::frozen(ms, 11));
                items.push(build(move |theme| {
                    div()
                        .relative()
                        .w(px(560.))
                        .h(px(72.))
                        .rounded(u(theme.radius.xl))
                        .bg(theme.content(0.06))
                        .px(u(14.))
                        .py(u(10.))
                        .text_px(14.)
                        .child(format!(
                            "{index}. Split the settings work across Codex and Claude ({ms} ms)."
                        ))
                        .child(constellation.clone())
                        .into_any_element()
                }));
            }
            cx.new(|_| Gallery {
                items,
                canvas: false,
            })
            .into()
        }
        _ => {
            let review = SecondOpinionMeta {
                from: HarnessId::Claude,
                to: HarnessId::Codex,
                request: None,
                files: Some(3),
                kind: None,
                extra: Extra::new(),
            };
            let split = SecondOpinionMeta {
                kind: Some(SecondOpinionKind::Handoff),
                files: None,
                ..review.clone()
            };
            fn bubble(theme: &Theme, child: AnyElement) -> AnyElement {
                div()
                    .w(px(320.))
                    .rounded(u(theme.radius.xl))
                    .bg(theme.content(0.06))
                    .px(u(14.))
                    .py(u(10.))
                    .child(child)
                    .into_any_element()
            }
            cx.new(|_| Gallery {
                items: vec![
                    build(move |theme| {
                        bubble(
                            theme,
                            second_opinion_card(review.clone()).into_any_element(),
                        )
                    }),
                    build(move |theme| {
                        bubble(theme, second_opinion_card(split.clone()).into_any_element())
                    }),
                    build(|_| {
                        div()
                            .w(px(420.))
                            .child(
                                handoff_mini_card(
                                    "handoff",
                                    HandoffCard {
                                        from: HarnessId::Claude,
                                        to: HarnessId::Codex,
                                        request: Some(
                                            "Finish the settings form and add the keyboard tests"
                                                .into(),
                                        ),
                                        files: Some(4),
                                    },
                                )
                                .on_dismiss(|_, _, _| {}),
                            )
                            .into_any_element()
                    }),
                ],
                canvas: false,
            })
            .into()
        }
    }
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    gpui_platform::application()
        .with_assets(monocode_ui::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            let appearance = AppearanceSettings {
                theme_preference: if args.light {
                    ThemePreference::Light
                } else {
                    ThemePreference::Dark
                },
                ..Default::default()
            };
            monocode_ui::init(appearance, cx);
            monocode_view_transcript::transcript::init(cx);
            monocode_view_composer::composer::init(cx);
            monocode_view_composer::pickers::init(cx);
            let (width, height) = args.size.unwrap_or_else(|| default_size(&args.scene));
            let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
            let screenshot = args.screenshot.is_some();
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // A screenshot window stays hidden and unfocused.
                focus: !screenshot,
                show: !screenshot,
                ..Default::default()
            };
            let scene = args.scene.clone();
            let window = cx
                .open_window(options, move |window, cx| {
                    monocode_ui::sync_window(window, cx);
                    window.set_background_appearance(gpui::WindowBackgroundAppearance::Opaque);
                    let view = build_scene(&scene, window, cx);
                    cx.new(|cx| Root::new(view, window, cx))
                })
                .expect("open the gallery window");
            match args.screenshot.clone() {
                Some(out) => capture_and_quit(window.into(), out, cx),
                None => cx.activate(true),
            }
        });
}

/// Redraw until images and fonts settle, then write the frame as a PNG.
fn capture_and_quit(window: gpui::AnyWindowHandle, out: PathBuf, cx: &mut App) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        for _ in 0..15 {
            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
            let _ = window.update(cx, |_, window, cx| {
                window.dispatch_event(
                    gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                        position: gpui::point(px(-1000.), px(-1000.)),
                        ..Default::default()
                    }),
                    cx,
                );
                window.refresh();
                window.draw(cx).clear();
            });
        }
        let result = window.update(cx, |_, window, cx| {
            window.draw(cx).clear();
            window.render_to_image()
        });
        let code = match result {
            Ok(Ok(image)) => {
                if let Some(parent) = out.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match image.save(&out) {
                    Ok(()) => {
                        eprintln!(
                            "wrote {} ({}x{})",
                            out.display(),
                            image.width(),
                            image.height()
                        );
                        0
                    }
                    Err(err) => {
                        eprintln!("screenshot failed: {err}");
                        1
                    }
                }
            }
            Ok(Err(err)) => {
                eprintln!("screenshot failed: {err:#}");
                1
            }
            Err(err) => {
                eprintln!("screenshot failed: {err:#}");
                1
            }
        };
        cx.update(|cx| cx.quit());
        std::process::exit(code);
    })
    .detach();
}
