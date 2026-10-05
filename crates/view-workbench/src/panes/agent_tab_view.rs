//! Port of src/features/sessions/ui/AgentTabView.tsx: one orchestration
//! worker, watched from its lead's workspace.
//!
//! It is read-only on purpose. The run belongs to the orchestrator, which
//! decides what each worker is asked and when; a composer here would put a
//! second voice into a conversation the lead holds. To change course, the
//! user says so in the lead's transcript.
//!
//! The transcript is a view the owner builds with
//! [`agent_transcript_options`]; saving notes goes through the engine with
//! the drafts [`agent_note`] and [`agent_selection_note`] describe.

use gpui::{
    AnyElement, AnyView, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};
use monocode_core::HarnessId;
use monocode_core::session::session_display_title;
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, ProviderLogo, Theme, UiStyled as _, icon, provider_logo, u};

/// The worker session as the tab shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTabSession {
    pub id: String,
    pub title: String,
    pub harness: HarnessId,
    pub cwd: String,
    /// `findModel(session.model)?.name ?? session.model`.
    pub model_name: String,
}

/// What the worker transcript allows, where the main chat differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentTranscriptOptions {
    /// `managed`: no composer affordances such as edit last turn.
    pub managed: bool,
    /// Save a turn as a note (`onSaveNote`), when notes are on.
    pub save_notes: bool,
    /// Add selected text to notes (`onSaveSelectionNote`).
    pub save_selection_notes: bool,
    /// Add selected text to the chat. Only the lead's chat offers it.
    pub add_to_chat: bool,
}

/// The transcript options for a worker tab.
pub fn agent_transcript_options(notes_enabled: bool) -> AgentTranscriptOptions {
    AgentTranscriptOptions {
        managed: true,
        save_notes: notes_enabled,
        save_selection_notes: notes_enabled,
        add_to_chat: false,
    }
}

/// A note to create (`createNote`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentNoteDraft {
    pub title: String,
    pub body: String,
    pub source_session_id: String,
    pub source_cwd: String,
}

/// `saveNote`: a saved turn takes the session's title, unless the session
/// has none, then the text's own title (`note_title`, the engine's
/// `noteTitle`).
pub fn agent_note(
    session: &AgentTabSession,
    text: &str,
    note_title: impl Fn(&str) -> String,
) -> AgentNoteDraft {
    let session_title = session_display_title(&session.title, session.harness);
    AgentNoteDraft {
        title: if !session_title.is_empty() && session_title != "New session" {
            session_title
        } else {
            note_title(text)
        },
        body: text.to_string(),
        source_session_id: session.id.clone(),
        source_cwd: session.cwd.clone(),
    }
}

/// `saveSelectionNote`: selected text is titled by its own content.
pub fn agent_selection_note(
    session: &AgentTabSession,
    text: &str,
    note_title: impl Fn(&str) -> String,
) -> AgentNoteDraft {
    AgentNoteDraft {
        title: note_title(text),
        body: text.to_string(),
        source_session_id: session.id.clone(),
        source_cwd: session.cwd.clone(),
    }
}

/// The worker tab.
pub struct AgentTabView {
    title: SharedString,
    session: Option<AgentTabSession>,
    transcript: Option<AnyView>,
}

impl AgentTabView {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            session: None,
            transcript: None,
        }
    }

    /// The worker, or `None` once it stopped running.
    pub fn set_session(&mut self, session: Option<AgentTabSession>, cx: &mut Context<Self>) {
        self.session = session;
        cx.notify();
    }

    /// The worker's transcript (with `TranscriptFind` over it).
    pub fn set_transcript(&mut self, transcript: Option<AnyView>, cx: &mut Context<Self>) {
        self.transcript = transcript;
        cx.notify();
    }
}

impl Render for AgentTabView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let Some(session) = self.session.clone() else {
            return div()
                .debug_selector(|| "agent-tab-gone".into())
                .flex()
                .size_full()
                .items_center()
                .justify_center()
                .px(u(24.))
                .child(
                    div()
                        .max_w(u(384.))
                        .text_center()
                        .text_px(12.)
                        .line_height(u(20.))
                        .text_color(theme.content(0.45))
                        .child(
                            "This agent is no longer running. Its work is summarised in the orchestrator's conversation.",
                        ),
                )
                .into_any_element();
        };
        let glyph: AnyElement = match ProviderLogo::from_id(session.harness.as_str()) {
            Some(logo) => provider_logo(logo).size(14.).into_any_element(),
            None => icon(IconName::Bot)
                .size(u(14.))
                .text_color(theme.content(0.45))
                .into_any_element(),
        };
        div()
            .debug_selector(|| "agent-tab".into())
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .children(self.transcript.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(6.))
                    .border_t_1()
                    .border_color(theme.colors.stroke)
                    .px(u(12.))
                    .py(u(6.))
                    .text_px(11.)
                    .text_color(theme.content(0.45))
                    .child(glyph)
                    .child(
                        div()
                            .id("agent-tab-model")
                            .min_w_0()
                            .truncate()
                            .tooltip(tooltip(self.title.clone()))
                            .child(format!(
                                "{} · {}",
                                session.model_name,
                                session.harness.title()
                            )),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .flex_none()
                            .child("Run by the orchestrator · read-only"),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    //! The parts of AgentTabView.test.ts that belong to the tab. Saving and
    //! its retry, and the selection toolbar, are the transcript's.

    use super::*;

    fn worker(title: &str) -> AgentTabSession {
        AgentTabSession {
            id: "worker-1".into(),
            title: title.into(),
            harness: HarnessId::Codex,
            cwd: "/project".into(),
            model_name: "GPT-5.5".into(),
        }
    }

    fn first_line(text: &str) -> String {
        text.lines().next().unwrap_or_default().to_string()
    }

    #[test]
    fn saves_worker_prompts_and_responses_with_their_source_session_and_project() {
        let session = worker("Inspect notifications");
        for body in ["Check notifications", "Notifications work."] {
            assert_eq!(
                agent_note(&session, body, first_line),
                AgentNoteDraft {
                    title: "Inspect notifications".into(),
                    body: body.into(),
                    source_session_id: "worker-1".into(),
                    source_cwd: "/project".into(),
                }
            );
        }
    }

    #[test]
    fn an_untitled_worker_titles_the_note_from_its_text() {
        let session = worker("codex");
        assert_eq!(
            agent_note(&session, "Keep me\nmore", first_line).title,
            "Keep me"
        );
        assert_eq!(
            agent_selection_note(&worker("Named"), "Keep this sentence", first_line).title,
            "Keep this sentence"
        );
    }

    #[test]
    fn offers_notes_but_not_add_to_chat_in_a_worker_chat() {
        let options = agent_transcript_options(true);
        assert!(options.managed && options.save_notes && options.save_selection_notes);
        assert!(!options.add_to_chat);
        assert!(!agent_transcript_options(false).save_notes);
    }
}
