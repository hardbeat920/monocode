//! A phone's pending review comments: kept in settings, and posted together
//! as one review.
use super::*;
use model::review::{self, PendingComment};
use monocode_core::mobile::inbox::ReviewLine;

impl Inbox {
    fn save_review(&self, key: &str) {
        let json = review::serialize(self.review.get(key).map_or(&[], Vec::as_slice));
        if let Ok(store) = locked(&self.store) {
            let _ = store.save_setting(&review::setting_key(key), &json);
        }
    }

    /// The pending comments of a pull request, saved ones read first.
    fn pending_review(&mut self, key: &str) -> &mut Vec<PendingComment> {
        let store = &self.store;
        self.review.entry(key.to_string()).or_insert_with(|| {
            locked(store)
                .ok()
                .and_then(|store| store.setting(&review::setting_key(key)).ok().flatten())
                .map(|json| review::parse(&json))
                .unwrap_or_default()
        })
    }

    pub(crate) fn phone_review(&mut self, key: &str) -> Vec<PendingComment> {
        self.pending_review(key).clone()
    }

    pub(crate) fn phone_review_add(&mut self, key: &str, line: ReviewLine) -> Vec<PendingComment> {
        let comments = self.pending_review(key);
        let id = review::next_id(comments);
        comments.push(PendingComment {
            id,
            path: line.path,
            line: line.line,
            side: line.side,
            text: line.text,
        });
        self.save_review(key);
        self.phone_review(key)
    }

    pub(crate) fn phone_review_remove(&mut self, key: &str, id: u64) -> Vec<PendingComment> {
        self.pending_review(key).retain(|comment| comment.id != id);
        self.save_review(key);
        self.phone_review(key)
    }

    /// Claims the review, so a second phone can't post the same comments while
    /// it is on its way. The comments to send.
    pub(crate) fn phone_review_start(&mut self, key: &str) -> Result<Vec<PendingComment>, String> {
        if self.review_posting {
            return Err("A review is already being posted".into());
        }
        let comments = self.pending_review(key).clone();
        if comments.is_empty() {
            return Err("There is nothing to submit".into());
        }
        self.review_posting = true;
        Ok(comments)
    }

    /// A phone's review ended. Comments written meanwhile stay pending, and so
    /// does everything when it failed. What is still pending.
    pub(crate) fn phone_review_finished(
        &mut self,
        key: &str,
        sent: &[PendingComment],
        posted: bool,
        cx: &mut Context<Self>,
    ) -> Vec<PendingComment> {
        self.review_posting = false;
        if posted {
            self.pending_review(key)
                .retain(|comment| !sent.iter().any(|gone| gone.id == comment.id));
            self.save_review(key);
            self.phone_discussed(key, cx);
        }
        self.phone_review(key)
    }
}
