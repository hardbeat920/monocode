//! Port of src/features/sessions/model/userQuestion.ts: the clarifying-question
//! model shared by every harness that can ask the user.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::js;

/// `CUSTOM_OPTION_ID`: the "Other" option the UI adds for free text.
pub const CUSTOM_OPTION_ID: &str = "__custom__";

/// Selected option ids per question id.
pub type QuestionAnswers = BTreeMap<String, Vec<String>>;
/// Free text per question id.
pub type QuestionCustom = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserQuestionOption {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserQuestion {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    pub prompt: String,
    pub multi_select: bool,
    pub allow_custom: bool,
    pub options: Vec<UserQuestionOption>,
}

/// Live clarifying questions. Request ids do not survive restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserQuestionPrompt {
    pub request_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub questions: Vec<UserQuestion>,
    /// Deadline owned by the harness. Interaction can disable automatic skipping.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_resolve_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum UserQuestionReply {
    #[serde(rename = "answered")]
    Answered {
        answers: QuestionAnswers,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        custom: Option<QuestionCustom>,
    },
    #[serde(rename = "skipped")]
    Skipped,
}

/// `questionsFromUnknown`: read questions from any harness payload.
pub fn questions_from_unknown(value: &Value) -> Vec<UserQuestion> {
    let rec = as_record(value);
    let nested = rec.and_then(|rec| {
        ["input", "params", "question"]
            .into_iter()
            .find_map(|key| rec.get(key).and_then(as_record))
    });
    let raw: &[Value] = rec
        .and_then(|rec| rec.get("questions"))
        .and_then(Value::as_array)
        .or_else(|| {
            nested
                .and_then(|nested| nested.get("questions"))
                .and_then(Value::as_array)
        })
        .or_else(|| value.as_array())
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut used_ids = HashSet::new();
    raw.iter()
        .enumerate()
        .filter_map(|(index, item)| question_from_unknown(item, index, &mut used_ids))
        .collect()
}

/// `questionPromptTitle`.
pub fn question_prompt_title(questions: &[UserQuestion]) -> String {
    match questions.first() {
        Some(first) => match first.header.as_deref().filter(|header| !header.is_empty()) {
            Some(header) => header.to_string(),
            None if !first.prompt.is_empty() => first.prompt.clone(),
            None => "Question".into(),
        },
        None => "Question".into(),
    }
}

fn custom_text<'a>(custom: &'a QuestionCustom, id: &str) -> Option<&'a str> {
    custom
        .get(id)
        .map(|text| js::trim(text))
        .filter(|text| !text.is_empty())
}

/// `questionIsComplete`.
pub fn question_is_complete(
    question: &UserQuestion,
    answers: &QuestionAnswers,
    custom: &QuestionCustom,
) -> bool {
    let selected = answers.get(&question.id).map(Vec::as_slice).unwrap_or(&[]);
    if selected.is_empty() {
        return question.allow_custom && custom_text(custom, &question.id).is_some();
    }
    if !question.multi_select && selected.len() != 1 {
        return false;
    }
    selected.iter().all(|id| {
        if !is_custom_selection(question, id) {
            return true;
        }
        custom_text(custom, &question.id).is_some()
    })
}

/// `questionAnswersComplete`.
pub fn question_answers_complete(
    questions: &[UserQuestion],
    answers: &QuestionAnswers,
    custom: &QuestionCustom,
) -> bool {
    if questions.is_empty() {
        return false;
    }
    questions
        .iter()
        .all(|question| question_is_complete(question, answers, custom))
}

/// `buildQuestionReply`: skipping every question skips the prompt; any
/// answered question answers with those only.
pub fn build_question_reply(
    questions: &[UserQuestion],
    answers: &QuestionAnswers,
    custom: &QuestionCustom,
) -> UserQuestionReply {
    let answered: Vec<&UserQuestion> = questions
        .iter()
        .filter(|question| question_is_complete(question, answers, custom))
        .collect();
    if answered.is_empty() {
        return UserQuestionReply::Skipped;
    }
    let mut next_answers = QuestionAnswers::new();
    let mut next_custom = QuestionCustom::new();
    for question in answered {
        if let Some(selected) = answers.get(&question.id).filter(|list| !list.is_empty()) {
            next_answers.insert(question.id.clone(), selected.clone());
        }
        if let Some(text) = custom_text(custom, &question.id) {
            next_custom.insert(question.id.clone(), text.to_string());
        }
    }
    UserQuestionReply::Answered {
        answers: next_answers,
        custom: (!next_custom.is_empty()).then_some(next_custom),
    }
}

/// `selectedAnswerLabels`: labels for an answered reply, with custom text in
/// place of "Other".
pub fn selected_answer_labels(
    question: &UserQuestion,
    answers: &QuestionAnswers,
    custom: Option<&QuestionCustom>,
) -> Vec<String> {
    let custom = custom.and_then(|custom| custom_text(custom, &question.id));
    let selected = answers.get(&question.id).map(Vec::as_slice).unwrap_or(&[]);
    if selected.is_empty()
        && let Some(text) = custom
        && question.allow_custom
    {
        return vec![text.to_string()];
    }
    selected
        .iter()
        .filter_map(|id| {
            if is_custom_selection(question, id) {
                return custom.map(str::to_string);
            }
            let option = question.options.iter().find(|item| &item.id == id);
            let label = option
                .map(|option| js::trim(&option.label))
                .filter(|label| !label.is_empty())
                .unwrap_or_else(|| js::trim(id));
            (!label.is_empty()).then(|| label.to_string())
        })
        .collect()
}

/// `isCustomSelection`.
pub fn is_custom_selection(question: &UserQuestion, option_id: &str) -> bool {
    if option_id == CUSTOM_OPTION_ID {
        return true;
    }
    question
        .options
        .iter()
        .find(|item| item.id == option_id)
        .is_some_and(is_other_option)
}

/// `isOtherOption`.
pub fn is_other_option(option: &UserQuestionOption) -> bool {
    option.id == CUSTOM_OPTION_ID || js::trim(&option.label).eq_ignore_ascii_case("other")
}

fn question_from_unknown(
    value: &Value,
    index: usize,
    used_ids: &mut HashSet<String>,
) -> Option<UserQuestion> {
    let rec = as_record(value)?;
    let prompt = ["question", "prompt", "text", "header"]
        .into_iter()
        .find_map(|key| string_field(rec, key));
    let options = options_from_unknown(rec.get("options"));
    let allow_custom = custom_allowed(rec, &options);
    if prompt.is_none() && options.is_empty() && !allow_custom {
        return None;
    }
    let header = string_field(rec, "header").or_else(|| string_field(rec, "title"));
    let fallback = format!("q{}", index + 1);
    let seed = string_field(rec, "id")
        .or(prompt.clone())
        .or(header.clone())
        .unwrap_or(fallback);
    let multi_select = ["multiSelect", "allowMultiple", "multiple"]
        .into_iter()
        .any(|key| rec.get(key) == Some(&Value::Bool(true)));
    Some(UserQuestion {
        id: unique_id(&seed, used_ids),
        prompt: prompt
            .or(header.clone())
            .unwrap_or_else(|| format!("Question {}", index + 1)),
        header,
        multi_select,
        allow_custom,
        options,
    })
}

fn options_from_unknown(value: Option<&Value>) -> Vec<UserQuestionOption> {
    let Some(items) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut used_ids = HashSet::new();
    items
        .iter()
        .filter_map(|item| {
            if let Some(text) = item.as_str() {
                let label = js::trim(text);
                if label.is_empty() {
                    return None;
                }
                return Some(UserQuestionOption {
                    id: unique_id(label, &mut used_ids),
                    label: label.to_string(),
                    description: None,
                });
            }
            let rec = as_record(item)?;
            let label = ["label", "value", "text", "id"]
                .into_iter()
                .find_map(|key| string_field(rec, key))?;
            let description =
                string_field(rec, "description").or_else(|| string_field(rec, "detail"));
            let seed = ["id", "optionId", "value"]
                .into_iter()
                .find_map(|key| string_field(rec, key))
                .unwrap_or_else(|| label.clone());
            Some(UserQuestionOption {
                id: unique_id(&seed, &mut used_ids),
                label,
                description,
            })
        })
        .collect()
}

fn custom_allowed(rec: &Map<String, Value>, options: &[UserQuestionOption]) -> bool {
    if let Some(Value::Bool(custom)) = rec.get("custom") {
        return *custom;
    }
    if let Some(Value::Bool(custom)) = rec.get("allowCustom") {
        return *custom;
    }
    if options.iter().any(is_other_option) {
        return true;
    }
    // Cursor-style options carry stable ids. Claude, OpenCode, and Grok add Other in the UI.
    if string_field(rec, "prompt").is_some() && string_field(rec, "question").is_none() {
        return false;
    }
    true
}

fn unique_id(seed: &str, used: &mut HashSet<String>) -> String {
    let trimmed = js::trim(seed);
    let base = if trimmed.is_empty() {
        "option"
    } else {
        trimmed
    };
    let mut next = base.to_string();
    let mut n = 2;
    while used.contains(&next) {
        next = format!("{base}:{n}");
        n += 1;
    }
    used.insert(next.clone());
    next
}

fn as_record(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

fn string_field(rec: &Map<String, Value>, key: &str) -> Option<String> {
    let text = js::trim(rec.get(key)?.as_str()?);
    (!text.is_empty()).then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

    // questionsFromUnknown
    #[test]
    fn parses_claude_style_ask_user_question_input() {
        let questions = questions_from_unknown(&json!({
            "questions": [
                {
                    "header": "Format",
                    "question": "How should I format the output?",
                    "multiSelect": false,
                    "options": [
                        { "label": "Summary", "description": "Brief overview" },
                        { "label": "Detailed", "description": "Full explanation" }
                    ]
                },
                {
                    "header": "Sections",
                    "question": "Which sections should I include?",
                    "multiSelect": true,
                    "options": [{ "label": "Introduction" }, { "label": "Conclusion" }]
                }
            ]
        }));
        assert_eq!(questions.len(), 2);
        assert_eq!(questions[0].prompt, "How should I format the output?");
        assert_eq!(questions[0].header.as_deref(), Some("Format"));
        assert!(!questions[0].multi_select);
        assert!(questions[0].allow_custom);
        assert!(questions[1].multi_select);
        assert_eq!(question_prompt_title(&questions), "Format");
    }

    #[test]
    fn parses_cursor_style_ask_question_params_without_adding_other() {
        let questions = questions_from_unknown(&json!({
            "title": "Need input",
            "questions": [{
                "id": "q1",
                "prompt": "Which mode should I use?",
                "allowMultiple": true,
                "options": [{ "id": "agent", "label": "Agent" }, { "id": "plan", "label": "Plan" }]
            }]
        }));
        assert_eq!(questions[0].id, "q1");
        assert_eq!(questions[0].prompt, "Which mode should I use?");
        assert!(questions[0].multi_select);
        assert!(!questions[0].allow_custom);
        let ids: Vec<&str> = questions[0].options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["agent", "plan"]);
    }

    #[test]
    fn honors_opencode_custom_and_multiple_flags() {
        let questions = questions_from_unknown(&json!({
            "questions": [{
                "question": "Pick a name",
                "multiple": false,
                "custom": false,
                "options": [{ "label": "Alpha" }]
            }]
        }));
        assert_eq!(questions[0].prompt, "Pick a name");
        assert!(!questions[0].multi_select);
        assert!(!questions[0].allow_custom);
    }

    #[test]
    fn makes_duplicate_ids_unique() {
        let questions = questions_from_unknown(&json!([
            { "question": "Same?", "options": ["a", "a", " "] },
            { "question": "Same?" }
        ]));
        assert_eq!(questions[0].id, "Same?");
        assert_eq!(questions[1].id, "Same?:2");
        let ids: Vec<&str> = questions[0].options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["a", "a:2"]);
    }

    fn file_question() -> (Vec<UserQuestion>, UserQuestion) {
        let questions = questions_from_unknown(&json!({
            "questions": [{ "question": "Which file?", "options": [{ "label": "a.ts" }, { "label": "b.ts" }] }]
        }));
        let question = questions[0].clone();
        (questions, question)
    }

    // question answers
    #[test]
    fn requires_a_selection_before_continue() {
        let (questions, question) = file_question();
        assert!(!question_answers_complete(
            &questions,
            &answers(&[]),
            &custom(&[])
        ));
        assert!(question_answers_complete(
            &questions,
            &answers(&[(&question.id, &["a.ts"])]),
            &custom(&[])
        ));
    }

    #[test]
    fn requires_custom_text_when_other_is_selected() {
        let (questions, question) = file_question();
        let other = answers(&[(&question.id, &[CUSTOM_OPTION_ID])]);
        assert!(!question_answers_complete(&questions, &other, &custom(&[])));
        assert!(question_answers_complete(
            &questions,
            &other,
            &custom(&[(&question.id, "c.ts")])
        ));
    }

    #[test]
    fn maps_selected_ids_back_to_labels_and_custom_text() {
        let (_, question) = file_question();
        assert_eq!(
            selected_answer_labels(&question, &answers(&[(&question.id, &["b.ts"])]), None),
            ["b.ts"]
        );
        assert_eq!(
            selected_answer_labels(
                &question,
                &answers(&[(&question.id, &[CUSTOM_OPTION_ID])]),
                Some(&custom(&[(&question.id, "c.ts")]))
            ),
            ["c.ts"]
        );
    }

    fn reply_questions() -> (Vec<UserQuestion>, UserQuestion, UserQuestion) {
        let questions = questions_from_unknown(&json!({
            "questions": [
                { "question": "Language?", "header": "Language", "options": [{ "label": "TypeScript" }, { "label": "JavaScript" }] },
                { "question": "Which files?", "header": "Files", "multiSelect": true, "options": [{ "label": "a.ts" }, { "label": "b.ts" }] }
            ]
        }));
        let language = questions[0].clone();
        let files = questions[1].clone();
        (questions, language, files)
    }

    // buildQuestionReply
    #[test]
    fn skips_the_whole_prompt_when_no_question_was_answered() {
        let (questions, language, _) = reply_questions();
        assert_eq!(
            build_question_reply(&questions, &answers(&[]), &custom(&[])),
            UserQuestionReply::Skipped
        );
        assert!(!question_is_complete(
            &language,
            &answers(&[]),
            &custom(&[])
        ));
    }

    #[test]
    fn keeps_answers_for_questions_that_were_filled_and_drops_skipped_ones() {
        let (questions, _, files) = reply_questions();
        assert_eq!(
            build_question_reply(
                &questions,
                &answers(&[(&files.id, &["a.ts", "b.ts"])]),
                &custom(&[])
            ),
            UserQuestionReply::Answered {
                answers: answers(&[(&files.id, &["a.ts", "b.ts"])]),
                custom: None,
            }
        );
    }

    #[test]
    fn includes_every_completed_question() {
        let (questions, language, files) = reply_questions();
        let reply = build_question_reply(
            &questions,
            &answers(&[(&language.id, &[CUSTOM_OPTION_ID]), (&files.id, &["a.ts"])]),
            &custom(&[(&language.id, "Python")]),
        );
        assert_eq!(
            reply,
            UserQuestionReply::Answered {
                answers: answers(&[(&language.id, &[CUSTOM_OPTION_ID]), (&files.id, &["a.ts"])]),
                custom: Some(custom(&[(&language.id, "Python")])),
            }
        );
        assert_eq!(
            serde_json::to_value(&reply).unwrap(),
            json!({
                "kind": "answered",
                "answers": { "Language?": ["__custom__"], "Which files?": ["a.ts"] },
                "custom": { "Language?": "Python" }
            })
        );
    }
}
