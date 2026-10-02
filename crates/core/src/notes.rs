//! Port of the note card types in src/features/notes/notes.ts.

use serde::{Deserialize, Serialize};

use crate::block::Extra;

/// The note a user turn carried, as saved on its block.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteCardMeta {
    pub id: String,
    pub slug: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_cwd: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Composer chip: display fields plus the body injected into the harness prompt.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteComposerCard {
    pub id: String,
    pub slug: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_cwd: Option<String>,
    pub body: String,
}

/// `noteCardMeta`: the part of a composer chip saved with the turn.
pub fn note_card_meta(card: &NoteComposerCard) -> NoteCardMeta {
    NoteCardMeta {
        id: card.id.clone(),
        slug: card.slug.clone(),
        title: card.title.clone(),
        source_cwd: card.source_cwd.clone().filter(|cwd| !cwd.is_empty()),
        extra: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_the_body_and_an_empty_source() {
        let card = NoteComposerCard {
            id: "n1".into(),
            slug: "todo".into(),
            title: "Todo".into(),
            source_cwd: Some(String::new()),
            body: "secret".into(),
        };
        assert_eq!(
            serde_json::to_value(note_card_meta(&card)).unwrap(),
            serde_json::json!({ "id": "n1", "slug": "todo", "title": "Todo" })
        );
    }
}
