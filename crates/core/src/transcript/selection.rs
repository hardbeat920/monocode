//! Port of src/features/sessions/model/transcriptSelection.ts: a text
//! selection counts only inside one settled response.

/// `TranscriptSelectionCandidate`: the selection and the response the anchor
/// and focus fall in (`data-selectable-agent-response`, here the block id
/// whose markdown or prompt holds the endpoint).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptSelectionCandidate<'a> {
    pub text: &'a str,
    pub collapsed: bool,
    pub anchor_response_id: Option<&'a str>,
    pub focus_response_id: Option<&'a str>,
}

/// `validateTranscriptSelection`: the trimmed text, or `None`.
pub fn validate_transcript_selection(
    candidate: &TranscriptSelectionCandidate<'_>,
) -> Option<String> {
    let text = crate::js::trim(candidate.text);
    if text.is_empty() || candidate.collapsed {
        return None;
    }
    let (Some(anchor), Some(focus)) = (candidate.anchor_response_id, candidate.focus_response_id)
    else {
        return None;
    };
    if anchor != focus {
        return None;
    }
    Some(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_non_empty_text_within_one_settled_response() {
        assert_eq!(
            validate_transcript_selection(&TranscriptSelectionCandidate {
                text: " selected ",
                collapsed: false,
                anchor_response_id: Some("response-1"),
                focus_response_id: Some("response-1"),
            })
            .as_deref(),
            Some("selected")
        );
    }

    #[test]
    fn rejects_an_invalid_selection() {
        for (collapsed, anchor, focus) in [
            (true, Some("response-1"), Some("response-1")),
            (false, None, Some("response-1")),
            (false, Some("response-1"), None),
            (false, Some("response-1"), Some("response-2")),
        ] {
            assert_eq!(
                validate_transcript_selection(&TranscriptSelectionCandidate {
                    text: "selected",
                    collapsed,
                    anchor_response_id: anchor,
                    focus_response_id: focus,
                }),
                None
            );
        }
    }
}
