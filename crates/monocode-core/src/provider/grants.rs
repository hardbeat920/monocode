//! Always allow: what each provider's confirmation saves, and the check that
//! answers a saved one before the card is shown.
use super::*;
use crate::permissions::{Grant, Key, Subject};

impl Protocol {
    /// Plan and read-only turns never use or create saved grants.
    pub(super) fn grants_apply(&self) -> bool {
        !self.request.settings.plan && self.request.settings.access != AccessMode::ReadOnly
    }

    /// Scopes `subject` to this repository, provider, account and installation.
    fn key(&self, subject: Subject) -> Option<Key> {
        let settings = &self.request.settings;
        let kind = self.request.kind;
        let mut account_home = if crate::accounts::supports_profiles(kind) {
            settings.account.home(kind).ok()?
        } else {
            PathBuf::new()
        };
        let variable = match kind {
            Kind::Codex => Some("CODEX_HOME"),
            Kind::Claude => Some("CLAUDE_CONFIG_DIR"),
            _ => None,
        };
        if settings.account.id == crate::accounts::DEFAULT {
            if let Some(home) = variable
                .and_then(|v| settings.environment.get(v))
                .filter(|s| !s.is_empty())
            {
                account_home = home.into();
            }
        }
        if kind == Kind::Codex {
            if let Some(mono) = &settings.mono {
                if let Some(home) =
                    crate::mono::codex_home(&mono.root, &settings.account.id).ok()?
                {
                    account_home = home;
                }
            }
        }
        if !account_home.as_os_str().is_empty() {
            account_home = crate::permissions::project(&account_home);
        }
        Some(Key {
            project: crate::permissions::repository(&self.request.cwd),
            provider: kind.id().into(),
            account: settings.account.id.clone(),
            account_home,
            // Codex standalone installs move on every update; other CLIs keep
            // a stable launcher path, so the link is not resolved.
            binary: if kind == Kind::Codex {
                crate::permissions::codex_binary(&self.request.binary)
            } else {
                self.request.binary.clone()
            },
            subject,
        })
    }

    /// Records what Always allow would save for approval `id`. Returns true
    /// when saved grants already cover it, so the caller allows it instead.
    pub(super) fn offer(&mut self, id: &str, subjects: Vec<Subject>, title: &str) -> bool {
        self.offers.remove(id);
        if !self.grants_apply() || subjects.is_empty() {
            return false;
        }
        let Some(grants) = subjects
            .into_iter()
            .map(|subject| Some(Grant::new(self.key(subject)?, title)))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        let keys: Vec<_> = grants.iter().map(|g| g.key.clone()).collect();
        if self.permissions.covers(&keys) {
            return true;
        }
        self.offers.insert(id.into(), grants);
        false
    }

    pub(super) fn remember_scope(&self, id: &str) -> Option<String> {
        crate::permissions::scope(self.offers.get(id)?)
    }

    /// Saves the grants offered for `id`, or returns the failure to show.
    /// The request stays pending on failure.
    pub(super) fn save_offer(&mut self, id: &str) -> Option<Event> {
        let result = self
            .offers
            .get(id)
            .filter(|_| self.grants_apply())
            .cloned()
            .ok_or_else(|| "This request cannot be remembered".to_owned())
            .and_then(|grants| self.permissions.allow_all(grants));
        match result {
            Ok(()) => {
                self.offers.remove(id);
                None
            }
            Err(error) => Some(Event::ApprovalSaveFailed {
                id: id.into(),
                error: format!("{error}. The request is still waiting for your decision."),
            }),
        }
    }

    pub(super) fn remember_approval(
        &mut self,
        id: &str,
        stdin: &mut impl Write,
    ) -> Result<Vec<Event>, String> {
        if !self.approvals.contains_key(id) {
            return Ok(Vec::new());
        }
        if let Some(failed) = self.save_offer(id) {
            return Ok(vec![failed]);
        }
        self.respond(
            Control {
                id: id.into(),
                decision: Decision::Allow,
            },
            stdin,
        )?;
        Ok(vec![Event::ApprovalResolved(id.into())])
    }

    /// Claude `can_use_tool` requests and Codex command, file and permission approvals.
    pub(super) fn approval_subjects(&self, kind: &str, request: &Value) -> Vec<Subject> {
        let cwd = &self.request.cwd;
        if kind == "claude" {
            let tool = request["tool_name"].as_str().unwrap_or_default();
            let input = &request["input"];
            let specific = Subject::mcp_tool(tool).or_else(|| match tool {
                "Bash" => input["command"].as_str().and_then(Subject::command),
                "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => input["file_path"]
                    .as_str()
                    .or_else(|| input["notebook_path"].as_str())
                    .and_then(|path| Subject::edit(cwd, path)),
                "WebFetch" => input["url"].as_str().and_then(Subject::fetch),
                _ => None,
            });
            return specific
                .or_else(|| exact(tool, input))
                .into_iter()
                .collect();
        }
        let item = request["itemId"]
            .as_str()
            .and_then(|id| self.tools.get(id))
            .and_then(|tool| serde_json::from_str::<Value>(&tool.input).ok())
            .unwrap_or_default();
        if kind.contains("fileChange") || kind == "applyPatchApproval" {
            let paths: Vec<String> = match request["fileChanges"].as_object() {
                Some(changes) => changes.keys().cloned().collect(),
                None => item["paths"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| p.as_str().map(str::to_owned))
                    .collect(),
            };
            let subjects: Option<Vec<_>> = paths.iter().map(|p| Subject::edit(cwd, p)).collect();
            return subjects.unwrap_or_default();
        }
        if kind.contains("permissions") {
            return exact("permissions", &request["permissions"])
                .into_iter()
                .collect();
        }
        Subject::command_value(&request["command"])
            .or_else(|| Subject::command_value(&item["command"]))
            .into_iter()
            .collect()
    }

    /// ACP `session/request_permission`, keyed on the tool call's kind.
    pub(super) fn acp_subjects(&self, tool: &Value, kind: Option<&str>) -> Vec<Subject> {
        let raw = tool
            .get("rawInput")
            .or_else(|| tool.get("raw_input"))
            .unwrap_or(&Value::Null);
        let specific = match kind {
            Some("execute") => Subject::command_value(&raw["command"]),
            Some("edit" | "delete" | "move") => {
                let paths: Vec<&str> = tool["locations"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|l| l["path"].as_str())
                    .collect();
                let subjects: Option<Vec<_>> = paths
                    .iter()
                    .map(|p| Subject::edit(&self.request.cwd, p))
                    .collect();
                match subjects {
                    Some(subjects) if !subjects.is_empty() => return subjects,
                    _ => None,
                }
            }
            Some("fetch") => raw["url"].as_str().and_then(Subject::fetch),
            _ => None,
        };
        specific
            .or_else(|| exact(tool["title"].as_str().unwrap_or_default(), raw))
            .into_iter()
            .collect()
    }
}

/// OpenCode `permission.asked`, keyed on its permission and patterns.
pub(super) fn opencode_subjects(cwd: &Path, action: &str, permission: &Value) -> Vec<Subject> {
    let patterns: Vec<&str> = match &permission["patterns"] {
        Value::Array(patterns) => patterns.iter().filter_map(Value::as_str).collect(),
        _ => permission["pattern"].as_str().into_iter().collect(),
    };
    let action = action.to_lowercase();
    let specific: Option<Vec<Subject>> = match action.as_str() {
        "bash" => match patterns.as_slice() {
            [command] => Subject::command(command).map(|s| vec![s]),
            _ => None,
        },
        "edit" | "write" | "patch" if !patterns.is_empty() => {
            patterns.iter().map(|p| Subject::edit(cwd, p)).collect()
        }
        "webfetch" => patterns
            .first()
            .copied()
            .or_else(|| permission["metadata"]["url"].as_str())
            .and_then(Subject::fetch)
            .map(|s| vec![s]),
        _ => None,
    };
    specific.unwrap_or_else(|| {
        let input = if patterns.is_empty() {
            permission["metadata"].clone()
        } else {
            json!(patterns)
        };
        exact(&action, &input).into_iter().collect()
    })
}

/// The fallback: the same tool with exactly the same input.
pub(super) fn exact(tool: &str, input: &Value) -> Option<Subject> {
    (!tool.trim().is_empty()).then(|| Subject::Request {
        tool: tool.into(),
        input: input.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        store: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        Fixture {
            store: dir.path().join("permissions.json"),
            root,
            _dir: dir,
        }
    }

    fn protocol(f: &Fixture, kind: Kind) -> Protocol {
        let mut p = Protocol::new(Request {
            kind,
            binary: "unused".into(),
            cwd: f.root.clone(),
            prompt: String::new(),
            provider_id: None,
            model: None,
            settings: Settings::default(),
        });
        p.permissions = crate::permissions::Store::new(f.store.clone());
        p.acp = acp::State::prompting();
        p
    }

    fn last(out: &[u8]) -> Value {
        let line = String::from_utf8_lossy(out)
            .lines()
            .last()
            .unwrap()
            .to_owned();
        serde_json::from_str(&line).unwrap()
    }

    /// Asks once and saves with Always allow, then checks a fresh session
    /// answers `same` by itself and still asks about `different`.
    fn remembers(kind: Kind, first: Value, same: Value, different: Value) -> Vec<u8> {
        let f = fixture();
        let mut p = protocol(&f, kind);
        let mut out = Vec::new();
        let events = p.frame(first, &mut out).unwrap();
        let [Event::Approval(approval)] = events.as_slice() else {
            panic!("expected an approval, got {events:?}");
        };
        assert!(approval.remember_scope.is_some(), "{approval:?}");
        assert!(out.is_empty());
        assert_eq!(
            p.remember_approval(&approval.id, &mut out).unwrap(),
            vec![Event::ApprovalResolved(approval.id.clone())]
        );
        let mut later = protocol(&f, kind);
        let mut answered = Vec::new();
        assert!(later.frame(same, &mut answered).unwrap().is_empty());
        assert!(!answered.is_empty());
        let mut out = Vec::new();
        assert!(matches!(
            later.frame(different, &mut out).unwrap().as_slice(),
            [Event::Approval(_)]
        ));
        assert!(out.is_empty());
        answered
    }

    fn claude(tool: &str, input: Value) -> Value {
        json!({"type":"control_request","request_id":"r","request":{"subtype":"can_use_tool","tool_name":tool,"input":input}})
    }

    #[test]
    fn claude_grants_match_the_command_mcp_tool_file_host_or_exact_request() {
        for (first, same, different) in [
            (
                claude(
                    "Bash",
                    json!({"command":"cargo build","description":"Build"}),
                ),
                claude(
                    "Bash",
                    json!({"command":"cargo build","description":"Again"}),
                ),
                claude("Bash", json!({"command":"cargo build && rm -rf ~"})),
            ),
            (
                claude("mcp__linear__save_issue", json!({"title":"a"})),
                claude("mcp__linear__save_issue", json!({"title":"b"})),
                claude("mcp__linear__delete_issue", json!({"id":"a"})),
            ),
            (
                claude("Write", json!({"file_path":"src/a.rs","content":"x"})),
                claude(
                    "Edit",
                    json!({"file_path":"src/a.rs","old_string":"x","new_string":"y"}),
                ),
                claude(
                    "Edit",
                    json!({"file_path":"src/b.rs","old_string":"x","new_string":"y"}),
                ),
            ),
            (
                claude(
                    "WebFetch",
                    json!({"url":"https://docs.rs/serde","prompt":"a"}),
                ),
                claude(
                    "WebFetch",
                    json!({"url":"https://docs.rs/tokio","prompt":"b"}),
                ),
                claude("WebFetch", json!({"url":"https://example.com"})),
            ),
            (
                claude("Skill", json!({"skill":"deploy"})),
                claude("Skill", json!({"skill":"deploy"})),
                claude("Skill", json!({"skill":"other"})),
            ),
        ] {
            let out = remembers(Kind::Claude, first, same, different);
            assert_eq!(last(&out)["response"]["response"]["behavior"], "allow");
        }
    }

    #[test]
    fn codex_grants_cover_commands_and_every_changed_file() {
        let out = remembers(
            Kind::Codex,
            json!({"id":1,"method":"item/commandExecution/requestApproval","params":{"command":"cargo build","itemId":"c1"}}),
            json!({"id":2,"method":"item/commandExecution/requestApproval","params":{"command":"cargo build","itemId":"c2"}}),
            json!({"id":3,"method":"item/commandExecution/requestApproval","params":{"command":"cargo publish","itemId":"c3"}}),
        );
        assert_eq!(last(&out)["result"]["decision"], "accept");
        let patch = |id: u64, files: &[&str]| {
            let changes: serde_json::Map<_, _> = files
                .iter()
                .map(|f| (f.to_string(), json!({"type":"update"})))
                .collect();
            json!({"id":id,"method":"applyPatchApproval","params":{"fileChanges":changes}})
        };
        let out = remembers(
            Kind::Codex,
            patch(1, &["src/a.rs", "src/b.rs"]),
            patch(2, &["src/b.rs"]),
            patch(3, &["src/a.rs", "src/c.rs"]),
        );
        assert_eq!(last(&out)["result"]["decision"], "approved");
    }

    #[test]
    fn acp_and_pi_grants_answer_with_a_one_time_allow() {
        let permission = |id: u64, tool: Value| {
            json!({"id":id,"method":"session/request_permission","params":{"toolCall":tool,
                "options":[{"kind":"allow_once","optionId":"yes"},{"kind":"allow_always","optionId":"always"},{"kind":"reject_once","optionId":"no"}]}})
        };
        let out = remembers(
            Kind::Antigravity,
            permission(
                1,
                json!({"kind":"execute","rawInput":{"command":["npm","test"]}}),
            ),
            permission(
                2,
                json!({"kind":"execute","rawInput":{"command":"npm test"}}),
            ),
            permission(
                3,
                json!({"kind":"execute","rawInput":{"command":"npm publish"}}),
            ),
        );
        assert_eq!(last(&out)["result"]["outcome"]["optionId"], "yes");
        let confirm = |id: &str, message: &str| json!({"type":"extension_ui_request","id":id,"method":"confirm","title":"Deploy?","message":message});
        let out = remembers(
            Kind::Pi,
            confirm("a", "staging"),
            confirm("b", "staging"),
            confirm("c", "production"),
        );
        assert_eq!(last(&out)["confirmed"], true);
    }

    #[test]
    fn restricted_modes_ask_every_time_and_cannot_save() {
        let f = fixture();
        let first = claude("Bash", json!({"command":"cargo build"}));
        let mut p = protocol(&f, Kind::Claude);
        let events = p.frame(first.clone(), &mut Vec::new()).unwrap();
        let [Event::Approval(approval)] = events.as_slice() else {
            panic!("expected an approval");
        };
        p.remember_approval(&approval.id, &mut Vec::new()).unwrap();
        let mut p = protocol(&f, Kind::Pi);
        p.request.settings.access = AccessMode::ReadOnly;
        let events = p
            .frame(
                json!({"type":"extension_ui_request","id":"a","method":"confirm","title":"Deploy?","message":"x"}),
                &mut Vec::new(),
            )
            .unwrap();
        let [Event::Approval(approval)] = events.as_slice() else {
            panic!("expected an approval");
        };
        assert!(approval.remember_scope.is_none());
        assert!(matches!(
            p.remember_approval(&approval.id, &mut Vec::new())
                .unwrap()
                .as_slice(),
            [Event::ApprovalSaveFailed { .. }]
        ));
        assert_eq!(p.permissions.list().unwrap().len(), 1);
    }

    #[test]
    fn opencode_permissions_map_to_commands_files_hosts_or_exact_patterns() {
        let cwd = Path::new("/work");
        assert_eq!(
            opencode_subjects(cwd, "bash", &json!({"patterns":["git status"]})),
            vec![Subject::command("git status").unwrap()]
        );
        assert_eq!(
            opencode_subjects(cwd, "edit", &json!({"patterns":["src/a.rs","src/b.rs"]})).len(),
            2
        );
        assert_eq!(
            opencode_subjects(cwd, "webfetch", &json!({"patterns":["https://docs.rs/x"]})),
            vec![Subject::fetch("https://docs.rs").unwrap()]
        );
        assert_eq!(
            opencode_subjects(cwd, "bash", &json!({"patterns":["git add .","git push"]})),
            vec![Subject::Request {
                tool: "bash".into(),
                input: json!(["git add .", "git push"])
            }]
        );
    }
}
