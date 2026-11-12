//! Saved "Always allow" grants. Reload under one lock so revocations reach live turns.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

static WRITER: Mutex<()> = Mutex::new(());
const VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub project: PathBuf,
    pub provider: String,
    pub account: String,
    pub account_home: PathBuf,
    pub binary: PathBuf,
    pub subject: Subject,
}

/// What a grant allows. Each kind matches on the part of the request that
/// stays the same when the agent asks again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Subject {
    /// A Codex MCP elicitation, matched on its stabilized confirmation.
    McpConfirmation { server: String, confirmation: Value },
    /// An MCP tool, with any arguments.
    McpTool { server: String, tool: String },
    /// A shell command, matched exactly.
    Command { command: String },
    /// Edits to one file, relative to the worktree root when inside it.
    Edit { path: PathBuf },
    /// Fetches from one host.
    Fetch { host: String },
    /// Anything else, matched on the exact tool and input.
    Request { tool: String, input: Value },
}

impl Subject {
    pub fn command(command: &str) -> Option<Self> {
        let command = command.trim();
        (!command.is_empty()).then(|| Self::Command {
            command: command.into(),
        })
    }

    /// `command` as a string, or as argv the way Codex and ACP agents send it.
    pub fn command_value(command: &Value) -> Option<Self> {
        match command {
            Value::String(command) => Self::command(command),
            Value::Array(argv) => {
                let argv: Option<Vec<&str>> = argv.iter().map(Value::as_str).collect();
                Self::command(&shlex::try_join(argv?).ok()?)
            }
            _ => None,
        }
    }

    pub fn edit(cwd: &Path, path: &str) -> Option<Self> {
        let path = path.trim();
        if path.is_empty() {
            return None;
        }
        // Resolve `..` before matching, so a path that does not exist yet
        // cannot keep a relative key that points outside the worktree.
        let absolute = lexical(&cwd.join(path));
        let root = worktree(cwd);
        let path = [project(&absolute), absolute.clone()]
            .into_iter()
            .find_map(|path| {
                [project(&root), root.clone()]
                    .iter()
                    .find_map(|root| path.strip_prefix(root).ok().map(Path::to_path_buf))
            })
            .unwrap_or_else(|| project(&absolute));
        Some(Self::Edit { path })
    }

    pub fn fetch(url: &str) -> Option<Self> {
        let url = url::Url::parse(url.trim()).ok()?;
        let host = url.host_str().filter(|h| !h.is_empty())?.to_lowercase();
        Some(Self::Fetch {
            host: match url.port() {
                Some(port) => format!("{host}:{port}"),
                None => host,
            },
        })
    }

    /// Claude-style `mcp__server__tool` names.
    pub fn mcp_tool(name: &str) -> Option<Self> {
        let (server, tool) = name.strip_prefix("mcp__")?.split_once("__")?;
        (!server.is_empty() && !tool.is_empty()).then(|| Self::McpTool {
            server: server.into(),
            tool: tool.into(),
        })
    }

    fn computer_use(&self) -> bool {
        matches!(self, Self::McpConfirmation { confirmation, .. }
            if confirmation["_meta"]["connector_id"] == "computer-use"
                && confirmation["_meta"]["tool_params"]["app"].is_string())
    }

    /// A short name for the saved grant, or `None` to use the request's title.
    fn label(&self) -> Option<String> {
        Some(match self {
            Self::McpTool { server, tool } => format!("Use {tool} from {server}"),
            Self::Command { command } => format!("Run {command}"),
            Self::Edit { path } => format!("Edit {}", path.display()),
            Self::Fetch { host } => format!("Fetch from {host}"),
            Self::McpConfirmation { .. } | Self::Request { .. } => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grant {
    pub key: Key,
    pub label: String,
}

impl Grant {
    pub fn new(key: Key, title: &str) -> Self {
        Self {
            label: key.subject.label().unwrap_or_else(|| title.into()),
            key,
        }
    }
}

/// The sentence on an approval card that says what Always allow saves.
pub fn scope(grants: &[Grant]) -> Option<String> {
    let first = grants.first()?;
    let what = if first.key.subject.computer_use() {
        "Allow Computer Use actions for this app".to_owned()
    } else if let [_, _, ..] = grants {
        let paths: Vec<_> = grants
            .iter()
            .filter_map(|g| match &g.key.subject {
                Subject::Edit { path } => Some(path.display().to_string()),
                _ => None,
            })
            .collect();
        format!("Always allow edits to {}", paths.join(", "))
    } else {
        match &first.key.subject {
            Subject::McpConfirmation { .. } => "Remember this exact confirmation".into(),
            Subject::McpTool { server, tool } => format!("Always allow {tool} from {server}"),
            Subject::Command { command } => format!("Always allow `{command}`"),
            Subject::Edit { path } => format!("Always allow edits to {}", path.display()),
            Subject::Fetch { host } => format!("Always allow fetches from {host}"),
            Subject::Request { .. } => "Remember this exact request".into(),
        }
    };
    Some(format!(
        "{what} in this project, across worktrees, chats and restarts. Remove it in Settings → Permissions."
    ))
}

#[derive(Serialize, Deserialize)]
struct File<G> {
    version: u32,
    grants: Vec<G>,
}

/// Version 1 only held Codex MCP confirmations.
#[derive(Deserialize)]
struct LegacyGrant {
    key: LegacyKey,
    label: String,
}

#[derive(Deserialize)]
struct LegacyKey {
    project: PathBuf,
    provider: String,
    account: String,
    account_home: PathBuf,
    binary: PathBuf,
    server: String,
    confirmation: Value,
}

impl From<LegacyGrant> for Grant {
    fn from(grant: LegacyGrant) -> Self {
        let LegacyKey {
            project,
            provider,
            account,
            account_home,
            binary,
            server,
            confirmation,
        } = grant.key;
        Self {
            label: grant.label,
            key: Key {
                project,
                provider,
                account,
                account_home,
                binary,
                subject: Subject::McpConfirmation {
                    server,
                    confirmation,
                },
            },
        }
    }
}

#[derive(Clone)]
pub struct Store {
    path: PathBuf,
}

impl Default for Store {
    fn default() -> Self {
        Self::new(crate::store::monocode_database().with_file_name("permissions.json"))
    }
}

impl Store {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn read(&self) -> Result<Vec<Grant>, String> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("Cannot read saved permissions: {e}")),
        };
        let unreadable = |e: serde_json::Error| format!("Cannot read saved permissions: {e}");
        let file: File<Value> = serde_json::from_slice(&bytes).map_err(unreadable)?;
        let stored: Vec<Grant> = match file.version {
            1 => file
                .grants
                .into_iter()
                .map(|g| serde_json::from_value::<LegacyGrant>(g).map(Grant::from))
                .collect::<Result<_, _>>()
                .map_err(unreadable)?,
            VERSION => file
                .grants
                .into_iter()
                .map(serde_json::from_value)
                .collect::<Result<_, _>>()
                .map_err(unreadable)?,
            _ => return Err("Unsupported saved permissions version".into()),
        };
        let mut grants: Vec<Grant> = Vec::with_capacity(stored.len());
        for mut grant in stored {
            if let Subject::McpConfirmation { confirmation, .. } = &mut grant.key.subject {
                stable(confirmation);
            }
            grant.key.project = repository(&grant.key.project);
            if grant.key.provider == "codex" {
                grant.key.binary = codex_binary(&grant.key.binary);
            }
            if !grants.iter().any(|g| g.key == grant.key) {
                grants.push(grant);
            }
        }
        Ok(grants)
    }

    pub fn list(&self) -> Result<Vec<Grant>, String> {
        let _lock = WRITER
            .lock()
            .map_err(|_| "Saved permissions are unavailable")?;
        self.read()
    }

    pub fn contains(&self, key: &Key) -> bool {
        self.covers(std::slice::from_ref(key))
    }

    /// Whether every key is saved. An empty list is never covered.
    pub fn covers(&self, keys: &[Key]) -> bool {
        !keys.is_empty()
            && self
                .list()
                .is_ok_and(|grants| keys.iter().all(|k| grants.iter().any(|g| &g.key == k)))
    }

    pub fn allow(&self, grant: Grant) -> Result<(), String> {
        self.allow_all(vec![grant])
    }

    pub fn allow_all(&self, new: Vec<Grant>) -> Result<(), String> {
        self.update(|grants| {
            for grant in new {
                if !grants.iter().any(|g| g.key == grant.key) {
                    grants.push(grant);
                }
            }
        })
    }

    pub fn remove(&self, key: &Key) -> Result<(), String> {
        self.update(|grants| grants.retain(|g| &g.key != key))
    }

    fn update(&self, change: impl FnOnce(&mut Vec<Grant>)) -> Result<(), String> {
        let _lock = WRITER
            .lock()
            .map_err(|_| "Saved permissions are unavailable")?;
        let mut grants = self.read()?;
        change(&mut grants);
        let parent = self
            .path
            .parent()
            .ok_or("Saved permissions path has no parent")?;
        fs::create_dir_all(parent).map_err(|e| format!("Cannot save permission: {e}"))?;
        let mut file = tempfile::NamedTempFile::new_in(parent)
            .map_err(|e| format!("Cannot save permission: {e}"))?;
        serde_json::to_writer_pretty(
            &mut file,
            &File {
                version: VERSION,
                grants,
            },
        )
        .map_err(|e| format!("Cannot save permission: {e}"))?;
        file.as_file()
            .sync_all()
            .map_err(|e| format!("Cannot save permission: {e}"))?;
        file.persist(&self.path)
            .map_err(|e| format!("Cannot save permission: {e}"))?;
        Ok(())
    }
}

/// Drop the per-call fields Codex attaches to `_meta`, so the same confirmation
/// in a later turn or chat produces the same key.
pub fn stable(confirmation: &mut Value) {
    if let Some(meta) = confirmation.get_mut("_meta").and_then(Value::as_object_mut) {
        if meta.get("codex_approval_kind").and_then(Value::as_str) == Some("mcp_tool_call")
            && meta.get("connector_id").and_then(Value::as_str) == Some("computer-use")
            && meta
                .get("tool_params")
                .and_then(|v| v.get("app"))
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty())
        {
            let app = meta["tool_params"]["app"].clone();
            *meta = serde_json::Map::from_iter([
                (
                    "codex_approval_kind".into(),
                    Value::String("mcp_tool_call".into()),
                ),
                ("connector_id".into(), Value::String("computer-use".into())),
                ("tool_params".into(), serde_json::json!({"app": app})),
            ]);
            return;
        }
        for field in ["callId", "x-codex-turn-metadata"] {
            meta.remove(field);
        }
    }
}

pub fn project(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Resolves `.` and `..` without touching the file system.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !out.pop() {
                    out.push(part);
                }
            }
            part => out.push(part),
        }
    }
    out
}

/// The checkout that contains `path`: a worktree root or the main repository.
fn worktree(path: &Path) -> PathBuf {
    path.ancestors()
        .find(|root| root.join(".git").exists())
        .unwrap_or(path)
        .to_path_buf()
}

pub fn repository(path: &Path) -> PathBuf {
    let path = project(path);
    for root in path.ancestors() {
        let dotgit = root.join(".git");
        if dotgit.is_dir() {
            return root.to_path_buf();
        }
        if let Ok(contents) = fs::read_to_string(&dotgit) {
            if let Some(gitdir) = contents.trim().strip_prefix("gitdir: ") {
                let gitdir = root.join(gitdir);
                if let Ok(common) = fs::read_to_string(gitdir.join("commondir")) {
                    let common = project(&gitdir.join(common.trim()));
                    if common.file_name().is_some_and(|name| name == ".git") {
                        if let Some(main) = common.parent() {
                            return main.to_path_buf();
                        }
                    }
                }
            }
            return root.to_path_buf();
        }
    }
    path
}

pub fn codex_binary(path: &Path) -> PathBuf {
    let path = project(path);
    if let Some(releases) = path.parent().and_then(Path::parent).and_then(Path::parent) {
        if path.file_name().is_some_and(|name| name == "codex")
            && path
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "bin")
            && releases.file_name().is_some_and(|name| name == "releases")
            && releases
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "standalone")
        {
            return releases.join("current/bin/codex");
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn grant() -> Grant {
        Grant {
            label: "Allow Computer Use to use Monocode Native?".into(),
            key: Key {
                project: "/project".into(),
                provider: "codex".into(),
                account: "default".into(),
                account_home: "/codex".into(),
                binary: "/bin/codex".into(),
                subject: Subject::McpConfirmation {
                    server: "cua_repl".into(),
                    confirmation: json!({"message":"Allow this app?","mode":"form","requestedSchema":{"type":"object","properties":{}}}),
                },
            },
        }
    }

    fn write_v1(path: &Path, grants: Value) {
        fs::write(
            path,
            serde_json::to_vec(&json!({"version":1,"grants":grants})).unwrap(),
        )
        .unwrap();
    }

    fn v1(binary: &str, call: &str) -> Value {
        json!({"label":"Allow?","key":{"project":"/project","provider":"codex","account":"default",
            "account_home":"/codex","binary":binary,"server":"cua_repl",
            "confirmation":{"_meta":{"callId":call,"tool_name":"t"}}}})
    }

    #[test]
    fn per_call_metadata_does_not_change_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("permissions.json"));
        let mut first = json!({"message":"m","_meta":{"callId":"a","tool_name":"t","x-codex-turn-metadata":{"turn_id":"1"}}});
        let mut second = json!({"message":"m","_meta":{"callId":"b","tool_name":"t","x-codex-turn-metadata":{"turn_id":"2"}}});
        stable(&mut first);
        stable(&mut second);
        assert_eq!(first, second);
        write_v1(
            &dir.path().join("permissions.json"),
            json!([v1("/bin/codex", "old"), v1("/bin/codex", "older")]),
        );
        let grants = store.list().unwrap();
        assert_eq!(grants.len(), 1);
        assert_eq!(
            grants[0].key.subject,
            Subject::McpConfirmation {
                server: "cua_repl".into(),
                confirmation: json!({"_meta":{"tool_name":"t"}})
            }
        );
    }

    #[test]
    fn version_one_files_upgrade_on_the_next_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        write_v1(&path, json!([v1("/bin/codex", "a")]));
        let store = Store::new(path.clone());
        let legacy = store.list().unwrap().pop().unwrap();
        let mut command = grant();
        command.key.subject = Subject::command("cargo build").unwrap();
        store.allow(command.clone()).unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["version"], 2);
        assert_eq!(
            saved["grants"][0]["key"]["subject"]["kind"],
            "mcp_confirmation"
        );
        assert_eq!(store.list().unwrap(), vec![legacy, command]);
    }

    #[test]
    fn legacy_codex_release_grants_survive_updates_but_not_installation_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        write_v1(
            &path,
            json!([v1("/opt/codex/standalone/releases/0.164.0/bin/codex", "a")]),
        );
        let store = Store::new(path);
        let mut later = store.list().unwrap()[0].key.clone();
        later.binary = codex_binary(Path::new(
            "/opt/codex/standalone/releases/0.165.0/bin/codex",
        ));
        assert!(store.contains(&later));
        later.binary = codex_binary(Path::new("/other/standalone/releases/0.165.0/bin/codex"));
        assert!(!store.contains(&later));
        store.remove(&store.list().unwrap()[0].key).unwrap();
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn persists_exact_grants_and_revokes_across_store_instances() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        let store = Store::new(path.clone());
        let grant = grant();
        store.allow(grant.clone()).unwrap();
        store.allow(grant.clone()).unwrap();
        let reopened = Store::new(path);
        assert!(reopened.contains(&grant.key));
        assert_eq!(reopened.list().unwrap(), vec![grant.clone()]);
        for field in ["project", "provider", "account", "account_home", "binary"] {
            let mut changed = serde_json::to_value(&grant.key).unwrap();
            changed[field] = json!("different");
            let changed = serde_json::from_value(changed).unwrap();
            assert!(!reopened.contains(&changed), "{field} must match");
        }
        for subject in [
            Subject::McpConfirmation {
                server: "other".into(),
                confirmation: json!({"message":"Allow this app?","mode":"form","requestedSchema":{"type":"object","properties":{}}}),
            },
            Subject::McpConfirmation {
                server: "cua_repl".into(),
                confirmation: json!({"message":"different"}),
            },
            Subject::Request {
                tool: "cua_repl".into(),
                input: json!({"message":"Allow this app?"}),
            },
        ] {
            let mut changed = grant.key.clone();
            changed.subject = subject;
            assert!(!reopened.contains(&changed));
        }
        reopened.remove(&grant.key).unwrap();
        assert!(!store.contains(&grant.key));
    }

    #[test]
    fn multi_file_grants_cover_only_when_every_file_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("permissions.json"));
        let edit = |path: &str| {
            let mut g = grant();
            g.key.subject = Subject::Edit { path: path.into() };
            g
        };
        store.allow_all(vec![edit("a.rs"), edit("b.rs")]).unwrap();
        assert!(store.covers(&[edit("a.rs").key, edit("b.rs").key]));
        assert!(!store.covers(&[edit("a.rs").key, edit("c.rs").key]));
        assert!(!store.covers(&[]));
    }

    #[test]
    fn subjects_normalize_what_varies_between_asks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        assert_eq!(
            Subject::edit(&root, "src/main.rs"),
            Subject::edit(
                &root.join("src"),
                &root.join("src/main.rs").display().to_string()
            )
        );
        assert_eq!(
            Subject::edit(&root, "src/new.rs"),
            Some(Subject::Edit {
                path: "src/new.rs".into()
            })
        );
        assert_eq!(
            Subject::edit(&root.join("src"), "../../outside/new.rs"),
            Some(Subject::Edit {
                path: dir.path().join("outside/new.rs")
            })
        );
        assert_eq!(
            Subject::edit(&root.join("src"), "./../src/new.rs"),
            Subject::edit(&root, "src/new.rs")
        );
        let elsewhere = dir.path().join("elsewhere").join("x.rs");
        assert_eq!(
            Subject::edit(&root, &elsewhere.display().to_string()),
            Some(Subject::Edit { path: elsewhere })
        );
        assert_eq!(
            Subject::fetch("https://Docs.rs/serde?x=1"),
            Subject::fetch("https://docs.rs/tokio")
        );
        assert_ne!(
            Subject::fetch("https://docs.rs"),
            Subject::fetch("https://docs.rs:8443")
        );
        assert_eq!(
            Subject::command_value(&json!(["git", "commit", "-m", "a b"])),
            Subject::command("git commit -m 'a b'")
        );
        assert_eq!(Subject::command("  "), None);
        assert_eq!(
            Subject::mcp_tool("mcp__linear_app__save_issue"),
            Some(Subject::McpTool {
                server: "linear_app".into(),
                tool: "save_issue".into()
            })
        );
        assert_eq!(Subject::mcp_tool("Bash"), None);
    }

    #[test]
    fn malformed_or_future_files_never_grant_or_get_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("permissions.json");
        let store = Store::new(path.clone());
        for bytes in [
            "broken",
            r#"{"version":3,"grants":[]}"#,
            r#"{"version":2,"grants":[{"label":"x","key":{"subject":{"kind":"future"}}}]}"#,
        ] {
            fs::write(&path, bytes).unwrap();
            assert!(!store.contains(&grant().key));
            assert!(store.allow(grant()).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn concurrent_writers_preserve_both_grants() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("permissions.json"));
        std::thread::scope(|scope| {
            for command in ["first", "second"] {
                let store = store.clone();
                scope.spawn(move || {
                    let mut grant = grant();
                    grant.key.subject = Subject::command(command).unwrap();
                    store.allow(grant).unwrap();
                });
            }
        });
        assert_eq!(store.list().unwrap().len(), 2);
    }
}
