//! Port of src/features/sessions/model/draftCache.ts: in-memory composer
//! drafts and their MCP tags, keyed by session id.
//!
//! A composer view lives only as long as its pane. This cache outlives it,
//! so text typed into a session comes back when its pane opens again. It
//! does not survive an app restart; the TypeScript noted that keeping drafts
//! across restarts needs the session record to store them.

use std::collections::HashMap;

use parking_lot::Mutex;

use super::mcp_picker::McpTag;

#[derive(Default)]
struct Drafts {
    drafts: HashMap<String, String>,
    mcp_tags: HashMap<String, Vec<McpTag>>,
}

/// The draft cache. The `Submit` entity holds one for the app.
#[derive(Default)]
pub struct ComposerDrafts {
    inner: Mutex<Drafts>,
}

impl ComposerDrafts {
    pub fn new() -> Self {
        Self::default()
    }

    /// `getComposerDraft`.
    pub fn get_composer_draft(&self, session_id: &str) -> Option<String> {
        self.inner.lock().drafts.get(session_id).cloned()
    }

    /// `setComposerDraft`: an empty draft clears the text and its tags.
    pub fn set_composer_draft(&self, session_id: &str, text: &str) {
        let mut inner = self.inner.lock();
        if text.is_empty() {
            inner.drafts.remove(session_id);
            inner.mcp_tags.remove(session_id);
        } else {
            inner
                .drafts
                .insert(session_id.to_string(), text.to_string());
        }
    }

    /// `getComposerMcpTags`.
    pub fn get_composer_mcp_tags(&self, session_id: &str) -> Vec<McpTag> {
        self.inner
            .lock()
            .mcp_tags
            .get(session_id)
            .cloned()
            .unwrap_or_default()
    }

    /// `setComposerMcpTags`.
    pub fn set_composer_mcp_tags(&self, session_id: &str, tags: Vec<McpTag>) {
        let mut inner = self.inner.lock();
        if tags.is_empty() {
            inner.mcp_tags.remove(session_id);
        } else {
            inner.mcp_tags.insert(session_id.to_string(), tags);
        }
    }

    /// `clearComposerDraft`.
    pub fn clear_composer_draft(&self, session_id: &str) {
        let mut inner = self.inner.lock();
        inner.drafts.remove(session_id);
        inner.mcp_tags.remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::submit::mcp::{McpConnection, McpProvider, McpScope};
    use crate::submit::mcp_picker::new_mcp_tag;

    #[test]
    fn returns_none_for_a_session_that_never_had_a_draft() {
        assert_eq!(ComposerDrafts::new().get_composer_draft("never-seen"), None);
    }

    #[test]
    fn gives_back_the_last_text_set_for_a_session_across_separate_reads() {
        let drafts = ComposerDrafts::new();
        drafts.set_composer_draft("s1", "half-typed message");
        assert_eq!(
            drafts.get_composer_draft("s1").as_deref(),
            Some("half-typed message")
        );
    }

    #[test]
    fn keeps_drafts_for_different_sessions_apart() {
        let drafts = ComposerDrafts::new();
        drafts.set_composer_draft("s2", "draft for session two");
        drafts.set_composer_draft("s3", "draft for session three");
        assert_eq!(
            drafts.get_composer_draft("s2").as_deref(),
            Some("draft for session two")
        );
        assert_eq!(
            drafts.get_composer_draft("s3").as_deref(),
            Some("draft for session three")
        );
    }

    #[test]
    fn treats_setting_an_empty_string_as_clearing_the_draft() {
        let drafts = ComposerDrafts::new();
        drafts.set_composer_draft("s4", "something");
        drafts.set_composer_draft("s4", "");
        assert_eq!(drafts.get_composer_draft("s4"), None);
    }

    #[test]
    fn clear_composer_draft_removes_a_stored_draft() {
        let drafts = ComposerDrafts::new();
        drafts.set_composer_draft("s5", "will be cleared");
        drafts.clear_composer_draft("s5");
        assert_eq!(drafts.get_composer_draft("s5"), None);
    }

    #[test]
    fn keeps_mcp_tag_metadata_with_its_session_draft() {
        let drafts = ComposerDrafts::new();
        let tag = new_mcp_tag(
            &McpConnection {
                provider: McpProvider::Codex,
                name: "docs".into(),
                scope: McpScope::User,
                config_path: "/config.toml".into(),
                transport: "stdio".into(),
                enabled: None,
            },
            &[],
        );
        drafts.set_composer_draft("mcp-one", &format!("Ask {}", tag.token));
        drafts.set_composer_mcp_tags("mcp-one", vec![tag.clone()]);
        drafts.set_composer_draft("mcp-two", "Another draft");
        assert_eq!(drafts.get_composer_mcp_tags("mcp-one"), vec![tag]);
        assert!(drafts.get_composer_mcp_tags("mcp-two").is_empty());
        drafts.set_composer_draft("mcp-one", "");
        assert!(drafts.get_composer_mcp_tags("mcp-one").is_empty());
    }
}
