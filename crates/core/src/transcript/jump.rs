//! Port of src/features/sessions/model/transcriptJump.ts: a pending request
//! to scroll a session's transcript to a block, kept until the transcript
//! that shows the session has carried it out.
//!
//! The TypeScript module kept one map in module scope. Here it is a value the
//! app holds (for example in a GPUI global), with a change counter instead of
//! listener callbacks.

use std::collections::HashMap;

/// `TranscriptJump`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptJump {
    pub block_id: String,
    pub query: Option<String>,
    pub token: u64,
}

/// The pending jumps, one per session.
#[derive(Debug, Default)]
pub struct TranscriptJumps {
    pending: HashMap<String, TranscriptJump>,
    next_token: u64,
    /// Bumped whenever a jump is requested or cleared, like the listeners the
    /// TypeScript notified.
    revision: u64,
}

impl TranscriptJumps {
    /// `requestTranscriptJump`.
    pub fn request(&mut self, session_id: &str, block_id: &str, query: Option<&str>) {
        self.next_token += 1;
        self.pending.insert(
            session_id.to_string(),
            TranscriptJump {
                block_id: block_id.to_string(),
                query: query.map(str::to_string),
                token: self.next_token,
            },
        );
        self.revision += 1;
    }

    /// `peekTranscriptJump`.
    pub fn peek(&self, session_id: &str) -> Option<&TranscriptJump> {
        self.pending.get(session_id)
    }

    /// `clearTranscriptJump`: only clears the jump `token` names, so a newer
    /// request survives an older navigation finishing.
    pub fn clear(&mut self, session_id: &str, token: u64) {
        if self.pending.get(session_id).map(|jump| jump.token) != Some(token) {
            return;
        }
        self.pending.remove(session_id);
        self.revision += 1;
    }

    /// Changes so far. Compare against a saved value to see whether anything
    /// was requested or cleared.
    pub fn revision(&self) -> u64 {
        self.revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_requested_message_until_its_navigation_completes() {
        let mut jumps = TranscriptJumps::default();
        jumps.request("search-test", "old", None);
        let old = jumps.peek("search-test").unwrap().clone();
        jumps.request("search-test", "new", None);
        let latest = jumps.peek("search-test").unwrap().clone();

        jumps.clear("search-test", old.token);
        assert_eq!(jumps.peek("search-test"), Some(&latest));
        jumps.clear("search-test", latest.token);
        assert_eq!(jumps.peek("search-test"), None);
        assert_eq!(jumps.revision(), 3);
    }
}
