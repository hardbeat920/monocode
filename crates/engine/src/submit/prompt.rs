//! Port of src/features/sessions/model/promptPreparation.ts, plus
//! `composeNoteMessage` and `injectNotePrompt` from src/features/notes.
//!
//! File mentions and `@note` references belong to other packages, so the
//! caller passes them in as futures (`SubmitPromptHooks` in the pipeline).

use std::future::Future;

use monocode_core::js;
use monocode_core::notes::NoteComposerCard;
use monocode_harness::core::native_commands::native_command_prompt;

use super::skills::{SkillCatalog, SkillCatalogContext};

/// `preparePrompt`: warm the provider's command catalog, keep native
/// command arguments exact, and otherwise expand file mentions, notes, and
/// skills in that order.
pub async fn prepare_prompt<FM, FN>(
    text: &str,
    context: &SkillCatalogContext,
    skills: &SkillCatalog,
    apply_file_mentions: impl FnOnce(String) -> FM,
    apply_notes: impl FnOnce(String) -> FN,
) -> String
where
    FM: Future<Output = String>,
    FN: Future<Output = String>,
{
    skills.warm_native_skills(context);
    if skills.is_native_command_prompt(text, context.harness) {
        return native_command_prompt(context.harness, text);
    }
    let with_files = apply_file_mentions(text.to_string()).await;
    let with_notes = apply_notes(with_files).await;
    skills.apply_skills_to_turn(&with_notes, context).await
}

/// A note as `injectNotePrompt` reads it.
pub struct NotePrompt<'a> {
    pub title: &'a str,
    pub body: &'a str,
}

/// `injectNotePrompt`: append referenced notes after the message.
pub fn inject_note_prompt(text: &str, notes: &[NotePrompt<'_>]) -> String {
    if notes.is_empty() {
        return text.to_string();
    }
    let mut lines = vec![
        js::trim_end(text).to_string(),
        String::new(),
        "---".to_string(),
    ];
    for note in notes {
        let heading = js::trim(note.title);
        let heading = if heading.is_empty() {
            "Untitled"
        } else {
            heading
        };
        lines.push(format!(
            "Referenced note \"{heading}\":\n\n{}",
            js::trim(note.body)
        ));
    }
    lines.join("\n")
}

/// `composeNoteMessage`: the message with a note chip's body attached.
pub fn compose_note_message(card: Option<&NoteComposerCard>, text: &str) -> String {
    let Some(card) = card else {
        return js::trim(text).to_string();
    };
    let trimmed = js::trim(text);
    let lead = if trimmed.is_empty() {
        "Use this note."
    } else {
        trimmed
    };
    inject_note_prompt(
        lead,
        &[NotePrompt {
            title: &card.title,
            body: &card.body,
        }],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::Arc;

    use futures::FutureExt;
    use futures::future::BoxFuture;
    use monocode_core::HarnessId;
    use monocode_harness::core::local_store::MemoryStore;
    use monocode_harness::core::native_commands::{
        CommandContext, NativeCommand, NativeCommandProvider,
    };
    use monocode_harness::core::task::SmolSpawner;
    use monocode_process::skills::DiscoveredSkill;

    use crate::submit::skills::SkillSources;

    struct Provider {
        raw: bool,
    }

    impl NativeCommandProvider for Provider {
        fn discover(
            &self,
            _context: CommandContext,
        ) -> BoxFuture<'_, anyhow::Result<Vec<NativeCommand>>> {
            async { Ok(Vec::new()) }.boxed()
        }

        fn raw_slash_commands(&self) -> bool {
            self.raw
        }
    }

    struct Sources;

    impl SkillSources for Sources {
        fn command_provider(&self, harness: HarnessId) -> Option<Arc<dyn NativeCommandProvider>> {
            match harness {
                HarnessId::Omp => Some(Arc::new(Provider { raw: true })),
                HarnessId::Pi => Some(Arc::new(Provider { raw: false })),
                _ => None,
            }
        }

        fn list_skills(
            &self,
            _cwd: String,
            _disabled: Vec<String>,
        ) -> BoxFuture<'static, Result<Vec<DiscoveredSkill>, String>> {
            async { Ok(Vec::new()) }.boxed()
        }

        fn read_text_file(&self, _path: String) -> BoxFuture<'static, Result<String, String>> {
            async { Ok(String::new()) }.boxed()
        }

        fn home_dir(&self) -> BoxFuture<'static, Result<String, String>> {
            async { Ok("/home".to_string()) }.boxed()
        }

        fn create_path(
            &self,
            _parent: String,
            _name: String,
            _is_dir: bool,
        ) -> BoxFuture<'static, Result<String, String>> {
            async { Ok(String::new()) }.boxed()
        }

        fn write_text_file(
            &self,
            _path: String,
            _content: String,
        ) -> BoxFuture<'static, Result<(), String>> {
            async { Ok(()) }.boxed()
        }
    }

    fn catalog() -> SkillCatalog {
        SkillCatalog::new(
            Arc::new(Sources),
            Arc::new(MemoryStore::new()),
            Arc::new(SmolSpawner),
        )
    }

    #[test]
    fn preserves_native_command_arguments() {
        let skills = catalog();
        for text in [
            "/workflow foo @README.md",
            "/Review_Code a:b",
            "/omp:compact custom instructions",
        ] {
            let calls = RefCell::new(0);
            let prepared = smol::block_on(prepare_prompt(
                text,
                &SkillCatalogContext::new(HarnessId::Omp, "/repo"),
                &skills,
                |text| {
                    *calls.borrow_mut() += 1;
                    async { text }
                },
                |text| {
                    *calls.borrow_mut() += 1;
                    async { text }
                },
            ));
            assert_eq!(prepared, text.replace("/omp:compact", "/compact"));
            assert_eq!(*calls.borrow(), 0);
        }
    }

    #[test]
    fn expands_file_mentions_then_notes_then_skills() {
        let skills = catalog();
        let events = RefCell::new(Vec::new());
        let prepared = smol::block_on(prepare_prompt(
            "hello",
            &SkillCatalogContext::new(HarnessId::Claude, "/repo"),
            &skills,
            |text| {
                events.borrow_mut().push(format!("files:{text}"));
                async { "with files".to_string() }
            },
            |text| {
                events.borrow_mut().push(format!("notes:{text}"));
                async move { text }
            },
        ));
        assert_eq!(prepared, "with files");
        assert_eq!(*events.borrow(), ["files:hello", "notes:with files"]);
    }

    #[test]
    fn composes_a_note_message() {
        let card = NoteComposerCard {
            id: "n".into(),
            slug: "auth".into(),
            title: " ".into(),
            source_cwd: None,
            body: " Use a cookie. ".into(),
        };
        assert_eq!(compose_note_message(None, "  hi "), "hi");
        assert_eq!(
            compose_note_message(Some(&card), ""),
            "Use this note.\n\n---\nReferenced note \"Untitled\":\n\nUse a cookie."
        );
    }
}
