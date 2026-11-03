//! AgentTranscript's HandoffDivider: a rule with the new provider's mark above
//! the first prompt another provider answers.
use super::*;

impl TranscriptView {
    /// The provider that answered, or is answering, the prompt at `start`.
    fn prompt_provider(&self, start: usize) -> Option<&str> {
        // Receipts are keyed by the first row after the prompt block.
        let turn = (start..self.source.len())
            .find(|&at| !self.source[at].is_prompt())
            .unwrap_or(self.source.len());
        self.turns
            .get(&turn)
            .map(|info| info.provider.as_str())
            .filter(|provider| !provider.is_empty())
            .or_else(|| {
                (self.streaming && start >= self.live_start).then_some(self.provider.as_str())
            })
    }

    pub(super) fn provider_separator(&self, start: usize) -> Option<gpui::AnyElement> {
        if self.mono || self.document || self.source.get(start)?.role != "user" {
            return None;
        }
        let previous = self.source[..start]
            .iter()
            .rposition(|row| row.first && row.role == "user")?;
        let provider = self.prompt_provider(start)?;
        if self.prompt_provider(previous)? == provider {
            return None;
        }
        let label = monocode_core::provider::Kind::from_id(provider).label();
        let rule = || div().h(px(1.)).min_w(px(16.)).flex_1().bg(content(0.12));
        Some(
            div()
                .id(SharedString::from(format!("provider-separator-{start}")))
                .debug_selector(|| "provider-separator".into())
                .role(Role::Label)
                .aria_label(format!("Continued with {label}"))
                .w_full()
                .flex()
                .justify_center()
                .child(
                    div()
                        .w_full()
                        .max_w(px(COLUMN))
                        .px_4()
                        .pt_5()
                        .pb_3()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(rule())
                        .child(provider_icon(provider, 14.))
                        .child(rule()),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn row(role: &str, text: &str) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first: true,
            sent_at: None,
        }
    }
    fn answered_by(provider: &str) -> monocode_core::agent::TurnInfo {
        monocode_core::agent::TurnInfo {
            provider: provider.into(),
            ..Default::default()
        }
    }

    #[gpui::test]
    fn a_rule_marks_only_the_first_prompt_another_provider_answers(cx: &mut TestAppContext) {
        let view = cx.new(|cx| {
            let mut view = TranscriptView::new();
            view.turns = [
                (1, answered_by("claude")),
                (3, answered_by("codex")),
                (5, answered_by("codex")),
            ]
            .into();
            view.restore_history(
                ["One", "Two", "Three"]
                    .into_iter()
                    .flat_map(|text| [row("user", text), row("assistant", text)])
                    .collect(),
                cx,
            );
            view
        });
        view.update(cx, |view, cx| {
            let marked = |view: &TranscriptView| {
                (0..view.source.len())
                    .filter(|&at| view.provider_separator(at).is_some())
                    .collect::<Vec<_>>()
            };
            assert_eq!(marked(view), [2]);

            // A live turn on a newly picked provider is marked before any receipt.
            view.identity("claude", "Opus 5.5");
            view.streaming(true, cx);
            view.patch(6, vec![row("user", "Four"), row("assistant", "")], cx);
            assert_eq!(marked(view), [2, 6]);
        });
    }
}
