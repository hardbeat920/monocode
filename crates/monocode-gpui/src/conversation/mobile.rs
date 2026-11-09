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
    pub(crate) fn mobile_usage_limit(&self, cx: &gpui::App) -> serde_json::Value {
        let limit = self.settings.usage_limit.clone().or_else(|| {
            self.is_remote()
                .then(|| self.account_recovery_limit(cx))
                .flatten()
        });
        usage_state(
            self.summary.id,
            &self.summary.provider,
            self.model.as_deref(),
            &self.settings,
            limit.as_ref(),
            self.busy || self.stopping || self.account_switching || self.needs_history_restore(),
            self.is_remote(),
        )
    }

    /// Validate against the live desktop state, then use the desktop action.
    pub(crate) fn mobile_resume_usage(
        &mut self,
        token: &serde_json::Value,
        automatic: Option<bool>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.settings.worktree_removed {
            return Err("This session’s working copy was removed. Select a branch or worktree on desktop to continue.".into());
        }
        if self.needs_history_restore() {
            self.load(cx);
            return Err("This session is still opening on desktop. Try again in a moment.".into());
        }
        let kind = Kind::from_id(&self.summary.provider);
        if automatic != Some(false)
            && self.settings.account.id != monocode_core::accounts::DEFAULT
            && crate::accounts::get(cx)
                .is_some_and(|center| !center.read(cx).book.exists(kind, &self.settings.account.id))
        {
            return Err(
                "This provider account was removed. Choose an account on desktop and try again."
                    .into(),
            );
        }
        let state = self.mobile_usage_limit(cx);
        if state.is_null() {
            return Err("This session is no longer paused by a usage limit".into());
        }
        if &state["token"] != token {
            return Err("This session changed. Wait for it to refresh and try again.".into());
        }
        let eligible = if automatic.is_some() {
            "canAutoResume"
        } else {
            "canResume"
        };
        if state[eligible] != true {
            return Err(format!(
                "{}. Check the session on desktop or try again after the allowance resets.",
                state["resetLabel"]
                    .as_str()
                    .unwrap_or("Cannot continue right now")
            ));
        }
        if let Some(enabled) = automatic {
            self.set_usage_resume_at_reset(enabled, cx);
        } else {
            self.resume_usage(cx);
        }
        Ok(())
    }

    pub(super) fn set_usage_resume_at_reset(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if let Some(limit) = &mut self.settings.usage_limit {
            limit.resume_at_reset = enabled;
            self.save_settings(cx);
            cx.emit(SessionChanged);
            cx.notify();
        }
    }

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

/// Saved and live watches use the same desktop-owned state and reset wording.
pub(crate) fn usage_state(
    session_id: i64,
    provider: &str,
    model: Option<&str>,
    settings: &Settings,
    limit: Option<&monocode_core::usage::Limit>,
    busy: bool,
    remote: bool,
) -> serde_json::Value {
    let Some(limit) = limit else {
        return serde_json::Value::Null;
    };
    let now = monocode_core::usage::now_ms();
    let waiting = limit.waiting(now);
    let supported = !remote && !settings.worktree_removed;
    // Match the desktop notice: automatic controls while waiting, manual
    // continuation after a known reset or when its time is unknown.
    // Use persisted turn metadata, not the paged transcript's row count, so
    // opening or paging an unchanged saved conversation cannot invalidate it.
    serde_json::json!({
        "token": serde_json::json!([
            session_id, provider, model, settings.account.id,
            settings.turns.len(), settings.turns.keys().max(), limit
        ]).to_string(),
        "resetsAt": limit.resets_at,
        "resumeAtReset": limit.resume_at_reset,
        "canResume": supported && !busy && !waiting,
        "canAutoResume": supported && !busy && waiting,
        "resetLabel": super::accounts::usage_reset_label(limit, now).unwrap_or_else(|| "Reset time unknown".into()),
        "limitation": if settings.worktree_removed {
            "This session’s working copy was removed. Select a branch or worktree on desktop to continue."
        } else if remote {
            "Continue this session on its remote desktop. Mobile usage-limit recovery is unavailable for this session."
        } else {
            "Automatic continuation requires Monocode running with this session loaded. Your preference is saved, but scheduling resumes only when the session loads again."
        },
    })
}

#[cfg(test)]
mod usage_tests {
    use super::*;
    use monocode_core::usage::{self, Limit};

    #[test]
    fn usage_capabilities_use_actual_reset_and_desktop_wording() {
        let reset = usage::now_ms() + 7 * 24 * 3_600_000;
        let limit = Limit {
            resets_at: Some(reset),
            resume_at_reset: false,
        };
        let state = usage_state(
            1,
            "codex",
            None,
            &Settings::default(),
            Some(&limit),
            false,
            false,
        );
        assert_eq!(state["resetsAt"], reset);
        assert_eq!(state["canResume"], false);
        assert_eq!(state["canAutoResume"], true);
        assert!(state["resetLabel"].as_str().unwrap().starts_with("Resets "));
        let unknown = usage_state(
            1,
            "codex",
            None,
            &Settings::default(),
            Some(&Limit::default()),
            false,
            false,
        );
        assert_eq!(unknown["resetLabel"], "Reset time unknown");
        assert_eq!(unknown["canResume"], true);
        assert_eq!(unknown["canAutoResume"], false);
        for (busy, remote) in [(true, false), (false, true)] {
            let blocked = usage_state(
                1,
                "codex",
                None,
                &Settings::default(),
                Some(&limit),
                busy,
                remote,
            );
            assert_eq!(blocked["canResume"], false);
            assert_eq!(blocked["canAutoResume"], false);
        }
        assert_ne!(
            state["token"],
            usage_state(
                2,
                "codex",
                None,
                &Settings::default(),
                Some(&limit),
                false,
                false
            )["token"]
        );
        assert!(usage_state(1, "codex", None, &Settings::default(), None, false, false).is_null());
    }

    #[test]
    fn automatic_controls_exist_only_while_waiting_even_if_already_armed() {
        let now = usage::now_ms();
        for reset in [None, Some(now - 60_000), Some(now + 60_000)] {
            for armed in [false, true] {
                let limit = Limit {
                    resets_at: reset,
                    resume_at_reset: armed,
                };
                let state = usage_state(
                    1,
                    "codex",
                    None,
                    &Settings::default(),
                    Some(&limit),
                    false,
                    false,
                );
                assert_eq!(state["canAutoResume"], limit.waiting(now));
                assert_eq!(state["canResume"], !limit.waiting(now));
            }
        }
    }

    #[gpui::test]
    fn mobile_usage_rejects_removed_worktrees_and_changed_model_account_or_desktop_resume(
        cx: &mut gpui::TestAppContext,
    ) {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(std::path::Path::new(":memory:")).unwrap(),
        ));
        let summary = locked(&store).unwrap().create("codex", "/fixture").unwrap();
        let view = cx.new(|cx| Conversation::new(summary, store, cx));
        cx.run_until_parked();
        view.update(cx, |v, cx| {
            v.loaded = true;
            v.settings.usage_limit = Some(Limit::default());
            let token = v.mobile_usage_limit(cx)["token"].clone();
            v.model = Some("different-model".into());
            assert!(
                v.mobile_resume_usage(&token, None, cx)
                    .unwrap_err()
                    .contains("changed")
            );
            v.model = None;
            v.settings.account.id = "different-account".into();
            assert!(v.mobile_resume_usage(&token, None, cx).is_err());
            v.settings.account = Default::default();
            v.settings.worktree_removed = true;
            assert_eq!(v.mobile_usage_limit(cx)["canResume"], false);
            assert_eq!(v.mobile_usage_limit(cx)["canAutoResume"], false);
            assert!(
                v.mobile_resume_usage(&token, None, cx)
                    .unwrap_err()
                    .contains("working copy was removed")
            );
            assert!(v.settings.usage_limit.is_some());
            v.settings.worktree_removed = false;
            // A desktop dismissal/resume invalidates every phone's old action.
            v.settings.usage_limit = None;
            assert!(
                v.mobile_resume_usage(&token, None, cx)
                    .unwrap_err()
                    .contains("no longer paused")
            );
        });
    }

    #[gpui::test]
    fn mobile_controls_persist_the_desktop_preference_and_use_its_mono_resume_and_scheduler(
        cx: &mut gpui::TestAppContext,
    ) {
        let store = Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(std::path::Path::new(":memory:")).unwrap(),
        ));
        let summary = locked(&store).unwrap().create("codex", "/fixture").unwrap();
        let id = summary.id;
        let view = cx.new(|cx| Conversation::new(summary, store.clone(), cx));
        cx.run_until_parked();
        let resumed = std::rc::Rc::new(std::cell::Cell::new(0));
        cx.update(|cx| {
            let count = resumed.clone();
            cx.subscribe(&view, move |_, _: &crate::conversation::UsageResumed, _| {
                count.set(count.get() + 1)
            })
            .detach();
        });
        view.update(cx, |v, cx| {
            v.loaded = true;
            v.settings.mono = Some(monocode_core::agent::MonoContext {
                mono: Default::default(),
                root: "/fixture".into(),
                executable: "/fixture/binary".into(),
            });
            v.settings.usage_limit = Some(Limit {
                resets_at: Some(usage::now_ms() + 60_000),
                resume_at_reset: false,
            });
            let token = v.mobile_usage_limit(cx)["token"].clone();
            assert!(v.mobile_resume_usage(&token, None, cx).is_err());
            assert!(v.mobile_resume_usage(&token, Some(true), cx).is_ok());
            assert!(v.mobile_resume_usage(&token, Some(false), cx).is_err()); // stale preference
            assert!(v.settings.usage_limit.as_ref().unwrap().resume_at_reset);
        });
        cx.run_until_parked();
        assert!(
            locked(&store)
                .unwrap()
                .agent_settings(id)
                .unwrap()
                .unwrap()
                .usage_limit
                .unwrap()
                .resume_at_reset
        );
        // A new desktop conversation restores the same persisted preference;
        // the phone does not own a timer or a second preference store.
        let summary = locked(&store)
            .unwrap()
            .sessions()
            .unwrap()
            .into_iter()
            .find(|s| s.id == id)
            .unwrap();
        let reopened = cx.new(|cx| Conversation::new(summary, store.clone(), cx));
        reopened.update(cx, |v, cx| v.load(cx));
        cx.run_until_parked();
        reopened.update(cx, |v, cx| {
            assert_eq!(v.mobile_usage_limit(cx)["resumeAtReset"], true)
        });

        view.update(cx, |v, cx| {
            let token = v.mobile_usage_limit(cx)["token"].clone();
            v.mobile_resume_usage(&token, Some(false), cx).unwrap();
            v.settings.usage_limit.as_mut().unwrap().resets_at = Some(usage::now_ms() - 31_000);
            v.tick_usage(cx);
            assert!(v.settings.usage_limit.is_some()); // disabling cancels the real scheduler
            let token = v.mobile_usage_limit(cx)["token"].clone();
            v.mobile_resume_usage(&token, None, cx).unwrap();
            assert!(v.mobile_resume_usage(&token, None, cx).is_err()); // duplicate/another device
            assert!(v.settings.usage_limit.is_none());
        });
        cx.run_until_parked();
        assert_eq!(resumed.get(), 1);
        view.update(cx, |v, cx| {
            v.settings.usage_limit = Some(Limit {
                resets_at: Some(usage::now_ms() + 60_000),
                resume_at_reset: false,
            });
            let token = v.mobile_usage_limit(cx)["token"].clone();
            v.mobile_resume_usage(&token, Some(true), cx).unwrap();
            v.settings.usage_limit.as_mut().unwrap().resets_at = Some(usage::now_ms() - 31_000);
            v.tick_usage(cx); // the existing desktop clock, without a phone
            assert!(v.settings.usage_limit.is_none());
            v.tick_usage(cx);
        });
        cx.run_until_parked();
        assert_eq!(resumed.get(), 2);
    }
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
