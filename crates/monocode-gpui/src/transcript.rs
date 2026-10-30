pub(crate) mod attachments;
pub(crate) mod burst;
pub(crate) mod dates;
mod drafts;
#[cfg(test)]
mod follow_tests;
pub(crate) mod git_turns;
mod history;
pub(crate) use history::{HistoryPager, PreparedHistory};
mod links;
mod live_phase;
mod mascot;
mod measurements;
mod mono_divider;
mod outline;
pub(crate) mod phases;
mod provider_switch;
mod remote_plans;
pub(crate) mod selection;
pub(crate) mod tasks;
mod tools;
mod user_links;
mod visuals;
#[cfg(test)]
pub(crate) use visuals::placements;

use crate::markdown_model::MarkdownParser;
use crate::popover::{Anchor, AnchorSlot, Side, popover};
use crate::response_actions::{self, Request as ResponseRequest};
use crate::syntax::{HighlightRequest, HighlightedDocument};
use crate::{
    markdown::{self, Fence, MarkdownRow, Part},
    reveal::{Disclosure, Motion, PacedText},
    theme::*,
};
use gpui::{
    AvailableSpace, Bounds, ClipboardItem, Context, FollowMode, ListAlignment, ListOffset,
    ListState, Pixels, Role, SharedString, Transformation, Window, canvas, div, list, percentage,
    prelude::*, px, rgb, size,
};
use monocode_core::tool::{ToolCall, ToolState, tasks::Panel};
use monocode_core::transcript::Row;
use monocode_core::user_links::{UserMessageLink, parse_user_message_link};
use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    sync::Arc,
    time::Instant,
};
/// Animates the transcript on the first display frame at or after `due`.
fn motion_frame(view: gpui::WeakEntity<TranscriptView>, due: Option<Instant>, window: &Window) {
    window.on_next_frame(move |window, cx| {
        if due.is_some_and(|due| Instant::now() < due) {
            motion_frame(view, due, window);
        } else {
            let _ = view.update(cx, |this, cx| this.animate_frame(cx));
        }
    });
}

/// Where a launched session stands, for its card in the chat.
#[derive(Clone, PartialEq)]
pub(crate) struct LaunchView {
    pub title: String,
    /// working, input, draft, failed, stopped or done.
    pub state: &'static str,
    pub harness: String,
    pub cwd: String,
}
/// A message the Mono has not taken yet, shown where it will land.
#[derive(Clone, PartialEq)]
pub(crate) struct PendingMessage {
    pub id: String,
    pub text: SharedString,
    pub error: Option<String>,
    pub sent_at: Option<u64>,
    pub attachments: Vec<std::path::PathBuf>,
}
struct TakenMessage {
    text: SharedString,
    removed_at: Instant,
    entrance: Option<Instant>,
}
pub(crate) enum MonoResponseAction {
    Quote {
        text: String,
        window: gpui::AnyWindowHandle,
    },
    Activity(usize),
    /// Retry or discard a message that was not sent.
    Queue {
        id: String,
        remove: bool,
    },
}
impl gpui::EventEmitter<MonoResponseAction> for TranscriptView {}

fn work_title(model: Option<&str>, elapsed_ms: Option<u64>, live: bool) -> String {
    let model = model.map(str::trim).filter(|model| !model.is_empty());
    let verb = match (live, model.is_some()) {
        (true, true) => "working",
        (true, false) => "Working",
        (false, true) => "worked",
        (false, false) => "Worked",
    };
    let title = model.map_or_else(|| verb.to_owned(), |model| format!("{model} {verb}"));
    let Some(ms) = elapsed_ms else {
        return if live { format!("{title}…") } else { title };
    };
    let seconds = ms.saturating_add(500).saturating_div(1000).max(1);
    let duration = if seconds < 60 {
        format!("{seconds}s")
    } else if seconds % 60 == 0 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}m {}s", seconds / 60, seconds % 60)
    };
    format!("{title} for {duration}")
}

struct Message {
    start: usize,
    display_start: usize,
    role: String,
    parser: MarkdownParser,
    markdown_blocks: Vec<RenderedBlock>,
    under_line: bool,
    /// The turn this message belongs to, as `Body::Actions` numbers it.
    turn: usize,
    attachments: Arc<[attachments::Attachment]>,
    /// A user message's displayed chunks and attachments, which its cached
    /// bubble width depends on.
    prompt_fingerprint: u64,
    /// parseUserMessageLink: the prompt's first web link, with the index of
    /// the text chunk that holds it.
    user_link: Option<(usize, Arc<UserMessageLink>)>,
}
struct RenderedBlock {
    source: Arc<crate::markdown_model::TopBlock>,
    rows: Vec<Arc<MarkdownRow>>,
    words: usize,
}
#[derive(Clone)]
enum Body {
    RemotePlan(crate::conversation::remote_plans::Plan),
    SavedDraft,
    Markdown(Arc<MarkdownRow>),
    /// A dithered break between text replies in one Mono turn.
    MonoDivider,
    Visual(Arc<monocode_core::visual::VisualDocument>),
    Plain(SharedString),
    Interjection(monocode_core::transcript::Interjection),
    MonoCard(serde_json::Value),
    /// A queued message's bubble, by its index in `pending`.
    Pending(usize),
    /// LaunchedSessionCard for a session the run started.
    Launch(i64),
    /// SessionReview, the latest settled turn's recorded changes.
    Review {
        turn: usize,
    },
    /// A commit pill turn's status line, then its card.
    GitAction(Arc<git_turns::GitCard>),
    Tool {
        call: Box<ToolCall>,
        group: Option<usize>,
        tail: bool,
        /// Its phase is still the live end of the turn.
        live: bool,
    },
    Phase {
        title: String,
        group: usize,
        icon: &'static str,
        active: bool,
    },
    WorkLine {
        turn: usize,
        expandable: bool,
    },
    Reasoning,
    Actions {
        turn: usize,
        complete: bool,
    },
}
#[derive(Clone)]
struct UiRow {
    key: (usize, usize),
    body: Body,
    message: usize,
    first: bool,
    last: bool,
    work: bool,
    work_tail: bool,
    /// Reasoning headers and their expanded Markdown share the tool phase.
    thought_phase: Option<(usize, bool)>,
}
impl UiRow {
    fn phase_group(&self) -> Option<usize> {
        self.thought_phase
            .map(|(group, _)| group)
            .or(match self.body {
                Body::Tool { group, .. } => group,
                _ => None,
            })
    }
    fn in_phase(&self) -> bool {
        self.phase_group().is_some()
    }
}
const TOP_INSET: f32 = 20.;
// Saved drafts are presentation rows, outside the provider transcript.
const SAVED_DRAFT_START: usize = usize::MAX - 1;
const BOTTOM_INSET: f32 = 40.;
#[cfg(target_os = "macos")]
const TOP_FADE: f32 = 24.;
/// AgentTranscript.tsx's user bubble: `text-sm`, `max-w-[36rem]` and `px-3`.
/// Measurement, line counting and render share these.
const PROMPT_TEXT_SIZE: Pixels = px(14.);
const PROMPT_BUBBLE_MAX: Pixels = px(576.);
/// Horizontal padding on both sides together.
const PROMPT_PAD_X: Pixels = px(24.);
/// Text width that leaves room for "Show less" on a blank last chunk.
const PROMPT_DISCLOSURE_MIN: Pixels = px(72.);
/// `.turn-actions-hover`'s 120ms opacity transition, in seconds.
const HOVER_FADE: f32 = 0.12;

/// useBottomChatMotion's follow: the list keeps its real position at the end
/// while the painted chat eases into it, so a sent message or a streamed reply
/// glides up instead of jumping.
#[derive(Default)]
struct Follow {
    offset: f32,
    velocity: f32,
    growth: f32,
    /// Growth since the spring last stepped, the feed-forward's sample.
    grown: f32,
    measured: Option<FollowMeasure>,
    last_frame: Option<Instant>,
    #[cfg(test)]
    frame_elapsed: Option<f32>,
    last_step: Option<Instant>,
    settled_at: Option<Instant>,
    /// Growth keeps easing this long after a reply ends.
    until: Option<Instant>,
}
/// Units are px per 60fps frame, as in useBottomChatMotion.ts.
const SPRING_FRAME: f32 = 1000. / 60.;
/// Row motion redraws the whole transcript, so it runs at no more than 60fps
/// even on 120Hz displays; the slack keeps a 60Hz display at its full rate.
const MOTION_FRAME: std::time::Duration = std::time::Duration::from_millis(15);
/// Mono replies can appear together; keep their glide below 1080px/second.
const CHAT_FOLLOW_MAX_SPEED: f32 = 18.;
/// Steady frames only measure the tail. A large reveal measures its full
/// new distance once before handing over to this bounded window.
const FOLLOW_ROWS: usize = 80;

struct FollowMeasure {
    viewport: gpui::Size<Pixels>,
    bottom: Pixels,
    window_size: gpui::Size<Pixels>,
    rows: Vec<((usize, usize), f32)>,
    /// Regular chats fill the space below an anchored prompt before scrolling.
    footer: f32,
    /// A top-aligned chat that fits the viewport has no scroll distance yet.
    slack: f32,
}
impl FollowMeasure {
    fn resized_from(&self, previous: &Self) -> bool {
        self.viewport.width != previous.viewport.width || self.window_size != previous.window_size
    }
    /// Compare the suffix below a surviving row, like React's connected anchor.
    /// The composer's height moves that anchor too. Window resizing and changes
    /// to wrapping start a fresh baseline; internal vertical layout joins the glide.
    fn shift_from(&self, previous: &Self) -> f32 {
        if self.resized_from(previous) {
            return 0.;
        }
        let common = self.rows.iter().enumerate().find_map(|(next, (key, _))| {
            previous
                .rows
                .iter()
                .position(|(old, _)| old == key)
                .map(|old| (next, old))
        });
        let (next, old) = match common {
            Some(indices) => indices,
            None if self.rows.is_empty() || previous.rows.is_empty() => (0, 0),
            None => return 0.,
        };
        self.rows[next..]
            .iter()
            .map(|(_, height)| height)
            .sum::<f32>()
            - previous.rows[old..]
                .iter()
                .map(|(_, height)| height)
                .sum::<f32>()
            + self.footer
            - previous.footer
            + self.slack
            - previous.slack
            - f32::from(self.bottom - previous.bottom)
    }
}
impl Follow {
    /// stepFollowSpring: advance over `frames` and return the distance left.
    fn step(
        &mut self,
        mut offset: f32,
        grew: f32,
        mut frames: f32,
        sample_frames: f32,
        max_speed: f32,
    ) -> f32 {
        if grew < -1. {
            self.growth = 0.;
        } else {
            let observed = max_speed.min(grew.max(0.) / sample_frames.max(0.25));
            self.growth += 0.12 * (observed - self.growth);
        }
        let lead = (self.growth * 9.).min(32.);
        let mut velocity = self.velocity;
        while frames > 0. && offset > 0. {
            let h = frames.min(1.);
            frames -= h;
            let diff = (offset - lead).max(0.);
            velocity += h * ((0.7 * velocity + 0.05 * diff) / 1.25 - velocity);
            velocity = velocity.min(max_speed - self.growth);
            offset = (offset - (velocity + self.growth) * h).max(0.);
        }
        self.velocity = velocity;
        offset
    }
    fn add_shift(&mut self, shift: f32, viewport_height: f32, mono: bool) {
        let limit = (viewport_height * 0.75).max(0.);
        let offset = self.offset + shift;
        self.offset = if mono {
            offset.max(-limit)
        } else {
            offset.clamp(-limit, limit)
        };
        self.grown += shift;
    }
    fn reset(&mut self) {
        let measured = self.measured.take();
        let until = self.until;
        *self = Self::default();
        self.measured = measured;
        self.until = until;
    }
    fn settle(&mut self, now: Instant) {
        let (velocity, growth) = (self.velocity, self.growth);
        self.reset();
        self.velocity = velocity;
        self.growth = growth;
        self.last_frame = Some(now);
        self.last_step = Some(now);
        self.settled_at = Some(now);
    }
}

/// Whether the Mono has said something the user can read in `from..to`: the
/// opening sentence of any run there. A message the user sends after it reads
/// as the next line of the chat, not as part of the prompt above.
pub(crate) fn mono_spoke(source: &[Row], from: usize, to: usize) -> bool {
    let mut working = false;
    for (at, row) in source[from.min(to)..to].iter().enumerate() {
        if row.is_prompt() {
            working = false;
        } else if row.first && row.role != "reasoning" {
            if !working && row.role == "assistant" {
                // One message spans its first row and the continuations after it.
                let at = from.min(to) + at;
                let text: String = std::iter::once(row)
                    .chain(source[at + 1..to].iter().take_while(|r| !r.first))
                    .map(|r| r.text.as_str())
                    .collect();
                let text = text.trim();
                if !text.is_empty() && !text.starts_with("<reaction>") {
                    return true;
                }
            }
            working = true;
        }
    }
    false
}
/// The first row of the prompt block that opens `turn`.
pub(crate) fn prompt_start(source: &[Row], turn: usize) -> usize {
    let mut at = turn.min(source.len());
    while at > 0 && source[at - 1].is_prompt() {
        at -= 1;
    }
    at
}
/// groupMonoChatTurns: the turn a completion run at `turn` continues, if any.
pub(crate) fn mono_continues(source: &[Row], turn: usize) -> Option<usize> {
    let last = turn.checked_sub(1)?;
    if !source.get(last).is_some_and(|r| r.is_prompt()) {
        return None;
    }
    let prompt = prompt_start(source, turn);
    if prompt == 0
        || source[prompt..turn].iter().any(|r| r.role != "mono-system")
        || source[prompt - 1].is_prompt()
        || dates::mono_opens_stretch(source, prompt)
    {
        return None;
    }
    Some(
        source[..prompt]
            .iter()
            .rposition(|r| r.is_prompt())
            .map_or(0, |i| i + 1),
    )
}
/// monoWorkStatus: a short status for the turn's latest activity.
pub(crate) fn mono_work_label(source: &[Row], group: usize) -> String {
    let turn = source[group.min(source.len())..]
        .iter()
        .rposition(|r| r.is_prompt())
        .map_or(group, |i| group + i + 1);
    const THINKING: [&str; 4] = [
        "Thinking…",
        "Pondering…",
        "Considering…",
        "Working it through…",
    ];
    let end = (turn..source.len())
        .find(|&i| source[i].is_prompt())
        .unwrap_or(source.len());
    let mut thought = 0usize;
    let mut latest = None;
    for (ix, row) in source[turn..end].iter().enumerate() {
        if !row.first {
            continue;
        }
        match row.role.as_str() {
            "assistant" | "reasoning" => {
                thought += 1;
                latest = None;
            }
            "tool" => latest = Some(turn + ix),
            _ => {}
        }
    }
    let Some(tool) = latest.and_then(|at| {
        let end = (at + 1..source.len())
            .find(|&i| source[i].first)
            .unwrap_or(source.len());
        let text: String = source[at..end].iter().map(|r| r.text.as_str()).collect();
        serde_json::from_str::<ToolCall>(&text).ok()
    }) else {
        return THINKING[thought.saturating_sub(1) % THINKING.len()].into();
    };
    if tool.is_milestone() {
        return if tool.state != ToolState::Running {
            "Thinking…".into()
        } else if tool.is_handoff() {
            format!("{}…", tool.summary().0)
        } else {
            "Compacting context…".into()
        };
    }
    match tool.summary().0.as_str() {
        "Run" => "Running command…",
        "Edit" | "Write" => "Editing file…",
        "Read" => "Reading file…",
        "Find" | "List" | "Search" | "Glob" | "Grep" => "Exploring project…",
        "Agent" | "Task" => "Running agent…",
        _ => "Using tool…",
    }
    .into()
}
/// isEmojiOnlyMessage: up to three emoji, which read as a reaction.
pub(crate) fn is_emoji_only(text: &str) -> bool {
    use unicode_segmentation::UnicodeSegmentation;
    let mut count = 0;
    for grapheme in text.trim().graphemes(true) {
        if grapheme.trim().is_empty() {
            continue;
        }
        let first = grapheme.chars().next().unwrap_or_default() as u32;
        let picture = matches!(first, 0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0x2300..=0x23FF | 0x3030 | 0x303D | 0x3297 | 0x3299 | 0x00A9 | 0x00AE | 0x203C | 0x2049 | 0x2122 | 0x2139 | 0x2194..=0x21AA)
            || grapheme.contains('\u{20E3}');
        count += 1;
        if !picture || count > 3 {
            return false;
        }
    }
    count > 0
}

/// A single clipping boundary across virtualized siblings, matching React's
/// grid body. Fully revealed steps keep their height; only the boundary row clips.
struct PhaseLayout {
    width: Pixels,
    height: f32,
    rows: HashMap<(usize, usize), (f32, f32)>,
}
impl PhaseLayout {
    fn row_progress(&self, key: (usize, usize), progress: f32) -> f32 {
        let (top, height) = self.rows[&key];
        if height <= 0. {
            return 0.;
        }
        ((self.height * progress - top) / height).clamp(0., 1.)
    }
}

pub(crate) struct TranscriptView {
    selection: selection::Selection,
    selection_focus: Option<gpui::FocusHandle>,
    links: links::State,
    work_item_cards: user_links::Cards,
    source: Vec<Row>,
    agent_panel_rows: BTreeSet<usize>,
    batching_updates: bool,
    pending_rebuild: Option<usize>,
    source_bytes: usize,
    attachment_images: HashMap<String, gpui::Entity<attachments::Photo>>,
    visuals: HashMap<usize, visuals::CachedVisual>,
    /// Visual ids the answers place in their prose.
    placed: HashSet<String>,
    /// Placed visuals by id: their message's start and document.
    published: HashMap<String, (usize, Arc<monocode_core::visual::VisualDocument>)>,
    history_position: Option<history::HistoryPosition>,
    history_start: usize,
    history_loading: bool,
    history_requested: bool,
    /// Rows of older pages above the store's window. Store row `i` is `base + i`.
    base: usize,
    /// The store cursor for the turns above the loaded rows, if any.
    history_before: Option<i64>,
    history_pager: Option<history::HistoryPager>,
    /// The last page read failed; only a scroll retries it.
    history_failed: bool,
    history_search: Option<(usize, String)>,
    prepared_parsers: HashMap<usize, MarkdownParser>,
    mono: bool,
    mono_groups: std::cell::RefCell<HashMap<usize, usize>>,
    /// The Mono's mascot, colour and name, which sign its turns.
    mono_look: Option<(String, u32, SharedString)>,
    /// The turn whose actions show, and since when, as `.turn-actions-hover`.
    hovered_turn: Option<(usize, Instant)>,
    /// The prompt message whose copy action shows.
    hovered_prompt: Option<usize>,
    /// The prompt whose action strip the pointer is on. The strip hangs
    /// below the row's hitbox, so leaving the row must not hide it.
    hovered_prompt_actions: Option<usize>,
    /// Every hovered row and its turn. Entering the next row can arrive
    /// before leaving the last, so the turn is hovered while any row is.
    hover_rows: HashMap<(usize, usize), usize>,
    /// Messages entering the chat, by source start, as useBottomChatMotion.
    entrances: HashMap<usize, Instant>,
    /// The emoji a Mono answered a prompt with, by the prompt's start.
    reactions: HashMap<usize, (String, Option<Instant>)>,
    follow: Follow,
    /// monoPendingTranscriptBlocks: queued messages, after the transcript.
    pending: Vec<PendingMessage>,
    /// The outbox keeps this message until storage accepts it, but its prompt
    /// and Thinking header are already painted in the source transcript.
    submitted_mono_message: Option<String>,
    saved_draft: Option<monocode_core::follow_up::Message>,
    /// Pending messages just taken, including any entrance still in flight.
    taken: Vec<TakenMessage>,
    pending_entrances: HashMap<String, Instant>,
    launches: HashMap<i64, LaunchView>,
    /// The session's latestTurnAccessory card.
    review: Option<gpui::AnyView>,
    /// The card has changes or an error to show, so it takes a row.
    review_shown: bool,
    /// A clock for the tool and task shimmers.
    started_at: Instant,
    /// One clock per frame for row motion. The list measures rows and the
    /// footer re-measures them; mixed times leave the spacer a few pixels
    /// short of a fast disclosure, so an anchored prompt bounces.
    frame_now: Instant,
    measurements: measurements::Measurements,
    markdown_cache: markdown::RenderCache,
    messages: Vec<Message>,
    rows: Vec<UiRow>,
    streaming: bool,
    chat_layout: bool,
    anchor_prompts: bool,
    smooth_reveal: bool,
    search_match: Option<(usize, String)>,
    highlights: HashMap<u64, Option<Arc<HighlightedDocument>>>,
    highlighting: HashSet<u64>,
    highlight_queue: VecDeque<Arc<Fence>>,
    highlight_active: bool,
    highlight_epoch: u64,
    highlight_order: VecDeque<(u64, usize)>,
    highlight_bytes: usize,
    copied: Option<u64>,
    copied_response: Option<(usize, Instant)>,
    note_pending: HashSet<usize>,
    note_feedback: HashMap<usize, Result<(), String>>,
    action_anchors: HashMap<(usize, &'static str), AnchorSlot>,
    hovered_action: Option<(usize, &'static str)>,
    pub(crate) target_action: Option<(usize, monocode_core::response::TransferKind)>,
    document: bool,
    side_question: bool,
    pub(crate) file_link_base: Option<String>,
    expanded: HashSet<usize>,
    agent_panels: tasks::AgentPanels,
    /// User messages opened past their four-line clamp, by start.
    expanded_prompts: HashSet<usize>,
    /// Collapsed user messages whose later chunks are hidden, by start.
    clamped_prompts: HashSet<usize>,
    phases: HashMap<usize, bool>,
    phase_toggle: Option<usize>,
    phase_layouts: HashMap<usize, PhaseLayout>,
    phase_scrolls: HashMap<usize, live_phase::LiveScroll>,
    windowed_phases: HashSet<usize>,
    phase_max_height: Pixels,
    work_folds: HashMap<usize, bool>,
    reveals: HashMap<usize, PacedText>,
    live_start: usize,
    motions: HashMap<(usize, usize), Motion>,
    reflow: crate::reflow_motion::ReflowMotion,
    frame_scheduled: bool,
    last_motion_frame: Option<Instant>,
    visible: bool,
    viewport: Bounds<Pixels>,
    anchor_turn: Option<usize>,
    prompt_rise: Option<(usize, Instant)>,
    /// The mode or /visualize burst inside a just-sent prompt.
    burst: Option<burst::Burst>,
    prompt_rise_pending: bool,
    introduced_prompt: Option<usize>,
    show_jump: bool,
    commit_files: git_turns::CommitFiles,
    copied_commit: Option<(String, Instant)>,
    /// The conversation's git action pill holds the column's left end, so the
    /// jump button moves from the center to the right end.
    jump_beside_pill: bool,
    jump_anchor: crate::popover::AnchorSlot,
    scroll_bound: bool,
    fold_targets: HashMap<usize, Option<usize>>,
    step_queue: HashMap<usize, Instant>,
    seen_steps: HashSet<usize>,
    pub(crate) turns: HashMap<usize, monocode_core::agent::TurnInfo>,
    provider: String,
    model: String,
    active_provider_turn_id: Option<String>,
    started: Option<std::time::Instant>,
    elapsed: HashMap<usize, std::time::Duration>,
    outline: outline::Outline,
    pub(crate) list: ListState,
    pub(crate) rendered: usize,
    pub(crate) render_passes: usize,
    #[cfg(test)]
    rebuild_passes: usize,
}
impl TranscriptView {
    pub(crate) fn side_question(&mut self) {
        self.side_question = true;
    }
    pub(crate) fn activity(rows: Vec<Row>, cx: &mut Context<Self>) -> Self {
        let mut view = Self::new();
        view.document = true;
        view.patch(0, rows, cx);
        view
    }
    /// AgentTranscript with `agentMascot`, `bottomAligned` and `inlineWork`.
    pub(crate) fn mono_mode(&mut self, mascot: &str, color: u32, name: &str) {
        self.mono_look = Some((mascot.into(), color, name.to_owned().into()));
        if self.mono {
            return;
        }
        self.mono = true;
        self.anchor_prompts = false;
        self.chat_layout = true;
        // A short chat rests on the composer, like `justify-end`.
        self.list = ListState::new(self.rows.len() + 1, ListAlignment::Bottom, px(320.));
        self.list.set_follow_mode(FollowMode::Tail);
        self.scroll_bound = false;
    }
    pub(crate) fn reveal_search(&mut self, row: usize, query: &str, cx: &mut Context<Self>) {
        if row < self.history_start {
            self.history_search = Some((row, query.into()));
            self.list.pause_following_tail();
            self.load_older_history(cx);
            return;
        }
        let start = self
            .messages
            .iter()
            .rev()
            .find(|m| m.start <= row)
            .map(|m| m.start)
            .unwrap_or(row);
        let turn = self
            .messages
            .iter()
            .rev()
            .find(|m| m.start <= row)
            .map(|m| m.turn)
            .unwrap_or(0);
        self.search_match = Some((start, query.to_owned()));
        self.expanded.insert(start);
        self.expanded_prompts.insert(start);
        self.work_folds.insert(turn, true);
        for message in &self.messages {
            if message.turn == turn {
                self.phases.insert(message.start, true);
            }
        }
        self.list.pause_following_tail();
        self.rebuild(0, cx);
        let candidates = || {
            self.rows
                .iter()
                .enumerate()
                .filter(|(_, r)| r.key.0 == start && !matches!(r.body, Body::WorkLine { .. }))
        };
        let matching = candidates()
            .find(|(_, r)| {
                let text = match &r.body {
                    Body::Markdown(md) => markdown::row_text(md),
                    Body::Plain(text) => text.to_string(),
                    Body::Interjection(note) => note.text.clone(),
                    Body::Tool { .. } => query.to_owned(),
                    _ => String::new(),
                };
                !monocode_core::search::literal_ranges(&text, query).is_empty()
            })
            .map(|(ix, _)| ix);
        if let Some(item_ix) = matching.or_else(|| candidates().next().map(|(ix, _)| ix)) {
            self.list.scroll_to(ListOffset {
                item_ix,
                offset_in_item: px(0.),
            });
        }
        cx.notify();
    }
    /// The rows of the store's window, numbered as the store numbers them.
    /// Older pages above it are read-only and never part of a save.
    pub(crate) fn source_rows(&self) -> &[Row] {
        &self.source[self.base..]
    }
    /// Every loaded row, older pages included, numbered as this view's turns
    /// and response actions are.
    pub(crate) fn history_rows(&self) -> &[Row] {
        &self.source
    }
    /// Whether `history_rows` holds every turn, not only the newest pages.
    pub(crate) fn history_complete(&self) -> bool {
        self.history_before.is_none()
    }
    pub(crate) fn document(text: String, cx: &mut Context<Self>) -> Self {
        let mut view = Self::new();
        view.document = true;
        // A document reads from its top. Chat's tail-follow would open it
        // scrolled to the end and keep it pinned there on every rebuild.
        view.list.set_follow_mode(FollowMode::Normal);
        view.patch(
            0,
            vec![Row {
                role: "assistant".into(),
                text,
                first: true,
                sent_at: None,
            }],
            cx,
        );
        view.list.scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
        view
    }
    /// liveAgents.ts's toLiveAgent: the last turn's latest tool, as "Read
    /// src/app.rs", when that turn was sent, and how long it ran once done.
    pub(crate) fn live_turn(&self, cwd: &str) -> (Option<String>, Option<u64>, Option<u64>) {
        let Some(prompt) = self.source.iter().rposition(|r| r.first && r.is_prompt()) else {
            return (None, None, None);
        };
        let turn = (prompt + 1..self.source.len())
            .find(|&at| self.source[at].first)
            .unwrap_or(self.source.len());
        let tool = (turn..self.source.len())
            .rev()
            .find(|&at| self.source[at].first && self.source[at].role == "tool")
            .and_then(|at| {
                let end = (at + 1..self.source.len())
                    .find(|&i| self.source[i].first)
                    .unwrap_or(self.source.len());
                let text: String = self.source[at..end]
                    .iter()
                    .map(|r| r.text.as_str())
                    .collect();
                serde_json::from_str::<ToolCall>(&text).ok()
            })
            .map(|tool| {
                let shown = tool.presentation_in(Some(cwd));
                [shown.action, shown.target]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .filter(|label| !label.is_empty());
        let duration = self
            .turns
            .get(&turn)
            .filter(|info| info.has_duration())
            .map(|info| info.duration_ms);
        (tool, self.source[prompt].sent_at, duration)
    }
    /// Follow-ups delivered during a provider run share its original receipt.
    pub(crate) fn active_turn(&self) -> Option<usize> {
        let start = self.live_start.min(self.source.len());
        let prompt = self.source[start..]
            .iter()
            .position(|row| row.first && row.is_prompt())?
            + start;
        Some(
            (prompt + 1..self.source.len())
                .find(|&at| self.source[at].first)
                .unwrap_or(self.source.len()),
        )
    }
    pub(crate) fn metrics(&mut self, metrics: monocode_core::agent::TurnMetrics) {
        let Some(turn) = self.active_turn() else {
            return;
        };
        self.turns.entry(turn).or_default().metrics = Some(metrics);
    }
    pub(crate) fn record_provider_turn(&mut self, id: String) {
        self.active_provider_turn_id = Some(id.clone());
        let Some(turn) = self.active_turn() else {
            return;
        };
        self.turns.entry(turn).or_default().provider_turn_id = Some(id);
    }
    pub(crate) fn response_rows(&self, turn: usize) -> (Vec<Row>, Vec<Row>) {
        let end = (turn..self.source.len())
            .find(|&i| {
                self.source[i].is_prompt() && !(self.mono && self.mono_next_run_continues(turn, i))
            })
            .unwrap_or(self.source.len());
        let mut start = self.source[..turn.min(self.source.len())]
            .iter()
            .rposition(|r| r.is_prompt())
            .unwrap_or(turn);
        while start > 0 && !self.source[start].first {
            start -= 1;
        }
        (
            self.source[start..end].to_vec(),
            self.source[..end].to_vec(),
        )
    }
    pub(crate) fn note_result(
        &mut self,
        turn: usize,
        result: Result<(), String>,
        cx: &mut Context<Self>,
    ) {
        self.note_pending.remove(&turn);
        let success = result.is_ok();
        self.note_feedback.insert(turn, result);
        if success && self.selection_turn() == Some(turn) {
            self.selection.clear();
        }
        cx.notify();
        if success {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(2))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.note_feedback.remove(&turn);
                    cx.notify();
                });
            })
            .detach();
        }
    }
    /// TurnDuration with `revealOnHover`: copy, save as note and activity in
    /// the turn's fixed gap, then its time, shown while the turn is hovered.
    fn mono_actions(
        &mut self,
        turn: usize,
        output: String,
        before_user: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let copied = self.copied_response.is_some_and(|(id, _)| id == turn);
        let saved = self.note_feedback.get(&turn).is_some_and(|r| r.is_ok());
        let pending = self.note_pending.contains(&turn);
        let opacity = match self.hovered_turn {
            Some((hovered, since)) if hovered == turn => {
                (since.elapsed().as_secs_f32() / HOVER_FADE).min(1.)
            }
            _ if copied || pending => 1.,
            _ => 0.,
        };
        let completed_at = self
            .turns
            .iter()
            .filter(|(run, _)| self.mono_group_start(**run) == turn)
            .max_by_key(|(run, _)| *run)
            .and_then(|(_, info)| info.completed_at);
        let dot = || {
            div()
                .size(px(3.))
                .flex_shrink_0()
                .rounded_full()
                .bg(content(0.25))
        };
        let mut actions = div().flex().flex_shrink_0().items_center().gap_1();
        if output.is_empty() {
            actions = actions.child(icon("check", 14.).text_color(content(0.4)));
        } else {
            let text = output.clone();
            let quote = output.clone();
            actions = actions
                .child(
                    response_actions::button(
                        turn,
                        "mono-copy-response",
                        if copied { "Copied" } else { "Copy response" },
                        if copied { "check" } else { "response-copy" },
                        false,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                        let born = Instant::now();
                        this.copied_response = Some((turn, born));
                        cx.notify();
                        cx.spawn(async move |this, cx| {
                            cx.background_executor()
                                .timer(std::time::Duration::from_secs(2))
                                .await;
                            let _ = this.update(cx, |this, cx| {
                                if this.copied_response == Some((turn, born)) {
                                    this.copied_response = None;
                                    cx.notify();
                                }
                            });
                        })
                        .detach();
                    })),
                )
                .child(
                    response_actions::button(
                        turn,
                        "mono-note-response",
                        if saved {
                            "Saved to Notes"
                        } else if pending {
                            "Saving note…"
                        } else {
                            "Save as note"
                        },
                        if saved { "check" } else { "response-note" },
                        false,
                    )
                    .when(pending, |d| d.opacity(0.4))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.note_pending.insert(turn) {
                            this.note_feedback.remove(&turn);
                            cx.emit(ResponseRequest::SaveNote {
                                turn,
                                text: output.clone(),
                                selection: false,
                            });
                            cx.notify();
                        }
                    })),
                )
                .child(
                    response_actions::button(
                        turn,
                        "mono-quote-response",
                        "Quote response",
                        "comment-add",
                        false,
                    )
                    .on_click(cx.listener(move |_, _, window, cx| {
                        cx.emit(MonoResponseAction::Quote {
                            text: quote.clone(),
                            window: window.window_handle(),
                        })
                    })),
                );
        }
        actions = actions.child(
            response_actions::button(
                turn,
                "mono-response-activity",
                "Show activity",
                "list-bullet",
                false,
            )
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(MonoResponseAction::Activity(turn)))),
        );
        div()
            .id(("mono-turn-actions", turn))
            .relative()
            .top(px(-4.))
            .h(px(24.))
            .mb(px(if before_user { 0. } else { 4. }))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(10.))
            .overflow_hidden()
            .text_size(px(14.))
            .text_color(content(0.4))
            .opacity(opacity)
            .child(actions)
            .when_some(completed_at, |d, time| {
                d.child(dot()).child(
                    div()
                        .flex_shrink_0()
                        .text_color(content(0.35))
                        .child(monocode_core::response::local_clock(time)),
                )
            })
            .into_any_element()
    }
    /// The Mono signs its turn with its mascot and name, beside a live ticker.
    fn mono_turn_header(&self, turn: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        // Completion runs share the original turn's header.
        let label = self
            .mono_activity()
            .filter(|(group, _)| *group == turn)
            .map(|(_, label)| label);
        self.mono_header(turn, label, cx)
    }
    /// The latest live activity, without relabeling a completed turn before
    /// the next prompt arrives.
    fn mono_activity(&self) -> Option<(usize, String)> {
        if !self.mono || !self.streaming || self.source.len() <= self.live_start {
            return None;
        }
        let latest = self
            .source
            .iter()
            .rposition(|r| r.is_prompt())
            .map_or(0, |ix| ix + 1);
        if latest < self.live_start && self.source[..latest].iter().any(Row::is_prompt) {
            return None;
        }
        let typing = (latest..self.source.len())
            .rev()
            .find(|&at| self.source[at].first)
            .is_some_and(|message| {
                self.source[message].role == "assistant"
                    && self.source[message..]
                        .iter()
                        .any(|row| !row.text.trim().is_empty())
                    && self.mono_prose_hidden(message)
            });
        let group = self.mono_group_start(latest);
        Some((
            group,
            if typing {
                "Typing…".into()
            } else {
                mono_work_label(&self.source, group)
            },
        ))
    }
    fn mono_header(
        &self,
        turn: usize,
        label: Option<String>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some((mascot, color, name)) = self.mono_look.clone() else {
            return div().into_any_element();
        };
        let pill = div()
            .debug_selector(|| "mono-signature-pill".into())
            .flex()
            .min_w_0()
            .flex_shrink_0()
            .max_w(px(320.))
            .items_center()
            .gap_1p5()
            .rounded_full()
            .bg(content(0.07))
            .py(px(2.))
            .pl_2()
            .pr_3()
            .child(
                div()
                    .size(px(14.))
                    .flex_shrink_0()
                    .child(crate::monos::avatar::avatar(
                        &mascot, color, "idle", 14., true,
                    )),
            )
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(content(0.8))
                    .child(name),
            );
        div()
            .id(("mono-turn-header", turn))
            .flex()
            .min_w_0()
            .items_center()
            .gap_1p5()
            .py_1()
            .text_size(px(14.))
            .line_height(px(20.))
            .text_color(content(0.5))
            .child(pill)
            .when_some(label, |d, label| {
                d.child(div().flex_shrink_0().text_color(content(0.25)).child("·"))
                    .child(
                        div()
                            .id(("mono-turn-ticker", turn))
                            .debug_selector(|| "mono-turn-ticker".into())
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .hover(|s| s.opacity(0.7))
                            .child(if cx.reduce_motion() {
                                div().child(label).into_any_element()
                            } else {
                                crate::activity::shimmer(
                                    label,
                                    self.started.map_or(0., |s| s.elapsed().as_secs_f32()),
                                )
                            })
                            .when(turn < self.source.len(), |d| {
                                d.role(Role::Button).aria_label("Show activity").on_click(
                                    cx.listener(move |_, _, _, cx| {
                                        cx.emit(MonoResponseAction::Activity(turn))
                                    }),
                                )
                            }),
                    )
            })
            .into_any_element()
    }
    /// MonoReactionBadge, on the top left corner of the message it answers.
    fn reaction_badge(&self, prompt: usize) -> Option<gpui::AnyElement> {
        let (emoji, born) = self.reactions.get(&prompt)?;
        // agent-chat-bubble-in: it lands with a small bounce.
        let (scale, opacity) = born.map_or((1., 1.), |born| {
            let t = (born.elapsed().as_secs_f32() / 0.32).min(1.);
            (
                0.9 + 0.1 * crate::reveal::bezier(t, 0.2, 1.4, 0.4, 1.),
                (t / 0.4).min(1.),
            )
        });
        let height = 22. * scale;
        Some(
            div()
                .id(("mono-reaction", prompt))
                .role(Role::Image)
                .absolute()
                .top(px(-10. - (height - 22.) / 2.))
                .left(px(-10.))
                .h(px(height))
                .px(px(7. * scale))
                .flex()
                .items_center()
                .rounded(px(8. * scale))
                .bg(content(0.17))
                .border_1()
                .border_color(content(0.1))
                .text_size(px(11. * scale))
                .line_height(px(11. * scale))
                .opacity(opacity)
                .aria_label(format!("Reacted {emoji}"))
                .child(emoji.clone())
                .into_any_element(),
        )
    }
    /// The sent message's actions, under its bubble while it is hovered.
    fn prompt_actions(&mut self, start: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let end = (start + 1..self.source.len())
            .find(|&i| self.source[i].first)
            .unwrap_or(self.source.len());
        let text: String = self.source[start..end]
            .iter()
            .map(|r| r.text.as_str())
            .collect();
        let copied = self.copied.is_some_and(|id| id == start as u64);
        let sent_at = self.source[start].sent_at;
        let base = self.base;
        let stamp_anchor = self
            .action_anchors
            .entry((start, "sent-at"))
            .or_default()
            .clone();
        div()
            .id(("prompt-actions", start))
            .absolute()
            .top_full()
            .right_0()
            .pt_1()
            .px_3()
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hovered_prompt_actions = Some(start);
                } else if this.hovered_prompt_actions == Some(start) {
                    this.hovered_prompt_actions = None;
                }
                cx.notify();
            }))
            .flex()
            .items_center()
            .gap_1()
            .child(
                response_actions::button(
                    start,
                    "mono-copy-message",
                    if copied { "Copied" } else { "Copy message" },
                    if copied { "check" } else { "response-copy" },
                    false,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                    this.copied = Some(start as u64);
                    cx.notify();
                })),
            )
            .when(
                !self.mono
                    && !self.side_question
                    && !self.streaming
                    && monocode_core::provider::supports_rewind(
                        monocode_core::provider::Kind::from_id(&self.provider),
                    )
                    && start >= base
                    && self
                        .source
                        .iter()
                        .rposition(|row| row.first && row.role == "user")
                        == Some(start),
                |d| {
                    d.child(
                        response_actions::button(
                            start,
                            "edit-last-message",
                            "Edit last message",
                            "pencil",
                            false,
                        )
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(ResponseRequest::EditLastTurn { turn: start - base })
                        })),
                    )
                },
            )
            .when_some(sent_at, |d, at| {
                let label = dates::full_stamp(at);
                d.child(
                    div()
                        .id(("message-sent-at", start))
                        .debug_selector(|| "message-sent-at".into())
                        .ml_1()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(content(0.4))
                        .aria_label(label.clone())
                        .child(stamp_anchor.probe())
                        .child(monocode_core::response::local_clock(at))
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            this.action_hover(start, "sent-at", *hovered, cx)
                        })),
                )
                .when(self.hovered_action == Some((start, "sent-at")), |d| {
                    d.child(
                        popover(
                            ("message-time-tooltip", start),
                            Anchor::Slot(stamp_anchor.clone()),
                        )
                        .side(Side::Top)
                        .padding(8.)
                        .child(div().text_size(px(12.)).child(label)),
                    )
                })
            })
            .into_any_element()
    }
    /// `.transcript-turn:hover` shows the reply's actions; hovering a prompt
    /// shows only its own.
    fn mono_hover(
        &mut self,
        prompt: bool,
        start: usize,
        turn: usize,
        key: (usize, usize),
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        if prompt {
            if hovered {
                self.hovered_prompt = Some(start);
            } else if self.hovered_prompt == Some(start) {
                self.hovered_prompt = None;
            }
        } else if hovered {
            self.hover_rows.insert(key, turn);
        } else {
            self.hover_rows.remove(&key);
        }
        let current = if !prompt && hovered {
            Some(turn)
        } else {
            self.hovered_turn
                .map(|(t, _)| t)
                .filter(|t| self.hover_rows.values().any(|v| v == t))
                .or_else(|| self.hover_rows.values().next().copied())
        };
        if current != self.hovered_turn.map(|(t, _)| t) {
            self.hovered_turn = current.map(|t| (t, Instant::now()));
        }
        cx.notify();
    }
    /// Measures the live turn's tail before the list lays it out, so growth
    /// the list follows is painted as an offset that eases away.
    fn update_follow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        let animated = self.streaming || self.follow.until.is_some_and(|until| now < until);
        if self.document || !self.visible || cx.reduce_motion() {
            self.follow = Follow::default();
            return;
        }
        if !self.list.is_following_tail() {
            // Hand the painted position to native scrolling once, like React's pause.
            self.list.scroll_by(px(-self.follow.offset));
            self.follow = Follow::default();
            return;
        }
        if self.viewport.size.width <= px(0.) {
            return;
        }
        let turn = self
            .source
            .iter()
            .rposition(|r| r.is_prompt())
            .map_or(0, |ix| ix + 1);
        let prompt = self.prompt_start(self.mono_group_start(turn));
        let group = self
            .messages
            .iter()
            .find(|m| m.start >= prompt)
            .map_or(0, |m| m.display_start);
        let tail = self.rows.len().saturating_sub(FOLLOW_ROWS);
        let baseline = group.max(tail);
        // Include the previous anchor for the handover to a new turn. Store only
        // the new group's baseline afterwards, so old turns aren't measured forever.
        let mut from = self
            .follow
            .measured
            .as_ref()
            .and_then(|m| m.rows.first())
            .and_then(|(key, _)| self.rows[..baseline].iter().position(|r| r.key == *key))
            .unwrap_or(baseline);
        let mut measured = FollowMeasure {
            viewport: self.viewport.size,
            bottom: self.viewport.bottom(),
            // These bounds belong to the previous layout until prepaint gives
            // us the current viewport, including the composer's new height.
            window_size: self
                .follow
                .measured
                .as_ref()
                .map_or_else(|| window.viewport_size(), |m| m.window_size),
            rows: Vec::new(),
            footer: 0.,
            slack: 0.,
        };
        for ix in from..self.rows.len() {
            let height = f32::from(self.measure_row_height(ix, window, cx));
            measured.rows.push((self.rows[ix].key, height));
        }
        if !self.mono {
            measured.footer = f32::from(self.footer_height(window, cx));
            let mut height = TOP_INSET
                + measured.footer
                + measured.rows.iter().map(|(_, height)| height).sum::<f32>();
            // A short tail can still belong to a long conversation. Measure
            // preceding rows only until the viewport is filled, so underflow
            // is exact without measuring the whole history on each frame.
            let mut leading = Vec::new();
            while from > 0 && height < f32::from(self.viewport.size.height) {
                from -= 1;
                let row_height = f32::from(self.measure_row_height(from, window, cx));
                height += row_height;
                leading.push((self.rows[from].key, row_height));
            }
            leading.reverse();
            leading.append(&mut measured.rows);
            measured.rows = leading;
            measured.slack = (f32::from(self.viewport.size.height) - height).max(0.);
        }
        // The spacer pins the prompt in native layout. A follow spring left
        // over from taller content must never paint it below that position.
        let prompt_overflow = self.anchor_row().and_then(|first| {
            if measured.footer > BOTTOM_INSET {
                Some(0.)
            } else if first >= from {
                Some(
                    (TOP_INSET
                        + BOTTOM_INSET
                        + measured.rows[first - from..]
                            .iter()
                            .map(|(_, height)| height)
                            .sum::<f32>()
                        - f32::from(self.viewport.size.height))
                    .max(0.),
                )
            } else {
                None
            }
        });
        let resized = self.follow.measured.as_ref().is_some_and(|m| {
            measured.resized_from(m)
                || (!self.mono && measured.viewport.height != m.viewport.height)
        });
        let shift = self.follow.measured.as_ref().map_or(0., |previous| {
            measured.shift_from(previous) - self.closing_shrink(&measured, previous)
        });
        measured.rows.drain(..baseline - from);
        self.follow.measured = Some(measured);
        if resized || prompt_overflow == Some(0.) || (!animated && shift.abs() > 0.) {
            self.follow.reset();
            return;
        }
        let mut elapsed = self
            .follow
            .last_frame
            .map_or(0., |at| now.duration_since(at).as_secs_f32() * 1000.)
            .min(64.);
        #[cfg(test)]
        if let Some(frame_elapsed) = self.follow.frame_elapsed {
            elapsed = frame_elapsed;
        }
        self.follow.last_frame = Some(now);
        if animated && shift.abs() > 0. {
            if self.follow.offset.abs() < 0.25 {
                elapsed = 0.;
            }
            if self.follow.offset.abs() < 0.25
                && self
                    .follow
                    .settled_at
                    .is_none_or(|at| now.duration_since(at).as_secs_f32() > 0.5)
            {
                self.follow.velocity = 0.;
                self.follow.growth = 0.;
                self.follow.last_step = Some(now);
            }
            self.follow.settled_at = None;
            self.follow
                .add_shift(shift, f32::from(self.viewport.size.height), self.mono);
        }
        if let Some(overflow) = prompt_overflow {
            self.follow.offset = self.follow.offset.min(overflow);
        }
        // As in React's measure/tick split, the first growth frame only
        // records distance. Sample its velocity after real frame time passes.
        if elapsed == 0. {
            return;
        }
        if self.follow.offset > 0. {
            let sample = self
                .follow
                .last_step
                .map_or(0., |at| now.duration_since(at).as_secs_f32() * 1000.)
                / SPRING_FRAME;
            let grown = self.follow.grown;
            self.follow.offset = self.follow.step(
                self.follow.offset,
                grown,
                elapsed / SPRING_FRAME,
                sample.min(60.),
                if self.mono {
                    CHAT_FOLLOW_MAX_SPEED
                } else {
                    f32::INFINITY
                },
            );
        } else {
            self.follow.offset *= (-elapsed / 85.).exp();
        }
        self.follow.grown = 0.;
        self.follow.last_step = Some(now);
        if self.follow.offset.abs() < 0.25 && self.follow.offset != 0. {
            self.follow.settle(now);
        }
    }
    /// Content is measured before layout; compensate for vertical layout once
    /// its actual bounds are known, before the list builds its painted rows.
    fn update_follow_viewport(&mut self, bounds: Bounds<Pixels>, window: &Window) -> bool {
        let Some(measured) = self.follow.measured.as_mut() else {
            return false;
        };
        let resized = measured.viewport.width != bounds.size.width
            || measured.window_size != window.viewport_size()
            || (!self.mono && measured.viewport.height != bounds.size.height);
        let shift = f32::from(measured.bottom - bounds.bottom());
        measured.viewport = bounds.size;
        measured.bottom = bounds.bottom();
        measured.window_size = window.viewport_size();
        if resized {
            self.follow.reset();
            // The old rows may wrap differently at the new width.
            self.follow.measured = None;
        } else if shift != 0. {
            let now = Instant::now();
            if self.streaming
                || self.follow.until.is_some_and(|until| now < until)
                || self.follow.offset.abs() >= 0.25
            {
                self.follow
                    .add_shift(shift, f32::from(bounds.size.height), self.mono);
            } else {
                self.follow.reset();
            }
        }
        resized
    }
    /// Height lost by rows easing closed, such as work folding behind its
    /// line. Their motion already eases it; following must not ease it again.
    fn closing_shrink(&self, measured: &FollowMeasure, previous: &FollowMeasure) -> f32 {
        if measured.resized_from(previous) {
            return 0.;
        }
        let before: HashMap<_, _> = previous.rows.iter().copied().collect();
        let shrink: f32 = measured
            .rows
            .iter()
            .filter(|(key, _)| self.motions.get(key).is_some_and(|m| !m.opening))
            .filter_map(|(key, height)| before.get(key).map(|old| height - old))
            .sum();
        // An anchored prompt's spacer absorbs closing height, so nothing
        // above moves. Only height beyond the spare space shifts the chat.
        let spare = ((previous.footer - BOTTOM_INSET).max(0.) + previous.slack).max(0.);
        if shrink < 0. {
            (shrink + spare).min(0.)
        } else {
            shrink
        }
    }
    fn footer_ix(&self) -> usize {
        self.rows.len()
    }
    /// Shows queued messages as bubbles at the end of the chat.
    pub(crate) fn set_pending(&mut self, pending: Vec<PendingMessage>, cx: &mut Context<Self>) {
        if self.pending == pending {
            return;
        }
        let now = Instant::now();
        let animate = self.visible && !cx.reduce_motion();
        for old in &self.pending {
            if !pending.iter().any(|p| p.id == old.id) {
                self.taken.push(TakenMessage {
                    text: old.text.clone(),
                    removed_at: now,
                    entrance: self.pending_entrances.get(&old.id).copied(),
                });
            }
        }
        self.taken
            .retain(|message| now.duration_since(message.removed_at).as_secs_f32() < 2.);
        let added: Vec<_> = pending
            .iter()
            .filter(|p| !self.pending.iter().any(|o| o.id == p.id))
            .map(|p| p.id.clone())
            .collect();
        self.pending = pending;
        if !self.pending.iter().any(|message| {
            message.error.is_none() && Some(&message.id) == self.submitted_mono_message.as_ref()
        }) {
            self.submitted_mono_message = None;
        }
        if animate {
            for id in added {
                if let Some(ix) = self.pending.iter().position(|p| p.id == id) {
                    self.pending_entrances
                        .insert(self.pending[ix].id.clone(), now);
                }
            }
            self.follow.until = Some(now + std::time::Duration::from_millis(1200));
        }
        let from = self.source.iter().rposition(|r| r.is_prompt()).unwrap_or(0);
        if self.source.is_empty() {
            self.rows.clear();
            self.messages.clear();
            self.push_pending(0);
            self.list.reset(self.rows.len() + 1);
            if self.list.is_following_tail() || self.rows.len() <= self.pending.len() {
                self.list.scroll_to_end();
            }
        } else {
            self.rebuild(from, cx);
        }
        cx.notify();
    }
    fn push_pending(&mut self, at: usize) {
        for i in 0..self.pending.len() {
            if Some(&self.pending[i].id) == self.submitted_mono_message.as_ref() {
                continue;
            }
            self.rows.push(UiRow {
                key: (at, 100_000 + i),
                body: Body::Pending(i),
                message: usize::MAX,
                first: false,
                last: false,
                work: false,
                work_tail: false,
                thought_phase: None,
            });
        }
    }
    /// Starting the provider precedes its new prompt. Until that prompt
    /// arrives, the previous answer remains a finished turn.
    fn mono_pending_joins_live_turn(&self) -> bool {
        self.streaming
            && self
                .source
                .iter()
                .rposition(Row::is_prompt)
                .is_some_and(|prompt| {
                    prompt >= self.live_start
                        && !mono_spoke(
                            &self.source,
                            self.mono_group_start(prompt + 1),
                            self.source.len(),
                        )
                })
    }
    /// Whether `at` sits in a completion run that continues the turn above,
    /// returning the turn that opens the group: groupMonoChatTurns.
    fn mono_group_start(&self, mut turn: usize) -> usize {
        let mut visited = Vec::new();
        let group = loop {
            if let Some(group) = self.mono_groups.borrow().get(&turn).copied() {
                break group;
            }
            visited.push(turn);
            match self
                .turns
                .get(&turn)
                .and_then(|info| info.joined_turn)
                // A steer stays beside the prompt it joins only until the
                // Mono has replied in words; after that, the chat reads in order.
                .filter(|&root| {
                    root < turn && !mono_spoke(&self.source, root, prompt_start(&self.source, turn))
                })
                .or_else(|| mono_continues(&self.source, turn))
            {
                Some(previous) => turn = previous,
                None => break turn,
            }
        };
        let mut groups = self.mono_groups.borrow_mut();
        for turn in visited {
            groups.insert(turn, group);
        }
        group
    }
    fn mono_next_run_continues(&self, turn: usize, end: usize) -> bool {
        end < self.source.len()
            && self.mono_group_start(
                (end..self.source.len())
                    .find(|&at| !self.source[at].is_prompt())
                    .unwrap_or(self.source.len()),
            ) == self.mono_group_start(turn)
    }
    /// The first row of the prompt block that opens `turn`.
    fn prompt_start(&self, turn: usize) -> usize {
        prompt_start(&self.source, turn)
    }
    /// groupMonoTurnItems: prose stays in the activity trail while its run is
    /// live, and when later work in the run follows it.
    fn mono_prose_hidden(&self, at: usize) -> bool {
        let end = (at + 1..self.source.len())
            .find(|&i| self.source[i].is_prompt())
            .unwrap_or(self.source.len());
        let turn = self.source[..at]
            .iter()
            .rposition(Row::is_prompt)
            .map_or(0, |prompt| prompt + 1);
        let internal = turn > 0 && self.source[turn - 1].role == "mono-system";
        // A direct reply keeps its opening sentence for the conversation.
        // Notification runs keep all commentary in activity and show only
        // their final message. Every Mono message appears whole.
        let start = self.source[..=at]
            .iter()
            .rposition(|r| r.first)
            .unwrap_or(at);
        let complete = (start + 1..self.source.len()).any(|i| self.source[i].first)
            || !(self.streaming && start >= self.live_start);
        if !internal
            && self.source[turn..start]
                .iter()
                .all(|r| !r.first || r.role == "reasoning")
        {
            return !complete;
        }
        // Starting a request precedes delivery of its prompt. History remains
        // settled during that gap, even though it is still the source's tail.
        (self.streaming && at >= self.live_start && end == self.source.len())
            || (self.source.get(end).is_some_and(|row| row.role == "user")
                && self.mono_next_run_continues(turn, end))
            || self.source[at + 1..end].iter().any(|r| {
                r.first
                    && (matches!(r.role.as_str(), "tool" | "reasoning")
                        || (internal && r.role == "assistant"))
            })
    }
    /// monoSpawnedSessions: sessions a run's `sessions.start` calls accepted.
    fn run_launches(&self, turn: usize, end: usize) -> Vec<i64> {
        let mut found = Vec::new();
        let mut at = turn;
        while at < end {
            let next = (at + 1..end).find(|&i| self.source[i].first).unwrap_or(end);
            if self.source[at].role == "mono-launch" {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&self.source[at].text)
                    && let Some(id) = value["sessionId"].as_i64()
                    && !found.contains(&id)
                {
                    found.push(id);
                }
            } else if self.source[at].role == "tool" {
                let text: String = self.source[at..next]
                    .iter()
                    .map(|r| r.text.as_str())
                    .collect();
                if let Ok(tool) = serde_json::from_str::<ToolCall>(&text)
                    && tool.state == ToolState::Completed
                    && let Ok(input) = serde_json::from_str::<serde_json::Value>(&tool.input)
                    && let Some(action) = input["command"]
                        .as_str()
                        .or_else(|| input["cmd"].as_str())
                        .and_then(monocode_core::mono::work::command_action)
                    && matches!(action.as_str(), "sessions.start" | "sessions.send")
                {
                    let receipts = serde_json::from_str::<serde_json::Value>(&tool.output)
                        .map(|v| vec![v])
                        .unwrap_or_else(|_| {
                            tool.output
                                .lines()
                                .filter_map(|line| serde_json::from_str(line.trim()).ok())
                                .collect()
                        });
                    for value in receipts {
                        if value["ok"] == true
                            && let Some(id) = value["result"]["sessionId"]
                                .as_i64()
                                .or_else(|| {
                                    monocode_core::mono::work::session_handle(
                                        &value["result"]["id"],
                                    )
                                })
                                .or_else(|| {
                                    (value["result"]["submitted"] == true)
                                        .then(|| {
                                            monocode_core::mono::work::session_handle(
                                                &value["result"]["sessionId"],
                                            )
                                        })
                                        .flatten()
                                })
                            && (action == "sessions.start" || self.launches.contains_key(&id))
                            && !found.contains(&id)
                        {
                            found.push(id);
                        }
                    }
                }
            }
            at = next;
        }
        found
    }
    /// The live state of the sessions this Mono launched, for their cards.
    pub(crate) fn set_launches(
        &mut self,
        launches: HashMap<i64, LaunchView>,
        cx: &mut Context<Self>,
    ) {
        if self.launches != launches {
            let structure_changed = self.launches.len() != launches.len()
                || self.launches.keys().any(|id| !launches.contains_key(id));
            self.launches = launches;
            if structure_changed {
                self.rebuild(0, cx);
            } else {
                cx.notify();
            }
        }
    }
    fn launch_row(&mut self, ix: usize, id: i64, cx: &mut Context<Self>) -> gpui::AnyElement {
        let (top, bottom) = self.mono_card_spacing(ix);
        let view = self.launches.get(&id).cloned();
        div()
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .relative()
                    .w_full()
                    .max_w(px(COLUMN))
                    .px_4()
                    .pt(px(top))
                    .pb(px(bottom))
                    .child(crate::monos::cards::launch(id, view.as_ref(), cx)),
            )
            .into_any_element()
    }
    fn mono_card_spacing(&self, ix: usize) -> (f32, f32) {
        let is_card = |row: &UiRow| matches!(row.body, Body::Launch(_) | Body::MonoCard(_));
        let top = if ix > 0 && is_card(&self.rows[ix - 1]) {
            0.
        } else {
            4.
        };
        // Prose already supplies its own 12px top gap. Do not add another
        // 12px below the card when a completion continues the conversation.
        let bottom = match self.rows.get(ix + 1) {
            Some(row) if is_card(row) => 8.,
            Some(row) if matches!(row.body, Body::Markdown(_)) => 0.,
            _ => 12.,
        };
        (top, bottom)
    }
    /// The session's review card, under the latest settled turn only.
    pub(crate) fn set_review(&mut self, review: gpui::AnyView) {
        self.review = Some(review);
    }
    /// The card gained or lost its content; the latest turn makes room for it.
    pub(crate) fn set_review_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        if self.review_shown == shown {
            return;
        }
        self.review_shown = shown;
        if !self.messages.is_empty() {
            let start = self
                .source
                .iter()
                .rposition(|r| r.is_prompt())
                .map_or(0, |i| i + 1);
            self.rebuild(start, cx);
        }
        cx.notify();
    }
    fn review_row(&self, turn: usize) -> gpui::AnyElement {
        let latest = self
            .source
            .iter()
            .rposition(|r| r.is_prompt())
            .map_or(0, |i| i + 1);
        let Some(review) = self
            .review
            .clone()
            .filter(|_| !self.streaming && turn == latest)
        else {
            return div().into_any_element();
        };
        div()
            .w_full()
            .flex()
            .justify_center()
            .child(
                div()
                    .relative()
                    .w_full()
                    .max_w(px(COLUMN))
                    .px_4()
                    .pt_1()
                    .pb_2()
                    .child(review),
            )
            .into_any_element()
    }
    /// A queued message's bubble, as UserMessageBlock draws one, with its
    /// delivery line when it could not be sent.
    fn pending_row(&mut self, ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(message) = self.pending.get(ix).cloned() else {
            return div().into_any_element();
        };
        let stacked = self
            .pending
            .get(ix + 1)
            .is_some_and(|next| !dates::opens_stretch(next.sent_at, message.sent_at));
        let thinking = ix + 1 == self.pending.len()
            && !self.mono_pending_joins_live_turn()
            && self
                .pending
                .first()
                .is_some_and(|message| message.error.is_none());
        let (mut opacity, mut rise) = (1., 0.);
        if let Some(born) = self.pending_entrances.get(&message.id)
            && !cx.reduce_motion()
        {
            let t = crate::reveal::bezier(
                (born.elapsed().as_secs_f32() / 0.36).min(1.),
                0.22,
                1.,
                0.36,
                1.,
            );
            opacity = t;
            rise = 16. * (1. - t);
        }
        let retry = message.id.clone();
        let discard = message.id.clone();
        let attachments = attachments::Attachment::from_paths(&message.attachments);
        let attachment_body = (!attachments.is_empty())
            .then(|| self.attachment_row(&attachments, &format!("pending-{}", message.id), cx));
        div()
            .id(SharedString::from(format!("mono-pending-{}", message.id)))
            .debug_selector(|| "mono-pending-message".into())
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .opacity(opacity)
            .children(self.pending_separator(ix))
            .child(
                div()
                    .relative()
                    .top(px(rise))
                    .w_full()
                    .max_w(px(COLUMN))
                    .min_w_0()
                    .pl(px(56.))
                    .pr(px(16.))
                    .pt_1()
                    .pb(px(if stacked { 0. } else { 20. }))
                    .flex()
                    .flex_col()
                    .items_end()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .child(
                        div()
                            .relative()
                            .max_w(px(576.))
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .px_3()
                            .py_2()
                            .child(
                                canvas(
                                    |_, _, _| {},
                                    move |bounds, _, window, _| {
                                        let radius = if bounds.size.height <= px(37.) {
                                            f32::from(bounds.size.height) * 0.5
                                        } else {
                                            12.
                                        };
                                        window.paint_quad(
                                            gpui::fill(bounds, user_bubble_fill())
                                                .corner_radii(px(radius)),
                                        );
                                    },
                                )
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full(),
                            )
                            .children(attachment_body.map(|body| {
                                div()
                                    .when(!message.text.is_empty(), |d| d.mb_2())
                                    .child(body)
                            }))
                            .when(!message.text.is_empty(), |d| d.child(message.text.clone())),
                    )
                    .when_some(message.error.clone(), |d, error| {
                        d.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .px_3()
                                .pt_1()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(content(0.5))
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "mono-pending-error-{}",
                                            message.id
                                        )))
                                        .role(Role::Alert)
                                        .child(format!("Not sent · {error}")),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "mono-pending-retry-{retry}"
                                        )))
                                        .role(Role::Button)
                                        .rounded(px(4.))
                                        .px_1()
                                        .py(px(2.))
                                        .text_color(content(0.7))
                                        .hover(|s| s.bg(content(0.08)))
                                        .child("Retry")
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.emit(MonoResponseAction::Queue {
                                                id: retry.clone(),
                                                remove: false,
                                            })
                                        })),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "mono-pending-discard-{discard}"
                                        )))
                                        .role(Role::Button)
                                        .rounded(px(4.))
                                        .px_1()
                                        .py(px(2.))
                                        .text_color(content(0.7))
                                        .hover(|s| s.bg(content(0.08)))
                                        .child("Discard")
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.emit(MonoResponseAction::Queue {
                                                id: discard.clone(),
                                                remove: true,
                                            })
                                        })),
                                ),
                        )
                    }),
            )
            .when(thinking, |d| {
                d.child(
                    div()
                        .w_full()
                        .max_w(px(COLUMN))
                        .min_w_0()
                        .px_4()
                        .child(self.mono_header(self.source.len(), Some("Thinking…".into()), cx)),
                )
            })
            .into_any_element()
    }
    /// isCompactFollowUp: the next message is the user's too, directly or
    /// past a reply that was only a reaction, so it stacks onto this one.
    fn stacks_on_next(&self, start: usize) -> bool {
        let mut at = start + 1;
        while at < self.source.len() && !self.source[at].first {
            at += 1;
        }
        if let Some(next) =
            (at..self.source.len()).find(|&i| self.source[i].first && self.source[i].is_prompt())
            && self.source[next].role == "user"
            && self.mono_next_run_continues(at, next)
        {
            return !self.mono_opens_stretch(next);
        }
        if let Some(message) = self
            .pending
            .iter()
            .find(|message| Some(&message.id) != self.submitted_mono_message.as_ref())
            && self.mono_pending_joins_live_turn()
            && self
                .source
                .iter()
                .rposition(Row::is_prompt)
                .is_some_and(|prompt| prompt < at)
            && !dates::opens_stretch(message.sent_at, self.source[start].sent_at)
        {
            return true;
        }
        while let Some(row) = self.source.get(at) {
            if row.is_prompt() {
                return !self.mono_opens_stretch(at);
            }
            let end = (at + 1..self.source.len())
                .find(|&i| self.source[i].first)
                .unwrap_or(self.source.len());
            let text: String = self.source[at..end]
                .iter()
                .map(|r| r.text.as_str())
                .collect();
            if !(row.role == "assistant" && text.trim().starts_with("<reaction>")) {
                return false;
            }
            at = end;
        }
        false
    }
    /// Motions a person started: these run at the display's full rate.
    fn chat_animating(&self) -> bool {
        !self.entrances.is_empty()
            || !self.pending_entrances.is_empty()
            || self.reactions.values().any(|(_, born)| born.is_some())
            || self
                .hovered_turn
                .is_some_and(|(_, since)| since.elapsed().as_secs_f32() < HOVER_FADE)
    }
    /// Motion the stream drives: words revealing and fading in and the tail
    /// easing after growth. Only while one is under way, never for as long as
    /// the provider is busy: a stalled stream must not keep redrawing.
    fn stream_animating(&self, now: Instant) -> bool {
        self.reveals.values().any(|r| r.active(now)) || self.follow.offset.abs() >= 0.25
    }
    fn action_hover(
        &mut self,
        turn: usize,
        action: &'static str,
        hovered: bool,
        cx: &mut Context<Self>,
    ) {
        if hovered {
            self.hovered_action = Some((turn, action));
        } else if self.hovered_action == Some((turn, action)) {
            self.hovered_action = None;
        }
        cx.notify();
    }
    fn response_actions(
        &mut self,
        turn: usize,
        complete: bool,
        before_user: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if !complete {
            // Reserve the action gap while the ticker works in the header.
            return div()
                .h(px(if self.mono {
                    if before_user { 24. } else { 28. }
                } else {
                    38.
                }))
                .into_any_element();
        }
        let (rows, _) = self.response_rows(turn);
        let output = monocode_core::response::prose(&rows);
        if self.mono {
            return self.mono_actions(turn, output, before_user, cx);
        }
        let copied = self.copied_response.is_some_and(|(id, _)| id == turn);
        let saved = self.note_feedback.get(&turn).is_some_and(|r| r.is_ok());
        let pending = self.note_pending.contains(&turn);
        let info = self.turns.get(&turn).cloned().unwrap_or_default();
        let metrics = info.metrics.filter(|m| m.reported());
        // Reserve the full icon strip before the sibling clock is laid out.
        // Intrinsic flex measurement otherwise reports only its widest button.
        let strip_width = if output.is_empty() { 14. } else { 44. }
            + if self.side_question { 0. } else { 52. }
            + if metrics.is_some() { 29. } else { 0. };
        let mut actions = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .flex_shrink_0()
            .w(px(strip_width));
        if !output.is_empty() {
            let copy_label = if copied { "Copied" } else { "Copy response" };
            let anchor = self
                .action_anchors
                .entry((turn, "copy"))
                .or_default()
                .clone();
            let text = output.clone();
            actions = actions.child(
                response_actions::button(
                    turn,
                    "response-copy",
                    copy_label,
                    if copied { "check" } else { "response-copy" },
                    false,
                )
                .ml(px(-4.))
                .child(anchor.probe())
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    this.action_hover(turn, "copy", *hovered, cx)
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                    let born = Instant::now();
                    this.copied_response = Some((turn, born));
                    cx.notify();
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_secs(2))
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            if this.copied_response == Some((turn, born)) {
                                this.copied_response = None;
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                })),
            );
            let anchor = self
                .action_anchors
                .entry((turn, "note"))
                .or_default()
                .clone();
            actions = actions.child(
                response_actions::button(
                    turn,
                    "response-note",
                    if saved {
                        "Saved to Notes"
                    } else if pending {
                        "Saving note…"
                    } else {
                        "Save as note"
                    },
                    if saved { "check" } else { "response-note" },
                    false,
                )
                .when(pending, |d| d.opacity(0.4))
                .child(anchor.probe())
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    this.action_hover(turn, "note", *hovered, cx)
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.note_pending.insert(turn) {
                        this.note_feedback.remove(&turn);
                        cx.emit(ResponseRequest::SaveNote {
                            turn,
                            text: output.clone(),
                            selection: false,
                        });
                        cx.notify();
                    }
                })),
            );
        } else {
            actions = actions.child(icon("check", 14.).text_color(content(0.4)));
        }
        for (action, label, glyph, kind) in [
            (
                "handoff",
                "Handoff",
                "response-handoff",
                monocode_core::response::TransferKind::Handoff,
            ),
            (
                "opinion",
                "Second opinion",
                "response-opinion",
                monocode_core::response::TransferKind::SecondOpinion,
            ),
        ]
        .into_iter()
        .filter(|_| !self.side_question)
        {
            let anchor = self
                .action_anchors
                .entry((turn, action))
                .or_default()
                .clone();
            let click_anchor = anchor.clone();
            let active = self.target_action == Some((turn, kind));
            actions = actions.child(
                response_actions::button(turn, glyph, label, glyph, active)
                    .aria_expanded(active)
                    .child(anchor.probe())
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        this.action_hover(turn, action, *hovered, cx)
                    }))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(ResponseRequest::Target {
                            turn,
                            kind,
                            anchor: click_anchor.clone(),
                        })
                    })),
            );
        }
        if metrics.is_some() {
            let anchor = self
                .action_anchors
                .entry((turn, "metrics"))
                .or_default()
                .clone();
            actions = actions.child(
                response_actions::button(
                    turn,
                    "response-metrics",
                    "Turn metrics",
                    "response-metrics",
                    false,
                )
                .role(Role::Image)
                .ml(px(3.))
                .child(anchor.probe())
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    this.action_hover(turn, "metrics", *hovered, cx)
                })),
            );
        }
        let dot = || {
            div()
                .size(px(3.))
                .flex_shrink_0()
                .rounded_full()
                .bg(content(0.25))
        };
        let mut row = div()
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(10.))
            .pt(px(4.))
            .pb(px(12.))
            .text_size(px(12.))
            .line_height(px(16.))
            .text_color(content(0.4))
            .child(actions)
            .when_some(info.completed_at, |d, time| {
                d.child(dot()).child(
                    div()
                        .flex_shrink_0()
                        .text_color(content(0.35))
                        .child(monocode_core::response::local_clock(time)),
                )
            });
        if let Some(Err(error)) = self.note_feedback.get(&turn) {
            row = row.child(
                div()
                    .id(("note-save-error", turn))
                    .role(Role::Alert)
                    .text_size(px(11.))
                    .child(format!("Could not save note. {error}")),
            );
        }
        if let Some((hover_turn, action)) = self.hovered_action
            && hover_turn == turn
        {
            let (headline, detail) = match action {
                "copy" => (if copied { "Copied" } else { "Copy response" }.into(), None),
                "note" => (
                    if saved {
                        "Saved to Notes"
                    } else {
                        "Save as note"
                    }
                    .into(),
                    None,
                ),
                "handoff" => ("Handoff".into(), None),
                "opinion" => ("Second opinion".into(), None),
                _ => {
                    let metrics = metrics.unwrap_or_default();
                    let mut summary = Vec::new();
                    if let Some(hit) = metrics.cache_hit_percent {
                        summary.push(format!("Cache hit {hit:.0}%"));
                    }
                    if info.duration_ms > 0 && metrics.output_tokens > 0 {
                        summary.push(format!(
                            "Output {:.0} tok/s",
                            metrics.output_tokens as f64 / (info.duration_ms as f64 / 1000.)
                        ));
                    }
                    let headline = if summary.is_empty() {
                        "Turn tokens".into()
                    } else {
                        summary.join(" · ")
                    };
                    let detail = [
                        (metrics.input_tokens, "input"),
                        (metrics.output_tokens, "output"),
                        (metrics.cache_read_tokens, "cached"),
                    ]
                    .into_iter()
                    .filter(|(n, _)| *n > 0)
                    .map(|(n, label)| {
                        format!("{} {label}", monocode_core::context::format_tokens(n))
                    })
                    .collect::<Vec<_>>()
                    .join(" · ");
                    (headline, Some(detail))
                }
            };
            if let Some(anchor) = self.action_anchors.get(&(turn, action)) {
                row = row.child(
                    popover(("response-tooltip", turn), Anchor::Slot(anchor.clone()))
                        .side(Side::Top)
                        .max_width(if action == "metrics" { 300. } else { 240. })
                        .padding(8.)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(content(1.))
                                .child(headline)
                                .when_some(detail, |d, detail| {
                                    d.child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(content(0.5))
                                            .child(detail),
                                    )
                                }),
                        ),
                );
            }
        }
        row.into_any_element()
    }
    pub(crate) fn new() -> Self {
        Self {
            selection: Default::default(),
            selection_focus: None,
            links: Default::default(),
            work_item_cards: Default::default(),
            source: Vec::new(),
            agent_panel_rows: BTreeSet::new(),
            batching_updates: false,
            pending_rebuild: None,
            source_bytes: 0,
            attachment_images: HashMap::new(),
            visuals: HashMap::new(),
            placed: HashSet::new(),
            published: HashMap::new(),
            history_position: None,
            history_start: 0,
            history_loading: false,
            history_requested: false,
            base: 0,
            history_before: None,
            history_pager: None,
            history_failed: false,
            history_search: None,
            prepared_parsers: HashMap::new(),
            mono: false,
            mono_groups: Default::default(),
            mono_look: None,
            hovered_turn: None,
            hovered_prompt: None,
            hovered_prompt_actions: None,
            hover_rows: HashMap::new(),
            entrances: HashMap::new(),
            reactions: HashMap::new(),
            follow: Follow::default(),
            pending: Vec::new(),
            submitted_mono_message: None,
            saved_draft: None,
            taken: Vec::new(),
            pending_entrances: HashMap::new(),
            launches: HashMap::new(),
            review: None,
            review_shown: false,
            started_at: Instant::now(),
            frame_now: Instant::now(),
            measurements: Default::default(),
            markdown_cache: Default::default(),
            messages: Vec::new(),
            rows: Vec::new(),
            streaming: false,
            chat_layout: true,
            anchor_prompts: true,
            smooth_reveal: true,
            search_match: None,
            highlights: HashMap::new(),
            highlighting: HashSet::new(),
            highlight_queue: VecDeque::new(),
            highlight_active: false,
            highlight_epoch: 0,
            highlight_order: VecDeque::new(),
            highlight_bytes: 0,
            copied: None,
            copied_response: None,
            note_pending: HashSet::new(),
            note_feedback: HashMap::new(),
            action_anchors: HashMap::new(),
            hovered_action: None,
            target_action: None,
            document: false,
            side_question: false,
            file_link_base: None,
            expanded: HashSet::new(),
            agent_panels: Default::default(),
            expanded_prompts: HashSet::new(),
            clamped_prompts: HashSet::new(),
            phases: HashMap::new(),
            phase_toggle: None,
            phase_layouts: HashMap::new(),
            phase_scrolls: HashMap::new(),
            windowed_phases: HashSet::new(),
            phase_max_height: px(280.),
            work_folds: HashMap::new(),
            reveals: HashMap::new(),
            live_start: 0,
            motions: HashMap::new(),
            reflow: Default::default(),
            frame_scheduled: false,
            last_motion_frame: None,
            visible: true,
            viewport: Bounds::default(),
            anchor_turn: None,
            prompt_rise: None,
            prompt_rise_pending: false,
            burst: None,
            introduced_prompt: None,
            show_jump: false,
            commit_files: HashMap::new(),
            copied_commit: None,
            jump_beside_pill: false,
            jump_anchor: Default::default(),
            scroll_bound: false,
            fold_targets: HashMap::new(),
            step_queue: HashMap::new(),
            seen_steps: HashSet::new(),
            turns: HashMap::new(),
            provider: "codex".into(),
            model: "Codex".into(),
            active_provider_turn_id: None,
            started: None,
            elapsed: HashMap::new(),
            outline: outline::Outline::default(),
            list: {
                // The trailing item holds the viewport's bottom padding and,
                // after a send, the unused part of the current turn's min-height.
                let list = ListState::new(1, ListAlignment::Top, px(320.));
                list.set_follow_mode(FollowMode::Tail);
                list
            },
            rendered: 0,
            render_passes: 0,
            #[cfg(test)]
            rebuild_passes: 0,
        }
    }
    pub(crate) fn apply_preferences(
        &mut self,
        p: &crate::settings::Preferences,
        cx: &mut Context<Self>,
    ) {
        let chat = p.transcript_layout == "chat" || self.mono;
        let anchor_prompts = p.anchor_prompts && !self.mono;
        if self.chat_layout == chat
            && self.anchor_prompts == anchor_prompts
            && self.smooth_reveal == p.paced_reveal
        {
            return;
        }
        self.chat_layout = chat;
        self.anchor_prompts = anchor_prompts;
        self.smooth_reveal = p.paced_reveal;
        self.measurements.clear();
        if !self.anchor_prompts {
            self.anchor_turn = None;
        }
        if !self.smooth_reveal {
            for reveal in self.reveals.values_mut() {
                reveal.settle(Instant::now());
            }
        }
        self.rebuild(0, cx);
        cx.notify();
    }
    /// Stop visible work immediately, while the provider drains and saves.
    pub(crate) fn stop_streaming(&mut self, cx: &mut Context<Self>) {
        let start = self
            .source
            .iter()
            .rposition(Row::is_prompt)
            .map_or(0, |at| at + 1);
        for row in self
            .source
            .iter_mut()
            .skip(start)
            .filter(|row| row.role == "tool")
        {
            if let Ok(mut tool) = serde_json::from_str::<ToolCall>(&row.text)
                && tool.state == ToolState::Running
            {
                tool.state = ToolState::Cancelled;
                if tool.output.is_empty() {
                    tool.output = "Cancelled".into();
                }
                let previous_bytes = row.text.len();
                row.text = serde_json::to_string(&tool).unwrap_or_else(|_| row.text.clone());
                self.source_bytes = self
                    .source_bytes
                    .saturating_sub(previous_bytes)
                    .saturating_add(row.text.len());
            }
        }
        let now = Instant::now();
        for reveal in self.reveals.values_mut() {
            reveal.settle(now);
        }
        self.streaming(false, cx);
        self.rebuild(start, cx);
        cx.notify();
    }

    pub(crate) fn streaming(&mut self, streaming: bool, cx: &mut Context<Self>) {
        if self.streaming == streaming {
            return;
        }
        // What the user already reads while the Mono works, such as its
        // opening sentence, must not enter again when the run settles.
        let shown: HashSet<usize> = if self.mono && !streaming {
            let run = self
                .source
                .iter()
                .rposition(|r| r.is_prompt())
                .map_or(0, |i| i + 1);
            (run..self.source.len())
                .filter(|&at| self.source[at].first && !self.mono_prose_hidden(at))
                .collect()
        } else {
            HashSet::new()
        };
        self.streaming = streaming;
        if streaming {
            self.active_provider_turn_id = None;
            self.live_start = self.source.len();
            // The provider starts before its prompt arrives. Keep an ordinary
            // chat's existing spacer and scroll position until patch accepts
            // that prompt, just as React pins on lastUserId rather than busy.
            // Otherwise the old spacer collapses for a frame before the new
            // prompt gets its own anchor and entrance.
            if self.mono {
                self.show_jump = false;
                self.list.set_follow_mode(FollowMode::Tail);
            }
            self.started = Some(std::time::Instant::now());
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(1))
                        .await;
                    let keep = this
                        .update(cx, |this, cx| {
                            // The working time redraws the whole transcript;
                            // skip it while the tab or the app is out of view.
                            if this.streaming && this.visible && crate::pulse::app_active(cx) {
                                cx.notify_paint();
                            }
                            this.streaming
                        })
                        .unwrap_or(false);
                    if !keep {
                        break;
                    }
                }
            })
            .detach();
        } else {
            self.follow.until = Some(Instant::now() + std::time::Duration::from_millis(1200));
            if let Some(started) = self.started.take()
                && let Some(turn) = self.active_turn()
            {
                self.elapsed.insert(turn, started.elapsed());
                let completed_at = monocode_core::time::now_ms();
                let started_at =
                    Some(completed_at.saturating_sub(started.elapsed().as_millis() as u64));
                let info = self.turns.entry(turn).or_default();
                info.provider = self.provider.clone();
                info.model = self.model.clone();
                info.started_at = started_at;
                info.duration_ms = started.elapsed().as_millis() as u64;
                info.duration_known = true;
                info.completed_at = started_at.and_then(|at| at.checked_add(info.duration_ms));
            }
        }
        if !streaming && self.mono && self.visible && !cx.reduce_motion() {
            // The reply a Mono kept in its trail while it worked appears now.
            let now = Instant::now();
            let run = self
                .source
                .iter()
                .rposition(|r| r.is_prompt())
                .map_or(0, |i| i + 1);
            for at in run..self.source.len() {
                let row = &self.source[at];
                if row.first
                    && row.role == "assistant"
                    && !shown.contains(&at)
                    && !row.text.trim_start().starts_with("<reaction>")
                {
                    self.entrances.insert(at, now);
                }
            }
        }
        // Starting a normal turn must not re-open the previous turn's work or
        // rebuild it as live while the new prompt is still in transit.
        if (!streaming || self.mono) && !self.messages.is_empty() {
            let start = self
                .source
                .iter()
                .rposition(|r| r.is_prompt())
                .map_or(0, |i| i + 1);
            self.rebuild(start, cx);
        } else {
            cx.notify();
        }
    }
    pub(crate) fn visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        self.selection.clear();
        let now = Instant::now();
        // Settled tabs retain their parsed rows, measured heights and scroll
        // anchor. Only undisplayed text or outgoing animation rows need a
        // source rebuild when visibility changes.
        let rebuild_from = self
            .reveals
            .iter()
            .filter(|(_, reveal)| reveal.behind())
            .map(|(&at, _)| at)
            .chain(
                self.motions
                    .iter()
                    .filter(|(_, motion)| !motion.opening)
                    .map(|(key, _)| key.0),
            )
            .min();
        let panels: HashSet<_> = self.agent_panels.animate().into_iter().collect();
        if !self.motions.is_empty() || !panels.is_empty() {
            self.phase_layouts.clear();
            for (ix, row) in self.rows.iter().enumerate() {
                if self.motions.contains_key(&row.key) || panels.contains(&row.key.0) {
                    self.list.remeasure_items(ix..ix + 1);
                }
            }
        }
        self.prompt_rise = None;
        self.prompt_rise_pending = false;
        self.burst = None;
        self.entrances.clear();
        self.follow = Follow::default();
        for reveal in self.reveals.values_mut() {
            reveal.settle(now);
        }
        self.visible = false;
        for scroll in self.phase_scrolls.values_mut() {
            scroll.enable(false);
        }
        self.motions.clear();
        self.agent_panels.settle();
        if let Some(start) = rebuild_from.filter(|_| !self.source.is_empty()) {
            self.rebuild(start.min(self.source.len() - 1), cx);
        }
        self.visible = visible;
        if visible {
            cx.notify();
        }
    }
    pub(crate) fn identity(&mut self, provider: &str, model: &str) {
        self.provider = provider.into();
        self.model = model.into();
        if self.streaming
            && let Some(turn) = self.active_turn()
        {
            let info = self.turns.entry(turn).or_default();
            info.provider = provider.into();
            info.model = model.into();
        }
    }
    /// Latest assistant message, including its virtualized continuation rows.
    pub(crate) fn notification_reply_body(&self) -> String {
        let Some(end) = self
            .source
            .iter()
            .rposition(|row| row.role == "assistant" && !row.text.trim().is_empty())
        else {
            return String::new();
        };
        let mut start = end;
        while start > 0 && !self.source[start].first && self.source[start - 1].role == "assistant" {
            start -= 1;
        }
        let mut text = String::new();
        for row in &self.source[start..=end] {
            text.push_str(&row.text);
            // A notification needs only the first paragraph and 240 code units.
            // Retain enough extra text to distinguish a 240-unit body from one
            // requiring the ellipsis, without cloning a giant reply.
            if text.encode_utf16().count() > 480 {
                break;
            }
        }
        crate::notifications::delivery::clip_body(&text)
    }

    /// Tauri's appendUser: paint the prompt and working receipt in the send
    /// handler. The worker replaces these same rows after saving them. Build
    /// the attachment manifest without touching the filesystem on the UI thread.
    pub(crate) fn submit_prompt(
        &mut self,
        prompt: &str,
        settings: &monocode_core::agent::Settings,
        cx: &mut Context<Self>,
    ) {
        let mut submitted = monocode_core::transcript::Transcript::default();
        submitted.begin(if settings.internal_message {
            "mono-system"
        } else {
            "user"
        });
        submitted.rows[0].sent_at = settings
            .mono_sent_at
            .or_else(|| {
                settings
                    .turn_model
                    .as_ref()
                    .and_then(|turn| turn.started_at)
            })
            .or(submitted.rows[0].sent_at);
        submitted.append(prompt);
        if !settings.attachments.is_empty() {
            submitted.rows.push(Row {
                role: monocode_core::transcript::attachments::ROLE.into(),
                text: monocode_core::transcript::attachments::from_paths(&settings.attachments)
                    .to_string(),
                first: true,
                sent_at: None,
            });
        }
        submitted.begin("assistant");
        if !settings.internal_message && !self.mono && self.visible && !cx.reduce_motion() {
            self.burst = burst::Kind::from_prompt(prompt, settings)
                .map(|kind| burst::Burst::new(self.source.len(), kind));
        }
        if self.mono {
            self.submitted_mono_message = settings.mono_message_id.clone();
        }
        self.patch_view(self.source.len(), submitted.rows, cx);
    }

    /// Patch the store's window from `start`, a row of `source_rows`.
    pub(crate) fn patch(&mut self, start: usize, rows: Vec<Row>, cx: &mut Context<Self>) {
        self.patch_view(self.base + start, rows, cx);
    }
    fn patch_view(&mut self, start: usize, rows: Vec<Row>, cx: &mut Context<Self>) {
        if rows.is_empty() {
            return;
        }
        assert!(
            start <= self.source.len(),
            "transcript patches must be contiguous"
        );
        let known = self.source.len();
        for (ix, row) in rows.into_iter().enumerate() {
            let ix = start + ix;
            self.agent_panel_rows.remove(&ix);
            if row.role == "tool"
                && row.first
                && serde_json::from_str::<ToolCall>(&row.text)
                    .is_ok_and(|call| matches!(call.panel(), Some(Panel::Agent(_))))
            {
                self.agent_panel_rows.insert(ix);
            }
            let bytes = row.text.capacity().saturating_add(row.role.capacity());
            if ix < self.source.len() {
                let old = &self.source[ix];
                self.source_bytes = self
                    .source_bytes
                    .saturating_sub(old.text.capacity().saturating_add(old.role.capacity()));
                self.source[ix] = row;
            } else {
                self.source.push(row);
            }
            self.source_bytes = self.source_bytes.saturating_add(bytes);
        }
        self.visuals.retain(|&start, _| {
            self.source
                .get(start)
                .is_some_and(|row| row.role == "visual")
        });
        if start == 0 {
            self.placed.clear();
        }
        self.published.retain(|_, (at, _)| *at < start);
        let placed = self.placed.len();
        for row in &self.source[start..] {
            if row.role == "assistant" {
                visuals::placements(&row.text, &mut self.placed);
            }
        }
        let placed = self.placed.len() != placed;
        if known == 0 && self.mono && !self.streaming {
            self.history_start = history::page_start(&self.source, self.source.len());
        }
        if self.mono {
            let affected = self.source[..start.min(self.source.len())]
                .iter()
                .rposition(|row| row.first && row.is_prompt())
                .unwrap_or(0);
            self.mono_groups
                .get_mut()
                .retain(|&turn, _| turn < affected);
        }
        if self.streaming
            && let Some(id) = self.active_provider_turn_id.clone()
            && let Some(turn) = self.active_turn()
        {
            self.turns.entry(turn).or_default().provider_turn_id = Some(id);
        }
        if self.streaming
            && let Some(turn) = self.active_turn()
        {
            let provider = self.provider.clone();
            let model = self.model.clone();
            self.turns
                .entry(turn)
                .or_insert_with(|| monocode_core::agent::TurnInfo {
                    provider,
                    model,
                    ..Default::default()
                });
        }
        if self.streaming {
            for prompt in known..self.source.len() {
                if self.source[prompt].first && self.source[prompt].is_prompt() {
                    let turn = (prompt + 1..self.source.len())
                        .find(|&at| self.source[at].first)
                        .unwrap_or(self.source.len());
                    let provider = self.provider.clone();
                    let model = self.model.clone();
                    let provider_turn_id = self.active_provider_turn_id.clone();
                    let joined_turn = self
                        .active_turn()
                        .filter(|&root| root < turn && self.source[prompt].role == "user");
                    self.turns
                        .entry(turn)
                        .or_insert_with(|| monocode_core::agent::TurnInfo {
                            provider,
                            model,
                            provider_turn_id,
                            joined_turn,
                            ..Default::default()
                        });
                }
            }
        }
        // React pins on a change to lastUserId, including an accepted steer
        // inside the current provider turn. Continuation rows and replayed
        // snapshots must not move the anchor or resume following the tail.
        if self.streaming
            && !self.mono
            && let Some(prompt) = (known..self.source.len())
                .rfind(|&ix| self.source[ix].first && self.source[ix].is_prompt())
        {
            self.anchor_turn = self.anchor_prompts.then_some(prompt);
            // React's ordinary transcript introduces a sent prompt itself.
            // Start a fresh follow baseline instead of adding a second glide.
            self.follow = Follow::default();
            self.show_jump = false;
            self.list.set_follow_mode(FollowMode::Tail);
        }
        if let Some(prompt) = self.anchor_turn
            && self.source.get(prompt).is_some_and(|r| r.is_prompt())
            && self.introduced_prompt != Some(prompt)
            && self.streaming
            && start <= prompt
        {
            self.introduced_prompt = Some(prompt);
            if self.visible && !cx.reduce_motion() {
                self.prompt_rise = Some((prompt, Instant::now()));
                self.prompt_rise_pending = true;
            }
        }
        // Messages that arrive while the chat is open slide and fade in;
        // history that loads or replays does not.
        if self.mono && self.streaming && self.visible && !cx.reduce_motion() {
            let now = Instant::now();
            for ix in start.max(known)..self.source.len() {
                let row = &self.source[ix];
                if !row.first || !matches!(row.role.as_str(), "user" | "assistant") {
                    continue;
                }
                let queued = if row.role == "user" {
                    // A long prompt spans storage rows. Match its full text so
                    // wrapping and newline chunking don't create another entrance.
                    let end = (ix + 1..self.source.len())
                        .find(|&at| self.source[at].first)
                        .unwrap_or(self.source.len());
                    let text: String = self.source[ix..end]
                        .iter()
                        .map(|r| r.text.as_str())
                        .collect();
                    self.pending
                        .iter()
                        .find(|message| text.starts_with(message.text.as_ref()))
                        .map(|message| self.pending_entrances.get(&message.id).copied())
                        .or_else(|| {
                            self.taken
                                .iter()
                                .rev()
                                .find(|message| {
                                    now.duration_since(message.removed_at).as_secs_f32() < 2.
                                        && text.starts_with(message.text.as_ref())
                                })
                                .map(|message| message.entrance)
                        })
                } else {
                    None
                };
                // Delivery keeps the same animation clock as the optimistic
                // bubble. A settled bubble stays settled; an in-flight slide
                // continues even when clearing a multiline composer resizes us.
                let entrance = queued.map_or(Some(now), |born| {
                    born.filter(|born| now.duration_since(*born).as_secs_f32() < 0.36)
                });
                if let Some(born) = entrance {
                    self.entrances.insert(ix, born);
                }
            }
        }
        self.outline.patched(&self.source, start);
        // A newly placed visual leaves its own row earlier in the turn, so the
        // whole turn rebuilds rather than just the changed suffix.
        let start = if placed {
            self.source[..start]
                .iter()
                .rposition(|r| r.is_prompt())
                .map_or(0, |i| i + 1)
        } else {
            start
        };
        self.rebuild(start, cx);
    }
    pub(crate) fn begin_update_batch(&mut self) {
        self.batching_updates = true;
    }
    pub(crate) fn end_update_batch(&mut self, cx: &mut Context<Self>) {
        self.batching_updates = false;
        if let Some(start) = self.pending_rebuild.take() {
            self.rebuild(start, cx);
        }
    }
    fn rebuild(&mut self, start: usize, cx: &mut Context<Self>) {
        self.rebuild_rows(start, cx);
        cx.notify();
    }
    /// `rebuild` for a caller that notifies itself.
    fn rebuild_rows(&mut self, start: usize, cx: &mut Context<Self>) {
        if self.batching_updates {
            self.pending_rebuild = Some(self.pending_rebuild.map_or(start, |old| old.min(start)));
            return;
        }

        #[cfg(test)]
        {
            self.rebuild_passes += 1;
        }
        self.phase_layouts.clear();
        let now = Instant::now();
        let following = self.list.is_following_tail();
        let offset = self.list.logical_scroll_top();
        let anchor = self.rows.get(offset.item_ix).map(|r| r.key);
        let current_turn = self.source[..(start + 1).min(self.source.len())]
            .iter()
            .rposition(|r| r.is_prompt())
            .map_or(0, |i| i + 1);
        let current_end = (current_turn..self.source.len())
            .find(|&i| self.source[i].is_prompt())
            .unwrap_or(self.source.len());
        // Advance incoming prose before choosing the displayed block. Its
        // first visible words and the previous block's replacement must be
        // committed together, including after an empty provider placeholder.
        if !self.mono
            && let Some(at) = (current_turn..current_end)
                .rev()
                .find(|&i| self.source[i].role == "assistant" && self.source[i].first)
        {
            let end = (at + 1..current_end)
                .find(|&i| self.source[i].first)
                .unwrap_or(current_end);
            let text: String = self.source[at..end]
                .iter()
                .map(|r| r.text.as_str())
                .collect();
            self.prepare_reveal(at, end, &text, true, now, cx.reduce_motion());
        }
        let candidate = (current_turn..current_end).rev().find(|&i| {
            self.source[i].role == "assistant" && self.source[i].first && self.answer_visible(i)
        });
        let target = candidate.filter(|&answer| {
            self.source[current_turn..answer]
                .iter()
                .any(|r| r.role == "tool")
        });
        let layout_changed = self.fold_targets.get(&current_turn).copied().flatten() != target;
        let known_message = self.messages.iter().any(|m| {
            m.start <= start
                && m.role == "assistant"
                && self
                    .source
                    .get(start)
                    .is_some_and(|r| r.role == "assistant")
        });
        let mut message_ix = self
            .messages
            .partition_point(|m| m.start <= start)
            .saturating_sub(1);
        while message_ix > 0
            && matches!(
                self.messages[message_ix].role.as_str(),
                "tool" | "reasoning"
            )
            && matches!(
                self.messages[message_ix - 1].role.as_str(),
                "tool" | "reasoning"
            )
        {
            message_ix -= 1;
        }
        if (layout_changed
            || !known_message
            || self.source.get(start).is_none_or(|r| r.role != "assistant"))
            && self.source[..start.min(self.source.len())]
                .iter()
                .rev()
                .take_while(|r| !r.is_prompt())
                .any(|r| r.role == "tool")
        {
            while message_ix > 0
                && !matches!(
                    self.messages[message_ix - 1].role.as_str(),
                    "user" | "mono-system"
                )
            {
                message_ix -= 1;
            }
        }
        if !self.mono
            && (self
                .agent_panel_rows
                .range(current_turn..current_end)
                .next()
                .is_some()
                || self.rows.iter().any(|row| {
                    self.messages
                        .get(row.message)
                        .is_some_and(|message| message.turn == current_turn)
                        && matches!(
                            &row.body,
                            Body::Tool { call, .. } if matches!(call.panel(), Some(Panel::Agent(_)))
                        )
                }))
        {
            // Pinned runs sit below the fold's work rather than at their
            // original source offset. Rebuild the whole turn so a suffix
            // update cannot cut across that reordered stack. Include the old
            // rows: a patch can remove the run that caused the reordering.
            while message_ix > 0 && self.messages[message_ix - 1].start >= current_turn {
                message_ix -= 1;
            }
        }
        if self.mono {
            // A run's prose hides behind later work, and a completion run
            // takes over the actions of the turn it continues, so rebuild from
            // the prompt that opens the whole group.
            let group = self.mono_group_start(current_turn);
            let prompt = self.prompt_start(group);
            while message_ix > 0
                && self
                    .messages
                    .get(message_ix)
                    .is_some_and(|m| m.start > prompt)
            {
                message_ix -= 1;
            }
        }
        let source_start = if message_ix == 0 {
            self.history_start
        } else {
            self.messages[message_ix].start
        };
        let display_start = if message_ix == 0 {
            0
        } else {
            // A hidden opening message can inherit a pinned run's later
            // position. Cut before every displayed row in the source suffix,
            // including work that now appears above that run.
            self.messages[message_ix..]
                .iter()
                .map(|message| message.display_start)
                .min()
                .unwrap_or(self.rows.len())
        };
        let old_count = self.rows.len();
        let old_layouts: Vec<_> = self.rows[display_start..]
            .iter()
            .enumerate()
            .map(|(ix, row)| {
                (
                    row.clone(),
                    measurements::after_receipt(&self.rows, &self.messages, display_start + ix),
                )
            })
            .collect();
        let mut previous: HashMap<_, _> = self
            .messages
            .split_off(message_ix)
            .into_iter()
            .map(|m| (m.start, m))
            .collect();
        let old_rows = self.rows.split_off(display_start);
        let activity = if self.mono {
            phases::Phases::default()
        } else {
            phases::collect(&self.source, source_start, self.streaming)
        };
        let mut at = source_start;
        let mut turn = 0;
        let mut turn_end = 0;
        let mut final_answer = None;
        let mut awaiting_answer = None;
        let mut has_work = false;
        let mut ready = false;
        let mut git_card: Option<Arc<git_turns::GitCard>> = None;
        let mut git_live_shown = false;
        let last_prompt = self
            .mono
            .then(|| self.source.iter().rposition(Row::is_prompt))
            .flatten();
        while at < self.source.len() {
            let mut end = at + 1;
            while end < self.source.len() && !self.source[end].first {
                end += 1;
            }
            let role = self.source[at].role.clone();
            let is_prompt = self.source[at].is_prompt();
            let text: String = self.source[at..end]
                .iter()
                .map(|r| r.text.as_str())
                .collect();
            // Older native versions saved user cancellation as an error row.
            let stopped_notice = role == "error" && text.trim() == "Stopped";
            let (display_text, attachments) = if role == "user" {
                attachments::message(&self.source, end, &text)
            } else {
                (None, Arc::default())
            };
            let old = previous.remove(&at).filter(|p| p.role == role);
            let mut user_link = None;
            let (mut parser, mut markdown_blocks) = old
                .map(|p| (p.parser, p.markdown_blocks))
                .unwrap_or_else(|| {
                    (
                        self.prepared_parsers.remove(&at).unwrap_or_default(),
                        Vec::new(),
                    )
                });
            let message = self.messages.len();
            let first = self.rows.len();
            if at >= turn_end {
                turn = if is_prompt {
                    (at..self.source.len())
                        .find(|&i| !self.source[i].is_prompt())
                        .unwrap_or(self.source.len())
                } else {
                    self.source[..at]
                        .iter()
                        .rposition(|r| r.is_prompt())
                        .map_or(0, |i| i + 1)
                };
                turn_end = (turn..self.source.len())
                    .find(|&i| self.source[i].is_prompt())
                    .unwrap_or(self.source.len());
                awaiting_answer = (turn..turn_end)
                    .rev()
                    .find(|&i| self.source[i].role == "assistant" && self.source[i].first)
                    .filter(|&i| !self.answer_visible(i));
                final_answer = (turn..turn_end).rev().find(|&i| {
                    self.source[i].role == "assistant"
                        && self.source[i].first
                        && self.answer_visible(i)
                });
                // A Mono's work stays in its activity panel, never folded inline.
                has_work = !self.mono
                    && final_answer.is_some_and(|answer| {
                        self.source[turn..answer].iter().any(|r| r.role == "tool")
                    });
                ready = final_answer.is_some_and(|answer| self.answer_visible(answer));
                self.fold_targets
                    .insert(turn, final_answer.filter(|_| has_work && ready));
                git_card = self.git_card(turn, turn_end, final_answer);
                git_live_shown = false;
            }
            let folded = !is_prompt
                && !activity.pinned.contains(&at)
                && has_work
                && ready
                && final_answer.is_some_and(|answer| at < answer)
                && !self.work_folds.get(&turn).copied().unwrap_or(false);
            if !self.document
                && !is_prompt
                && !stopped_notice
                && !matches!(
                    role.as_str(),
                    "mono-card"
                        | "remote-plan"
                        | "visual"
                        | "mono-document"
                        | "mono-note"
                        | "mono-launch"
                        | monocode_core::transcript::attachments::ROLE
                )
                && !(self.mono && text.trim().starts_with("<reaction>"))
                && (at == 0
                    || self.source[..at]
                        .iter()
                        .rev()
                        .find(|row| row.role != monocode_core::transcript::attachments::ROLE)
                        .is_some_and(Row::is_prompt)
                    // A run on a later day, such as a habit report, signs
                    // again under its own day.
                    || (self.mono && dates::opens_later_day(&self.source, at)))
                && !(self.mono
                    && self.mono_group_start(turn) != turn
                    && !(self.history_start > 0 && self.prompt_start(turn) == self.history_start))
            {
                self.rows.push(UiRow {
                    key: (0, 0),
                    body: Body::WorkLine {
                        turn,
                        expandable: has_work && ready,
                    },
                    message,
                    first: false,
                    last: false,
                    work: false,
                    work_tail: false,
                    thought_phase: None,
                });
            }
            let phase = activity.at(at);
            let grouped = phase.is_some_and(|group| group.steps > 1);
            let active = phase.is_some_and(|group| {
                self.streaming
                    && (group.end == self.source.len() || awaiting_answer == Some(group.end))
            });
            let phase_open = phase.is_none_or(|group| {
                !grouped || self.phases.get(&group.start).copied().unwrap_or(active)
            });
            // A running pill turn shows one status line in place of its work
            // and prose; a settled one's card leads its reply.
            let git_stand_in = git_card.clone().filter(|card| {
                !is_prompt
                    && if card.live() {
                        matches!(role.as_str(), "tool" | "reasoning" | "assistant")
                    } else {
                        card.answer == Some(at)
                    }
            });
            if let Some(card) = &git_stand_in
                && !(card.live() && git_live_shown)
            {
                git_live_shown = true;
                self.rows.push(UiRow {
                    key: (0, 0),
                    body: Body::GitAction(card.clone()),
                    message,
                    first: false,
                    last: false,
                    work: false,
                    work_tail: false,
                    thought_phase: None,
                });
            }
            let git_hidden = git_stand_in.as_ref().is_some_and(|card| card.live());
            if !self.mono
                && !folded
                && !git_hidden
                && grouped
                && phase.is_some_and(|group| group.start == at)
            {
                let group = phase.unwrap();
                let (title, icon) = if group.calls.is_empty() {
                    (if active { "Thinking…" } else { "Thought" }.into(), "minus")
                } else {
                    crate::activity::summary(&group.calls, active)
                };
                self.rows.push(UiRow {
                    key: (0, 0),
                    body: Body::Phase {
                        title,
                        group: group.start,
                        icon,
                        active,
                    },
                    message,
                    first: false,
                    last: false,
                    work: false,
                    work_tail: false,
                    thought_phase: None,
                });
            }
            let reaction =
                self.mono && role == "assistant" && text.trim().starts_with("<reaction>");
            if git_hidden {
                if role == "tool" {
                    self.seen_steps.insert(at);
                }
            } else if role == "interjection" {
                if let Ok(note) = serde_json::from_str(&text) {
                    self.rows.push(UiRow {
                        key: (0, 0),
                        body: Body::Interjection(note),
                        message,
                        first: false,
                        last: false,
                        work: false,
                        work_tail: false,
                        thought_phase: None,
                    });
                }
            } else if role == "remote-plan" {
                if let Ok(plan) = serde_json::from_str(&text) {
                    self.rows.push(UiRow {
                        key: (0, 0),
                        body: Body::RemotePlan(plan),
                        message,
                        first: false,
                        last: false,
                        work: false,
                        work_tail: false,
                        thought_phase: None,
                    });
                }
            } else if role == "visual" {
                let document = serde_json::from_str::<monocode_core::visual::VisualDocument>(&text)
                    .ok()
                    .filter(|document| document.validate().is_ok());
                // An answer that places the visual draws it in its prose. While
                // the turn runs, the call that published it stands for it, so
                // it never appears above the answer and then jumps into it.
                let elsewhere = document.as_ref().is_some_and(|document| {
                    self.placed.contains(&document.id) || (self.streaming && at >= self.live_start)
                });
                if !elsewhere {
                    let body = match document {
                        Some(document) => Body::Visual(Arc::new(document)),
                        None => Body::Plain("This visual could not be loaded.".into()),
                    };
                    self.rows.push(UiRow {
                        key: (0, 0),
                        body,
                        message,
                        first: false,
                        last: false,
                        work: false,
                        work_tail: false,
                        thought_phase: None,
                    });
                }
            } else if matches!(role.as_str(), "mono-card" | "mono-note") {
                if let Ok(value) = serde_json::from_str(&text) {
                    self.rows.push(UiRow {
                        key: (0, 0),
                        body: Body::MonoCard(value),
                        message,
                        first: false,
                        last: false,
                        work: false,
                        work_tail: false,
                        thought_phase: None,
                    });
                }
            } else if stopped_notice
                || matches!(
                    role.as_str(),
                    "mono-document" | "mono-launch" | monocode_core::transcript::attachments::ROLE
                )
            {
                // Saved work references are rendered once under the settled reply.
            } else if reaction {
                // MonoReactionBadge: the emoji sits on the message it answers.
                if let Some(emoji) = text
                    .trim()
                    .strip_prefix("<reaction>")
                    .and_then(|s| s.strip_suffix("</reaction>"))
                    .filter(|e| e.chars().count() <= 16)
                    && let Some(prompt) = self.source[..at].iter().rposition(|r| r.is_prompt())
                {
                    let prompt = self.source[..=prompt]
                        .iter()
                        .rposition(|r| r.first)
                        .unwrap_or(prompt);
                    let live = self.streaming && at >= self.live_start && self.visible;
                    match self.reactions.get(&prompt) {
                        Some((current, _)) if current == emoji => {}
                        _ => {
                            self.reactions
                                .insert(prompt, (emoji.into(), live.then_some(now)));
                        }
                    }
                }
            } else if self.mono
                && (matches!(role.as_str(), "tool" | "reasoning")
                    || (role == "assistant" && self.mono_prose_hidden(at)))
            {
                // groupMonoTurnItems: the header carries the process, and the
                // prose said while working stays in the activity trail.
                if role == "tool" {
                    self.seen_steps.insert(at);
                }
            } else if folded || activity.hidden.contains(&at) || !phase_open {
                if role == "tool" {
                    self.seen_steps.insert(at);
                }
                // WorkFoldLine puts earlier commentary and work behind its receipt.
            } else if role == "tool"
                && let Ok(tool) = serde_json::from_str::<ToolCall>(&text)
            {
                self.rows.push(UiRow {
                    key: (0, 0),
                    body: Body::Tool {
                        call: Box::new(tool),
                        group: phase.filter(|_| grouped).map(|group| group.start),
                        tail: phase.is_none_or(|group| group.last == at),
                        live: active,
                    },
                    message,
                    first: false,
                    last: false,
                    work: false,
                    work_tail: false,
                    thought_phase: None,
                });
            } else if matches!(role.as_str(), "assistant" | "reasoning") {
                if role == "reasoning" {
                    self.rows.push(UiRow {
                        key: (0, 0),
                        body: Body::Reasoning,
                        message,
                        first: false,
                        last: false,
                        work: false,
                        work_tail: false,
                        thought_phase: None,
                    });
                }
                self.prepare_reveal(at, end, &text, role == "assistant", now, cx.reduce_motion());
                parser.set_text(self.reveals.get(&at).map_or(text.as_str(), PacedText::text));
                let tree = if self.reveals.get(&at).is_some_and(PacedText::behind)
                    || (self.streaming && end == self.source.len())
                {
                    parser.display_tree()
                } else {
                    parser.finish_display();
                    parser.tree().clone()
                };
                let mut word_cursor = 0;
                let mut cached = Vec::new();
                for (block_ix, block) in tree
                    .blocks
                    .iter()
                    .filter(|_| role != "reasoning" || self.expanded.contains(&at))
                    .enumerate()
                {
                    let (rows, words) = if let Some(old) = markdown_blocks
                        .get(block_ix)
                        .filter(|old| Arc::ptr_eq(&old.source, block))
                        .filter(|old| {
                            old.rows
                                .first()
                                .is_none_or(|row| row.word_start == word_cursor)
                        }) {
                        (old.rows.clone(), old.words)
                    } else {
                        let mut base = word_cursor;
                        let rows = markdown::rows(&block.block)
                            .into_iter()
                            .map(|mut row| {
                                row.word_start = base;
                                base += markdown::word_count(&row);
                                Arc::new(row)
                            })
                            .collect();
                        (rows, base - word_cursor)
                    };
                    for row in &rows {
                        self.rows.push(UiRow {
                            key: (0, 0),
                            body: Body::Markdown(row.clone()),
                            message,
                            first: false,
                            last: false,
                            work: false,
                            work_tail: false,
                            thought_phase: None,
                        });
                    }
                    word_cursor += words;
                    cached.push(RenderedBlock {
                        source: block.clone(),
                        rows,
                        words,
                    });
                }
                markdown_blocks = cached;
                if let Some(paced) = self.reveals.get_mut(&at) {
                    paced.words(word_cursor, now);
                }
            } else {
                let text_rows = if let Some(text) = display_text {
                    let mut transcript = monocode_core::transcript::Transcript::default();
                    transcript.begin("user");
                    transcript.append(&text);
                    transcript
                        .rows
                        .into_iter()
                        .map(|row| row.text)
                        .collect::<Vec<_>>()
                } else {
                    self.source[at..end]
                        .iter()
                        .filter(|r| r.role != "mono-system")
                        .map(|row| row.text.clone())
                        .collect()
                };
                if role == "user" {
                    user_link = text_rows.iter().enumerate().find_map(|(chunk, text)| {
                        parse_user_message_link(text).map(|link| (chunk, Arc::new(link)))
                    });
                }
                // `line-clamp-4`: a collapsed message shows its first lines only.
                // A message with a link preview is never clamped, as in React.
                let clamped = role == "user"
                    && user_link.is_none()
                    && !self.expanded_prompts.contains(&at)
                    && text_rows.len() > 1;
                if clamped {
                    self.clamped_prompts.insert(at);
                } else {
                    self.clamped_prompts.remove(&at);
                }
                let shown = if clamped { 1 } else { text_rows.len() };
                for text in text_rows.into_iter().take(shown) {
                    self.rows.push(UiRow {
                        key: (0, 0),
                        body: Body::Plain(text.into()),
                        message,
                        first: false,
                        last: false,
                        work: false,
                        work_tail: false,
                        thought_phase: None,
                    });
                }
            }
            let mut prose_ordinal = 16;
            for row in &mut self.rows[first..] {
                if role == "reasoning" && matches!(row.body, Body::Reasoning | Body::Markdown(_)) {
                    row.thought_phase = phase
                        .filter(|_| grouped)
                        .map(|group| (group.start, group.last == at));
                }
                let ordinal = match row.body {
                    Body::WorkLine { .. } => 0,
                    Body::Phase { .. } => 1,
                    Body::Tool { .. } => 2,
                    Body::Reasoning => 4,
                    _ => {
                        let key = prose_ordinal;
                        prose_ordinal += 1;
                        key
                    }
                };
                row.key = (at, ordinal);
                row.work = has_work
                    && ready
                    && role != "visual"
                    && !activity.pinned.contains(&at)
                    && final_answer.is_some_and(|answer| at < answer)
                    && !matches!(row.body, Body::WorkLine { .. });
            }
            if let Some(row) = self.rows.get_mut(first) {
                row.first = true;
            }
            if self.rows.len() > first {
                self.rows.last_mut().unwrap().last = true;
            }
            let prompt_fingerprint = if role == "user" {
                measurements::prompt_fingerprint(&self.rows[first..], &attachments)
            } else {
                0
            };
            self.messages.push(Message {
                start: at,
                display_start: first,
                role,
                parser,
                markdown_blocks,
                user_link,
                under_line: has_work && final_answer == Some(at),
                turn: if self.mono {
                    self.mono_group_start(turn)
                } else {
                    turn
                },
                attachments,
                prompt_fingerprint,
            });
            if self.mono
                && is_prompt
                && self.mono_pending_joins_live_turn()
                && last_prompt.is_some_and(|p| end > p)
            {
                // The running turn keeps the user's messages together above its work.
                self.push_pending(at);
            }
            if self.mono && !is_prompt && end == turn_end {
                // Starting a provider precedes its hidden notification prompt.
                // Cards from the completed run stay in place during that gap.
                let settled = !(self.streaming
                    && turn_end == self.source.len()
                    && turn_end > self.live_start);
                if settled {
                    let mut documents = Vec::new();
                    for row in &self.source[self.prompt_start(turn)..turn_end] {
                        if row.role == "mono-document"
                            && let Ok(value) = serde_json::from_str::<serde_json::Value>(&row.text)
                        {
                            if let Some(old) =
                                documents.iter_mut().find(|old: &&mut serde_json::Value| {
                                    old["documentId"] == value["documentId"]
                                })
                            {
                                *old = value;
                            } else {
                                documents.push(value);
                            }
                        }
                    }
                    for (k, value) in documents.into_iter().enumerate() {
                        self.rows.push(UiRow {
                            key: (at, usize::MAX - 128 + k.min(60)),
                            body: Body::MonoCard(value),
                            message,
                            first: false,
                            last: false,
                            work: false,
                            work_tail: false,
                            thought_phase: None,
                        });
                    }
                    for (k, id) in self.run_launches(turn, turn_end).into_iter().enumerate() {
                        self.rows.push(UiRow {
                            key: (at, usize::MAX - 64 + k.min(60)),
                            body: Body::Launch(id),
                            message,
                            first: false,
                            last: false,
                            work: false,
                            work_tail: false,
                            thought_phase: None,
                        });
                    }
                }
            }
            if !self.document
                && !is_prompt
                && self.source[at].role != "mono-card"
                && self.source[at].role != monocode_core::transcript::attachments::ROLE
                && !reaction
                && end == turn_end
                && !(self.mono && self.mono_next_run_continues(turn, turn_end))
            {
                if !self.mono
                    && self.review_shown
                    && self.review.is_some()
                    && turn_end == self.source.len()
                {
                    self.rows.push(UiRow {
                        key: (at, usize::MAX - 2),
                        body: Body::Review { turn },
                        message,
                        first: false,
                        last: false,
                        work: false,
                        work_tail: false,
                        thought_phase: None,
                    });
                }
                self.rows.push(UiRow {
                    key: (at, usize::MAX - 1),
                    body: Body::Actions {
                        turn: if self.mono {
                            self.mono_group_start(turn)
                        } else {
                            turn
                        },
                        complete: !self.streaming || end < self.source.len(),
                    },
                    message,
                    first: false,
                    last: false,
                    work: false,
                    work_tail: false,
                    thought_phase: None,
                });
            }
            at = end;
        }
        self.prepared_parsers.clear();
        if self.mono && !self.rows.iter().any(|r| matches!(r.body, Body::Pending(_))) {
            self.push_pending(self.source.len());
        }
        if self.mono {
            // An accepted steer stays beside the original user prompt, both
            // while streaming and after the provider's final answer is saved.
            let messages = &self.messages;
            let pending_group = self.mono_pending_joins_live_turn().then(|| {
                let latest = self
                    .source
                    .iter()
                    .rposition(Row::is_prompt)
                    .map_or(0, |at| at + 1);
                self.mono_group_start(latest)
            });
            let tail = self.source.len();
            self.rows[display_start..].sort_by_key(|row| {
                let message = messages.get(row.message);
                let pending = matches!(row.body, Body::Pending(_));
                let group = message
                    .map(|m| m.turn)
                    .or_else(|| pending.then_some(pending_group).flatten())
                    .unwrap_or(tail);
                (
                    group,
                    !(pending || message.is_some_and(|m| m.role == "user")),
                    row.key,
                )
            });
            mono_divider::insert(&mut self.rows, &self.messages, display_start);
        }
        self.push_saved_draft();
        let mut pinned_answers = HashMap::new();
        if !activity.pinned.is_empty() {
            let mut parked: HashMap<usize, Vec<UiRow>> = HashMap::new();
            let mut ordered = Vec::new();
            for row in self.rows.drain(display_start..) {
                let answer = self
                    .fold_targets
                    .get(&self.messages[row.message].turn)
                    .copied()
                    .flatten();
                if activity.pinned.contains(&row.key.0)
                    && matches!(row.body, Body::Tool { .. })
                    && let Some(answer) = answer.filter(|answer| row.key.0 < *answer)
                {
                    pinned_answers.insert(row.key.0, answer);
                    parked.entry(answer).or_default().push(row);
                } else {
                    let mut due: Vec<_> = parked
                        .keys()
                        .copied()
                        .filter(|answer| *answer <= row.key.0)
                        .collect();
                    due.sort_unstable();
                    for answer in due {
                        ordered.extend(parked.remove(&answer).unwrap());
                    }
                    ordered.push(row);
                }
            }
            // A folded answer is always present; retain runs defensively if
            // a transient source patch has not made its prose row yet.
            let mut remaining: Vec<_> = parked.into_iter().collect();
            remaining.sort_by_key(|(answer, _)| *answer);
            for (_, runs) in remaining {
                ordered.extend(runs);
            }
            self.rows.extend(ordered);
        }
        // A phase and its nested steps are one sibling in the outer work tree.
        // Its final outer branch ends at the header, above the inner tool rail.
        let mut next_work_sibling = false;
        for row in self.rows[display_start..].iter_mut().rev() {
            row.work_tail = row.work && !next_work_sibling;
            if !row.in_phase() {
                next_work_sibling = row.work;
            }
        }
        let target = self.rows.split_off(display_start);
        let merged = if self.visible && !cx.reduce_motion() {
            let replaced_work = self
                .fold_targets
                .get(&current_turn)
                .copied()
                .flatten()
                .filter(|_| !self.work_folds.get(&current_turn).copied().unwrap_or(false))
                .filter(|_| layout_changed)
                .map(|answer| current_turn..answer);
            if replaced_work.is_some() {
                // A replacement establishes a new block's baseline. The old
                // block's removed height is not a scroll animation to replay.
                self.follow.measured = None;
            }
            self.reconcile(old_rows, target, now, &pinned_answers, replaced_work)
        } else {
            self.motions.clear();
            target
        };
        self.phase_toggle = None;
        self.rows.extend(merged);
        self.windowed_phases = self
            .rows
            .iter()
            .filter_map(|row| match row.body {
                Body::Phase { group, active, .. }
                    if active
                        || (self.phase_scrolls.contains_key(&group)
                            && !self.phases.get(&group).copied().unwrap_or(active)) =>
                {
                    Some(group)
                }
                _ => None,
            })
            .collect();
        // A closing live phase keeps its bounded window for the exit. Once
        // settled, an explicitly opened group returns to virtualized rows.
        for (&group, scroll) in &mut self.phase_scrolls {
            let enabled = self.visible && self.rows.iter().any(|row| {
                matches!(row.body, Body::Phase { group: id, active: true, .. } if id == group)
            }) && self.phases.get(&group).copied().unwrap_or(true);
            scroll.enable(enabled);
        }
        self.index_messages();
        // Preserve surviving indices and scroll offsets while the tail reflows.
        let common = old_count.min(self.rows.len());
        self.measurements.rows_changed();
        for ix in display_start..common {
            let (old, after_receipt) = &old_layouts[ix - display_start];
            if !measurements::same_layout(old, &self.rows[ix])
                || *after_receipt != measurements::after_receipt(&self.rows, &self.messages, ix)
            {
                self.list.remeasure_items(ix..ix + 1);
            }
        }
        if old_count != self.rows.len() {
            self.list
                .splice(common..old_count, self.rows.len() - common);
        }
        self.list
            .remeasure_items(self.footer_ix()..self.footer_ix() + 1);
        if self.mono && display_start > 0 {
            // A new message can stack the one before it, changing its spacing.
            self.list
                .remeasure_items(display_start.saturating_sub(8)..display_start.min(common));
        }
        if !self.hover_rows.is_empty() {
            let keys: HashSet<_> = self.rows.iter().map(|r| r.key).collect();
            self.hover_rows.retain(|key, _| keys.contains(key));
        }
        if following {
            self.list.scroll_to_end();
        } else if let Some(key) = anchor
            && let Some(item_ix) = self.rows.iter().position(|r| r.key == key)
        {
            self.list.scroll_to(ListOffset {
                item_ix,
                offset_in_item: offset.offset_in_item,
            });
        }
        self.prune_reveals(now);
    }
    fn prune_reveals(&mut self, now: Instant) {
        let latest = self
            .streaming
            .then(|| self.source.iter().rposition(|row| row.first))
            .flatten();
        self.reveals
            .retain(|at, reveal| Some(*at) == latest || reveal.active(now));
    }
    fn prepare_reveal(
        &mut self,
        at: usize,
        end: usize,
        text: &str,
        assistant: bool,
        now: Instant,
        reduced: bool,
    ) {
        if !self.reveals.contains_key(&at)
            && !(self.visible
                && self.smooth_reveal
                && !reduced
                && self.streaming
                && end == self.source.len()
                && at >= self.live_start
                && assistant)
        {
            return;
        }
        let paced = self.reveals.entry(at).or_insert_with(|| {
            PacedText::new(
                text,
                self.visible
                    && self.smooth_reveal
                    && !reduced
                    && self.streaming
                    && at >= self.live_start
                    && assistant,
                now,
            )
        });
        paced.update(text, now);
        if !self.visible || !self.smooth_reveal || reduced {
            paced.settle(now);
        }
        paced.step(now, self.streaming && end == self.source.len());
    }
    fn answer_visible(&self, answer: usize) -> bool {
        if self.source[answer].text.trim().is_empty() {
            return false;
        }
        if answer < self.live_start || !self.visible || !self.smooth_reveal {
            return true;
        }
        self.reveals
            .get(&answer)
            .is_none_or(|r| !r.text().trim().is_empty())
    }
    fn index_messages(&mut self) {
        let mut first = HashMap::new();
        for (ix, row) in self.rows.iter().enumerate() {
            first.entry(row.message).or_insert(ix);
        }
        let mut next = self.rows.len();
        for (ix, message) in self.messages.iter_mut().enumerate().rev() {
            if let Some(at) = first.get(&ix) {
                next = *at;
            }
            message.display_start = next;
        }
    }
    fn reconcile(
        &mut self,
        mut old: Vec<UiRow>,
        target: Vec<UiRow>,
        now: Instant,
        pinned_answers: &HashMap<usize, usize>,
        replaced_work: Option<std::ops::Range<usize>>,
    ) -> Vec<UiRow> {
        // Exit rows outlive a source patch, but message ordinals do not.
        let owners: HashMap<_, _> = self
            .messages
            .iter()
            .enumerate()
            .map(|(index, message)| (message.start, index))
            .collect();
        old.retain_mut(|row| {
            if let Some(&message) = owners.get(&row.key.0) {
                row.message = message;
                true
            } else {
                self.motions.remove(&row.key);
                false
            }
        });
        let phase_toggle = self.phase_toggle.take();
        if old.is_empty() && !self.streaming && self.motions.is_empty() {
            return target;
        }
        let order = |row: &UiRow| {
            if matches!(row.body, Body::Tool { .. })
                && let Some(&answer) = pinned_answers.get(&row.key.0)
            {
                (answer, 0, row.key.0, row.key.1)
            } else {
                (row.key.0, 1, row.key.0, row.key.1)
            }
        };
        // Source keys stay stable when runs are lifted below the work. The
        // answer boundary can advance between renders, so place outgoing
        // commentary in the fold above the pinned stack before merging it.
        old.sort_by_key(&order);
        let wanted: HashSet<_> = target.iter().map(|r| r.key).collect();
        let previous: HashMap<_, _> = old.iter().enumerate().map(|(ix, r)| (r.key, ix)).collect();
        let mut merged = Vec::new();
        let mut cursor = 0;
        let source = &self.source;
        let windowed = &self.windowed_phases;
        let keep_old = |row: &UiRow, motions: &mut HashMap<_, Motion>| -> bool {
            // "Show less" hides the rest of a message at once, as in React.
            if matches!(row.body, Body::Plain(_))
                && wanted.contains(&(row.key.0, 16))
                && source.get(row.key.0).is_some_and(|r| r.role == "user")
            {
                return false;
            }
            if matches!(
                row.body,
                Body::Actions { .. }
                    | Body::MonoDivider
                    | Body::Pending(_)
                    | Body::Launch(_)
                    | Body::Review { .. }
                    | Body::SavedDraft
            ) {
                return false;
            }
            if wanted.contains(&row.key) {
                return false;
            }
            // React replaces the live prose/work block when its answer moves
            // on. Keeping both blocks in layout for an exit animation can
            // temporarily overflow an otherwise short anchored turn.
            // Explicit disclosure toggles still animate in place.
            if replaced_work
                .as_ref()
                .is_some_and(|range| range.contains(&row.key.0))
            {
                motions.remove(&row.key);
                return false;
            }
            let motion = motions.entry(row.key).or_insert_with(|| {
                if let Some(group) = row
                    .phase_group()
                    .filter(|g| Some(*g) == phase_toggle || windowed.contains(g))
                {
                    Motion::phase(group, false, now)
                } else {
                    Motion::new(false, false, now)
                }
            });
            if motion.opening {
                *motion = motion.redirect(false, now);
            }
            !motion.finished(now)
        };
        for row in target {
            // Outgoing work stays above the new final answer during its exit.
            // New rows have no previous index to anchor the old suffix to.
            while cursor < old.len() && order(&old[cursor]) < order(&row) {
                if keep_old(&old[cursor], &mut self.motions) {
                    merged.push(old[cursor].clone());
                }
                cursor += 1;
            }
            if let Some(&at) = previous.get(&row.key) {
                if at >= cursor {
                    for old in &old[cursor..at] {
                        if keep_old(old, &mut self.motions) {
                            merged.push(old.clone());
                        }
                    }
                    cursor = at + 1;
                }
                if self.motions.get(&row.key).is_some_and(|m| !m.opening) {
                    let motion = self.motions[&row.key].redirect(true, now);
                    self.motions.insert(row.key, motion);
                }
            } else {
                let unseen = if matches!(row.body, Body::Tool { .. }) {
                    self.seen_steps.insert(row.key.0)
                } else {
                    false
                };
                let phase = row.phase_group().filter(|g| Some(*g) == phase_toggle);
                let step = phase.is_none()
                    && self.streaming
                    && row.key.0 >= self.live_start
                    && unseen
                    && !row.work;
                if step
                    || matches!(row.body, Body::Tool { .. } | Body::Reasoning)
                    || self.expanded.contains(&row.key.0)
                    || row.work
                {
                    let mut motion = phase.map_or_else(
                        || Motion::new(true, step, now),
                        |group| Motion::phase(group, true, now),
                    );
                    if step {
                        let group = match row.body {
                            Body::Tool { group, .. } => group.unwrap_or(row.key.0),
                            _ => row.key.0,
                        };
                        let start = self.step_queue.get(&group).copied().unwrap_or(now).max(now);
                        let wait = start.duration_since(now).as_secs_f32();
                        let pace = (0.48 * ((2. - wait) / (2. - 0.96)).min(1.)).max(0.16);
                        motion.start = start;
                        motion.duration = std::time::Duration::from_secs_f32(pace);
                        self.step_queue.insert(group, start + motion.duration);
                    }
                    self.motions.insert(row.key, motion);
                }
            }
            merged.push(row);
        }
        for row in &old[cursor..] {
            if keep_old(row, &mut self.motions) {
                merged.push(row.clone());
            }
        }
        merged
    }
    fn schedule_frame(&mut self, resized: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.visible || cx.reduce_motion() || self.frame_scheduled {
            return;
        }
        let now = Instant::now();
        let smooth = resized
            || !self.motions.is_empty()
            || self.agent_panels.animating()
            || self
                .prompt_rise
                .is_some_and(|(_, born)| born.elapsed().as_secs_f32() < 0.88)
            || self.burst.is_some_and(|burst| burst.active(now))
            || self.chat_animating();
        if smooth {
            self.frame_scheduled = true;
            let due = self.last_motion_frame.map(|at| at + MOTION_FRAME);
            motion_frame(cx.entity().downgrade(), due, window);
        } else if self.stream_animating(now) {
            // Stream presentation keeps the shared 30Hz budget. Pure fade
            // frames reuse the parsed blocks, prepared text and measured rows.
            self.frame_scheduled = true;
            let view = cx.entity().downgrade();
            crate::pulse::request_frame(crate::pulse::Rate::Smooth, window, cx, move |cx| {
                let _ = view.update(cx, |this, cx| this.animate_frame(cx));
            });
        }
    }
    fn animate_frame(&mut self, cx: &mut Context<Self>) {
        self.last_motion_frame = Some(Instant::now());
        self.frame_scheduled = false;
        // A frame queued before switching tabs must not invalidate the hidden
        // transcript's retained layout or advance its presentation animations.
        if !self.visible {
            return;
        }
        let mut changed_phases = HashSet::new();
        // Revealing words keeps every size but the revealed rows', which
        // measure again by their content; anything else here resizes rows.
        let mut resized = self.agent_panels.animating() || !self.motions.is_empty();
        for start in self.agent_panels.animate() {
            for (ix, row) in self.rows.iter().enumerate() {
                if row.key.0 == start {
                    self.list.remeasure_items(ix..ix + 1);
                    changed_phases.extend(row.phase_group());
                }
            }
        }
        let now = Instant::now();
        let mut changed = None;
        for (&at, reveal) in &mut self.reveals {
            if reveal.step(now, self.streaming) {
                changed = Some(changed.map_or(at, |old: usize| old.min(at)));
            }
        }
        if let Some(start) = changed {
            self.rebuild_rows(start, cx);
        }
        let mut removed = Vec::new();
        let offset = self.list.logical_scroll_top();
        let anchor = (!self.list.is_following_tail())
            .then(|| self.rows.get(offset.item_ix).map(|r| r.key))
            .flatten();
        for (ix, row) in self.rows.iter().enumerate() {
            if let Some(motion) = self.motions.get(&row.key) {
                changed_phases.extend(row.phase_group());
                if motion.finished(now) && !motion.opening {
                    removed.push(ix);
                } else {
                    self.list.remeasure_items(ix..ix + 1);
                }
            }
        }
        for (ix, row) in self.rows.iter().enumerate() {
            if matches!(row.body, Body::Phase { group, .. } if changed_phases.contains(&group) && self.windowed_phase(group))
            {
                self.list.remeasure_items(ix..ix + 1);
            }
        }
        for ix in removed.into_iter().rev() {
            self.rows.remove(ix);
            self.list.splice(ix..ix + 1, 0);
        }
        if let Some(key) = anchor
            && let Some(item_ix) = self.rows.iter().position(|r| r.key == key)
            && item_ix != offset.item_ix
        {
            self.list.scroll_to(ListOffset {
                item_ix,
                offset_in_item: offset.offset_in_item,
            });
        }
        self.motions.retain(|_, motion| !motion.finished(now));
        self.burst = self.burst.filter(|burst| burst.active(now));
        self.entrances
            .retain(|_, born| now.duration_since(*born).as_secs_f32() < 0.4);
        self.pending_entrances
            .retain(|_, born| now.duration_since(*born).as_secs_f32() < 0.4);
        self.prune_reveals(now);
        for (_, born) in self.reactions.values_mut() {
            if born.is_some_and(|b| now.duration_since(b).as_secs_f32() >= 0.4) {
                *born = None;
            }
        }
        self.list
            .remeasure_items(self.footer_ix()..self.footer_ix() + 1);
        self.index_messages();
        resized |= self.agent_panels.animating() || !self.motions.is_empty();
        if resized {
            cx.notify();
        } else {
            cx.notify_paint();
        }
    }
    fn highlight(&mut self, fence: &Arc<Fence>, cx: &mut Context<Self>) {
        if self.highlights.contains_key(&fence.key) || self.highlighting.contains(&fence.key) {
            return;
        }
        // Replace an obsolete queued version of a growing fence before it starts.
        self.highlight_queue.retain(|queued| {
            let obsolete =
                fence.language == queued.language && fence.code.starts_with(queued.code.as_ref());
            if obsolete {
                self.highlighting.remove(&queued.key);
            }
            !obsolete
        });
        if self.highlight_queue.len() >= 8 {
            return;
        }
        self.highlighting.insert(fence.key);
        self.highlight_queue.push_back(fence.clone());
        self.pump_highlights(cx);
    }
    fn pump_highlights(&mut self, cx: &mut Context<Self>) {
        if self.highlight_active {
            return;
        }
        let Some(fence) = self.highlight_queue.pop_front() else {
            return;
        };
        self.highlight_active = true;
        let epoch = self.highlight_epoch;
        let key = fence.key;
        let bytes = fence.code.len();
        let code = fence.code.to_string();
        let language = fence.language.clone();
        let task = cx.background_executor().spawn(async move {
            crate::syntax::highlight(HighlightRequest {
                source: &code,
                path: None,
                fence_tag: Some(&language),
            })
            .ok()
            .map(Arc::new)
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.highlight_epoch != epoch {
                    return;
                }
                this.highlighting.remove(&key);
                this.highlight_active = false;
                let bytes = if result.is_some() { bytes } else { 0 };
                while this.highlight_order.len() >= 100 || this.highlight_bytes + bytes > 500_000 {
                    let Some((key, bytes)) = this.highlight_order.pop_front() else {
                        break;
                    };
                    this.highlights.remove(&key);
                    this.highlight_bytes -= bytes;
                }
                // Oversized fences stay readable without retaining their tokens.
                this.highlight_order.push_back((key, bytes.min(500_000)));
                this.highlight_bytes += bytes.min(500_000);
                this.highlights
                    .insert(key, if bytes <= 500_000 { result } else { None });
                this.pump_highlights(cx);
                cx.notify();
            });
        })
        .detach();
    }
    fn code(
        &mut self,
        fence: Arc<Fence>,
        lines: std::ops::Range<usize>,
        ix: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        self.highlight(&fence, cx);
        let first = lines.start == 0;
        let last = lines.end == fence.lines.len();
        let key = fence.key;
        let code = fence.code.clone();
        let copied = self.copied == Some(key);
        let file_link = fence.path.as_ref().map(|raw| {
            let (path, location) = crate::file_editor::file_location(
                raw,
                self.file_link_base.as_deref().unwrap_or(""),
            );
            let (line, column) = location.unwrap_or((fence.start_line, 1));
            format!("{path}:{line}:{column}")
        });
        div()
            .relative()
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .rounded_t(px(if first { 10. } else { 0. }))
            .rounded_b(px(if last { 10. } else { 0. }))
            .border_x_1()
            .when(first, |d| d.border_t_1())
            .when(last, |d| d.border_b_1())
            .border_color(content(0.1))
            .bg(content(0.06))
            .when(first, |d| {
                d.child(
                    div()
                        .when(!fence.label.is_empty(), |d| d.min_h(px(36.)))
                        .when(fence.label.is_empty(), |d| {
                            d.absolute().top(px(6.)).right(px(6.))
                        })
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(12.))
                        .text_color(content(0.65))
                        .when(!fence.label.is_empty(), |d| {
                            let extension = match fence.language.as_str() {
                                "rust" => "rs",
                                "javascript" => "js",
                                "typescript" => "ts",
                                "python" => "py",
                                "bash" | "shell" => "sh",
                                "markdown" => "md",
                                other => other,
                            };
                            let file = if fence.label.contains('.') {
                                fence.label.clone()
                            } else {
                                format!("code.{extension}")
                            };
                            d.child(crate::activity::file_icon(&file, 16.))
                        })
                        .child(
                            div()
                                .id(("code-file-link", ix))
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .when_some(file_link, |d, path| {
                                    let menu_path = path.clone();
                                    d.role(Role::Link)
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(rgb(0x7dd3fc)))
                                        .on_click(move |event, window, cx| {
                                            if event.click_count() == 1 {
                                                window.dispatch_action(
                                                    Box::new(crate::file_editor::OpenPath(
                                                        path.clone(),
                                                    )),
                                                    cx,
                                                );
                                            }
                                        })
                                        .on_mouse_down(
                                            gpui::MouseButton::Right,
                                            move |event, window, cx| {
                                                markdown::link_menu(
                                                    &menu_path,
                                                    event.position,
                                                    window,
                                                    cx,
                                                );
                                                cx.stop_propagation();
                                            },
                                        )
                                })
                                .child(fence.label.clone()),
                        )
                        .child(
                            icon_button(
                                "copy-code",
                                if copied { "Copied" } else { "Copy code" },
                                if copied { "check" } else { "copy" },
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        code.strip_suffix("\r\n")
                                            .or_else(|| code.strip_suffix('\n'))
                                            .unwrap_or(&code)
                                            .to_owned(),
                                    ));
                                    this.copied = Some(key);
                                    cx.notify();
                                    cx.spawn(async move |this, cx| {
                                        cx.background_executor()
                                            .timer(std::time::Duration::from_millis(1500))
                                            .await;
                                        let _ = this.update(cx, |this, cx| {
                                            if this.copied == Some(key) {
                                                this.copied = None;
                                                cx.notify();
                                            }
                                        });
                                    })
                                    .detach();
                                },
                            )),
                        ),
                )
            })
            .child(
                div()
                    .id(("code-scroll", ix))
                    .overflow_x_scroll()
                    .w_full()
                    .py(px(10.))
                    .pr_2()
                    .when(first && !fence.label.is_empty(), |d| {
                        d.border_t_1().border_color(content(0.1))
                    })
                    .child(markdown::code_lines_selected(
                        &fence,
                        lines,
                        self.highlights.get(&key).and_then(|v| v.as_deref()),
                        self.search_match.as_ref().map_or("", |(_, q)| q.as_str()),
                        self.search_match
                            .as_ref()
                            .is_some_and(|(start, _)| *start == self.rows[ix].key.0),
                        self.text_selection(
                            self.rows[ix].key,
                            self.messages[self.rows[ix].message].start,
                            self.messages[self.rows[ix].message].role == "assistant",
                            cx,
                        ),
                    )),
            )
            .into_any_element()
    }
    fn row(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        self.rendered += 1;
        if let Some(group) = self.rows[ix].phase_group()
            && self.windowed_phase(group)
        {
            // Keep source/list indices stable for message anchors, selection
            // and reconciliation. The phase header owns these rows' layout
            // while live; settled, opened history is virtualized normally.
            return div().h_0().into_any_element();
        }
        if let Body::Phase { group, active, .. } = self.rows[ix].body
            && self.windowed_phase(group)
        {
            return self.phase_window(ix, group, active, window, cx);
        }
        match self.rows[ix].body {
            Body::SavedDraft => return self.saved_draft_row(cx),
            Body::Pending(i) => return self.pending_row(i, cx),
            Body::Launch(id) => return self.launch_row(ix, id, cx),
            Body::Review { turn } => return self.review_row(turn),
            _ => {}
        }
        let group = self.motions.get(&self.rows[ix].key).and_then(|m| m.group);
        if let Some(group) = group
            && self
                .phase_layouts
                .get(&group)
                .is_none_or(|layout| layout.width != self.viewport.size.width)
        {
            // Measure once per content/width change while the body moves.
            // Retain individual list items so settled history stays virtualized.
            let indices: Vec<_> = self
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| {
                    self.motions
                        .get(&row.key)
                        .is_some_and(|m| m.group == Some(group))
                })
                .map(|(ix, _)| ix)
                .collect();
            let mut layout = PhaseLayout {
                width: self.viewport.size.width,
                height: 0.,
                rows: HashMap::new(),
            };
            for ix in indices {
                let natural = self.row_body(ix, false, window, cx).layout_as_root(
                    size(
                        AvailableSpace::Definite(layout.width),
                        AvailableSpace::MinContent,
                    ),
                    window,
                    cx,
                );
                let height = f32::from(natural.height);
                layout
                    .rows
                    .insert(self.rows[ix].key, (layout.height, height));
                layout.height += height;
            }
            self.phase_layouts.insert(group, layout);
        }
        self.row_body(ix, true, window, cx)
    }
    fn windowed_phase(&self, group: usize) -> bool {
        self.windowed_phases.contains(&group)
    }
    fn phase_window(
        &mut self,
        ix: usize,
        group: usize,
        active: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let header = self.row_body(ix, true, window, cx);
        let open = self.phases.get(&group).copied().unwrap_or(active);
        let indices: Vec<_> = (ix + 1..self.rows.len())
            .take_while(|&i| self.rows[i].phase_group() == Some(group))
            .collect();
        let progress = indices
            .iter()
            .find_map(|&i| {
                self.motions
                    .get(&self.rows[i].key)
                    .filter(|m| m.group == Some(group))
                    .map(|m| m.value(self.frame_now))
            })
            .unwrap_or((1., 1.));
        let mut steps = div().w_full().min_w_0().flex().flex_col();
        for i in indices {
            steps = steps.child(self.row_body(i, true, window, cx));
        }
        let body = live_phase::live_phase(
            steps.into_any_element(),
            self.phase_scrolls.entry(group).or_default(),
            live_phase::max_height(window),
            active && open && self.visible,
            cx,
        );
        div()
            .id(("phase-window", group))
            .w_full()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .opacity(progress.1)
                    .child(Disclosure::new(body, self.viewport.size.width, progress.0)),
            )
            .into_any_element()
    }
    /// The first reply the user sent after `message`, queued ones included,
    /// which answers any choices that message offered.
    fn reply_after(&self, message: usize) -> Option<String> {
        self.messages
            .get(message + 1..)?
            .iter()
            .find(|m| m.role == "user")
            .and_then(|m| self.source.get(m.start))
            .map(|row| row.text.clone())
            .or_else(|| self.pending.first().map(|p| p.text.to_string()))
    }
    fn row_body(
        &mut self,
        ix: usize,
        animate: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let row = self.rows[ix].clone();
        let prompt_width = self.split_prompt_width(ix, window);
        let message = &self.messages[row.message];
        let user = message.role == "user";
        let stacked = user && self.mono && self.stacks_on_next(message.start);
        let message_turn = message.turn;
        let error = message.role == "error";
        let reasoning = message.role == "reasoning";
        let message_start = message.start;
        let first = row.first;
        let last = row.last;
        // The final answer is under the receipt even while its expanded work
        // is leaving. Its padding must not change when those rows are removed.
        let after_receipt = message.under_line
            || (ix > 0 && matches!(self.rows[ix - 1].body, Body::WorkLine { .. }));
        let first_prose = row.key.1 == 16;
        let nested_phase = row.in_phase();
        let thought_header = matches!(row.body, Body::Reasoning);
        let attachments = message.attachments.clone();
        // A Mono's chat shows sent images above the bubble, like photos in a
        // messaging app; only other files stay inside it.
        let (photos, bubble_files) = if user && first && self.mono {
            attachments::split_photos(&attachments)
        } else {
            (Vec::new(), attachments.to_vec())
        };
        let photo_body = (!photos.is_empty())
            .then(|| self.photo_row(&photos, &format!("sent-{message_start}"), cx));
        let attachment_body = (user && first && !bubble_files.is_empty())
            .then(|| self.attachment_row(&bubble_files, &format!("sent-{message_start}"), cx));
        let step_opacity = self
            .motions
            .get(&row.key)
            .filter(|m| m.step)
            .map_or(1., |m| m.value(self.frame_now).1);
        let is_tool = matches!(
            row.body,
            Body::Tool { .. } | Body::Phase { .. } | Body::Reasoning
        );
        let body = match row.body {
            Body::GitAction(card) => self.git_card_row(&card, message_start, cx),
            Body::RemotePlan(plan) => self.remote_plan_card(&plan, cx),
            Body::Visual(document) => self.visual_row(message_start, document, cx),
            Body::Interjection(ref note) => {
                let open = self.expanded.contains(&message_start);
                let long = note.text.lines().count() > 4 || note.text.len() > 300;
                let severity = note.severity.as_deref();
                let color = match severity {
                    Some("blocker") => rgb(0xf87171).into(),
                    Some("concern") => rgb(0xfbbf24).into(),
                    _ => content(0.55),
                };
                div()
                    .w_full()
                    .min_w_0()
                    .py_4()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().h(px(1.)).flex_1().bg(content(0.12)))
                            .child(
                                div()
                                    .id(("interjection-label", message_start))
                                    .role(Role::Group)
                                    .aria_label(format!("Interjection: {}", note.label()))
                                    .text_size(px(12.))
                                    .text_color(content(0.55))
                                    .child(note.label().to_owned()),
                            )
                            .when_some(severity, |d, severity| {
                                let label = match severity {
                                    "blocker" => "Blocker",
                                    "concern" => "Concern",
                                    "nit" => "Nit",
                                    value => value,
                                };
                                d.child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(color)
                                        .child(label.to_owned()),
                                )
                            })
                            .child(div().h(px(1.)).flex_1().bg(content(0.12))),
                    )
                    .when(!note.text.is_empty(), |d| {
                        d.child(
                            div()
                                .w_full()
                                .text_size(px(12.5))
                                .line_height(px(20.))
                                .text_color(content(0.7))
                                .when(!open, |d| d.max_h(px(80.)).overflow_hidden())
                                .child(note.text.clone()),
                        )
                    })
                    .when(long, |d| {
                        d.child(
                            div()
                                .id(("interjection-expand", message_start))
                                .role(Role::Button)
                                .aria_expanded(open)
                                .text_size(px(11.))
                                .text_color(content(0.5))
                                .child(if open { "Show less" } else { "Show more" })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle(message_start, cx);
                                })),
                        )
                    })
                    .into_any_element()
            }
            Body::MonoCard(ref value) => {
                let chosen = self.reply_after(row.message);
                crate::monos::cards::render(value, chosen.as_deref(), cx)
            }
            Body::MonoDivider => mono_divider::render(),
            Body::Pending(_) | Body::Launch(_) | Body::Review { .. } | Body::SavedDraft => {
                div().into_any_element()
            }
            Body::Actions { turn, complete } => {
                // Tighten only a visible Mono reply followed by a user bubble;
                // stacked user messages keep their existing spacing.
                let before_user = self.mono
                    && ix > 0
                    && matches!(self.rows[ix - 1].body, Body::Markdown(_))
                    && self.messages[self.rows[ix - 1].message].role == "assistant"
                    && self.rows.get(ix + 1).is_some_and(|next| {
                        matches!(next.body, Body::Pending(_))
                            || (matches!(next.body, Body::Plain(_))
                                && self.messages[next.message].role == "user")
                    });
                self.response_actions(turn, complete, before_user, cx)
            }
            Body::WorkLine { turn, .. } if self.mono => self.mono_turn_header(turn, cx),
            Body::WorkLine { turn, expandable } => {
                let live = self.streaming
                    && message_start >= self.live_start
                    && self.source[message_start..].iter().all(|r| !r.is_prompt());
                let elapsed = if live {
                    self.started.map(|at| at.elapsed())
                } else {
                    self.elapsed.get(&turn).copied().or_else(|| {
                        self.turns
                            .get(&turn)
                            .filter(|info| info.has_duration())
                            .map(|info| std::time::Duration::from_millis(info.duration_ms))
                    })
                };
                let model = self
                    .turns
                    .get(&turn)
                    .map(|info| info.model.trim())
                    .filter(|model| !model.is_empty())
                    .or_else(|| live.then_some(self.model.as_str()));
                let title = work_title(
                    model,
                    elapsed.map(|duration| duration.as_millis() as u64),
                    live,
                );
                let open = self.work_folds.get(&turn).copied().unwrap_or(false);
                let label = if expandable {
                    if open {
                        "Hide the work".into()
                    } else {
                        "Show the work".into()
                    }
                } else {
                    title.clone()
                };
                div()
                    .id(("work-fold", turn))
                    .role(if expandable {
                        Role::Button
                    } else {
                        Role::Label
                    })
                    .aria_label(label)
                    .aria_expanded(open)
                    .group("work-fold")
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .py_1()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(content(0.5))
                    .child(
                        div()
                            .relative()
                            .size(px(14.))
                            .flex_shrink_0()
                            .when(!open, |d| {
                                d.child(
                                    provider_icon(
                                        self.turns
                                            .get(&turn)
                                            .map(|info| info.provider.as_str())
                                            .filter(|provider| !provider.is_empty())
                                            .unwrap_or(self.provider.as_str()),
                                        14.,
                                    )
                                    .when(expandable, |i| {
                                        i.group_hover("work-fold", |s| s.opacity(0.))
                                    }),
                                )
                            })
                            .when(expandable, |d| {
                                d.child(
                                    icon(
                                        if open {
                                            "chevron-down"
                                        } else {
                                            "chevron-right"
                                        },
                                        14.,
                                    )
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .when(!open, |i| {
                                        i.opacity(0.).group_hover("work-fold", |s| s.opacity(1.))
                                    }),
                                )
                            }),
                    )
                    .child(div().min_w_0().flex_1().truncate().child(
                        if live && !cx.reduce_motion() {
                            crate::activity::shimmer(
                                title,
                                self.started.map_or(0., |s| s.elapsed().as_secs_f32()),
                            )
                        } else {
                            div().child(title).into_any_element()
                        },
                    ))
                    .when(expandable, |d| d.hover(|s| s.text_color(content(0.8))))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if expandable {
                            this.work_folds.insert(turn, !open);
                            this.list.pause_following_tail();
                            this.rebuild(turn, cx);
                        }
                    }))
                    .into_any_element()
            }
            Body::Phase {
                title,
                group,
                icon: glyph,
                active,
            } => {
                let open = self.phases.get(&group).copied().unwrap_or(active);
                let rotation = self
                    .motions
                    .values()
                    .find(|m| m.group == Some(group))
                    .map_or(if open { 1. } else { 0. }, |m| m.rotation(self.frame_now));
                div()
                    .id(("phase", group))
                    .role(Role::Button)
                    .aria_label(format!(
                        "{} the steps for {title}",
                        if open { "Hide" } else { "Show" }
                    ))
                    .aria_expanded(open)
                    .group("activity-phase")
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .py_1()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(content(0.5))
                    .child(
                        div()
                            .relative()
                            .size(px(14.))
                            .flex_shrink_0()
                            .child(if active && !cx.reduce_motion() {
                                // The compaction step's dots stand in for the
                                // group's icon until its work settles.
                                div()
                                    .size_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(crate::session_mark::spinner(
                                        ("phase-loader", group),
                                        crate::pulse::Rate::Half,
                                    ))
                                    .group_hover("activity-phase", |s| s.opacity(0.))
                                    .into_any_element()
                            } else {
                                crate::activity::glyph(glyph, 14.)
                                    .group_hover("activity-phase", |s| s.opacity(0.))
                                    .into_any_element()
                            })
                            .child(
                                icon("chevron-right", 14.)
                                    .with_transformation(Transformation::rotate(percentage(
                                        rotation * 0.25,
                                    )))
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .opacity(0.)
                                    .group_hover("activity-phase", |s| s.opacity(1.)),
                            ),
                    )
                    .child(div().min_w_0().flex_1().truncate().child(
                        if active && !cx.reduce_motion() {
                            crate::activity::shimmer(
                                title,
                                self.started.map_or(0., |s| s.elapsed().as_secs_f32()),
                            )
                        } else {
                            div().child(title).into_any_element()
                        },
                    ))
                    .hover(|s| s.text_color(content(0.8)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_phase(group, !open, cx);
                    }))
                    .into_any_element()
            }
            Body::Tool {
                call,
                group,
                tail,
                live,
            } => {
                // A lone step leads with the spinner its phase header shows once
                // a second step joins it. It keeps spinning after it settles,
                // until whatever comes next arrives, so the hand-off to the
                // group header never drops the loader.
                let loading = group.is_none()
                    && (call.state == ToolState::Running || live)
                    && !call.is_milestone()
                    && call.panel().is_none()
                    && !cx.reduce_motion();
                div()
                    .relative()
                    .w_full()
                    .when(group.is_some(), |d| {
                        d.pl(px(20.)).child(crate::activity::rail(tail))
                    })
                    .when(loading, |d| {
                        d.flex().items_start().gap_1p5().child(
                            div()
                                .h(px(28.))
                                .flex_shrink_0()
                                .opacity(step_opacity)
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .size(px(14.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_color(content(0.5))
                                        .child(crate::session_mark::spinner(
                                            ("tool-loader", message_start),
                                            crate::pulse::Rate::Half,
                                        )),
                                ),
                        )
                    })
                    .child(
                        div()
                            .when(loading, |d| d.min_w_0().flex_1())
                            .opacity(step_opacity)
                            .child(self.tool(*call, message_start, cx)),
                    )
                    .into_any_element()
            }
            Body::Reasoning => {
                let summary = self.messages[row.message]
                    .parser
                    .tree()
                    .blocks
                    .iter()
                    .flat_map(|block| markdown::rows(&block.block))
                    .map(|row| markdown::row_text(&row))
                    .find(|text| !text.trim().is_empty())
                    .unwrap_or_else(|| "Thinking…".into())
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                let open = self.expanded.contains(&message_start);
                div()
                    .id(("thinking", message_start))
                    .debug_selector(move || format!("thinking-{message_start}"))
                    .role(Role::Button)
                    .aria_label(format!(
                        "{} thinking: {summary}",
                        if open { "Hide" } else { "Show" }
                    ))
                    .aria_expanded(open)
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .py_1()
                    .line_height(px(20.))
                    .text_size(px(14.))
                    .text_color(content(0.5))
                    .hover(|s| s.text_color(content(0.8)))
                    .when(!nested_phase, |d| d.child(icon("minus", 14.)))
                    .child(div().min_w_0().flex_1().truncate().child(if open {
                        "Thinking".into()
                    } else {
                        summary
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(message_start, cx)))
                    .into_any_element()
            }
            Body::Markdown(ref md) => {
                let prepared = self.markdown_cache.get(row.key, md);
                let md = &prepared.row;
                let body = if let Part::Code { fence, lines } = &md.part {
                    self.code(fence.clone(), lines.clone(), ix, cx)
                } else if let Part::Visual { id, caption } = &md.part {
                    self.placed_visual(id, caption, cx)
                } else {
                    markdown::body_selected(
                        md,
                        format!("md-{ix}").into(),
                        if reasoning {
                            markdown::Tone::Reasoning
                        } else if row.work {
                            markdown::Tone::Work
                        } else {
                            markdown::Tone::Normal
                        },
                        self.reveals.get(&message_start).map(|r| &r.fades),
                        self.frame_now,
                        if reasoning {
                            None
                        } else {
                            self.text_selection(row.key, message_start, !user && !error, cx)
                        },
                        Some(&prepared),
                    )
                    .into_any_element()
                };
                div()
                    .when(self.mono, |d| {
                        let key = row.key;
                        d.debug_selector(move || format!("mono-prose-{}-{}", key.0, key.1))
                    })
                    .w_full()
                    .flex()
                    .pt(px(if first_prose {
                        if after_receipt || row.work { 4. } else { 12. }
                    } else if row.work {
                        md.gap.min(12.)
                    } else {
                        md.gap
                    }))
                    .pl(px(md.indent.saturating_sub(1) as f32 * 24.))
                    .when(md.quote > 0, |d| {
                        d.ml(px(4.)).pl_4().border_l_4().border_color(content(0.2))
                    })
                    .when(md.indent > 0, |d| {
                        d.child(
                            div()
                                .w(px(24.))
                                .flex_shrink_0()
                                .text_color(content(0.78))
                                .child(md.marker.clone().unwrap_or_default()),
                        )
                    })
                    .child(body)
                    .into_any_element()
            }
            Body::Plain(text)
                if user
                    && self.mono
                    && first
                    && last
                    && attachments.is_empty()
                    && is_emoji_only(&text) =>
            {
                // In a Mono's chat a few emoji stand alone: no bubble or actions.
                div()
                    .relative()
                    .text_size(px(60.))
                    .line_height(px(60.))
                    .child(text.trim().to_owned())
                    .children(self.reaction_badge(message_start))
                    .into_any_element()
            }
            Body::Plain(text) => {
                let burst = self.burst.filter(|burst| {
                    user && first && burst.prompt == message_start && burst.active(self.frame_now)
                });
                let now = self.frame_now;
                // UserLinkPreview: the prompt's first link, drawn in its chunk.
                let message_link = self.messages[row.message].user_link.clone();
                let link = message_link
                    .as_ref()
                    .filter(|(chunk, _)| user && *chunk + 16 == row.key.1)
                    .map(|(_, link)| link.clone());
                let collapsed = !self.expanded_prompts.contains(&message_start);
                let clamp = user && first && collapsed && message_link.is_none();
                // Images sent on their own need no bubble beneath them.
                let bare = photo_body.is_some() && text.is_empty() && attachment_body.is_none();
                // `overflows`: hidden chunks, or more than four wrapped lines.
                // React never clamps a message that shows a link preview.
                let overflows = user
                    && message_link.is_none()
                    && last
                    && (!first
                        || self.clamped_prompts.contains(&message_start)
                        || self.prompt_lines(&text, window) > 4);
                let bubble = div()
                    .when(photo_body.is_none(), |d| d.w_full())
                    .relative()
                    .flex()
                    .flex_col()
                    .text_color(if error {
                        rgb(0xf87171).into()
                    } else {
                        content(1.)
                    })
                    .when(user && !bare, |d| {
                        // AgentTranscript.tsx: text-sm (14px / 20px), px-3 py-2.
                        // Measurement shapes this font at the same size.
                        d.font(gpui::font(crate::fonts::UI))
                            .text_size(PROMPT_TEXT_SIZE)
                            .line_height(px(20.))
                            .px(PROMPT_PAD_X * 0.5)
                            .pt(px(if first { 8. } else { 0. }))
                            .pb(px(if last { 8. } else { 0. }))
                            .child(
                                canvas(
                                    |_, _, _| {},
                                    move |bounds, _, window, cx| {
                                        // Match React's measured single-line bubble, including
                                        // when a window resize makes the same text wrap.
                                        let radius =
                                            if first && last && bounds.size.height <= px(37.) {
                                                f32::from(bounds.size.height) * 0.5
                                            } else {
                                                12.
                                            };
                                        let corners = gpui::Corners {
                                            top_left: px(if first { radius } else { 0. }),
                                            top_right: px(if first { radius } else { 0. }),
                                            bottom_left: px(if last { radius } else { 0. }),
                                            bottom_right: px(if last { radius } else { 0. }),
                                        };
                                        window.paint_quad(
                                            gpui::fill(bounds, user_bubble_fill())
                                                .corner_radii(corners),
                                        );
                                        if let Some(burst) = burst.filter(burst::Burst::behind_text)
                                        {
                                            burst.paint(bounds, corners, now, window, cx);
                                        }
                                    },
                                )
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full(),
                            )
                    })
                    .children(
                        attachment_body
                            .map(|body| div().when(!text.is_empty(), |d| d.mb_2()).child(body)),
                    )
                    .when(!text.is_empty(), |d| {
                        // A commit pill's message leads with that action's icon.
                        let action_icon = (user && first && last)
                            .then(|| {
                                crate::conversation::git_actions::GitAction::message_icon(&text)
                            })
                            .flatten();
                        d.when_some(action_icon, |d, path| {
                            d.flex_row()
                                .items_center()
                                .gap(px(6.))
                                .child(icon(path, 14.).flex_shrink_0().text_color(content(0.6)))
                        })
                        .when_some(link.clone(), |d, link| {
                            d.child(self.user_link_text(&text, &link, row.key, message_start, cx))
                        })
                        .when(link.is_none(), |d| {
                            d.child(
                                div()
                                    .when(clamp, |d| d.line_clamp(4).text_ellipsis())
                                    .child({
                                        let styled = gpui::StyledText::new(text.clone())
                                            .with_highlights(markdown::mark_styles(
                                                &text,
                                                vec![],
                                                self.search_match
                                                    .as_ref()
                                                    .map_or("", |(_, q)| q.as_str()),
                                                self.search_match.as_ref().is_some_and(
                                                    |(start, _)| *start == message_start,
                                                ),
                                            ));
                                        let layout = styled.layout().clone();
                                        let child = styled.into_any_element();
                                        if let Some(selection) =
                                            self.text_selection(row.key, message_start, false, cx)
                                        {
                                            selection.wrap(text.clone(), layout, child)
                                        } else {
                                            child
                                        }
                                    }),
                            )
                        })
                    })
                    .children(burst.filter(|burst| !burst.behind_text()).map(|burst| {
                        let now = self.frame_now;
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, cx| {
                                // The same corners the bubble's fill paints.
                                let radius = if last && bounds.size.height <= px(37.) {
                                    f32::from(bounds.size.height) * 0.5
                                } else {
                                    12.
                                };
                                let bottom = px(if last { radius } else { 0. });
                                let corners = gpui::Corners {
                                    top_left: px(radius),
                                    top_right: px(radius),
                                    bottom_left: bottom,
                                    bottom_right: bottom,
                                };
                                burst.paint(bounds, corners, now, window, cx);
                            },
                        )
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                    }))
                    .when(overflows, |d| {
                        d.child(
                            div().flex().child(
                                div()
                                    .id(("prompt-more", message_start))
                                    .role(Role::Button)
                                    .aria_expanded(!collapsed)
                                    .mt_1()
                                    .px_1()
                                    .py(px(2.))
                                    .rounded(px(4.))
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(content(0.6))
                                    .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                                    .child(if collapsed { "Show more" } else { "Show less" })
                                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.toggle_prompt(message_start, cx)
                                    })),
                            ),
                        )
                    })
                    .when(user && self.mono && first, |d| {
                        d.children(self.reaction_badge(message_start))
                    })
                    .when(
                        user && last
                            && !stacked
                            && (self.hovered_prompt == Some(message_start)
                                || self.hovered_prompt_actions == Some(message_start)),
                        |d| d.child(self.prompt_actions(message_start, cx)),
                    );
                match photo_body {
                    // Photos and their bubble enter as one message.
                    Some(photos) => div()
                        .min_w_0()
                        .max_w_full()
                        .flex()
                        .flex_col()
                        .items_end()
                        .gap_1()
                        .child(photos)
                        .child(bubble)
                        .into_any_element(),
                    None => bubble.into_any_element(),
                }
            }
        };
        let body = if let Some((_, tail)) = row.thought_phase {
            div()
                .relative()
                .w_full()
                .pl(px(20.))
                .when(!tail || thought_header, |d| {
                    d.child(crate::activity::rail_branch(tail, thought_header))
                })
                .child(body)
                .into_any_element()
        } else {
            body
        };
        let key = row.key;
        let receipt = matches!(self.rows[ix].body, Body::WorkLine { .. });
        let inside_tools = row.thought_phase.is_some_and(|(_, tail)| !tail || !last)
            || matches!(
                self.rows[ix].body,
                Body::Tool {
                    group: Some(_),
                    tail: false,
                    ..
                }
            );
        let receipt_open = match self.rows[ix].body {
            Body::WorkLine { turn, .. } => self.work_folds.get(&turn).copied().unwrap_or(false),
            _ => false,
        };
        let prose = matches!(self.rows[ix].body, Body::Markdown(..));
        let rail_branch = matches!(self.rows[ix].body, Body::Phase { .. })
            || (!nested_phase && (thought_header || (prose && first_prose)))
            || matches!(self.rows[ix].body, Body::Tool { group: None, .. });
        // Keep a phase header the same height whether its body is present or
        // leaving. The last tool carries the group's animated bottom padding.
        let card = matches!(self.rows[ix].body, Body::MonoCard(_));
        let (card_top, card_bottom) = if card && self.mono {
            self.mono_card_spacing(ix)
        } else if self
            .rows
            .get(ix + 1)
            .is_some_and(|r| matches!(r.body, Body::MonoCard(_) | Body::Launch(_)))
        {
            (4., 8.)
        } else {
            (4., 12.)
        };
        let bottom = if card {
            card_bottom
        } else if inside_tools || matches!(self.rows[ix].body, Body::Phase { .. }) {
            0.
        } else if receipt || (last && is_tool) {
            4.
        } else if last {
            // A prompt sits between two turns: match the room above it, so it
            // doesn't read as part of the reply below.
            8.
        } else {
            0.
        };
        let natural = div()
            .relative()
            .w_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .children(if first {
                self.mono_separator(message_start)
            } else {
                None
            })
            .children(if first {
                self.provider_separator(message_start)
            } else {
                None
            })
            .child(
                div()
                    .id(("row", ix))
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(COLUMN))
                            .min_w_0()
                            .px_4()
                            .pt(px(if card && self.mono {
                                card_top
                            } else if first && !is_tool && !prose {
                                if user {
                                    4.
                                } else if receipt && !self.mono {
                                    12.
                                } else {
                                    0.
                                }
                            } else {
                                0.
                            }))
                            .pb(px(if row.work {
                                0.
                            } else if self.mono && user {
                                // `pt-1 pb-5`, or `pb-0` when the next message stacks on it.
                                if last && !stacked { 20. } else { 0. }
                            } else if self.mono && receipt {
                                0.
                            } else {
                                bottom
                            }))
                            .text_size(px(14.))
                            .line_height(px(24.))
                            .when(user && self.chat_layout, |d| {
                                d.pl(px(56.)).flex().justify_end()
                            })
                            .when(user && self.mono, |d| d.pr(px(16.)))
                            .child(
                                div()
                                    .relative()
                                    .min_w_0()
                                    .when(user && self.chat_layout, |d| d.max_w(PROMPT_BUBBLE_MAX))
                                    .when_some(prompt_width, |d, width| d.w(width))
                                    .when(user && !self.chat_layout, |d| d.w_full())
                                    .when(!user, |d| d.w_full())
                                    .when(row.work, |d| {
                                        d.pl(px(20.)).pb(px(bottom)).when(
                                            !nested_phase || !row.work_tail,
                                            |d| {
                                                d.child(crate::activity::rail_branch(
                                                    row.work_tail,
                                                    rail_branch,
                                                ))
                                            },
                                        )
                                    })
                                    .when(receipt_open, |d| {
                                        d.child(
                                            div()
                                                .absolute()
                                                .left(px(6.))
                                                .top(px(22.))
                                                .bottom(px(-4.))
                                                .w(px(1.))
                                                .bg(rgb(0x323232)),
                                        )
                                    })
                                    .child(body),
                            ),
                    ),
            );
        let outer = div()
            .id(gpui::SharedString::from(format!(
                "motion-{}-{}",
                key.0, key.1
            )))
            .w_full()
            .flex()
            .flex_col();
        let mut opacity = 1.;
        let mut translate_y = 0.;
        let motion = self
            .motions
            .get(&key)
            .filter(|_| animate)
            .filter(|m| m.group.is_none_or(|group| !self.windowed_phase(group)))
            .map(|m| (m.value(self.frame_now), m.step));
        if let Some(((_, fade), step)) = motion {
            opacity = if step { 1. } else { fade };
        }
        if let Some((prompt, born)) = self.prompt_rise
            && message_start >= prompt
            && !cx.reduce_motion()
        {
            let elapsed = born.elapsed().as_secs_f32();
            if message_start == prompt {
                let rise = crate::reveal::bezier((elapsed / 0.56).min(1.), 0.22, 1., 0.36, 1.);
                let fade = crate::reveal::bezier((elapsed / 0.48).min(1.), 0., 0., 0.58, 1.);
                translate_y =
                    (f32::from(self.viewport.size.height) * 0.3 - TOP_INSET).max(0.) * (1. - rise);
                opacity *= fade;
            } else {
                let t = ((elapsed - 0.56) / 0.32).clamp(0., 1.);
                let reveal = crate::reveal::bezier(t, 0.22, 1., 0.36, 1.);
                translate_y = 10. * (1. - reveal);
                opacity *= reveal;
            }
        }
        if matches!(self.rows[ix].body, Body::Plain(_) | Body::Markdown(_))
            && let Some(born) = self.entrances.get(&message_start)
            && !cx.reduce_motion()
        {
            // The identity line and footer persist while only the message body
            // enters, matching React's data-chat-message boundary.
            // useBottomChatMotion's entrance: a sent message rises 16px over
            // 360ms, a reply 10px over 280ms.
            let (rise, duration) = if user { (16., 0.36) } else { (10., 0.28) };
            let t = crate::reveal::bezier(
                (born.elapsed().as_secs_f32() / duration).min(1.),
                0.22,
                1.,
                0.36,
                1.,
            );
            translate_y += rise * (1. - t);
            opacity *= t;
        }
        // ListState positions the layout root itself. Translate its child so
        // the motion survives prepaint_as_root without affecting row height.
        let natural = natural.relative().top(px(translate_y));
        let body = if let Some(((mut height, _), _)) = motion {
            if let Some(group) = self.motions[&key].group {
                height = self.phase_layouts[&group].row_progress(key, height);
            }
            Disclosure::new(natural, self.viewport.size.width, height).into_any_element()
        } else {
            natural.into_any_element()
        };
        outer
            .when(motion.is_some(), |d| d.overflow_hidden())
            .when(self.mono || user, |d| {
                d.on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    this.mono_hover(user, message_start, message_turn, key, *hovered, cx)
                }))
            })
            .opacity(opacity)
            .child(body)
            .into_any_element()
    }

    fn anchor_row(&self) -> Option<usize> {
        self.anchor_turn.and_then(|prompt| {
            if prompt == SAVED_DRAFT_START {
                self.rows
                    .iter()
                    .position(|row| matches!(row.body, Body::SavedDraft))
            } else {
                self.messages
                    .binary_search_by_key(&prompt, |m| m.start)
                    .ok()
                    .map(|message| self.messages[message].display_start)
            }
        })
    }
    // Only measure enough of the live turn to fill the viewport. Older history
    // stays virtualized, and a long reply needs no additional blank space.
    fn footer_height(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Pixels {
        let mut remaining = 0.;
        if let Some(first) = self.anchor_row() {
            remaining = (f32::from(self.viewport.size.height) - TOP_INSET - BOTTOM_INSET).max(0.);
            for ix in first..self.rows.len() {
                if remaining <= 0. {
                    break;
                }
                remaining -= f32::from(self.measure_row_height(ix, window, cx));
            }
        }
        px(BOTTOM_INSET + remaining.max(0.))
    }
    fn footer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .h(self.footer_height(window, cx))
            .w_full()
            .into_any_element()
    }
    fn update_jump(&mut self, cx: &mut Context<Self>) {
        let show =
            !self.list.is_following_tail() && self.list.max_offset_for_scrollbar().y > px(1.);
        if self.show_jump != show {
            self.show_jump = show;
            cx.notify();
        }
    }
    pub(crate) fn set_jump_beside_pill(&mut self, beside: bool, cx: &mut Context<Self>) {
        if self.jump_beside_pill != beside {
            self.jump_beside_pill = beside;
            cx.notify();
        }
    }
    pub(crate) fn jump_bounds(&self) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.show_jump.then(|| self.jump_anchor.get()).flatten()
    }
    fn jump_to_bottom(&mut self, cx: &mut Context<Self>) {
        self.follow = Follow::default();
        self.list.set_follow_mode(FollowMode::Tail);
        self.list.scroll_to_end();
        self.show_jump = false;
        cx.notify();
    }
    fn toggle(&mut self, start: usize, cx: &mut Context<Self>) {
        if !self.expanded.remove(&start) {
            self.expanded.insert(start);
        }
        self.list.pause_following_tail();
        self.rebuild(start, cx);
    }
    /// "Show more" opens a message in place, without a transition.
    fn toggle_prompt(&mut self, start: usize, cx: &mut Context<Self>) {
        if !self.expanded_prompts.remove(&start) {
            self.expanded_prompts.insert(start);
        }
        self.list.pause_following_tail();
        self.rebuild(start, cx);
    }
    /// Wrapped lines of a user message's text at the bubble's text width.
    fn prompt_lines(&self, text: &str, window: &Window) -> usize {
        let column = self.viewport.size.width.min(px(COLUMN));
        // Row padding (`pl-14 pr-4` in the chat layout, `px-4` otherwise),
        // the bubble's 36rem cap and its `px-3`.
        let bubble = if self.chat_layout {
            (column - px(72.)).min(PROMPT_BUBBLE_MAX)
        } else {
            column - px(32.)
        };
        let text = SharedString::from(text.trim_end().to_owned());
        measurements::shape_prompt(text, Some(bubble - PROMPT_PAD_X), window)
            .map(|line| line.wrap_boundaries.len() + 1)
            .sum()
    }
    fn toggle_phase(&mut self, group: usize, open: bool, cx: &mut Context<Self>) {
        self.phase_toggle = Some(group);
        self.phases.insert(group, open);
        self.list.pause_following_tail();
        self.rebuild(group, cx);
    }
}
impl gpui::EventEmitter<ResponseRequest> for TranscriptView {}
impl Render for TranscriptView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let top = self.list.logical_scroll_top();
        // Rows that don't fill the viewport can't scroll, so their top being
        // on screen asks for older turns too. Layout must have run first.
        let short = top.item_ix == 0
            && top.offset_in_item <= px(0.)
            && !self.history_failed
            && (self.history_start > 0 || self.history_before.is_some())
            && self.list.bounds_for_item(0).is_some();
        if (std::mem::take(&mut self.history_requested)
            && !self.list.is_following_tail()
            && top.item_ix <= 2)
            || short
        {
            self.load_older_history(cx);
        }
        if self.selection_focus.is_none() {
            self.selection_focus = Some(cx.focus_handle());
        }
        self.selection.begin_frame();
        self.phase_max_height = live_phase::max_height(window);
        self.frame_now = Instant::now();
        for reveal in self.reveals.values_mut() {
            reveal.fades.advance(self.frame_now);
        }
        self.markdown_cache
            .begin_frame(&self.file_link_base, &self.search_match);
        self.begin_measurements(window, cx);
        if !self.scroll_bound {
            self.scroll_bound = true;
            self.list
                // The list holds its state borrow while delivering the event.
                // Read the final scroll geometry in our trailing layout probe.
                .set_scroll_handler(cx.listener(|this, _, _, cx| {
                    this.selection.clear();
                    this.history_requested =
                        this.history_start > 0 || this.history_before.is_some();
                    cx.notify()
                }));
        }
        self.update_follow(window, cx);
        let now = Instant::now();
        if self.prompt_rise_pending {
            self.prompt_rise_pending = false;
            if let Some((_, born)) = self.prompt_rise.as_mut() {
                *born = now;
            }
        }
        self.rendered = 0;
        self.render_passes += 1;
        let view = cx.entity().downgrade();
        let after = cx.entity().downgrade();
        div()
            .id("transcript")
            .key_context("TranscriptSelection")
            .track_focus(self.selection_focus.as_ref().unwrap())
            .on_action(cx.listener(Self::copy_selection))
            .on_action(cx.listener(Self::open_link_menu))
            .capture_any_mouse_down(cx.listener(
                |this, event: &gpui::MouseDownEvent, window, cx| {
                    if event.button == gpui::MouseButton::Right && this.links.open.is_none() {
                        window.focus(this.selection_focus.as_ref().unwrap(), cx);
                    }
                },
            ))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.selection.clear_if_not_dragging();
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && this.selection.active() {
                    this.selection.clear();
                    cx.notify();
                    cx.stop_propagation()
                }
            }))
            .on_mouse_move(cx.listener(Self::move_selection))
            .on_mouse_up(gpui::MouseButton::Left, cx.listener(Self::finish_selection))
            .on_mouse_up_out(gpui::MouseButton::Left, cx.listener(Self::finish_selection))
            .role(Role::Log)
            .aria_label(if self.document {
                "Note body"
            } else {
                "Conversation transcript"
            })
            .relative()
            .size_full()
            .overflow_hidden()
            .child(
                canvas(
                    move |bounds, window, cx| {
                        let _ = view.update(cx, |this, cx| {
                            if this.viewport != bounds {
                                this.selection.clear();
                                this.viewport = bounds;
                                this.list
                                    .remeasure_items(this.footer_ix()..this.footer_ix() + 1);
                            }
                            let resized = this.update_follow_viewport(bounds, window);
                            // Refresh the row-height baseline after a real resize even
                            // when the chat is idle, ready for the next send.
                            this.schedule_frame(resized, window, cx);
                        });
                    },
                    |bounds, _, window, _| paint_edge_fade(bounds, true, window),
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(
                list(
                    self.list.clone(),
                    cx.processor(|this, ix: usize, window, cx| {
                        if ix == this.footer_ix() {
                            this.footer(window, cx)
                        } else {
                            let row = this.row(ix, window, cx);
                            this.reflow.wrap(this.rows[ix].key, row, this.follow.offset)
                        }
                    }),
                )
                .pt(px(TOP_INSET))
                .size_full(),
            )
            .child(
                canvas(
                    move |_, _, cx| {
                        let _ = after.update(cx, |this, cx| {
                            this.update_jump(cx);
                            this.update_outline(cx);
                        });
                    },
                    |bounds, _, window, _| paint_edge_fade(bounds, false, window),
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .children(self.outline(window, cx))
            .children(self.selection_menu(cx))
            .child(self.link_overlays(cx))
            .children(self.work_item_card(window, cx))
            .when(self.show_jump, |d| {
                // Centered, or the column's right end when the commit pill
                // takes the left.
                let beside_pill = self.jump_beside_pill;
                d.child(
                    div()
                        .absolute()
                        .bottom(px(8.))
                        .left_0()
                        .w_full()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .w_full()
                                .max_w(px(crate::theme::COLUMN))
                                .px(px(16.))
                                .flex()
                                .map(|d| {
                                    if beside_pill {
                                        d.justify_end()
                                    } else {
                                        d.justify_center()
                                    }
                                })
                                .child(
                                    div()
                                        .id("jump-to-bottom")
                                        .child(self.jump_anchor.probe())
                                        .role(Role::Button)
                                        .aria_label("Scroll to bottom")
                                        .relative()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .size(px(30.))
                                        .rounded_full()
                                        .border_1()
                                        .border_color(content(0.1))
                                        .text_color(content(0.7))
                                        .hover(|s| s.text_color(content(1.)).bg(content(0.04)))
                                        .child(floating_surface())
                                        .child(icon("chevron-down", 16.))
                                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.jump_to_bottom(cx)),
                                        ),
                                ),
                        ),
                )
            })
    }
}

/// The frosted fill behind the round controls floating over the transcript.
pub(crate) fn floating_surface() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |bounds, _, window, _| {
            let radius = bounds.size.height / 2.;
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            window.paint_quad(
                gpui::fill(bounds, content(0.04))
                    .corner_radii(radius)
                    .border_color(gpui::backdrop_blur_marker(12. * window.scale_factor())),
            );
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            window.paint_quad(gpui::fill(bounds, rgb(CHAT)).corner_radii(radius));
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

fn paint_edge_fade(bounds: Bounds<Pixels>, begin: bool, window: &mut Window) {
    #[cfg(target_os = "macos")]
    window.paint_quad(gpui::fill(bounds, gpui::black()).border_color(
        gpui_apple::metal_renderer::edge_fade_marker(
            begin,
            TOP_FADE * window.scale_factor(),
            BOTTOM_INSET * window.scale_factor(),
        ),
    ));
    // A color overlay cannot mask text without tinting the pane and creating
    // a seam above the composer. Only the Metal backend supports a real mask.
    #[cfg(not(target_os = "macos"))]
    let _ = (bounds, begin, window);
}

#[cfg(test)]
mod scroll_tests {
    use super::*;
    use gpui::{TestAppContext, WindowBounds, WindowOptions, point};

    fn row(role: &str, text: &str) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first: true,
            sent_at: None,
        }
    }
    fn settle(view: &mut TranscriptView, cx: &mut Context<TranscriptView>) {
        for reveal in view.reveals.values_mut() {
            reveal.settle(Instant::now());
        }
        view.prompt_rise = None;
        view.motions.clear();
        view.rebuild(0, cx);
    }

    #[gpui::test]
    fn overlapping_stream_updates_rebuild_once(cx: &mut TestAppContext) {
        let view = cx.new(|_| TranscriptView::new());
        let reference = cx.new(|_| TranscriptView::new());
        for entity in [&view, &reference] {
            entity.update(cx, |view, cx| {
                view.visible(false, cx);
                view.streaming(true, cx);
                view.patch(0, vec![row("user", "Prompt"), row("assistant", "a")], cx);
            });
        }
        let patches = vec![
            (1, vec![row("assistant", "ab")]),
            (1, vec![row("assistant", "abc"), row("assistant", "final")]),
            (0, vec![row("user", "Updated prompt")]),
        ];
        reference.update(cx, |view, cx| {
            for (start, rows) in patches.clone() {
                view.patch(start, rows, cx);
            }
            view.streaming(false, cx);
        });
        view.update(cx, |view, cx| {
            let before = view.rebuild_passes;
            view.begin_update_batch();
            for (start, rows) in patches {
                view.patch(start, rows, cx);
            }
            view.streaming(false, cx);
            assert_eq!(view.rebuild_passes, before);
            view.end_update_batch(cx);
            assert_eq!(view.rebuild_passes, before + 1);
            assert_eq!(view.source[1].text, "abc");
            assert_eq!(view.source[2].text, "final");
        });
        cx.update(|cx| {
            let actual = view.read(cx);
            let reference = reference.read(cx);
            assert_eq!(
                actual.rows.len(),
                reference.rows.len(),
                "batch={:?} sequential={:?}",
                actual
                    .rows
                    .iter()
                    .map(|r| (r.key, std::mem::discriminant(&r.body)))
                    .collect::<Vec<_>>(),
                reference
                    .rows
                    .iter()
                    .map(|r| (r.key, std::mem::discriminant(&r.body)))
                    .collect::<Vec<_>>()
            );
            assert!(
                actual
                    .rows
                    .iter()
                    .zip(&reference.rows)
                    .all(|(a, b)| a.key == b.key
                        && match (&a.body, &b.body) {
                            (Body::Plain(a), Body::Plain(b)) => a == b,
                            (Body::Markdown(a), Body::Markdown(b)) => a == b,
                            (
                                Body::WorkLine {
                                    turn: a,
                                    expandable: x,
                                },
                                Body::WorkLine {
                                    turn: b,
                                    expandable: y,
                                },
                            ) => a == b && x == y,
                            (
                                Body::Actions {
                                    turn: a,
                                    complete: x,
                                },
                                Body::Actions {
                                    turn: b,
                                    complete: y,
                                },
                            ) => a == b && x == y,
                            _ => false,
                        })
            );
        });
    }

    #[gpui::test]
    fn agent_panel_index_tracks_replaced_tool_rows(cx: &mut TestAppContext) {
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            let call = ToolCall::new("agent-1", "agent");
            view.patch(
                0,
                vec![
                    row("user", "Prompt"),
                    row("tool", &serde_json::to_string(&call).unwrap()),
                ],
                cx,
            );
            assert!(view.agent_panel_rows.contains(&1));
            let call = ToolCall::new("read-1", "read_file");
            view.patch(
                1,
                vec![row("tool", &serde_json::to_string(&call).unwrap())],
                cx,
            );
            assert!(view.agent_panel_rows.is_empty());
        });
    }

    #[gpui::test]
    #[ignore]
    fn long_tool_turn_rebuild_timing(cx: &mut TestAppContext) {
        let mut source = vec![row("user", "Inspect the files")];
        source.extend((0..1_999).map(|i| {
            let mut call = ToolCall::new(&i.to_string(), "read_file");
            call.output = "file contents\n".repeat(8);
            call.state = monocode_core::tool::ToolState::Completed;
            row("tool", &serde_json::to_string(&call).unwrap())
        }));
        for trial in 0..5 {
            let view = cx.new(|_| TranscriptView::new());
            view.update(cx, |view, cx| view.patch(0, source.clone(), cx));
            let start = Instant::now();
            for _ in 0..20 {
                view.update(cx, |view, cx| view.rebuild(1_999, cx));
            }
            let elapsed = start.elapsed();
            view.update(cx, |view, _| {
                assert_eq!(view.source.len(), 2_000);
                assert!(!view.rows.is_empty());
                assert!(view.messages.iter().any(|message| message.role == "tool"));
            });
            eprintln!(
                "tool-turn trial={trial} rebuilds=20 elapsed_ms={:.3}",
                elapsed.as_secs_f64() * 1000.
            );
        }
    }

    #[gpui::test]
    fn expanded_user_chunks_paint_one_aligned_bubble(cx: &mut TestAppContext) {
        // Compare settled bubble geometry rather than per-row resize glides.
        cx.update(|cx| cx.set_reduce_motion(true));
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(900.), px(1000.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| TranscriptView::new()),
            )
            .unwrap()
        });
        // Newline splitting produces three rows, with the widest text in the
        // middle. Check both natural-width and capped, wrapped messages.
        for (wide, width) in [
            ("a medium length line".to_owned(), 900.),
            ("wide text ".repeat(20), 900.),
            ("wide text ".repeat(20), 400.),
        ] {
            cx.simulate_window_resize(window.into(), size(px(width), px(1000.)));
            let mut transcript = monocode_core::transcript::Transcript::default();
            transcript.begin("user");
            transcript.append(&format!(
                "{}\n{wide}\n{}",
                "x\n".repeat(12),
                "y\n".repeat(12)
            ));
            assert!(transcript.rows.len() >= 3);
            window
                .update(cx, |view, _, cx| {
                    view.patch(0, transcript.rows, cx);
                    if !view.expanded_prompts.contains(&0) {
                        view.toggle_prompt(0, cx);
                    }
                    settle(view, cx);
                })
                .unwrap();
            // The viewport probe invalidates retained list heights after a
            // resize; draw again to inspect the resulting geometry.
            for _ in 0..2 {
                window.update(cx, |_, _, cx| cx.notify()).unwrap();
                cx.refresh().unwrap();
                cx.run_until_parked();
            }
            window
                .update(cx, |view, window, _| {
                    let mut bubbles: Vec<_> = window
                        .painted_quads()
                        .into_iter()
                        .filter(|quad| quad.background == user_bubble_fill().into())
                        .collect();
                    bubbles.sort_by(|a, b| a.bounds.top().0.total_cmp(&b.bounds.top().0));
                    assert_eq!(bubbles.len(), view.rows.len());
                    assert!(bubbles.len() >= 3);
                    let first = &bubbles[0];
                    let last = bubbles.last().unwrap();
                    assert!(first.corner_radii.top_left.0 > 0.);
                    assert!(last.corner_radii.bottom_left.0 > 0.);
                    for pair in bubbles.windows(2) {
                        let (a, b) = (&pair[0], &pair[1]);
                        assert_eq!(a.bounds.left(), b.bounds.left(), "shared left edge");
                        assert_eq!(a.bounds.size.width, b.bounds.size.width, "shared width");
                        assert_eq!(a.bounds.bottom(), b.bounds.top(), "no gaps between chunks");
                        assert_eq!(a.corner_radii.bottom_left.0, 0.);
                        assert_eq!(b.corner_radii.top_left.0, 0.);
                    }
                    assert!(first.bounds.size.width.0 / window.scale_factor() <= 576.);
                })
                .unwrap();
        }
        window
            .update(cx, |view, _, cx| {
                // A source patch updates rows in place; it does not truncate
                // the previous prompt's continuation rows.
                *view = TranscriptView::new();
                view.patch(0, vec![row("user", "Hi")], cx);
                settle(view, cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        window
            .update(cx, |_, window, _| {
                let bubble = window
                    .painted_quads()
                    .into_iter()
                    .find(|quad| quad.background == user_bubble_fill().into())
                    .unwrap();
                assert!(
                    bubble.bounds.size.width.0 / window.scale_factor() < 96.,
                    "short messages still shrink to their text"
                );
                assert_eq!(
                    bubble.corner_radii.top_left.0 * 2.,
                    bubble.bounds.size.height.0
                );
                assert_eq!(
                    bubble.corner_radii.top_left,
                    bubble.corner_radii.bottom_left
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn settled_tab_switches_preserve_rows_layout_and_scroll(cx: &mut TestAppContext) {
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            view.patch(
                0,
                vec![
                    row("user", "Question"),
                    row("assistant", "A settled **answer**."),
                ],
                cx,
            );
            settle(view, cx);
            view.list.pause_following_tail();
            view.list.scroll_to(ListOffset {
                item_ix: 1,
                offset_in_item: px(7.),
            });
            let position = view.list.logical_scroll_top();
            let rows = view.rows.as_ptr();
            let rebuilds = view.rebuild_passes;
            view.phase_layouts.insert(
                999,
                PhaseLayout {
                    width: px(600.),
                    height: 42.,
                    rows: HashMap::new(),
                },
            );
            for _ in 0..20 {
                view.visible(false, cx);
                view.animate_frame(cx);
                view.visible(true, cx);
            }
            assert_eq!(view.rebuild_passes, rebuilds);
            assert_eq!(view.rows.as_ptr(), rows);
            assert!(view.phase_layouts.contains_key(&999));
            let restored = view.list.logical_scroll_top();
            assert_eq!(restored.item_ix, position.item_ix);
            assert_eq!(restored.offset_in_item, position.offset_in_item);
            assert!(!view.list.is_following_tail());
        });
    }

    #[gpui::test]
    fn exit_rows_survive_a_patch_that_merges_messages(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(false));
        let window = cx.update(|cx| {
            cx.open_window(WindowOptions::default(), |_, cx| {
                cx.new(|_| TranscriptView::new())
            })
            .unwrap()
        });
        window
            .update(cx, |view, _, cx| {
                view.patch(
                    0,
                    vec![row("user", "Question"), row("assistant", "Answer")],
                    cx,
                );
                settle(view, cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                // The second source row now continues the first message, so
                // the answer's row exits while only one message remains.
                assert!(view.rows.iter().any(|r| r.key.0 == 1));
                let mut continuation = row("assistant", " continued");
                continuation.first = false;
                view.patch(0, vec![row("assistant", "Answer"), continuation], cx);
                assert_eq!(view.messages.len(), 1);
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                assert!(view.rows.iter().all(|r| r.message < view.messages.len()));
            })
            .unwrap();
    }

    #[gpui::test]
    fn hiding_settles_live_text_and_showing_reuses_hidden_updates(cx: &mut TestAppContext) {
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            view.streaming = true;
            view.live_start = 1;
            view.patch(
                0,
                vec![
                    row("user", "Question"),
                    row("assistant", "The complete live answer has arrived."),
                ],
                cx,
            );
            // Reproduce a response still catching up to its received text.
            view.reveals.insert(
                1,
                PacedText::new(
                    "The complete live answer has arrived.",
                    true,
                    Instant::now(),
                ),
            );
            assert!(view.reveals[&1].behind());
            let before = view.rebuild_passes;
            view.visible(false, cx);
            assert_eq!(view.rebuild_passes, before + 1);
            assert!(!view.reveals[&1].behind());
            view.patch(1, vec![row("assistant", "Updated while hidden.")], cx);
            let updated = view.rebuild_passes;
            view.visible(true, cx);
            assert_eq!(view.rebuild_passes, updated);
            assert!(
                view.rows
                    .iter()
                    .any(|row| matches!(&row.body, Body::Markdown(md)
                if crate::markdown::selection_text(md).contains("Updated while hidden.")))
            );
        });
    }

    #[test]
    fn saved_work_line_uses_turn_provenance_and_never_invents_legacy_model_or_duration() {
        assert_eq!(
            work_title(Some("Saved Model"), Some(9500), false),
            "Saved Model worked for 10s"
        );
        assert_eq!(work_title(None, Some(60000), false), "Worked for 1m");
        assert_eq!(work_title(None, None, false), "Worked");
        assert_eq!(work_title(Some(""), None, false), "Worked");
        assert_eq!(
            work_title(Some("Saved Model"), Some(0), false),
            "Saved Model worked for 1s"
        );
        assert_eq!(
            work_title(Some("Live Model"), None, true),
            "Live Model working…"
        );
    }

    #[gpui::test]
    fn fresh_follow_ups_keep_provider_metrics_and_timing_on_the_original_turn(
        cx: &mut TestAppContext,
    ) {
        let view = cx.new(|_| TranscriptView::new());
        view.update(cx, |view, cx| {
            view.identity("codex", "Chosen Model");
            view.streaming(true, cx);
            view.patch(
                0,
                vec![row("user", "Prompt"), row("assistant", "Working")],
                cx,
            );
            view.record_provider_turn("provider-turn".into());
            view.patch(
                2,
                vec![row("user", "Also this"), row("assistant", "Done")],
                cx,
            );
            view.metrics(monocode_core::agent::TurnMetrics {
                output_tokens: 20,
                ..Default::default()
            });
            view.streaming(false, cx);
            assert_eq!(view.turns[&1].model, "Chosen Model");
            assert_eq!(
                view.turns[&1].provider_turn_id.as_deref(),
                Some("provider-turn")
            );
            assert_eq!(view.turns[&1].metrics.unwrap().output_tokens, 20);
            assert!(view.turns[&1].has_duration());
            assert!(view.turns[&1].completed_at.is_some());
            assert_eq!(view.turns[&3].model, "Chosen Model");
            assert!(
                !view.turns[&3].has_duration(),
                "a steer has provenance but no separate run duration"
            );
            assert!(view.turns[&3].metrics.is_none());
        });
    }

    #[test]
    fn phase_reveals_one_clipped_body_and_keeps_visible_steps_at_their_natural_height() {
        let layout = PhaseLayout {
            width: px(600.),
            height: 100.,
            rows: HashMap::from([
                ((1, 2), (0., 20.)),
                ((2, 2), (20., 50.)),
                ((3, 2), (70., 30.)),
            ]),
        };
        assert_eq!(layout.row_progress((1, 2), 0.5), 1.);
        assert_eq!(layout.row_progress((2, 2), 0.5), 0.6);
        assert_eq!(layout.row_progress((3, 2), 0.5), 0.);
        for progress in [0., 0.2, 0.5, 0.9, 1.] {
            let visible: f32 = layout
                .rows
                .iter()
                .map(|(key, (_, height))| height * layout.row_progress(*key, progress))
                .sum();
            assert!((visible - layout.height * progress).abs() < 0.001);
        }
    }

    #[gpui::test]
    fn clicking_a_phase_uses_one_reveal_and_rapid_toggles_preserve_its_progress(
        cx: &mut TestAppContext,
    ) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|_| TranscriptView::new())
            })
            .unwrap()
        });
        window
            .update(cx, |view, _, cx| {
                let tool = |id: &str| {
                    row(
                        "tool",
                        &serde_json::to_string(&ToolCall {
                            id: id.into(),
                            name: "Read".into(),
                            input: format!(r#"{{"file_path":"{id}.rs"}}"#),
                            output: "tool output".into(),
                            state: ToolState::Completed,
                            panel: None,
                            display: None,
                        })
                        .unwrap(),
                    )
                };
                view.patch(
                    0,
                    vec![
                        row("user", "Check"),
                        tool("one"),
                        tool("two"),
                        row("assistant", "Done"),
                    ],
                    cx,
                );
                view.work_folds.insert(1, true);
                view.rebuild(1, cx);
                view.motions.clear();
                view.toggle_phase(1, true, cx);
                let motions: Vec<_> = view
                    .rows
                    .iter()
                    .filter(|r| r.in_phase())
                    .map(|r| view.motions[&r.key].clone())
                    .collect();
                assert_eq!(motions.len(), 2, "the two compact steps share the body");
                for motion in &motions {
                    assert_eq!(motion.group, Some(1));
                    assert_eq!(motion.start, motions[0].start);
                    assert!(!motion.step);
                }
                view.toggle_phase(1, false, cx);
                assert!(
                    view.rows.iter().any(|r| r.in_phase()),
                    "closing retains the body through its exit"
                );
                assert!(
                    view.motions
                        .values()
                        .all(|m| m.group == Some(1) && !m.opening)
                );
                view.toggle_phase(1, true, cx);
                assert!(
                    view.motions
                        .values()
                        .all(|m| m.group == Some(1) && m.opening)
                );
                assert_eq!(view.phase_toggle, None);
                // Hold a partial reveal long enough to inspect actual list geometry.
                let start = Instant::now() - std::time::Duration::from_secs(25);
                for motion in view.motions.values_mut() {
                    *motion = Motion::phase(1, true, start);
                    motion.duration = std::time::Duration::from_secs(100);
                }
                view.visible = false;
                cx.notify();
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        window.update(cx, |view, _, _| {
            let layout = &view.phase_layouts[&1];
            let steps: Vec<_> = view.rows.iter().enumerate().filter(|(_, r)| r.in_phase()).collect();
            let (first_ix, first) = steps.first().unwrap();
            let (last_ix, last) = steps.last().unwrap();
            let first_bounds = view.list.bounds_for_item(*first_ix).unwrap();
            let last_bounds = view.list.bounds_for_item(*last_ix).unwrap();
            assert!((f32::from(first_bounds.size.height) - layout.rows[&first.key].1).abs() < 0.1,
                "a revealed row must retain its natural height, rather than shrink with the group");
            assert!(f32::from(last_bounds.size.height) < layout.rows[&last.key].1,
                "the reveal clips the bottom of the body");
        }).unwrap();
    }

    #[gpui::test]
    fn closing_a_phase_keeps_its_header_and_final_frame_in_place(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|_| TranscriptView::new())
            })
            .unwrap()
        });
        for width in [900., 320.] {
            cx.simulate_window_resize(window.into(), size(px(width), px(1400.)));
            window
                .update(cx, |view, _, cx| {
                    view.visible = false;
                    let mut source = vec![
                        row("user", "Check the collapse"),
                        row("assistant", "I will inspect the tools."),
                    ];
                    source.extend((0..5).map(|ix| {
                        row(
                            "tool",
                            &serde_json::to_string(&ToolCall {
                                id: ix.to_string(),
                                name: "Read".into(),
                                input: format!(r#"{{"file_path":"file-{ix}.rs"}}"#),
                                output: "A tool result that can wrap at a narrower width. "
                                    .repeat(3),
                                state: ToolState::Completed,
                                panel: None,
                                display: None,
                            })
                            .unwrap(),
                        )
                    }));
                    source.push(row("assistant", "The tools have finished."));
                    source.push(row("assistant", "Here is the final answer."));
                    view.patch(0, source, cx);
                    view.work_folds.insert(1, true);
                    view.phases.insert(2, true);
                    view.expanded.insert(3);
                    view.rebuild(0, cx);
                    view.list.pause_following_tail();
                    view.list.scroll_to(ListOffset {
                        item_ix: 0,
                        offset_in_item: px(0.),
                    });
                })
                .unwrap();
            cx.refresh().unwrap();
            let header = window
                .update(cx, |view, _, cx| {
                    assert_eq!(view.viewport.size.width, px(width));
                    let ix = view
                        .rows
                        .iter()
                        .position(|r| matches!(r.body, Body::Phase { group: 2, .. }))
                        .unwrap();
                    let header = view.list.bounds_for_item(ix).unwrap();
                    view.visible = true;
                    view.toggle_phase(2, false, cx);
                    view.visible = false;
                    header
                })
                .unwrap();
            cx.refresh().unwrap();
            let mut previous = window
                .update(cx, |view, _, _| {
                    let ix = view
                        .rows
                        .iter()
                        .position(|r| matches!(r.body, Body::Phase { group: 2, .. }))
                        .unwrap();
                    assert_eq!(
                        view.list.bounds_for_item(ix).unwrap(),
                        header,
                        "closing the body must not move or resize its header"
                    );
                    (0..view.rows.len())
                        .map(|ix| view.list.bounds_for_item(ix).unwrap())
                        .collect::<Vec<_>>()
                })
                .unwrap();
            for elapsed_ms in [40, 160, 220, 280, 310, 319, 320] {
                window
                    .update(cx, |view, _, cx| {
                        let start = Instant::now() - std::time::Duration::from_millis(elapsed_ms);
                        for motion in view.motions.values_mut() {
                            motion.start = start;
                        }
                        view.list.remeasure_items(0..view.rows.len() + 1);
                        cx.notify();
                    })
                    .unwrap();
                cx.refresh().unwrap();
                previous = window.update(cx, |view, _, _| {
                    let current: Vec<_> = (0..view.rows.len())
                        .map(|ix| view.list.bounds_for_item(ix).unwrap()).collect();
                    for (before, after) in previous.iter().zip(&current) {
                        assert!(after.top() <= before.top() + px(0.1),
                            "closing must not shift rows down at {elapsed_ms}ms: {before:?} vs {after:?}");
                    }
                    current
                }).unwrap();
            }
            let finishing = window
                .update(cx, |view, _, cx| {
                    let finishing: Vec<_> = view
                        .rows
                        .iter()
                        .enumerate()
                        .filter(|(_, r)| !r.in_phase())
                        .map(|(ix, r)| (r.key, view.list.bounds_for_item(ix).unwrap()))
                        .collect();
                    // Hidden transcripts intentionally ignore queued frames.
                    // Drive this final visible frame explicitly for the test.
                    view.visible = true;
                    view.animate_frame(cx);
                    view.visible = false;
                    assert!(!view.rows.iter().any(|r| r.in_phase()));
                    finishing
                })
                .unwrap();
            cx.refresh().unwrap();
            window
                .update(cx, |view, _, _| {
                    for (key, before) in finishing {
                        let ix = view.rows.iter().position(|r| r.key == key).unwrap();
                        assert_eq!(
                            view.list.bounds_for_item(ix).unwrap(),
                            before,
                            "removing the closed rows must not nudge the transcript"
                        );
                    }
                })
                .unwrap();
        }
    }

    #[gpui::test]
    fn work_and_phase_disclosures_finish_at_their_natural_height_after_resizing(
        cx: &mut TestAppContext,
    ) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|_| TranscriptView::new())
            })
            .unwrap()
        });
        window
            .update(cx, |view, _, cx| {
                let tool = |id: &str| {
                    row(
                        "tool",
                        &serde_json::to_string(&ToolCall {
                            id: id.into(),
                            name: "Read".into(),
                            input: format!(r#"{{"file_path":"{id}.rs"}}"#),
                            output: String::new(),
                            state: ToolState::Completed,
                            panel: None,
                            display: None,
                        })
                        .unwrap(),
                    )
                };
                view.patch(
                    0,
                    vec![
                        row("user", "Check the layout"),
                        row(
                            "assistant",
                            &"Earlier commentary that wraps onto several lines. ".repeat(8),
                        ),
                        tool("first"),
                        tool("second"),
                        row(
                            "assistant",
                            "The final answer stays below the work.\n\nIts spacing stays fixed.",
                        ),
                    ],
                    cx,
                );
                view.list.pause_following_tail();
            })
            .unwrap();
        // Exercise first opening, nested tools and reopening at a new width,
        // without relying on any previously painted height.
        for width in [900., 320., 640.] {
            window
                .update(cx, |view, w, cx| {
                    w.resize(size(px(width), px(1200.)));
                    view.visible = true;
                    view.work_folds.insert(1, true);
                    view.phases.insert(2, true);
                    view.rebuild(1, cx);
                    view.motions.clear();
                    for row in view.rows.iter().filter(|row| row.work) {
                        view.motions.insert(
                            row.key,
                            Motion::new(
                                true,
                                false,
                                Instant::now() - std::time::Duration::from_secs(1),
                            ),
                        );
                    }
                    // Retain the finishing frame until its geometry is inspected.
                    view.visible = false;
                    view.list.scroll_to(ListOffset {
                        item_ix: 0,
                        offset_in_item: px(0.),
                    });
                    cx.notify();
                })
                .unwrap();
            cx.refresh().unwrap();
            cx.run_until_parked();
            let finishing = window
                .update(cx, |view, _, cx| {
                    let bounds: Vec<_> = (0..view.rows.len())
                        .map(|ix| view.list.bounds_for_item(ix).unwrap())
                        .collect();
                    view.motions.clear();
                    view.list.remeasure_items(0..view.rows.len() + 1);
                    cx.notify();
                    bounds
                })
                .unwrap();
            cx.refresh().unwrap();
            cx.run_until_parked();
            window.update(cx, |view, _, cx| {
                for (ix, before) in finishing.iter().enumerate() {
                    let after = view.list.bounds_for_item(ix).unwrap();
                    assert!((before.top() - after.top()).abs() < px(0.1));
                    assert!((before.size.height - after.size.height).abs() < px(0.1),
                        "finishing a disclosure must not move the content below it: {before:?} vs {after:?}");
                }
                assert!(view.action_anchors.contains_key(&(1, "copy")));
                assert!(view.action_anchors.contains_key(&(1, "note")));
                assert!(!view.action_anchors.contains_key(&(1, "activity")));
                view.work_folds.insert(1, false);
                view.rebuild(1, cx);
            }).unwrap();
        }
    }

    #[gpui::test]
    fn mono_turn_actions_stay_while_moving_between_its_rows_in_any_event_order(
        cx: &mut TestAppContext,
    ) {
        let view = cx.new(|_| {
            let mut view = TranscriptView::new();
            view.mono_mode("cat", 0xe86b5f, "MonoCat");
            view
        });
        view.update(cx, |view, cx| {
            view.patch(
                0,
                vec![row("user", "Hi"), row("assistant", "Hello there")],
                cx,
            );
            // The next row reports its enter before the last one leaves.
            view.mono_hover(false, 1, 1, (1, 16), true, cx);
            view.mono_hover(false, 1, 1, (1, usize::MAX - 1), true, cx);
            view.mono_hover(false, 1, 1, (1, 16), false, cx);
            assert_eq!(view.hovered_turn.map(|(t, _)| t), Some(1));
            view.mono_hover(false, 1, 1, (1, usize::MAX - 1), false, cx);
            assert!(view.hovered_turn.is_none());
        });
    }

    #[gpui::test]
    fn mono_messages_stack_past_a_reaction_and_queued_bubbles_hand_over(cx: &mut TestAppContext) {
        let view = cx.new(|_| {
            let mut view = TranscriptView::new();
            view.mono_mode("cat", 0xe86b5f, "MonoCat");
            view
        });
        view.update(cx, |view, cx| {
            view.patch(
                0,
                vec![
                    row("user", "nice"),
                    row("assistant", "<reaction>👍</reaction>"),
                    row("user", "what else"),
                ],
                cx,
            );
            assert!(
                view.stacks_on_next(0),
                "a reaction-only reply keeps the stack"
            );
            assert!(!view.stacks_on_next(2));
            view.set_pending(
                vec![PendingMessage {
                    id: "q".into(),
                    text: "Queued".into(),
                    error: None,
                    sent_at: None,
                    attachments: Vec::new(),
                }],
                cx,
            );
            assert!(matches!(view.rows.last().unwrap().body, Body::Pending(0)));
            let born = view.pending_entrances["q"];
            view.streaming(true, cx);
            view.patch(3, vec![row("user", "Queued")], cx);
            view.set_pending(Vec::new(), cx);
            assert_eq!(
                view.entrances.get(&3),
                Some(&born),
                "delivery continues the same entrance without restarting it"
            );
            assert!(!view.rows.iter().any(|r| matches!(r.body, Body::Pending(_))));
            assert_eq!(view.list.item_count(), view.rows.len() + 1);
        });
    }

    #[gpui::test]
    fn mono_turns_merge_follow_ups_hide_live_prose_and_queue_above_the_work(
        cx: &mut TestAppContext,
    ) {
        let view = cx.new(|_| {
            let mut view = TranscriptView::new();
            view.mono_mode("cat", 0xe86b5f, "MonoCat");
            view
        });
        let start = serde_json::to_string(&ToolCall {
            id: "t".into(),
            name: "exec_command".into(),
            input: r#"{"cmd":"'/Applications/Monocode Native.app/Contents/MacOS/monocode-gpui' app sessions.start --json '{}'"}"#.into(),
            output: r#"{"ok":true,"result":{"sessionId":776,"accepted":true}}"#.into(),
            state: ToolState::Completed,
            panel: None,
            display: None,
        })
        .unwrap();
        view.update(cx, |view, cx| {
            view.patch(
                0,
                vec![
                    row("user", "Start a Luna session"),
                    row("assistant", "I'll start a Luna session."),
                    row("tool", &start),
                    row("assistant", "Started session 776."),
                    row("mono-system", "Session 776 finished"),
                    row("assistant", "Luna replied."),
                ],
                cx,
            );
            let headers = view
                .rows
                .iter()
                .filter(|r| matches!(r.body, Body::WorkLine { .. }))
                .count();
            let actions: Vec<_> = view
                .rows
                .iter()
                .filter_map(|r| match r.body {
                    Body::Actions { turn, .. } => Some(turn),
                    _ => None,
                })
                .collect();
            assert_eq!(
                headers, 1,
                "the follow-up continues the turn under one header"
            );
            assert_eq!(
                actions,
                vec![1],
                "one set of actions, at the end of the group"
            );
            assert!(
                !view.mono_prose_hidden(1),
                "the opening sentence shows before the work"
            );
            assert!(!view.mono_prose_hidden(3));
            assert!(
                view.rows
                    .iter()
                    .any(|r| matches!(r.body, Body::Launch(776))),
                "the accepted launch gets its card"
            );
            view.streaming(true, cx);
            assert!(
                view.rows
                    .iter()
                    .any(|r| matches!(r.body, Body::Launch(776))),
                "starting the next provider run must keep the existing launch card"
            );
            view.patch(6, vec![row("user", "And another thing")], cx);
            view.patch(7, vec![row("assistant", "Working on it")], cx);
            assert!(
                view.mono_prose_hidden(7),
                "a Mono message appears whole, not while it streams"
            );
            view.patch(8, vec![row("tool", &start)], cx);
            assert!(
                !view.mono_prose_hidden(7),
                "the finished opening sentence shows while the Mono works"
            );
            view.set_pending(
                vec![PendingMessage {
                    id: "q".into(),
                    text: "One more".into(),
                    error: None,
                    sent_at: None,
                    attachments: Vec::new(),
                }],
                cx,
            );
            let pending = view
                .rows
                .iter()
                .position(|r| matches!(r.body, Body::Pending(0)))
                .unwrap();
            let header = view
                .rows
                .iter()
                .rposition(|r| matches!(r.body, Body::WorkLine { .. }))
                .unwrap();
            assert!(
                pending > header,
                "a message queued after the Mono spoke follows its words"
            );
        });
    }

    #[gpui::test]
    fn desktop_work_metadata_renders_once_after_the_reply_and_never_as_prose(
        cx: &mut TestAppContext,
    ) {
        let view = cx.new(|_| {
            let mut view = TranscriptView::new();
            view.mono_mode("cat", 0xe86b5f, "MonoCat");
            view
        });
        view.update(cx, |view,cx| {
            view.patch(0,vec![
                row("user","Write a report"),
                row("mono-document",r#"{"id":"artifact:report","type":"document","documentId":"report","title":"Report","kind":"html"}"#),
                row("mono-launch",r#"{"sessionId":7,"title":"Review","cwd":"/repo","harness":"codex"}"#),
                row("assistant","Written and checked."),
            ],cx);
            assert_eq!(view.rows.iter().filter(|r| matches!(r.body,Body::MonoCard(_))).count(),1);
            assert_eq!(view.rows.iter().filter(|r| matches!(r.body,Body::Launch(7))).count(),1);
            assert!(!view.rows.iter().any(|r| matches!(&r.body,Body::Plain(text) if text.contains("documentId") || text.contains("sessionId"))));
            assert!(!view.rows.iter().any(|r| r.work));
            let answer=view.rows.iter().position(|r| matches!(r.body,Body::Markdown(_))).unwrap();
            let document=view.rows.iter().position(|r| matches!(r.body,Body::MonoCard(_))).unwrap();
            assert!(document > answer);
        });
    }

    #[gpui::test]
    fn mono_document_and_session_cards_are_one_line_like_react(cx: &mut TestAppContext) {
        cx.update(|cx| cx.set_reduce_motion(true));
        let start = serde_json::to_string(&ToolCall {
            id: "t".into(),
            name: "Bash".into(),
            input: r#"{"command":"monocode app sessions.start --json {}"}"#.into(),
            output: r#"{"ok":true,"result":{"sessionId":7,"accepted":true}}"#.into(),
            state: ToolState::Completed,
            panel: None,
            display: None,
        })
        .unwrap();
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(900.), px(600.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|cx| {
                        let mut view = TranscriptView::new();
                        view.mono_mode("cat", 0xe86b5f, "MonoCat");
                        view.patch(
                            0,
                            vec![
                                row("user", "Start one and write it up"),
                                row("tool", &start),
                                row("assistant", "Started it."),
                                row(
                                    "mono-card",
                                    r#"{"id":"c","type":"document","documentId":"d","title":"Test Document"}"#,
                                ),
                            ],
                            cx,
                        );
                        view.set_launches(
                            HashMap::from([(
                                7,
                                LaunchView {
                                    title: "Fix the composer".into(),
                                    state: "working",
                                    harness: "codex".into(),
                                    cwd: "/tmp/project".into(),
                                },
                            )]),
                            cx,
                        );
                        view
                    })
                },
            )
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let document = visual.debug_bounds("mono-artifact-card").unwrap();
        let session = visual.debug_bounds("mono-launch-card-7").unwrap();
        // ArtifactCard: min-h-[38px] and w-fit; LaunchedSessionCard: py-1.5
        // around its project chip, both at most 24rem wide.
        assert_eq!(document.size.height, px(38.));
        assert!(document.size.width < px(384.));
        assert_eq!(session.size.height, px(40.));
        assert!(session.size.width <= px(384.));
    }

    #[gpui::test]
    fn mono_chat_rests_on_the_composer_and_slides_sent_messages_in(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(900.), px(600.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|_| {
                        let mut view = TranscriptView::new();
                        view.mono_mode("cat", 0xe86b5f, "MonoCat");
                        view
                    })
                },
            )
            .unwrap()
        });
        let bubble_top = |w: &mut Window| {
            w.painted_quads()
                .into_iter()
                .filter(|q| {
                    q.corner_radii.top_left.0 > 0.
                        && q.corner_radii.top_left.0 * 2. == q.bounds.size.height.0
                })
                .map(|q| q.bounds.top().0 / w.scale_factor())
                .fold(None, |low: Option<f32>, top| {
                    Some(low.map_or(top, |l| l.max(top)))
                })
                .expect("single-line bubble is fully rounded")
        };
        // Loaded history neither animates nor starts at the top.
        window
            .update(cx, |view, _, cx| {
                view.patch(0, vec![row("user", "Earlier")], cx)
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, w, _| {
                assert!(view.entrances.is_empty(), "history must not slide in");
                assert!(
                    bubble_top(w) > 400.,
                    "a short Mono chat sits at the bottom, like justify-end"
                );
            })
            .unwrap();
        window
            .update(cx, |view, _, cx| {
                view.patch(
                    0,
                    vec![
                        row("user", "Hello"),
                        row("assistant", "<reaction>👍</reaction>"),
                    ],
                    cx,
                );
                view.streaming(true, cx);
                view.patch(2, vec![row("user", "A sent line")], cx);
                assert!(view.entrances.contains_key(&2), "the sent message enters");
                assert_eq!(view.reactions.get(&0).map(|r| r.0.as_str()), Some("👍"));
                view.entrances
                    .insert(2, Instant::now() - std::time::Duration::from_millis(10));
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        let entering = window
            .update(cx, |view, w, _| {
                assert!(
                    view.follow.offset > 0.,
                    "the chat glides up to the sent message instead of jumping"
                );
                bubble_top(w)
            })
            .unwrap();
        window
            .update(cx, |view, _, cx| {
                view.entrances.clear();
                cx.notify();
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        let settled = window.update(cx, |_, w, _| bubble_top(w)).unwrap();
        assert!(
            entering > settled + 10.,
            "a sent message rises into place ({entering} vs {settled})"
        );
    }

    #[gpui::test]
    fn prompt_rise_moves_the_painted_bubble_without_moving_its_scroll_anchor(
        cx: &mut TestAppContext,
    ) {
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(900.), px(600.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| TranscriptView::new()),
            )
            .unwrap()
        });
        window
            .update(cx, |view, _, cx| {
                view.streaming(true, cx);
                view.patch(
                    0,
                    vec![row("user", "A single line"), row("assistant", "")],
                    cx,
                );
                view.streaming(false, cx);
                view.prompt_rise_pending = false;
                view.prompt_rise = Some((0, Instant::now() - std::time::Duration::from_millis(40)));
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        let (rising_y, scroll_anchor) = window
            .update(cx, |view, w, cx| {
                let bubble = w
                    .painted_quads()
                    .into_iter()
                    .find(|q| {
                        q.corner_radii.top_left.0 > 0.
                            && q.corner_radii.top_left.0 * 2. == q.bounds.size.height.0
                    })
                    .expect("single-line bubble is fully rounded");
                assert_eq!(
                    bubble.bounds.size.height.0 / w.scale_factor(),
                    36.,
                    "React's 20px line height plus 8px padding above and below"
                );
                let position = bubble.bounds.top().0 / w.scale_factor();
                let anchor = view.list.logical_scroll_top();
                view.prompt_rise =
                    Some((0, Instant::now() - std::time::Duration::from_millis(900)));
                cx.notify();
                (position, anchor)
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, w, _| {
                let bubble = w
                    .painted_quads()
                    .into_iter()
                    .find(|q| {
                        q.corner_radii.top_left.0 > 0.
                            && q.corner_radii.top_left.0 * 2. == q.bounds.size.height.0
                    })
                    .unwrap();
                let settled_y = bubble.bounds.top().0 / w.scale_factor();
                assert!(
                    rising_y > settled_y + 30.,
                    "the painted prompt must visibly rise, rather than only fade"
                );
                assert_eq!(
                    view.list.logical_scroll_top().item_ix,
                    scroll_anchor.item_ix
                );
                assert_eq!(
                    view.list.logical_scroll_top().offset_in_item,
                    scroll_anchor.offset_in_item
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn steered_prompts_take_the_top_anchor_without_restarting_the_provider_turn(
        cx: &mut TestAppContext,
    ) {
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(900.), px(600.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| TranscriptView::new()),
            )
            .unwrap()
        });
        window
            .update(cx, |view, _, cx| {
                view.patch(
                    0,
                    vec![
                        row("user", "Earlier question"),
                        row("assistant", "Earlier answer"),
                    ],
                    cx,
                );
                view.streaming(true, cx);
                view.patch(
                    2,
                    vec![row("user", "Start working"), row("assistant", "Working")],
                    cx,
                );
                settle(view, cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();

        for start in [4, 7] {
            window
                .update(cx, |view, _, cx| {
                    view.list.scroll_by(px(-100.));
                    view.update_jump(cx);
                    assert!(!view.list.is_following_tail());
                    let mut continuation = row("user", " and its continuation");
                    continuation.first = false;
                    view.patch(
                        start,
                        vec![
                            row("user", "Change direction"),
                            continuation,
                            row("assistant", "Following the steer"),
                        ],
                        cx,
                    );
                    assert_eq!(
                        view.live_start, 2,
                        "steering keeps the provider turn running"
                    );
                    assert_eq!(view.anchor_turn, Some(start));
                    assert_eq!(view.prompt_rise.map(|(at, _)| at), Some(start));
                    assert!(view.list.is_following_tail());
                    assert!(!view.show_jump);
                    settle(view, cx);
                })
                .unwrap();
            cx.refresh().unwrap();
            cx.run_until_parked();
            window
                .update(cx, |view, window, cx| {
                    let prompt = view.messages.iter().find(|m| m.start == start).unwrap();
                    let bounds = view.list.bounds_for_item(prompt.display_start).unwrap();
                    assert!(
                        (bounds.top() - view.viewport.top()).abs() < px(1.),
                        "each accepted steer must replace the previous prompt's top anchor"
                    );
                    window.resize(size(px(700.), px(760.)));
                    cx.notify();
                })
                .unwrap();
            cx.refresh().unwrap();
            cx.run_until_parked();
        }

        window
            .update(cx, |view, _, cx| {
                let prompt = view.messages.iter().find(|m| m.start == 7).unwrap();
                let bounds = view.list.bounds_for_item(prompt.display_start).unwrap();
                assert!((bounds.top() - view.viewport.top()).abs() < px(1.));
                view.list.scroll_by(px(-100.));
                let offset = view.list.logical_scroll_top();
                view.patch(0, view.source.clone(), cx);
                assert!(
                    !view.list.is_following_tail(),
                    "a replay must respect the reader's scroll"
                );
                assert_eq!(view.list.logical_scroll_top().item_ix, offset.item_ix);
                assert_eq!(
                    view.list.logical_scroll_top().offset_in_item,
                    offset.offset_in_item
                );
                assert!(
                    view.prompt_rise.is_none(),
                    "a replay must not repeat the entrance"
                );
            })
            .unwrap();
    }

    #[gpui::test]
    fn live_prompt_anchors_respect_history_visibility_and_the_anchor_preference(
        cx: &mut TestAppContext,
    ) {
        let entity = cx.new(|_| TranscriptView::new());
        entity.update(cx, |view, cx| {
            view.patch(0, vec![row("user", "Restored history")], cx);
            assert!(view.anchor_turn.is_none());
            assert!(view.prompt_rise.is_none());
            view.streaming(true, cx);
            view.visible(false, cx);
            view.patch(1, vec![row("user", "A steer while hidden")], cx);
            assert_eq!(view.anchor_turn, Some(1));
            assert!(view.prompt_rise.is_none());
            view.visible(true, cx);
            view.patch(1, vec![row("user", "A steer while hidden")], cx);
            assert!(
                view.prompt_rise.is_none(),
                "showing a tab must not replay a steer"
            );

            view.apply_preferences(
                &crate::settings::Preferences {
                    anchor_prompts: false,
                    ..Default::default()
                },
                cx,
            );
            view.list.pause_following_tail();
            view.patch(2, vec![row("user", "A steer with anchoring disabled")], cx);
            assert!(view.anchor_turn.is_none());
            assert!(view.prompt_rise.is_none());
            assert!(
                view.list.is_following_tail(),
                "a newly sent message still resumes following"
            );
        });
    }

    #[gpui::test]
    fn sent_prompt_stays_at_the_top_then_long_replies_follow_until_scrolled_away(
        cx: &mut TestAppContext,
    ) {
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(900.), px(600.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| TranscriptView::new()),
            )
            .unwrap()
        });
        window
            .update(cx, |view, _, cx| {
                view.patch(
                    0,
                    vec![
                        row("user", "Earlier question"),
                        row("assistant", "Earlier answer"),
                    ],
                    cx,
                );
                view.streaming(true, cx);
                view.patch(
                    2,
                    vec![
                        row("user", "The newly sent prompt"),
                        row("assistant", "A short reply"),
                    ],
                    cx,
                );
                view.streaming(false, cx);
                settle(view, cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, w, cx| {
                #[cfg(target_os = "macos")]
                {
                    let markers: Vec<_> = w
                        .painted_quads()
                        .into_iter()
                        .filter(|q| {
                            q.border_color
                                == gpui_apple::metal_renderer::edge_fade_marker(
                                    true,
                                    TOP_FADE * w.scale_factor(),
                                    BOTTOM_INSET * w.scale_factor(),
                                )
                                || q.border_color
                                    == gpui_apple::metal_renderer::edge_fade_marker(
                                        false,
                                        TOP_FADE * w.scale_factor(),
                                        BOTTOM_INSET * w.scale_factor(),
                                    )
                        })
                        .collect();
                    assert_eq!(
                        markers.len(),
                        2,
                        "both mask boundaries must be painted inside the viewport"
                    );
                    assert_eq!(markers[0].bounds, markers[1].bounds);
                }
                let prompt = view
                    .rows
                    .iter()
                    .position(|r| view.messages[r.message].start == 2)
                    .unwrap();
                let bounds = view.list.bounds_for_item(prompt).unwrap();
                assert!(
                    (bounds.top() - view.viewport.top()).abs() < px(1.),
                    "prompt row must stay at the top, below the list's fade padding"
                );
                assert!(view.list.is_following_tail());
                w.resize(size(px(700.), px(760.)));
                cx.notify();
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                let prompt = view
                    .rows
                    .iter()
                    .position(|r| view.messages[r.message].start == 2)
                    .unwrap();
                assert!(
                    (view.list.bounds_for_item(prompt).unwrap().top() - view.viewport.top()).abs()
                        < px(1.),
                    "resizing must retain the anchored prompt"
                );
                view.patch(
                    3,
                    vec![row(
                        "assistant",
                        &"A paragraph of streamed reply text.\n\n".repeat(80),
                    )],
                    cx,
                );
                settle(view, cx);
            })
            .unwrap();
        cx.refresh().unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                assert!(view.list.is_following_tail());
                view.list.scroll_by(px(-150.));
                view.update_jump(cx);
                assert!(view.show_jump);
                let offset = view.list.logical_scroll_top();
                view.patch(
                    3,
                    vec![row(
                        "assistant",
                        &"A paragraph of streamed reply text.\n\n".repeat(90),
                    )],
                    cx,
                );
                settle(view, cx);
                assert_eq!(view.list.logical_scroll_top().item_ix, offset.item_ix);
                assert_eq!(
                    view.list.logical_scroll_top().offset_in_item,
                    offset.offset_in_item
                );
                view.jump_to_bottom(cx);
                assert!(!view.show_jump && view.list.is_following_tail());
            })
            .unwrap();
    }
}
