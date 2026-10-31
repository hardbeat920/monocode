//! A paired phone's view of and commands for this chat, through the same paths
//! the composer and its pickers use.
use super::*;
use crate::notifications::approvals::InputKind;
use monocode_core::agent::{Question, QuestionItem, QuestionOption};
use serde_json::{Value, json};

/// The phone's own "Other" choice, as the desktop's question form names it.
const CUSTOM_OPTION_ID: &str = "__custom__";
/// The longest typed answer a phone may send.
const MAX_CUSTOM_ANSWER: usize = 10_000;

/// What a chat waits on the user for, so the phone can say so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Waiting {
    Approval,
    Question,
}

impl Conversation {
    /// A phone's message, as if typed: it starts a turn, or while one runs it
    /// steers or queues. The desktop composer's own draft and files stay put.
    pub(crate) fn mobile_submit(
        &mut self,
        prompt: String,
        attachments: Vec<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if !self.history_ready() {
            self.defer_history_action(
                move |this, cx| this.mobile_submit(prompt, attachments, cx),
                cx,
            );
            return;
        }
        let composer_busy = self.attaching
            || !self.input.read(cx).content.is_empty()
            || !self.attachments.is_empty();
        if self.busy
            || self.stopping
            || self.preparing_worktree
            || composer_busy
            || self.editing_last_turn.pending()
            || self.settings.pending_provider_switch.is_some()
            || (!self.follow_ups.state.messages.is_empty() && !self.follow_ups.state.paused)
        {
            let composer = std::mem::replace(&mut self.attachments, attachments);
            self.enqueue_follow_up(prompt, None, cx);
            self.attachments = composer;
            if !self.busy {
                self.pump_follow_ups(cx);
            }
            cx.emit(SessionChanged);
            cx.notify();
            return;
        }
        self.attachments = attachments;
        self.input.update(cx, |i, cx| i.set_text(prompt, cx));
        self.send(cx);
        cx.emit(SessionChanged);
    }

    /// The phone's model and access sheets. A provider switch is armed as the
    /// desktop picker arms it, and hands off on the next send.
    pub(crate) fn mobile_configure(
        &mut self,
        model: Option<(Kind, Option<String>)>,
        model_settings: &std::collections::HashMap<String, String>,
        access: Option<AccessMode>,
        cx: &mut Context<Self>,
    ) {
        if let Some((kind, model)) = model
            && (self.summary.provider != kind.id() || self.model != model)
        {
            let picked = ModelPicked(kind, model);
            if self.apply_model_pick(&picked, cx) {
                remember_model(kind, picked.1, cx);
            }
        }
        for (key, value) in model_settings {
            match key.as_str() {
                "fast" => self.menu_choice(
                    if value == "true" {
                        "fast:on"
                    } else {
                        "fast:off"
                    },
                    cx,
                ),
                "thinking" => {
                    self.settings.thinking = Some(value == "true");
                    self.save_settings(cx);
                }
                key if is_effort_key(key) => self.menu_choice(&format!("effort:{value}"), cx),
                _ => {
                    if let Ok(choice) = serde_json::to_string(&(key, value)) {
                        self.menu_choice(&format!("model-setting:{choice}"), cx);
                    }
                }
            }
        }
        if let Some(access) = access {
            self.menu_choice(&format!("access:{}", access.id()), cx);
        }
        cx.emit(SessionChanged);
        cx.notify();
    }

    /// The current model id, as a picker would select it.
    pub(crate) fn mobile_model(&self) -> Option<&str> {
        self.model.as_deref().or(self.resolved_model.as_deref())
    }

    /// The model sheet's settings as the phone names them.
    pub(crate) fn mobile_model_settings(&self) -> std::collections::HashMap<String, String> {
        let mut values = self.settings.model_settings.clone();
        if let Some(effort) = &self.settings.effort {
            values.insert("effort".into(), effort.clone());
        }
        values.insert("fast".into(), self.settings.fast.to_string());
        if let Some(thinking) = self.settings.thinking {
            values.insert("thinking".into(), thinking.to_string());
        }
        values
    }

    /// The approvals waiting on the desktop, for the phone's transcript;
    /// saved rows never record them. Questions are left out: the phone shows
    /// the waiting one as a card of its own (see `mobile_question`).
    pub(crate) fn mobile_pending(&self, cx: &gpui::App) -> Vec<crate::mobile::transcript::Pending> {
        self.pending_notification_inputs(cx)
            .into_iter()
            .filter(|input| input.kind != InputKind::Question)
            .map(|input| crate::mobile::transcript::Pending {
                id: input.request_id,
                question: input.kind == InputKind::Question,
                label: input.label,
            })
            .collect()
    }

    /// The question the agent waits on, as the phone's `QuestionPrompt`; null
    /// for none. Only the first: the desktop answers its questions in order.
    pub(crate) fn mobile_question(&self, cx: &gpui::App) -> Value {
        self.questions.front().map_or(Value::Null, |(card, _)| {
            question_prompt(card.read(cx).request())
        })
    }

    /// A phone's reply to the question it was shown, or with no reply its ask
    /// to keep a timed question open while it is answered. Goes through the
    /// card, so it is sent and cleared just as one given on the desktop.
    pub(crate) fn mobile_answer_question(
        &mut self,
        request_id: u64,
        reply: Option<&Value>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let card = self
            .questions
            .front()
            .map(|(card, _)| card.clone())
            .filter(|card| question_request_id(card.read(cx).id()) == request_id)
            .ok_or("This question was already answered")?;
        match reply {
            None => card.update(cx, |card, cx| card.keep_open(cx)),
            Some(reply) => {
                let answers = question_answers(card.read(cx).request(), reply)?;
                card.update(cx, |card, cx| card.reply(answers, cx));
            }
        }
        Ok(())
    }

    pub(crate) fn mobile_waiting(&self, cx: &gpui::App) -> Option<Waiting> {
        let inputs = self.pending_notification_inputs(cx);
        if inputs.iter().any(|input| input.kind == InputKind::Approval) {
            Some(Waiting::Approval)
        } else if inputs.iter().any(|input| input.kind == InputKind::Question) {
            Some(Waiting::Question)
        } else {
            None
        }
    }
}

/// A question's number for the phone, which names questions by number: the
/// provider's ID hashed (FNV-1a) into the integers a JavaScript number holds
/// exactly, so it needs no table and stays the same across frames.
fn question_request_id(id: &str) -> u64 {
    let hash = id.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    hash & ((1 << 53) - 1)
}

/// IDs the phone keys answers by, made up from position where a provider
/// sent none.
fn item_id(item: &QuestionItem, ix: usize) -> String {
    if item.id.is_empty() {
        format!("q{ix}")
    } else {
        item.id.clone()
    }
}
fn option_id(option: &QuestionOption, ix: usize) -> String {
    if option.id.is_empty() {
        format!("o{ix}")
    } else {
        option.id.clone()
    }
}
/// An option that stands for a typed answer, as the phone and the desktop's
/// question form treat one labelled "Other".
fn is_other(option: &QuestionOption) -> bool {
    option.label.trim().eq_ignore_ascii_case("other")
}

/// The question as the phone's `QuestionPrompt`.
fn question_prompt(request: &Question) -> Value {
    let questions: Vec<Value> = request
        .items
        .iter()
        .enumerate()
        .map(|(ix, item)| {
            let options: Vec<Value> = item
                .options
                .iter()
                .enumerate()
                .map(|(oi, option)| {
                    let mut value = json!({"id": option_id(option, oi), "label": option.label});
                    if !option.description.is_empty() {
                        value["description"] = json!(option.description);
                    }
                    value
                })
                .collect();
            json!({
                "id": item_id(item, ix),
                "prompt": item.prompt,
                "multiSelect": item.multiple,
                "allowCustom": item.allow_custom,
                "options": options,
            })
        })
        .collect();
    let mut prompt = json!({
        "requestId": question_request_id(&request.id),
        "questions": questions,
    });
    if !request.title.is_empty() {
        prompt["title"] = json!(request.title);
    }
    if let Some(at) = request.auto_resolve_at {
        prompt["autoResolveAt"] = json!(at);
    }
    prompt
}

/// The phone's `QuestionReply` as the card's answers: each question's chosen
/// option labels, with its typed text as the card adds it. None skips.
fn question_answers(request: &Question, reply: &Value) -> Result<Option<Vec<Vec<String>>>, String> {
    match reply["kind"].as_str() {
        Some("skipped") => return Ok(None),
        Some("answered") => {}
        _ => return Err("Choose an answer or skip the question".into()),
    }
    let answers: Vec<Vec<String>> = request
        .items
        .iter()
        .enumerate()
        .map(|(ix, item)| {
            let id = item_id(item, ix);
            let chosen: Vec<&str> = reply["answers"][&id]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let typed = reply["custom"][&id]
                .as_str()
                .map(str::trim)
                .filter(|text| !text.is_empty() && text.len() <= MAX_CUSTOM_ANSWER)
                .filter(|_| item.allow_custom);
            let mut answers: Vec<String> = Vec::new();
            for choice in chosen {
                if choice == CUSTOM_OPTION_ID {
                    continue;
                }
                let Some(option) = item
                    .options
                    .iter()
                    .enumerate()
                    .find(|(oi, option)| option_id(option, *oi) == choice)
                    .map(|(_, option)| option)
                else {
                    continue;
                };
                // "Other" stands for the typed answer added below.
                if (is_other(option) && item.allow_custom) || answers.contains(&option.label) {
                    continue;
                }
                answers.push(option.label.clone());
            }
            // As the card: a typed answer replaces a single choice.
            if let Some(text) = typed {
                if !item.multiple {
                    answers.clear();
                }
                answers.push(text.to_owned());
            }
            if !item.multiple {
                answers.truncate(1);
            }
            answers
        })
        .collect();
    if answers.iter().all(Vec::is_empty) {
        return Err("Choose an answer or skip the question".into());
    }
    Ok(Some(answers))
}

/// The permission modes a phone offers, with the desktop picker's hints.
pub(crate) fn mobile_access_modes()
-> impl Iterator<Item = (&'static str, &'static str, &'static str)> {
    ACCESS_MODES
        .iter()
        .map(|(id, label, hint, _)| (*id, *label, *hint))
}

#[cfg(test)]
mod question_tests {
    use super::*;

    fn question() -> Question {
        Question {
            id: "toolu_1".into(),
            title: String::new(),
            items: vec![
                QuestionItem {
                    id: "fruit".into(),
                    prompt: "Which fruit?".into(),
                    multiple: false,
                    allow_custom: true,
                    options: vec![
                        QuestionOption {
                            id: "a".into(),
                            label: "Apple".into(),
                            description: "Crisp".into(),
                        },
                        QuestionOption {
                            id: "p".into(),
                            label: "Pear".into(),
                            description: String::new(),
                        },
                    ],
                },
                QuestionItem {
                    id: String::new(),
                    prompt: "Toppings?".into(),
                    multiple: true,
                    allow_custom: false,
                    options: vec![QuestionOption {
                        id: String::new(),
                        label: "Honey".into(),
                        description: String::new(),
                    }],
                },
            ],
            auto_resolve_at: Some(1_000),
        }
    }

    #[test]
    fn prompts_match_the_phone_and_name_questions_by_a_stable_safe_number() {
        let prompt = question_prompt(&question());
        let id = prompt["requestId"].as_u64().unwrap();
        assert_eq!(id, question_request_id("toolu_1"));
        assert!(id < 1 << 53);
        assert_ne!(id, question_request_id("toolu_2"));
        assert_eq!(prompt["autoResolveAt"], 1_000);
        assert!(prompt.get("title").is_none());
        let fruit = &prompt["questions"][0];
        assert_eq!(fruit["multiSelect"], false);
        assert_eq!(fruit["allowCustom"], true);
        assert_eq!(
            fruit["options"][0],
            json!({"id":"a","label":"Apple","description":"Crisp"})
        );
        assert!(fruit["options"][1].get("description").is_none());
        // A provider's missing IDs are made from position.
        assert_eq!(prompt["questions"][1]["id"], "q1");
        assert_eq!(prompt["questions"][1]["options"][0]["id"], "o0");
    }

    #[test]
    fn replies_become_the_cards_answers() {
        let request = question();
        let picked = json!({"kind":"answered","answers":{"fruit":["p"],"q1":["o0"]}});
        assert_eq!(
            question_answers(&request, &picked).unwrap(),
            Some(vec![vec!["Pear".to_owned()], vec!["Honey".to_owned()]])
        );
        // A typed answer replaces a single choice; unknown IDs carry nothing.
        let typed = json!({"kind":"answered","answers":{"fruit":["a","__custom__"],"q1":["gone"]},
            "custom":{"fruit":"  Plum "}});
        assert_eq!(
            question_answers(&request, &typed).unwrap(),
            Some(vec![vec!["Plum".to_owned()], Vec::new()])
        );
        assert_eq!(
            question_answers(&request, &json!({"kind":"skipped"})).unwrap(),
            None
        );
        assert!(question_answers(&request, &json!({"kind":"answered","answers":{}})).is_err());
        assert!(question_answers(&request, &json!({"kind":"maybe"})).is_err());
    }
}
