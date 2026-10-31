//! The shared question contract and each provider's original reply shape.
use super::*;
use crate::agent::{Question, QuestionItem, QuestionOption};
impl Protocol {
    pub(super) fn question(
        &mut self,
        wire: Value,
        kind: &str,
        request: Value,
        events: &mut Vec<Event>,
        stdin: &mut impl Write,
    ) -> Result<(), String> {
        if let Err(error) =
            self.collect_question(wire.clone(), kind, request.clone(), events, stdin)
        {
            events.push(Event::Status(error));
            let question = Question {
                id: wire.to_string(),
                title: String::new(),
                items: Vec::new(),
                auto_resolve_at: None,
            };
            respond(stdin, wire, kind, &request, &question, Decision::Deny)?;
        }
        Ok(())
    }

    fn collect_question(
        &mut self,
        wire: Value,
        kind: &str,
        request: Value,
        events: &mut Vec<Event>,
        stdin: &mut impl Write,
    ) -> Result<(), String> {
        let id = wire.to_string();
        let raw = if request["questions"].is_array() {
            &request
        } else {
            request
                .get("input")
                .or_else(|| request.get("params"))
                .or_else(|| request.get("question"))
                .unwrap_or(&request)
        };
        let rows: Vec<Value> = if kind == "pi" {
            vec![
                json!({"id":request["id"],"question":request["title"],"options":request["options"],"allowCustom":request["method"] != "select"}),
            ]
        } else {
            raw["questions"].as_array().cloned().unwrap_or_default()
        };
        let mut items = Vec::new();
        let mut seen = std::collections::HashSet::new();
        if kind == "codex" && rows.is_empty() {
            return Err("Codex sent an unsupported user-input request.".into());
        }
        for (ix, row) in rows.iter().enumerate() {
            if row["isSecret"] == true {
                return Err("The provider requested secret input, which cannot be collected in the conversation.".into());
            }
            if kind == "codex"
                && (row["id"]
                    .as_str()
                    .is_none_or(|id| id.is_empty() || id.trim() != id)
                    || row["question"].as_str().is_none_or(|q| q.trim().is_empty())
                    || row
                        .get("options")
                        .filter(|v| !v.is_null())
                        .is_some_and(|v| {
                            !v.is_array()
                                || v.as_array().is_some_and(|options| {
                                    options.iter().any(|o| {
                                        o["label"].as_str().is_none_or(|l| l.trim().is_empty())
                                    })
                                })
                        }))
            {
                return Err("Codex sent an unsupported user-input question.".into());
            }
            let Some(prompt) = row["question"]
                .as_str()
                .or_else(|| row["prompt"].as_str())
                .or_else(|| row["text"].as_str())
                .or_else(|| row["header"].as_str())
            else {
                continue;
            };
            let id = row["id"].as_str().map(str::to_owned).unwrap_or_else(|| {
                if kind == "codex" {
                    format!("q{}", ix + 1)
                } else {
                    prompt.to_owned()
                }
            });
            if !seen.insert(id.clone()) {
                return Err("Provider sent duplicate question IDs".into());
            }
            let options: Vec<_> = row["options"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|option| {
                    let label = option
                        .as_str()
                        .or_else(|| option["label"].as_str())
                        .or_else(|| option["value"].as_str())
                        .or_else(|| option["text"].as_str())
                        .or_else(|| option["id"].as_str())?;
                    Some(QuestionOption {
                        id: option["id"]
                            .as_str()
                            .or_else(|| option["optionId"].as_str())
                            .or_else(|| option["value"].as_str())
                            .map(str::to_owned)
                            .unwrap_or_else(|| label.to_owned()),
                        label: label.into(),
                        description: option["description"]
                            .as_str()
                            .or_else(|| option["detail"].as_str())
                            .unwrap_or("")
                            .into(),
                    })
                })
                .collect();
            let allow_custom = if kind == "codex" || kind == "codex-async" {
                options.is_empty() || row["isOther"] == true
            } else {
                row["custom"]
                    .as_bool()
                    .or_else(|| row["allowCustom"].as_bool())
                    .unwrap_or(kind != "cursor" && kind != "pi")
                    || options.is_empty()
            };
            items.push(QuestionItem {
                id,
                prompt: prompt.into(),
                multiple: row["multiSelect"] == true
                    || row["multiple"] == true
                    || row["allowMultiple"] == true,
                allow_custom,
                options,
            });
        }
        let question = Question {
            id: id.clone(),
            title: request["title"]
                .as_str()
                .unwrap_or("The agent has a question")
                .into(),
            items,
            auto_resolve_at: None,
        };
        if question.items.is_empty() {
            return respond(stdin, wire, kind, &request, &question, Decision::Deny);
        }
        self.questions
            .insert(id.clone(), (wire, kind.into(), request, question));
        self.question_order.push_back(id);
        events.extend(self.show_next_question());
        Ok(())
    }

    pub(super) fn show_next_question(&mut self) -> Vec<Event> {
        if self
            .visible_question
            .as_ref()
            .is_some_and(|id| self.questions.contains_key(id))
        {
            return Vec::new();
        }
        self.visible_question = None;
        while let Some(id) = self.question_order.pop_front() {
            let Some((_, kind, request, question)) = self.questions.get_mut(&id) else {
                continue;
            };
            if request["isBlocking"] == false || kind == "codex-async" {
                question.auto_resolve_at = Some(crate::time::now_ms().saturating_add(120_000));
            }
            self.visible_question = Some(id);
            return vec![Event::Question(question.clone())];
        }
        Vec::new()
    }

    pub(super) fn expire_questions(
        &mut self,
        stdin: &mut impl Write,
    ) -> Result<Vec<Event>, String> {
        let mut events = Vec::new();
        let expired = self.visible_question.as_ref().and_then(|id| {
            self.questions
                .get(id)
                .and_then(|(_, _, _, q)| q.auto_resolve_at)
                .filter(|at| *at <= crate::time::now_ms())
                .map(|_| id.clone())
        });
        if let Some(id) = expired {
            self.respond(
                Control {
                    id: id.clone(),
                    decision: Decision::Deny,
                },
                stdin,
            )?;
            events.push(Event::QuestionResolved(id));
        }
        events.extend(self.show_next_question());
        Ok(events)
    }

    pub(super) fn codex_async_questions(
        &mut self,
        value: &Value,
        events: &mut Vec<Event>,
        stdin: &mut impl Write,
    ) -> Result<(), String> {
        let item = &value["params"]["item"];
        if item["type"] != "agentMessage"
            || item["delivery"] != "async"
            || item["questions"].is_null()
        {
            return Ok(());
        }
        let Some(id) = item["id"].as_str() else {
            return Ok(());
        };
        if !self.async_question_items.insert(id.into()) {
            return Ok(());
        }
        let Some(rows) = item["questions"].as_array() else {
            events.push(Event::Status(
                "Codex sent an unsupported async user-input request.".into(),
            ));
            return Ok(());
        };
        let mut questions = Vec::new();
        for (ix, row) in rows.iter().enumerate() {
            if row["title"].as_str().is_none_or(|t| t.trim().is_empty())
                || row
                    .get("options")
                    .filter(|v| !v.is_null())
                    .is_some_and(|v| {
                        !v.is_array()
                            || v.as_array().is_some_and(|a| {
                                a.iter()
                                    .any(|o| o.as_str().is_none_or(|t| t.trim().is_empty()))
                            })
                    })
            {
                events.push(Event::Status(
                    "Codex sent an unsupported async user-input question.".into(),
                ));
                return Ok(());
            }
            questions.push(json!({"id":format!("q{}", ix+1),"question":row["title"],"options":row["options"],"isOther":true}));
        }
        self.question(json!(format!("native-async-{id}")), "codex-async",
            json!({"questions":questions,"threadId":self.request.provider_id,"turnId":self.active_turn,"isBlocking":false}), events, stdin)
    }
}

pub(super) fn respond(
    stdin: &mut impl Write,
    wire: Value,
    kind: &str,
    request: &Value,
    question: &Question,
    decision: Decision,
) -> Result<(), String> {
    let answered = matches!(decision, Decision::Answer(_));
    let answers = if let Decision::Answer(answers) = decision {
        answers
    } else {
        Vec::new()
    };
    let pairs = question
        .items
        .iter()
        .enumerate()
        .map(|(ix, q)| (q, answers.get(ix).cloned().unwrap_or_default()));
    let result = match kind {
        "codex" => {
            json!({"answers":pairs.filter(|(_,a)|!a.is_empty()).map(|(q,a)|(q.id.clone(),json!({"answers":a}))).collect::<serde_json::Map<_,_>>()})
        }
        "codex-async" => return Ok(()), // Answers are sent through turn/steer by Protocol::respond.
        "claude" => {
            let mut input = request["input"].clone();
            input["answers"] = pairs
                .map(|(q, a)| (q.prompt.clone(), json!(a.join(", "))))
                .collect::<serde_json::Map<_, _>>()
                .into();
            return write_json(
                stdin,
                json!({"type":"control_response","response":{"subtype":"success","request_id":wire,"response":if answered {json!({"behavior":"allow","updatedInput":input})} else {json!({"behavior":"deny","message":"User skipped the question"})}}}),
            );
        }
        "cursor" => {
            if !answered {
                json!({"outcome":{"outcome":"skipped","reason":"User skipped"}})
            } else {
                json!({"outcome":{"outcome":"answered","answers":pairs.map(|(q,a)|json!({"questionId":q.id,"selectedOptionIds":q.options.iter().filter(|o|a.contains(&o.label)).map(|o|&o.id).collect::<Vec<_>>()})).collect::<Vec<_>>()}})
            }
        }
        "grok" => {
            if !answered {
                json!({"outcome":"skip_interview"})
            } else {
                json!({"outcome":"accepted","answers":pairs.map(|(q,a)|(q.prompt.clone(),if q.multiple {json!(a)} else {json!(a.first().cloned().unwrap_or_default())})).collect::<serde_json::Map<_,_>>()})
            }
        }
        "pi" => {
            return write_json(
                stdin,
                if let Some(value) = answers.first().and_then(|a| a.first()) {
                    json!({"type":"extension_ui_response","id":wire,"value":value})
                } else {
                    json!({"type":"extension_ui_response","id":wire,"cancelled":true})
                },
            );
        }
        _ => json!({}),
    };
    write_json(stdin, json!({"jsonrpc":"2.0","id":wire,"result":result}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn protocol(kind: Kind) -> Protocol {
        Protocol::new(Request {
            kind,
            binary: "unused".into(),
            cwd: "/tmp".into(),
            prompt: "Hello".into(),
            provider_id: None,
            model: None,
            settings: Default::default(),
        })
    }
    fn answer(p: &mut Protocol, event: &Event, answers: Vec<Vec<String>>) -> Value {
        let Event::Question(question) = event else {
            panic!("expected question")
        };
        let mut out = Vec::new();
        p.respond(
            Control {
                id: question.id.clone(),
                decision: Decision::Answer(answers),
            },
            &mut out,
        )
        .unwrap();
        serde_json::from_slice(&out).unwrap()
    }
    #[test]
    fn codex_questions_wait_for_answers_and_reply_by_question_id() {
        let mut p = protocol(Kind::Codex);
        let mut out = Vec::new();
        let events = p.frame(json!({"id":"request","method":"item/tool/requestUserInput","params":{"questions":[{"id":"q","question":"Choose","options":[{"label":"A"},{"label":"B"}]}]}}),&mut out).unwrap();
        assert!(out.is_empty());
        let response = answer(&mut p, &events[0], vec![vec!["B".into()]]);
        assert_eq!(response["result"]["answers"]["q"]["answers"], json!(["B"]));
        assert!(p.questions.is_empty());
    }
    #[test]
    fn cursor_answers_keep_opaque_option_ids_and_grok_uses_prompts() {
        for (kind, method) in [
            (Kind::Cursor, "cursor/ask_question"),
            (Kind::Grok, "_x.ai/ask_user_question"),
        ] {
            let mut p = protocol(kind);
            let events = p.frame(json!({"id":99,"method":method,"params":{"questions":[{"id":"q","question":"Choose","options":[{"id":"opaque","label":"A"}]}]}}),&mut Vec::new()).unwrap();
            let response = answer(&mut p, &events[0], vec![vec!["A".into()]]);
            if kind == Kind::Cursor {
                assert_eq!(
                    response["result"]["outcome"]["answers"][0]["selectedOptionIds"],
                    json!(["opaque"])
                );
            } else {
                assert_eq!(response["result"]["answers"]["Choose"], "A");
            }
        }
    }
    #[test]
    fn claude_questions_return_the_original_input_and_typed_answers() {
        let mut p = protocol(Kind::Claude);
        let events = p.frame(json!({"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","input":{"questions":[{"question":"Name?","options":[]}]}}}),&mut Vec::new()).unwrap();
        let response = answer(&mut p, &events[0], vec![vec!["Native".into()]]);
        assert_eq!(
            response["response"]["response"]["updatedInput"]["answers"]["Name?"],
            "Native"
        );
        assert_eq!(
            response["response"]["response"]["updatedInput"]["questions"][0]["question"],
            "Name?"
        );
    }
    #[test]
    fn omp_extension_questions_and_skips_resolve_the_provider_request() {
        let mut p = protocol(Kind::Omp);
        let events = p.frame(json!({"type":"extension_ui_request","id":"r","method":"select","title":"Which?","options":["First","Second"]}),&mut Vec::new()).unwrap();
        let response = answer(&mut p, &events[0], vec![vec!["Second".into()]]);
        assert_eq!(
            response,
            json!({"type":"extension_ui_response","id":"r","value":"Second"})
        );
    }
    #[test]
    fn secret_and_duplicate_questions_are_rejected() {
        for rows in [
            json!([{"id":"q","question":"Secret?","isSecret":true}]),
            json!([{"id":"q","question":"A?"},{"id":"q","question":"B?"}]),
        ] {
            let mut p = protocol(Kind::Codex);
            let mut out = Vec::new();
            let events = p.frame(json!({"id":1,"method":"item/tool/requestUserInput","params":{"questions":rows}}), &mut out).unwrap();
            assert!(events.iter().any(|event| matches!(event, Event::Status(_))));
            assert!(!events
                .iter()
                .any(|event| matches!(event, Event::Question(_) | Event::Done)));
            let response: Value = serde_json::from_slice(&out).unwrap();
            assert_eq!(response["id"], 1);
            assert_eq!(response["result"]["answers"], json!({}));
            assert!(p.questions.is_empty());
            assert!(!p.done);
        }
    }
}
