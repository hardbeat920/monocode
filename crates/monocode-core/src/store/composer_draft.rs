//! A thread's unsent composer text, shared with a paired phone. The desktop's
//! `composer-draft:<session>` row is the one copy. Each change gets a new
//! revision, and the phone writes against the revision its text was based on,
//! so a write made offline cannot wipe newer text.
use super::Store;
use crate::follow_up::Message;
use serde_json::{json, Value};

/// A composer draft's text and the revision it was saved at. A cleared draft
/// is revision 0.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComposerText {
    pub text: String,
    pub rev: u64,
}
impl ComposerText {
    pub fn json(&self) -> Value {
        json!({"text": self.text, "rev": self.rev})
    }
}

/// What came of a phone's write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComposerWrite {
    /// Saved, or the draft already had this text.
    Saved(ComposerText),
    /// The draft changed since the phone's base; this is the desktop's copy.
    Stale(ComposerText),
}

/// Revisions are times in milliseconds and always past any given out before,
/// so a revision never comes back after a clear resets it to 0.
pub(super) fn next_rev(last: u64) -> u64 {
    static ISSUED: std::sync::Mutex<u64> = std::sync::Mutex::new(0);
    let mut issued = ISSUED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *issued = crate::time::now_ms().max(last + 1).max(*issued + 1);
    *issued
}

pub(super) fn row(draft: &Message, rev: u64) -> Result<String, String> {
    let mut value = serde_json::to_value(draft).map_err(|e| e.to_string())?;
    value["rev"] = rev.into();
    Ok(value.to_string())
}

/// Whether a write of `text`, based on revision `base`, may replace `current`.
pub fn accepts(current: &ComposerText, text: &str, base: u64) -> bool {
    current.rev == base || current.text == text
}

/// Whether a message sent from the phone uses up the composer's text: the
/// phone's text was based on this revision, or it is the same words.
pub fn sent_from(current: &ComposerText, sent: &str, based_on: Option<u64>) -> bool {
    !current.text.is_empty()
        && (based_on == Some(current.rev) || current.text.trim() == sent.trim())
}

impl Store {
    pub fn composer_text(&self, session: i64) -> Result<ComposerText, String> {
        let Some(raw) = self
            .setting(&format!("composer-draft:{session}"))?
            .filter(|raw| !raw.is_empty())
        else {
            return Ok(ComposerText::default());
        };
        let value: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        Ok(ComposerText {
            text: value["text"].as_str().unwrap_or_default().to_owned(),
            // Saved before drafts had revisions, but still not a cleared one.
            rev: value["rev"].as_u64().unwrap_or(1),
        })
    }

    /// The phone's text for the draft, kept only when nothing changed since
    /// `base`. The draft's files stay.
    pub fn write_composer_text(
        &self,
        session: i64,
        text: &str,
        base: u64,
    ) -> Result<ComposerWrite, String> {
        let current = self.composer_text(session)?;
        if !accepts(&current, text, base) {
            return Ok(ComposerWrite::Stale(current));
        }
        if current.text == text {
            return Ok(ComposerWrite::Saved(current));
        }
        let mut draft = self.composer_draft(session)?.unwrap_or_default();
        draft.text = text.to_owned();
        // As the desktop composer decides: blank text and no files is no draft.
        let keep = !text.trim().is_empty() || !draft.attachments.is_empty();
        let rev = self.save_composer_draft(session, keep.then_some(&draft))?;
        Ok(ComposerWrite::Saved(ComposerText {
            text: if keep { draft.text } else { String::new() },
            rev,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn store() -> (Store, i64) {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let id = store.create("codex", "/tmp").unwrap().id;
        (store, id)
    }
    fn saved(write: ComposerWrite) -> ComposerText {
        match write {
            ComposerWrite::Saved(text) => text,
            ComposerWrite::Stale(text) => panic!("refused: {text:?}"),
        }
    }

    #[test]
    fn each_change_gets_a_newer_revision_and_a_clear_is_zero() {
        let (store, id) = store();
        assert_eq!(store.composer_text(id).unwrap(), ComposerText::default());
        let draft = |text: &str| Message {
            text: text.into(),
            ..Default::default()
        };
        let first = store.save_composer_draft(id, Some(&draft("a"))).unwrap();
        let second = store.save_composer_draft(id, Some(&draft("ab"))).unwrap();
        assert!(second > first);
        assert_eq!(
            store.composer_text(id).unwrap(),
            ComposerText {
                text: "ab".into(),
                rev: second
            }
        );
        // The revision is not part of the message the composer restores.
        assert_eq!(store.composer_draft(id).unwrap(), Some(draft("ab")));
        assert_eq!(store.save_composer_draft(id, None).unwrap(), 0);
        assert_eq!(store.composer_text(id).unwrap().rev, 0);
        assert!(store.save_composer_draft(id, Some(&draft("c"))).unwrap() > second);
    }

    #[test]
    fn a_phone_write_from_an_old_revision_keeps_the_newer_text() {
        let (store, id) = store();
        let first = saved(store.write_composer_text(id, "from the phone", 0).unwrap());
        assert_eq!(first.text, "from the phone");
        assert_eq!(store.composer_text(id).unwrap(), first);

        let desktop = Message {
            text: "typed on the desktop".into(),
            ..Default::default()
        };
        let newer = store.save_composer_draft(id, Some(&desktop)).unwrap();
        // Written offline against the first revision, then the link came back.
        let ComposerWrite::Stale(current) = store
            .write_composer_text(id, "from the phone, later", first.rev)
            .unwrap()
        else {
            panic!("an old write must not land");
        };
        assert_eq!(current.text, "typed on the desktop");
        assert_eq!(current.rev, newer);

        // The same text is no conflict, which also makes a retry harmless.
        let same = saved(
            store
                .write_composer_text(id, "typed on the desktop", first.rev)
                .unwrap(),
        );
        assert_eq!(same.rev, newer);
        let next = saved(store.write_composer_text(id, "both", newer).unwrap());
        assert!(next.rev > newer);
    }

    #[test]
    fn the_phone_changes_the_text_and_leaves_the_desktops_files() {
        let (store, id) = store();
        let files = vec![PathBuf::from("/tmp/shot.png")];
        let draft = Message {
            text: "look".into(),
            attachments: files.clone(),
            ..Default::default()
        };
        let rev = store.save_composer_draft(id, Some(&draft)).unwrap();
        let rev = saved(store.write_composer_text(id, "look at this", rev).unwrap()).rev;
        let kept = store.composer_draft(id).unwrap().unwrap();
        assert_eq!(
            (kept.text.as_str(), &kept.attachments),
            ("look at this", &files)
        );

        // Emptied, the files still make it a draft.
        let emptied = saved(store.write_composer_text(id, "", rev).unwrap());
        assert!(emptied.rev > rev);
        assert_eq!(
            store.composer_draft(id).unwrap().unwrap().attachments,
            files
        );
        assert!(store.composer_draft_sessions().unwrap().contains(&id));

        // Without files, blank text is no draft at all.
        let (store, id) = self::store();
        let rev = saved(store.write_composer_text(id, "hi", 0).unwrap()).rev;
        let blank = saved(store.write_composer_text(id, "  ", rev).unwrap());
        assert_eq!(blank, ComposerText::default());
        assert!(store.composer_draft_sessions().unwrap().is_empty());
    }

    #[test]
    fn a_row_saved_before_revisions_is_not_a_cleared_one() {
        let (store, id) = store();
        store
            .save_setting(&format!("composer-draft:{id}"), r#"{"text":"old"}"#)
            .unwrap();
        assert_eq!(store.composer_text(id).unwrap().rev, 1);
        assert!(matches!(
            store.write_composer_text(id, "new", 0).unwrap(),
            ComposerWrite::Stale(_)
        ));
    }

    #[test]
    fn a_phone_send_uses_up_the_text_it_was_based_on_or_the_same_words() {
        let current = ComposerText {
            text: "hel".into(),
            rev: 7,
        };
        assert!(sent_from(&current, "hello", Some(7)));
        assert!(sent_from(&current, " hel ", None));
        assert!(!sent_from(&current, "hello", Some(6)));
        assert!(!sent_from(&ComposerText::default(), "", Some(0)));
    }
}
