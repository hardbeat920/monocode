//! Port of src/integrations/harness/providers/codex/codexQuestions.ts: the
//! `item/tool/requestUserInput` request and its reply.

use serde_json::{Map, Value, json};

use monocode_core::js;
use monocode_core::user_question::{
    UserQuestion, UserQuestionReply, questions_from_unknown, selected_answer_labels,
};

use super::json::as_record;

/// `codexQuestions`: the questions in a request, or an error when MonoCode
/// cannot show them faithfully.
pub fn codex_questions(params: &Value) -> Result<Vec<UserQuestion>, String> {
    let raw = match as_record(Some(params)).and_then(|rec| rec.get("questions")) {
        Some(Value::Array(raw)) if !raw.is_empty() => raw,
        _ => return Err("Codex sent an unsupported user-input request.".into()),
    };
    // The shared question UI stores answers in the transcript and is not a
    // secure place to enter credentials. Never send secret questions to it.
    if raw.iter().any(|question| {
        as_record(Some(question)).and_then(|rec| rec.get("isSecret")) == Some(&Value::Bool(true))
    }) {
        return Err(
            "Codex requested secret input. MonoCode cannot collect secret answers securely.".into(),
        );
    }
    let mut mapped = Vec::with_capacity(raw.len());
    for question in raw {
        let Some(rec) = as_record(Some(question)).filter(|rec| valid_question(rec)) else {
            return Err("Codex sent an unsupported user-input question.".into());
        };
        let options = rec.get("options").filter(|options| !options.is_null());
        let allow_custom = rec.get("isOther") == Some(&Value::Bool(true))
            || options.is_none()
            || matches!(options, Some(Value::Array(list)) if list.is_empty());
        let mut next: Map<String, Value> = rec.clone();
        next.insert("allowCustom".into(), Value::Bool(allow_custom));
        mapped.push(Value::Object(next));
    }
    let questions = questions_from_unknown(&json!({ "questions": mapped }));
    let mut ids: Vec<Option<&Value>> = Vec::new();
    for question in raw {
        let id = as_record(Some(question)).and_then(|rec| rec.get("id"));
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.len() != raw.len() {
        return Err("Codex sent duplicate user-input question IDs.".into());
    }
    Ok(questions)
}

fn valid_question(rec: &Map<String, Value>) -> bool {
    let Some(Value::String(id)) = rec.get("id") else {
        return false;
    };
    if js::trim(id).is_empty() || id != js::trim(id) {
        return false;
    }
    let Some(Value::String(question)) = rec.get("question") else {
        return false;
    };
    if js::trim(question).is_empty() {
        return false;
    }
    match rec.get("options") {
        None | Some(Value::Null) => true,
        Some(Value::Array(options)) => options.iter().all(|option| {
            matches!(
                as_record(Some(option)).and_then(|option| option.get("label")),
                Some(Value::String(label)) if !js::trim(label).is_empty()
            )
        }),
        Some(_) => false,
    }
}

/// `codexQuestionResponse`: answers keyed by the provider's question ids.
pub fn codex_question_response(questions: &[UserQuestion], reply: &UserQuestionReply) -> Value {
    let UserQuestionReply::Answered { answers, custom } = reply else {
        return json!({ "answers": {} });
    };
    let mut out = Map::new();
    for question in questions {
        let labels = selected_answer_labels(question, answers, custom.as_ref());
        if !labels.is_empty() {
            out.insert(question.id.clone(), json!({ "answers": labels }));
        }
    }
    json!({ "answers": out })
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::user_question::{CUSTOM_OPTION_ID, QuestionAnswers, QuestionCustom};

    fn answers(pairs: &[(&str, &[&str])]) -> QuestionAnswers {
        pairs
            .iter()
            .map(|(id, list)| (id.to_string(), list.iter().map(|s| s.to_string()).collect()))
            .collect()
    }

    fn custom(pairs: &[(&str, &str)]) -> QuestionCustom {
        pairs
            .iter()
            .map(|(id, text)| (id.to_string(), text.to_string()))
            .collect()
    }

    #[test]
    fn returns_selected_labels_and_free_text_under_the_providers_ids() {
        let questions = codex_questions(&json!({
            "questions": [
                { "id": "scope", "question": "Which scope?", "isOther": true, "options": [{ "label": "Workspace" }] },
                { "id": "reason", "question": "Why?", "isOther": false, "options": null },
                { "id": "skipped", "question": "Anything else?", "options": null },
            ]
        }))
        .unwrap();
        let reply = UserQuestionReply::Answered {
            answers: answers(&[("scope", &[CUSTOM_OPTION_ID])]),
            custom: Some(custom(&[
                ("scope", "Selected folder"),
                ("reason", "Read docs"),
            ])),
        };
        assert_eq!(
            codex_question_response(&questions, &reply),
            json!({
                "answers": {
                    "scope": { "answers": ["Selected folder"] },
                    "reason": { "answers": ["Read docs"] },
                }
            })
        );
    }

    #[test]
    fn does_not_offer_an_extra_free_text_choice_for_a_closed_question() {
        let questions = codex_questions(&json!({
            "questions": [{
                "id": "q", "question": "Continue?", "isOther": false,
                "options": [{ "label": "Yes" }, { "label": "No" }],
            }]
        }))
        .unwrap();
        assert!(!questions[0].allow_custom);
        let reply = UserQuestionReply::Answered {
            answers: answers(&[("q", &["No"])]),
            custom: None,
        };
        assert_eq!(
            codex_question_response(&questions, &reply),
            json!({ "answers": { "q": { "answers": ["No"] } } })
        );
    }

    #[test]
    fn rejects_unsupported_input_without_exposing_a_broken_form() {
        for input in [
            json!({ "questions": [] }),
            json!({ "questions": [{ "question": "Missing ID" }] }),
            json!({ "questions": [{ "id": "a", "question": "One" }, { "id": "a", "question": "Two" }] }),
            json!({ "questions": [{ "id": " key ", "question": "Invalid ID" }] }),
            json!({ "questions": [{ "id": "q", "question": "Malformed choices", "options": {} }] }),
            json!({ "questions": [{ "id": "q", "question": "Missing choice label", "options": [{}] }] }),
            json!({ "questions": [{ "id": "q", "question": "Secret", "isSecret": true }] }),
        ] {
            assert!(codex_questions(&input).is_err(), "{input}");
        }
    }

    #[test]
    fn skipped_reply_sends_no_answers() {
        assert_eq!(
            codex_question_response(&[], &UserQuestionReply::Skipped),
            json!({ "answers": {} })
        );
    }
}
