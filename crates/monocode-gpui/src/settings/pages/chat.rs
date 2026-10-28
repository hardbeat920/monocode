//! ChatPage.
use crate::settings::SettingsView;
use gpui::{AnyElement, Context};

impl SettingsView {
    pub(super) fn chat_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = self.prefs.clone();
        let transcript = vec![
            self.row(
                Some("transcript-layout"),
                true,
                "Transcript layout",
                Some("Show prompts at full width or as right-aligned chat bubbles.".into()),
                vec![self.segmented(
                    "transcript-layout",
                    "Transcript layout",
                    &p.transcript_layout,
                    &[("full", "Full width"), ("chat", "Chat")],
                    cx,
                    |this, v, cx| this.update(cx, |p| p.transcript_layout = v.into()),
                )],
            ),
            self.row(
                Some("anchor-prompts"),
                false,
                "Anchor prompts to top",
                Some("Place new prompts at the top, with replies below.".into()),
                vec![self.toggle(
                    "anchor-prompts",
                    "Anchor prompts to top",
                    p.anchor_prompts,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.anchor_prompts = on),
                )],
            ),
            self.row(
                Some("paced-reveal"),
                false,
                "Smooth text reveal",
                Some("Reveal incoming text gradually.".into()),
                vec![self.toggle(
                    "paced-reveal",
                    "Smooth text reveal",
                    p.paced_reveal,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.paced_reveal = on),
                )],
            ),
        ];
        let composer = vec![
            self.row(
                Some("follow-up"),
                true,
                "Follow-up behavior",
                Some("Queue follow-ups until the active turn finishes, or steer the active turn immediately.".into()),
                vec![self.segmented(
                    "follow-up",
                    "Follow-up behavior",
                    &p.follow_up,
                    &[("queue", "Queue"), ("steer", "Steer")],
                    cx,
                    |this, v, cx| this.update(cx, |p| p.follow_up = v.into()),
                )],
            ),
            self.row(
                Some("model-controls"),
                false,
                "Model controls",
                Some("Show model options beside the picker instead of inside the model menu.".into()),
                vec![self.segmented(
                    "model-controls",
                    "Model controls",
                    &p.model_controls,
                    &[("menu", "Menu"), ("beside", "Beside")],
                    cx,
                    |this, v, cx| this.update(cx, |p| p.model_controls = v.into()),
                )],
            ),
            self.row(
                Some("composer-autocorrect"),
                false,
                "Autocorrect",
                Some("Spell check and autocorrect prompts.".into()),
                vec![self.toggle(
                    "composer-autocorrect",
                    "Autocorrect",
                    p.autocorrect,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.autocorrect = on),
                )],
            ),
        ];
        let editor = vec![self.row(
            Some("format-on-save"),
            true,
            "Format on save",
            Some("Format supported files with Prettier before saving.".into()),
            vec![self.toggle(
                "format-on-save",
                "Format on save",
                p.format_on_save,
                false,
                cx,
                |this, on, cx| this.update(cx, |p| p.format_on_save = on),
            )],
        )];
        let review = vec![self.row(
            Some("diff-view"),
            true,
            "Diff view",
            Some("Review changes in the editor or in a combined diff.".into()),
            vec![self.segmented(
                "diff-view",
                "Diff view",
                &p.diff_view,
                &[("editor", "Editor"), ("unified", "Unified")],
                cx,
                |this, v, cx| this.update(cx, |p| p.diff_view = v.into()),
            )],
        )];
        let extras = vec![
            self.row(
                Some("composer-mascot"),
                true,
                "Composer mascot",
                Some("Show the project mascot while an agent works.".into()),
                vec![self.toggle(
                    "composer-mascot",
                    "Composer mascot",
                    p.composer_mascot,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.composer_mascot = on),
                )],
            ),
            self.row(
                Some("empty-session-games"),
                false,
                "Empty session games",
                Some("Show playable games in empty sessions.".into()),
                vec![self.toggle(
                    "empty-session-games",
                    "Empty session games",
                    p.empty_session_games,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.empty_session_games = on),
                )],
            ),
        ];
        vec![
            self.group(None, true, "Transcript", None, None, transcript),
            self.group(None, false, "Composer", None, None, composer),
            self.group(None, false, "Editor", None, None, editor),
            self.group(None, false, "Code review", None, None, review),
            self.group(None, false, "Extras", None, None, extras),
        ]
    }
}
