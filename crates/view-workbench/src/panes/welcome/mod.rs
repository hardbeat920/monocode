//! The model welcome screens: a decorative layer over a session when the
//! user picks Opus 5.5 or Astra. Ports of src/features/sessions/ui/
//! OpusWelcome.tsx and AstraWelcome.tsx with their CSS, and of
//! src/features/sessions/model/opusWelcome.ts and astraWelcome.ts.
//!
//! The CSS keyframes become painters that draw one frame for an elapsed
//! time. [`ModelWelcome`] runs the clock, asks for animation frames, and
//! reports [`ModelWelcomeEvent::Done`] when the scene ends, as `onDone` did.

mod astra;
mod opus;
pub mod paint;

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use gpui::{
    Context, EventEmitter, IntoElement, ParentElement as _, Render, Styled as _, Task, Window,
    canvas, div,
};
use monocode_core::models::AgentModel;
use monocode_ui::Theme;
use regex::Regex;

pub use astra::ASTRA_DURATION_MS;
pub use opus::OPUS_DURATION_MS;

static OPUS_55: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(^|[^a-z0-9])opus[\s-]?5[.-]5").expect("valid regex"));
static ASTRA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(^|[^a-z0-9])astra([^a-z0-9]|$)").expect("valid regex"));

fn model_names(model: &AgentModel) -> impl Iterator<Item = &str> {
    [
        Some(model.id.as_str()),
        model.native_id.as_deref(),
        Some(model.name.as_str()),
    ]
    .into_iter()
    .flatten()
}

/// `isOpus55Model`: matches Opus 5.5 across catalog ids (`opus-5-5`), names
/// (`Opus 5.5`), and dated native ids. The TypeScript pattern ended in the
/// lookahead `(?![0-9.])`, which the regex crate lacks, so the character
/// after each match is checked by hand.
pub fn is_opus55_model(model: &AgentModel) -> bool {
    model_names(model).any(|value| {
        OPUS_55.find_iter(value).any(|found| {
            !value[found.end()..]
                .chars()
                .next()
                .is_some_and(|next| next.is_ascii_digit() || next == '.')
        })
    })
}

/// `isAstraModel`.
pub fn is_astra_model(model: &AgentModel) -> bool {
    model_names(model).any(|value| ASTRA.is_match(value))
}

/// Which welcome plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WelcomeKind {
    Opus,
    Astra,
}

/// The welcome for a newly picked model: Astra first, then Opus 5.5, as
/// SessionPane checked them.
pub fn welcome_kind(model: &AgentModel) -> Option<WelcomeKind> {
    if is_astra_model(model) {
        Some(WelcomeKind::Astra)
    } else if is_opus55_model(model) {
        Some(WelcomeKind::Opus)
    } else {
        None
    }
}

const MIN_STAGE_BELOW: f32 = 140.0;
const MAX_STAGE_ABOVE: f32 = 240.0;

/// The composer's top and bottom edges, relative to the pane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComposerBand {
    pub top: f32,
    pub bottom: f32,
}

/// `opusStage`: the pane-relative band for the Opus scene, as (top,
/// height). It is the free space under a centered composer, or a band just
/// above a docked one that leaves no room below.
pub fn opus_stage(pane_height: f32, composer: Option<ComposerBand>) -> (f32, f32) {
    let Some(composer) = composer else {
        let top = (pane_height * 0.55).round();
        return (top, pane_height - top);
    };
    let below = pane_height - composer.bottom;
    if below >= MIN_STAGE_BELOW {
        return (composer.bottom, below);
    }
    let height = 0.0f32.max(MAX_STAGE_ABOVE.min(composer.top));
    (composer.top - height, height)
}

/// What the welcome reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelWelcomeEvent {
    /// `onDone`: the scene ended, or reduced motion skipped it.
    Done,
}

/// A welcome scene confined to its session pane. It intercepts no input:
/// render it as the pane's last child, absolutely positioned over it.
pub struct ModelWelcome {
    kind: WelcomeKind,
    started: Instant,
    frozen: Option<Duration>,
    composer: Option<ComposerBand>,
    done: bool,
    _timer: Task<()>,
}

impl EventEmitter<ModelWelcomeEvent> for ModelWelcome {}

impl ModelWelcome {
    pub fn new(kind: WelcomeKind, cx: &mut Context<Self>) -> Self {
        let duration = if cx.reduce_motion() {
            Duration::ZERO
        } else {
            Self::duration(kind)
        };
        let timer = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            this.update(cx, |this, cx| this.finish(cx)).ok();
        });
        Self {
            kind,
            started: Instant::now(),
            frozen: None,
            composer: None,
            done: false,
            _timer: timer,
        }
    }

    /// The scene's length: 7s for Opus, 7.6s for Astra.
    pub fn duration(kind: WelcomeKind) -> Duration {
        let ms = match kind {
            WelcomeKind::Opus => OPUS_DURATION_MS,
            WelcomeKind::Astra => ASTRA_DURATION_MS,
        };
        Duration::from_millis(ms as u64)
    }

    pub fn kind(&self) -> WelcomeKind {
        self.kind
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Where the composer sits in the pane, which places the Opus staff.
    pub fn set_composer_band(&mut self, band: Option<ComposerBand>, cx: &mut Context<Self>) {
        if self.composer != band {
            self.composer = band;
            cx.notify();
        }
    }

    /// Holds the scene at one moment, for screenshots.
    pub fn freeze_at(&mut self, elapsed: Duration, cx: &mut Context<Self>) {
        self.frozen = Some(elapsed);
        self._timer = Task::ready(());
        cx.notify();
    }

    fn finish(&mut self, cx: &mut Context<Self>) {
        if self.done {
            return;
        }
        self.done = true;
        cx.emit(ModelWelcomeEvent::Done);
        cx.notify();
    }

    fn elapsed_ms(&self) -> f32 {
        let elapsed = self.frozen.unwrap_or_else(|| self.started.elapsed());
        elapsed.as_secs_f32() * 1000.0
    }
}

impl Render for ModelWelcome {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let layer = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .overflow_hidden();
        if self.done || cx.reduce_motion() {
            return layer;
        }
        let elapsed = self.elapsed_ms();
        if self.frozen.is_none() {
            window.request_animation_frame();
        }
        let light = !Theme::of(cx).is_dark();
        let kind = self.kind;
        let composer = self.composer;
        layer.child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| match kind {
                    WelcomeKind::Opus => opus::paint(window, bounds, composer, elapsed, light),
                    WelcomeKind::Astra => astra::paint(window, bounds, elapsed, light),
                },
            )
            .size_full(),
        )
    }
}

#[cfg(test)]
mod tests {
    use monocode_core::HarnessId;

    use super::*;

    fn opus() -> AgentModel {
        let mut model = AgentModel::new("claude:opus-5-5", HarnessId::Claude, "Claude Opus 5.5");
        model.native_id = Some("claude-opus-5-5".into());
        model
    }

    fn with(id: &str, name: &str, native_id: Option<&str>) -> AgentModel {
        let mut model = opus();
        model.id = id.into();
        model.name = name.into();
        model.native_id = native_id.map(str::to_string);
        model
    }

    #[test]
    fn recognizes_opus_55_in_catalog_names_and_ids_without_matching_its_siblings() {
        assert!(is_opus55_model(&opus()));
        assert!(is_opus55_model(&with(
            "pi:new",
            "New",
            Some("anthropic/claude-opus-5-5-20260901")
        )));
        assert!(is_opus55_model(&with("opencode:x", "Opus 5.5", None)));
        assert!(!is_opus55_model(&with(
            "claude:opus-5",
            "Claude Opus 5",
            Some("claude-opus-5")
        )));
        assert!(!is_opus55_model(&with(
            "claude:opus-5-6",
            "Opus 5.6",
            Some("claude-opus-5-6")
        )));
        assert!(!is_opus55_model(&with(
            "claude:opus-5-55",
            "Opus 5.55",
            Some("claude-opus-5-55")
        )));
    }

    #[test]
    fn stages_the_scene_under_a_centered_composer_or_above_a_docked_one() {
        let band = |top, bottom| Some(ComposerBand { top, bottom });
        assert_eq!(opus_stage(800.0, band(360.0, 480.0)), (480.0, 320.0));
        assert_eq!(opus_stage(800.0, band(680.0, 780.0)), (440.0, 240.0));
        assert_eq!(opus_stage(200.0, band(100.0, 190.0)), (0.0, 100.0));
        assert_eq!(opus_stage(800.0, None), (440.0, 360.0));
    }

    #[test]
    fn recognizes_astra_in_catalog_names_and_ids_without_matching_unrelated_names() {
        let astra = AgentModel::new("codex:gpt-6-astra", HarnessId::Codex, "Astra");
        assert!(is_astra_model(&astra));
        let mut renamed = astra.clone();
        renamed.id = "codex:new".into();
        renamed.name = "ASTRA".into();
        assert!(is_astra_model(&renamed));
        let mut by_id = astra.clone();
        by_id.name = "New model".into();
        assert!(is_astra_model(&by_id));
        let mut native = astra.clone();
        native.id = "pi:new".into();
        native.name = "New".into();
        native.native_id = Some("openai/astra".into());
        assert!(is_astra_model(&native));
        let mut astral = astra.clone();
        astral.id = "pi:astral".into();
        astral.name = "Astral".into();
        assert!(!is_astra_model(&astral));
    }

    #[test]
    fn astra_wins_over_opus() {
        assert_eq!(welcome_kind(&opus()), Some(WelcomeKind::Opus));
        let both = with("x:astra", "Opus 5.5", None);
        assert_eq!(welcome_kind(&both), Some(WelcomeKind::Astra));
        assert_eq!(welcome_kind(&with("x:y", "Sonnet", None)), None);
    }
}
