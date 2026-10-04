use std::collections::HashSet;

use anyhow::{Result, anyhow, bail};
use monocode_core::user_question::{
    CUSTOM_OPTION_ID, UserQuestion, UserQuestionOption, UserQuestionReply,
};
use serde_json::{Map, Value, json};

const OMIT: &str = "__omit__";

/// Conditional fields are asked in their declared order, after earlier answers exist.
pub struct Form {
    pub id: String,
    pub session: String,
    pub title: String,
    fields: Vec<Value>,
    index: usize,
    answers: Map<String, Value>,
    omitted: HashSet<String>,
    retry_visible: Option<String>,
}

impl Form {
    pub fn new(value: &Value) -> Result<Self> {
        let fields = value
            .get("fields")
            .and_then(Value::as_array)
            .filter(|fields| !fields.is_empty())
            .ok_or_else(|| anyhow!("OpenCode form has no fields"))?;
        let mut keys = HashSet::new();
        for field in fields {
            let key = required(field, "key")?;
            if !keys.insert(key) {
                bail!("OpenCode form repeats a field key");
            }
            if !matches!(
                required(field, "type")?,
                "string" | "number" | "integer" | "boolean" | "multiselect" | "external"
            ) {
                bail!("OpenCode form uses an unsupported field type");
            }
        }
        Ok(Self {
            id: required(value, "id")?.into(),
            session: required(value, "sessionID")?.into(),
            title: value
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Question")
                .into(),
            fields: fields.clone(),
            index: 0,
            answers: Map::new(),
            omitted: HashSet::new(),
            retry_visible: None,
        })
    }

    pub fn answer(&self) -> Value {
        self.answers.clone().into()
    }

    /// The server validates JavaScript patterns and string formats authoritatively.
    pub fn retry_rejected(&mut self, message: &str) {
        self.index = self
            .fields
            .iter()
            .position(|field| {
                field["key"]
                    .as_str()
                    .is_some_and(|key| message.ends_with(&format!(": {key}")))
            })
            .unwrap_or(0);
        if let Some(key) = self.fields[self.index]["key"].as_str() {
            self.answers.remove(key);
            self.omitted.remove(key);
            self.retry_visible = Some(key.into());
        }
    }

    pub fn next_question(&mut self) -> Result<Option<UserQuestion>> {
        while let Some(field) = self.fields.get(self.index) {
            let key = required(field, "key")?;
            let active = field
                .get("when")
                .and_then(Value::as_array)
                .is_none_or(|conditions| {
                    conditions.iter().all(|condition| {
                        let Some(value) = condition
                            .get("key")
                            .and_then(Value::as_str)
                            .and_then(|key| self.answers.get(key))
                        else {
                            return false;
                        };
                        let expected = &condition["value"];
                        let equal = value
                            .as_array()
                            .map_or(value == expected, |values| values.contains(expected));
                        match condition["op"].as_str() {
                            Some("eq") => equal,
                            Some("neq") => !equal,
                            _ => false,
                        }
                    })
                });
            if !active {
                self.answers.remove(key);
                self.omitted.remove(key);
                self.index += 1;
                continue;
            }
            if self.answers.contains_key(key) || self.omitted.contains(key) {
                self.index += 1;
                continue;
            }
            if field["hidden"].as_bool() == Some(true) && self.retry_visible.as_deref() != Some(key)
            {
                if let Some(value) = field.get("default") {
                    self.answers.insert(key.into(), value.clone());
                    self.index += 1;
                    continue;
                }
                if field["required"].as_bool() != Some(true) {
                    self.omitted.insert(key.into());
                    self.index += 1;
                    continue;
                }
            }
            let kind = required(field, "type")?;
            let mut options: Vec<_> = field
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|option| {
                    Some(UserQuestionOption {
                        id: required(option, "value").ok()?.into(),
                        label: required(option, "label").ok()?.into(),
                        description: option
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    })
                })
                .collect();
            if kind == "boolean" {
                options = vec![option("true", "Yes"), option("false", "No")];
            }
            if kind == "external" {
                options = vec![option("true", "I completed this step")];
            }
            if kind != "external" && field["required"].as_bool() != Some(true) {
                options.push(option(OMIT, "Leave this field empty"));
            }
            let prompt = if kind == "external" {
                format!(
                    "{}\n{}",
                    field["description"]
                        .as_str()
                        .unwrap_or("Complete the linked step, then confirm."),
                    required(field, "url")?
                )
            } else {
                field["description"]
                    .as_str()
                    .or_else(|| field["title"].as_str())
                    .unwrap_or(key)
                    .to_string()
            };
            return Ok(Some(UserQuestion {
                id: key.into(),
                header: field["title"].as_str().map(str::to_string),
                prompt,
                multi_select: kind == "multiselect",
                allow_custom: match kind {
                    "number" | "integer" => true,
                    "string" => {
                        field.get("options").is_none() || field["custom"].as_bool() == Some(true)
                    }
                    "multiselect" => field["custom"].as_bool() == Some(true),
                    _ => false,
                },
                options,
            }));
        }
        Ok(None)
    }

    /// Invalid answers leave the field active so the UI can ask it again.
    pub fn apply(&mut self, reply: UserQuestionReply) -> Result<()> {
        let UserQuestionReply::Answered { answers, custom } = reply else {
            bail!("OpenCode form was skipped");
        };
        let field = self
            .fields
            .get(self.index)
            .ok_or_else(|| anyhow!("OpenCode form is already complete"))?;
        let key = required(field, "key")?;
        let selected = answers.get(key).cloned().unwrap_or_default();
        if selected.iter().any(|value| value == OMIT) {
            if field["required"].as_bool() == Some(true) || selected.len() != 1 {
                bail!("This field requires an answer");
            }
            self.omitted.insert(key.into());
            self.index += 1;
            return Ok(());
        }
        let mut values: Vec<String> = selected
            .into_iter()
            .filter(|value| value != CUSTOM_OPTION_ID)
            .collect();
        if let Some(value) = custom
            .as_ref()
            .and_then(|custom| custom.get(key))
            .filter(|value| !value.is_empty())
        {
            values.push(value.clone());
        }
        let kind = required(field, "type")?;
        let value = match kind {
            "multiselect" => json!(values),
            _ => {
                if values.len() != 1 {
                    bail!("This field requires one answer");
                }
                let value = &values[0];
                match kind {
                    "boolean" => json!(
                        value
                            .parse::<bool>()
                            .map_err(|_| anyhow!("Choose Yes or No"))?
                    ),
                    "external" if value == "true" => json!(true),
                    "external" => bail!("Confirm that the linked step is complete"),
                    "number" | "integer" => {
                        let number = value
                            .parse::<f64>()
                            .map_err(|_| anyhow!("Enter a number"))?;
                        if !number.is_finite() || kind == "integer" && number.fract() != 0.0 {
                            bail!("Enter a finite {kind}");
                        }
                        json!(number)
                    }
                    _ => json!(value),
                }
            }
        };
        validate(field, &value)?;
        self.answers.insert(key.into(), value);
        self.retry_visible = None;
        self.index += 1;
        Ok(())
    }
}

fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("OpenCode form is missing {key}"))
}

fn option(id: &str, label: &str) -> UserQuestionOption {
    UserQuestionOption {
        id: id.into(),
        label: label.into(),
        description: None,
    }
}

fn validate(field: &Value, value: &Value) -> Result<()> {
    if let Some(number) = value.as_f64()
        && (field["minimum"].as_f64().is_some_and(|min| number < min)
            || field["maximum"].as_f64().is_some_and(|max| number > max))
    {
        bail!("The number is outside this field's limits");
    }
    let size = value
        .as_str()
        .map(|value| value.encode_utf16().count())
        .or_else(|| value.as_array().map(Vec::len));
    if let Some(size) = size {
        let (min, max) = if value.is_array() {
            ("minItems", "maxItems")
        } else {
            ("minLength", "maxLength")
        };
        if field["required"].as_bool() == Some(true) && size == 0
            || field[min].as_u64().is_some_and(|min| (size as u64) < min)
            || field[max].as_u64().is_some_and(|max| (size as u64) > max)
        {
            bail!("The answer is outside this field's limits");
        }
    }
    if let Some(options) = field["options"]
        .as_array()
        .filter(|_| field["custom"].as_bool() != Some(true))
    {
        let allowed = |value: &Value| {
            options
                .iter()
                .any(|option| option.get("value") == Some(value))
        };
        if value.as_array().map_or(!allowed(value), |values| {
            values.iter().any(|value| !allowed(value))
        }) {
            bail!("Choose a listed option");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn reply(key: &str, values: &[&str]) -> UserQuestionReply {
        UserQuestionReply::Answered {
            answers: BTreeMap::from([(
                key.into(),
                values.iter().map(|value| value.to_string()).collect(),
            )]),
            custom: None,
        }
    }

    #[test]
    fn rejected_hidden_default_requires_a_user_answer_instead_of_resubmitting_it() {
        let mut form = Form::new(&json!({"id":"frm_owned","sessionID":"ses_owned","fields":[{"key":"address","type":"string","format":"email","required":true,"hidden":true,"default":"invalid"}]})).unwrap();
        assert!(form.next_question().unwrap().is_none());
        form.retry_rejected("Expected email for form field: address");
        assert_eq!(form.next_question().unwrap().unwrap().id, "address");
        form.apply(reply("address", &["owner@example.test"]))
            .unwrap();
        assert!(form.next_question().unwrap().is_none());
        assert_eq!(form.answer(), json!({"address":"owner@example.test"}));
    }

    #[test]
    fn optional_omission_survives_a_later_authoritative_retry() {
        let mut form = Form::new(&json!({"id":"frm_owned","sessionID":"ses_owned","fields":[{"key":"nickname","type":"string"},{"key":"address","type":"string","required":true}]})).unwrap();
        form.next_question().unwrap();
        form.apply(reply("nickname", &[OMIT])).unwrap();
        form.next_question().unwrap();
        form.apply(reply("address", &["invalid"])).unwrap();
        form.retry_rejected("Expected email for form field: address");
        assert_eq!(form.next_question().unwrap().unwrap().id, "address");
        assert!(form.answer().get("nickname").is_none());
    }

    #[test]
    fn preserves_typed_answers_and_conditional_visibility() {
        let mut form = Form::new(&json!({"id":"frm_owned","sessionID":"ses_owned","title":"Choices","fields":[
            {"key":"enabled","type":"boolean","required":true},
            {"key":"irrelevant","type":"string","required":true,"when":[{"key":"enabled","op":"eq","value":false}]},
            {"key":"count","type":"integer","minimum":1,"maximum":5,"required":true,"when":[{"key":"enabled","op":"eq","value":true}]},
            {"key":"tags","type":"multiselect","options":[{"value":"one","label":"First"},{"value":"two","label":"Second"}],"required":true},
            {"key":"hidden","type":"string","hidden":true,"default":"default"}
        ]})).unwrap();
        assert_eq!(form.next_question().unwrap().unwrap().id, "enabled");
        form.apply(reply("enabled", &["true"])).unwrap();
        assert_eq!(form.next_question().unwrap().unwrap().id, "count");
        assert!(form.apply(reply("count", &["1.5"])).is_err());
        assert_eq!(form.next_question().unwrap().unwrap().id, "count");
        form.apply(reply("count", &["3"])).unwrap();
        assert!(form.next_question().unwrap().unwrap().multi_select);
        form.apply(reply("tags", &["one", "two"])).unwrap();
        assert!(form.next_question().unwrap().is_none());
        assert_eq!(
            form.answer(),
            json!({"enabled":true,"count":3.0,"tags":["one","two"],"hidden":"default"})
        );
    }

    #[test]
    fn external_fields_require_explicit_acknowledgement_and_optional_fields_can_be_omitted() {
        let mut form = Form::new(&json!({"id":"frm_owned","sessionID":"ses_owned","fields":[{"key":"link","type":"external","url":"https://example.test/task"},{"key":"note","type":"string"}]})).unwrap();
        assert!(
            form.next_question()
                .unwrap()
                .unwrap()
                .prompt
                .contains("https://example.test/task")
        );
        assert!(form.apply(reply("link", &["false"])).is_err());
        form.apply(reply("link", &["true"])).unwrap();
        form.next_question().unwrap();
        form.apply(reply("note", &[OMIT])).unwrap();
        assert!(form.next_question().unwrap().is_none());
        assert_eq!(form.answer(), json!({"link":true}));
    }

    #[test]
    fn authoritative_invalid_string_retry_keeps_prior_typed_answers() {
        let mut form = Form::new(&json!({"id":"frm_owned","sessionID":"ses_owned","fields":[{"key":"enabled","type":"boolean","required":true},{"key":"address","type":"string","format":"email","pattern":"(?=.+@)","required":true}]})).unwrap();
        form.next_question().unwrap();
        form.apply(reply("enabled", &["true"])).unwrap();
        form.next_question().unwrap();
        form.apply(reply("address", &["invalid"])).unwrap();
        assert!(form.next_question().unwrap().is_none());
        form.retry_rejected("Expected email for form field: address");
        assert_eq!(form.next_question().unwrap().unwrap().id, "address");
        assert_eq!(form.answer(), json!({"enabled":true}));
        form.apply(reply("address", &["owner@example.test"]))
            .unwrap();
        assert!(form.next_question().unwrap().is_none());
        assert_eq!(
            form.answer(),
            json!({"enabled":true,"address":"owner@example.test"})
        );
    }
}
