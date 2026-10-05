//! Port of `ChatPage` in SettingsView.tsx.

use gpui::{Context, IntoElement, ParentElement as _, Render, Styled as _, Task, Window, div};
use monocode_core::appearance::{TRANSCRIPT_ANCHOR_KEY, TranscriptLayout};
use monocode_core::settings::{DiffViewer, FollowUpBehavior, ModelControls};
use monocode_settings::Subscription;
use monocode_settings::settings_store as ss;
use monocode_settings::storage_flags::read_flag;

use super::chrome::{group, row};
use super::controls::{segmented, toggle, watch_keys};
use super::section::SectionContext;
use super::store;

pub struct ChatSection {
    ctx: SectionContext,
    pub transcript_layout: TranscriptLayout,
    pub transcript_anchor: bool,
    pub follow_up_behavior: FollowUpBehavior,
    pub model_controls: ModelControls,
    pub diff_viewer: DiffViewer,
    pub format_on_save: bool,
    pub composer_runner: bool,
    pub grid_arcade_enabled: bool,
    _watch: (Vec<Subscription>, Task<()>),
}

impl ChatSection {
    pub fn new(ctx: SectionContext, cx: &mut Context<Self>) -> Self {
        let kv = &ctx.kv;
        let appearance = store::load_appearance(kv, ctx.platform);
        // `TRANSCRIPT_ANCHOR_CHANGE_EVENT`: the transcript can flip it too.
        let watch = watch_keys(
            kv,
            &[TRANSCRIPT_ANCHOR_KEY],
            |this: &mut Self, cx| {
                this.transcript_anchor = read_flag(&this.ctx.kv, TRANSCRIPT_ANCHOR_KEY)
                    .unwrap_or(monocode_core::appearance::TRANSCRIPT_ANCHOR_DEFAULT);
                cx.notify();
            },
            cx,
        );
        Self {
            transcript_layout: appearance.transcript_layout,
            transcript_anchor: appearance.transcript_anchor,
            follow_up_behavior: ss::load_follow_up_behavior(kv),
            model_controls: ss::load_model_controls(kv),
            diff_viewer: ss::load_diff_viewer(kv),
            format_on_save: ss::load_format_on_save(kv),
            composer_runner: ss::load_composer_runner(kv),
            grid_arcade_enabled: ss::load_grid_arcade_enabled(kv),
            _watch: watch,
            ctx,
        }
    }
}

impl Render for ChatSection {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reveal = self.ctx.reveal(cx);
        let transcript = group(&reveal, "Transcript")
            .first(true)
            .description("How a conversation reads as it grows.")
            .child(
                row(&reveal, "Transcript layout")
                    .id("transcript-layout")
                    .description("Full width keeps user prompts as a spanning card. Chat aligns them to the right with a max width, like a messaging app.")
                    .child(
                        segmented(
                            "Transcript layout",
                            self.transcript_layout.as_str(),
                            [("full", "Full width"), ("chat", "Chat")],
                        )
                        .on_change(cx.listener(|this, value: &str, _, cx| {
                            let next = TranscriptLayout::parse(Some(value));
                            store::save_transcript_layout(&this.ctx.kv, next);
                            this.transcript_layout = next;
                            cx.notify();
                        })),
                    ),
            )
            .child(
                row(&reveal, "Anchor prompts to top")
                    .id("anchor-prompts")
                    .description("When you send, the new prompt sits at the top of the transcript and the reply grows into the space below. Turn this off to keep the classic layout, with the latest message resting on the composer.")
                    .switch_only()
                    .child(
                        toggle("Anchor prompts to top", self.transcript_anchor).on_change(
                            cx.listener(|this, next: &bool, _, cx| {
                                store::save_transcript_anchor(&this.ctx.kv, *next);
                                this.transcript_anchor = *next;
                                cx.notify();
                            }),
                        ),
                    ),
            );

        let composer = group(&reveal, "Composer")
            .description("What the composer does with what you type.")
            .child(
                row(&reveal, "Follow-up behavior")
                    .id("follow-up")
                    .description("Queue follow-ups until the active turn finishes, or steer the active turn immediately.")
                    .child(
                        segmented(
                            "Follow-up behavior",
                            self.follow_up_behavior.as_str(),
                            [("queue", "Queue"), ("steer", "Steer")],
                        )
                        .on_change(cx.listener(|this, value: &str, _, cx| {
                            let next = FollowUpBehavior::parse(Some(value));
                            ss::save_follow_up_behavior(&this.ctx.kv, next);
                            this.follow_up_behavior = next;
                            cx.notify();
                        })),
                    ),
            )
            .child(
                row(&reveal, "Model controls")
                    .id("model-controls")
                    .description("Show model options beside the picker instead of inside the model menu.")
                    .child(
                        segmented(
                            "Model controls",
                            self.model_controls.as_str(),
                            [("menu", "Menu"), ("beside", "Beside")],
                        )
                        .on_change(cx.listener(|this, value: &str, _, cx| {
                            let next = ModelControls::parse(Some(value));
                            ss::save_model_controls(&this.ctx.kv, next);
                            this.model_controls = next;
                            cx.notify();
                        })),
                    ),
            );

        let editor = group(&reveal, "Editor")
            .description("What happens when you save a file in the workspace editor.")
            .child(
                row(&reveal, "Format on save")
                    .id("format-on-save")
                    .description("Run Prettier on supported files before writing. Off keeps the text you typed, including quote style.")
                    .switch_only()
                    .child(toggle("Format on save", self.format_on_save).on_change(cx.listener(
                        |this, next: &bool, _, cx| {
                            ss::save_format_on_save(&this.ctx.kv, *next);
                            this.format_on_save = *next;
                            cx.notify();
                        },
                    ))),
            );

        let review = group(&reveal, "Code review")
            .description("Where a turn's changes open when you go to read them.")
            .child(
                row(&reveal, "Diff view")
                    .id("diff-view")
                    .description("Editor keeps working-tree changes in the file. Unified stacks every changed file in one review, with sticky headers and collapsed unchanged lines.")
                    .child(
                        segmented(
                            "Diff view",
                            self.diff_viewer.as_str(),
                            [("editor", "Editor"), ("unified", "Unified")],
                        )
                        .on_change(cx.listener(|this, value: &str, _, cx| {
                            let next = DiffViewer::parse(Some(value));
                            ss::save_diff_viewer(&this.ctx.kv, next);
                            this.diff_viewer = next;
                            cx.notify();
                        })),
                    ),
            );

        let extras = group(&reveal, "Extras")
            .description("Idle animation, and nothing else. Turn both off for a still workspace.")
            .child(
                row(&reveal, "Composer mascot")
                    .id("composer-mascot")
                    .description("When a turn is running, the project mascot runs along the composer, bonks the scroll-to-latest button the first time, then jumps it, and sometimes grabs a coin.")
                    .switch_only()
                    .child(toggle("Composer mascot", self.composer_runner).on_change(cx.listener(
                        |this, next: &bool, _, cx| {
                            ss::save_composer_runner(&this.ctx.kv, *next);
                            this.composer_runner = *next;
                            cx.notify();
                        },
                    ))),
            )
            .child(
                row(&reveal, "Empty session games")
                    .id("empty-session-games")
                    .description("Pac-man and snake idle on the empty-session grid. Hover the band to take control of whichever is on screen. Turn this off to keep the pane still.")
                    .switch_only()
                    .child(
                        toggle("Empty session games", self.grid_arcade_enabled).on_change(
                            cx.listener(|this, next: &bool, _, cx| {
                                ss::save_grid_arcade_enabled(&this.ctx.kv, *next);
                                this.grid_arcade_enabled = *next;
                                cx.notify();
                            }),
                        ),
                    ),
            );

        div()
            .flex()
            .flex_col()
            .child(transcript)
            .child(composer)
            .child(editor)
            .child(review)
            .child(extras)
    }
}
