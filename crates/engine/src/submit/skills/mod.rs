//! Port of src/features/skills/model/skills.ts: the per-project skill
//! catalog, `/skill` parsing, skill prompt injection, and blank skills.
//!
//! The TypeScript kept the catalog in a module-level map and read the
//! registry, the file system, and localStorage directly. Here the catalog is
//! a [`SkillCatalog`] value with its IO behind [`SkillSources`] and its
//! preferences in a [`LocalStore`]. Loads run on a spawner the way promises
//! ran whether or not anyone awaited them.

pub mod create_skill;
pub mod slash_commands;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Shared};
use monocode_core::HarnessId;
use monocode_core::js;
use monocode_harness::core::local_store::LocalStore;
use monocode_harness::core::native_commands::{
    CommandContext, NativeCommand, NativeCommandProvider, Unsubscribe,
};
use monocode_harness::core::task::SharedSpawner;
use monocode_process::skills::DiscoveredSkill;
use parking_lot::Mutex;
use serde::Serialize;

use crate::runtime::util::project_path::normalize_project_path;
use crate::submit::paths::{is_local_project, join_path};
use crate::submit::quote_draft::is_markdown_blockquote_position;

pub use create_skill::{CREATE_SKILL_BODY, CREATE_SKILL_DESCRIPTION, CREATE_SKILL_NAME};
pub use slash_commands::{
    MAX_PICKER, SlashToken, rank_skills, replace_slash_token, slash_token_at,
};

/// `DISABLED_SKILL_PATHS_KEY`.
pub const DISABLED_SKILL_PATHS_KEY: &str = "monocode.disabledSkillPaths";

const NATIVE_SKILL_TTL_MS: i64 = 30_000;
const NATIVE_SKILL_RETRY_MS: i64 = 5_000;

/// `FileSkill.scope`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum FileSkillScope {
    #[serde(rename = "project")]
    Project,
    #[serde(rename = "user")]
    User,
}

/// `FileSkill`: a SKILL.md on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileSkill {
    pub name: String,
    pub description: String,
    pub invocation: String,
    pub path: String,
    pub scope: FileSkillScope,
    /// `SkillSource`: `agents`, `monocode`, or a harness id.
    pub source: String,
}

/// `BuiltinSkill`: a MonoCode command or the bundled create-skill skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct BuiltinSkill {
    pub name: &'static str,
    pub description: &'static str,
    pub invocation: &'static str,
    /// Always `builtin`.
    pub scope: &'static str,
    /// Always `monocode`.
    pub source: &'static str,
}

impl BuiltinSkill {
    pub const fn new(
        name: &'static str,
        invocation: &'static str,
        description: &'static str,
    ) -> Self {
        Self {
            name,
            description,
            invocation,
            scope: "builtin",
            source: "monocode",
        }
    }
}

/// `Skill`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind")]
pub enum Skill {
    #[serde(rename = "file")]
    File(FileSkill),
    #[serde(rename = "builtin")]
    Builtin(BuiltinSkill),
    /// `NativeSkill`: a provider-owned command.
    #[serde(rename = "native")]
    Native(NativeCommand),
}

impl Skill {
    pub fn name(&self) -> &str {
        match self {
            Skill::File(skill) => &skill.name,
            Skill::Builtin(skill) => skill.name,
            Skill::Native(skill) => &skill.name,
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Skill::File(skill) => &skill.description,
            Skill::Builtin(skill) => skill.description,
            Skill::Native(skill) => &skill.description,
        }
    }

    pub fn invocation(&self) -> &str {
        match self {
            Skill::File(skill) => &skill.invocation,
            Skill::Builtin(skill) => skill.invocation,
            Skill::Native(skill) => &skill.invocation,
        }
    }
}

/// `BUILTIN_CREATE_SKILL`.
pub const BUILTIN_CREATE_SKILL: BuiltinSkill = BuiltinSkill::new(
    CREATE_SKILL_NAME,
    CREATE_SKILL_NAME,
    CREATE_SKILL_DESCRIPTION,
);

/// `SkillCatalogContext`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCatalogContext {
    pub harness: HarnessId,
    pub cwd: String,
    pub session_id: Option<String>,
}

impl SkillCatalogContext {
    pub fn new(harness: HarnessId, cwd: impl Into<String>) -> Self {
        Self {
            harness,
            cwd: cwd.into(),
            session_id: None,
        }
    }

    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }
}

/// The IO the catalog needs. [`ProcessSkillSources`] is the real one.
pub trait SkillSources: Send + Sync {
    /// `getHarness(harness)?.commands`.
    fn command_provider(&self, harness: HarnessId) -> Option<Arc<dyn NativeCommandProvider>>;

    /// `list_skills`: SKILL.md files for a project, with disabled paths
    /// excluded before same-name deduplication.
    fn list_skills(
        &self,
        cwd: String,
        disabled: Vec<String>,
    ) -> BoxFuture<'static, Result<Vec<DiscoveredSkill>, String>>;

    fn read_text_file(&self, path: String) -> BoxFuture<'static, Result<String, String>>;

    /// `homeDir`.
    fn home_dir(&self) -> BoxFuture<'static, Result<String, String>>;

    /// `createPath`.
    fn create_path(
        &self,
        parent: String,
        name: String,
        is_dir: bool,
    ) -> BoxFuture<'static, Result<String, String>>;

    fn write_text_file(
        &self,
        path: String,
        content: String,
    ) -> BoxFuture<'static, Result<(), String>>;

    /// `Date.now()`.
    fn now_ms(&self) -> i64 {
        monocode_core::reducer::now_ms()
    }
}

/// Skill IO over the process and git crates, with each blocking call on
/// smol's blocking pool, and native commands from the harness registry.
pub struct ProcessSkillSources {
    pub registry: monocode_harness::core::HarnessRegistry,
}

impl SkillSources for ProcessSkillSources {
    fn command_provider(&self, harness: HarnessId) -> Option<Arc<dyn NativeCommandProvider>> {
        self.registry.get_harness(harness)?.commands()
    }

    fn list_skills(
        &self,
        cwd: String,
        disabled: Vec<String>,
    ) -> BoxFuture<'static, Result<Vec<DiscoveredSkill>, String>> {
        smol::unblock(move || monocode_process::skills::list_skills(cwd, Some(disabled))).boxed()
    }

    fn read_text_file(&self, path: String) -> BoxFuture<'static, Result<String, String>> {
        smol::unblock(move || monocode_git::fs::read_text_file(path)).boxed()
    }

    fn home_dir(&self) -> BoxFuture<'static, Result<String, String>> {
        smol::unblock(|| {
            std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .map_err(|_| "Could not resolve the home directory".to_string())
        })
        .boxed()
    }

    fn create_path(
        &self,
        parent: String,
        name: String,
        is_dir: bool,
    ) -> BoxFuture<'static, Result<String, String>> {
        smol::unblock(move || monocode_git::fs::create_path(parent, name, is_dir)).boxed()
    }

    fn write_text_file(
        &self,
        path: String,
        content: String,
    ) -> BoxFuture<'static, Result<(), String>> {
        smol::unblock(move || monocode_git::fs::write_text_file(path, content)).boxed()
    }
}

/// `loadDisabledSkillPaths`.
pub fn load_disabled_skill_paths(store: &dyn LocalStore) -> Vec<String> {
    let Some(raw) = store
        .get_item(DISABLED_SKILL_PATHS_KEY)
        .filter(|raw| !raw.is_empty())
    else {
        return Vec::new();
    };
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(serde_json::Value::Array(items)) => items
            .into_iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// A pending catalog load. Clones share one result.
pub type SkillsLoad = Shared<BoxFuture<'static, Vec<Skill>>>;

fn ready(skills: Vec<Skill>) -> SkillsLoad {
    futures::future::ready(skills).boxed().shared()
}

struct InFlight {
    generation: u64,
    id: u64,
    load: SkillsLoad,
}

struct CatalogEntry {
    /// Stands in for object identity: a replaced entry gets a new id.
    id: u64,
    cwd: String,
    skills: Option<Vec<Skill>>,
    loaded_at: i64,
    retry_at: i64,
    generation: u64,
    in_flight: Option<InFlight>,
}

type ChangeListener = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct CatalogState {
    entries: HashMap<String, CatalogEntry>,
    next_id: u64,
    listeners: Vec<(u64, ChangeListener)>,
}

impl CatalogState {
    fn next(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
}

struct Inner {
    sources: Arc<dyn SkillSources>,
    store: Arc<dyn LocalStore>,
    spawner: SharedSpawner,
    state: Mutex<CatalogState>,
}

/// The composer skill catalog (`catalogEntries` and the functions over
/// it). Clones share one catalog.
#[derive(Clone)]
pub struct SkillCatalog {
    inner: Arc<Inner>,
}

impl SkillCatalog {
    pub fn new(
        sources: Arc<dyn SkillSources>,
        store: Arc<dyn LocalStore>,
        spawner: SharedSpawner,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                sources,
                store,
                spawner,
                state: Mutex::default(),
            }),
        }
    }

    /// `loadDisabledSkillPaths`.
    pub fn load_disabled_skill_paths(&self) -> Vec<String> {
        load_disabled_skill_paths(self.inner.store.as_ref())
    }

    /// `saveDisabledSkillPaths`: store the list, drop every cached catalog,
    /// and tell change listeners (`SKILLS_CHANGE_EVENT`).
    pub fn save_disabled_skill_paths(&self, paths: &[String]) -> Result<(), String> {
        let raw = serde_json::to_string(paths).unwrap_or_else(|_| "[]".into());
        if self
            .inner
            .store
            .set_item(DISABLED_SKILL_PATHS_KEY, &raw)
            .is_err()
        {
            return Err("Could not save skill preferences".into());
        }
        // The composer catalog caches per context; drop it so the next picker
        // or prompt sees the change immediately.
        self.invalidate_skills(None);
        let listeners: Vec<ChangeListener> = self
            .inner
            .state
            .lock()
            .listeners
            .iter()
            .map(|(_, listener)| listener.clone())
            .collect();
        for listener in listeners {
            listener();
        }
        Ok(())
    }

    /// Listen for `SKILLS_CHANGE_EVENT`. Returns an id for
    /// [`SkillCatalog::unsubscribe_changes`].
    pub fn subscribe_changes(&self, listener: impl Fn() + Send + Sync + 'static) -> u64 {
        let mut state = self.inner.state.lock();
        let id = state.next();
        state.listeners.push((id, Arc::new(listener)));
        id
    }

    pub fn unsubscribe_changes(&self, id: u64) {
        self.inner
            .state
            .lock()
            .listeners
            .retain(|(entry, _)| *entry != id);
    }

    fn provider(&self, harness: HarnessId) -> Option<Arc<dyn NativeCommandProvider>> {
        self.inner.sources.command_provider(harness)
    }

    /// `skillCatalogKey`.
    pub fn skill_catalog_key(&self, context: &SkillCatalogContext) -> String {
        // TODO(port): the TypeScript scoped a catalog to the session when the
        // provider had a `subscribe` method. `NativeCommandProvider` cannot say
        // that without subscribing, so raw slash command runtimes (OMP, the
        // only one with live updates) stand in for it. See NEEDS.md.
        let session_scoped = self
            .provider(context.harness)
            .is_some_and(|provider| provider.raw_slash_commands());
        let mut key = format!(
            "{}\0{}",
            context.harness,
            normalize_project_path(&context.cwd)
        );
        if session_scoped
            && let Some(session_id) = context.session_id.as_deref().filter(|id| !id.is_empty())
        {
            key.push('\0');
            key.push_str(session_id);
        }
        key
    }

    /// `hasNativeCommands`.
    pub fn has_native_commands(&self, harness: HarnessId) -> bool {
        self.provider(harness).is_some()
    }

    /// `isNativeCommandPrompt`: a leading `/command` for a provider that
    /// owns its slash arguments.
    pub fn is_native_command_prompt(&self, text: &str, harness: HarnessId) -> bool {
        self.provider(harness)
            .is_some_and(|provider| provider.raw_slash_commands())
            && leading_native_command(text)
    }

    /// `subscribeSkills`: a live update supersedes any cold probe already in
    /// flight.
    pub fn subscribe_skills(
        &self,
        context: &SkillCatalogContext,
        on_skills: impl Fn(Vec<Skill>) + Send + Sync + 'static,
    ) -> Unsubscribe {
        let Some(provider) = self.provider(context.harness) else {
            return Box::new(|| {});
        };
        let catalog = self.clone();
        let key_context = context.clone();
        let listener = Arc::new(move |commands: Vec<NativeCommand>| {
            let key = catalog.skill_catalog_key(&key_context);
            let skills: Vec<Skill> = commands.into_iter().map(Skill::Native).collect();
            {
                let mut state = catalog.inner.state.lock();
                let generation = state.entries.get(&key).map_or(0, |entry| entry.generation) + 1;
                let id = state.next();
                let loaded_at = catalog.inner.sources.now_ms();
                state.entries.insert(
                    key,
                    CatalogEntry {
                        id,
                        cwd: normalize_project_path(&key_context.cwd),
                        skills: Some(skills.clone()),
                        loaded_at,
                        retry_at: 0,
                        generation,
                        in_flight: None,
                    },
                );
            }
            on_skills(skills);
        });
        provider
            .subscribe(command_context(context), listener)
            .unwrap_or_else(|| Box::new(|| {}))
    }

    /// `peekSkills`.
    pub fn peek_skills(&self, context: &SkillCatalogContext) -> Option<Vec<Skill>> {
        let key = self.skill_catalog_key(context);
        self.inner
            .state
            .lock()
            .entries
            .get(&key)
            .and_then(|entry| entry.skills.clone())
    }

    /// `invalidateSkills`: every catalog, or the ones for one project.
    pub fn invalidate_skills(&self, cwd: Option<&str>) {
        let mut state = self.inner.state.lock();
        let Some(cwd) = cwd else {
            state.entries.clear();
            return;
        };
        let cwd = normalize_project_path(cwd);
        for entry in state.entries.values_mut() {
            if entry.cwd != cwd {
                continue;
            }
            entry.generation += 1;
            entry.loaded_at = 0;
            entry.retry_at = 0;
            entry.in_flight = None;
        }
    }

    /// `loadSkills`.
    pub fn load_skills(&self, context: &SkillCatalogContext, refresh: bool) -> SkillsLoad {
        let normalized = SkillCatalogContext {
            harness: context.harness,
            cwd: normalize_project_path(&context.cwd),
            session_id: context.session_id.clone().filter(|id| !id.is_empty()),
        };
        let key = self.skill_catalog_key(&normalized);
        let native = self.has_native_commands(normalized.harness);
        let now = self.inner.sources.now_ms();
        let mut state = self.inner.state.lock();
        if !state.entries.contains_key(&key) {
            let id = state.next();
            state.entries.insert(
                key.clone(),
                CatalogEntry {
                    id,
                    cwd: normalized.cwd.clone(),
                    skills: None,
                    loaded_at: 0,
                    retry_at: 0,
                    generation: 0,
                    in_flight: None,
                },
            );
        }
        let load_id = state.next();
        let entry = state.entries.get_mut(&key).expect("entry inserted above");

        if refresh {
            if let Some(in_flight) = &entry.in_flight
                && in_flight.generation == entry.generation
            {
                return in_flight.load.clone();
            }
            entry.generation += 1;
            entry.retry_at = 0;
            return self.start_catalog_load(state, key, load_id, normalized);
        }

        if let Some(in_flight) = &entry.in_flight
            && in_flight.generation == entry.generation
        {
            return in_flight.load.clone();
        }
        if !native && let Some(skills) = &entry.skills {
            return ready(skills.clone());
        }
        if native
            && let Some(skills) = &entry.skills
            && now - entry.loaded_at < NATIVE_SKILL_TTL_MS
        {
            return ready(skills.clone());
        }
        if native && now < entry.retry_at {
            return ready(entry.skills.clone().unwrap_or_default());
        }
        self.start_catalog_load(state, key, load_id, normalized)
    }

    fn start_catalog_load(
        &self,
        mut state: parking_lot::MutexGuard<'_, CatalogState>,
        key: String,
        load_id: u64,
        context: SkillCatalogContext,
    ) -> SkillsLoad {
        let entry = state.entries.get_mut(&key).expect("entry exists");
        let entry_id = entry.id;
        let generation = entry.generation;
        let (done, result) = oneshot::channel::<Vec<Skill>>();
        let load: SkillsLoad = result
            .map(|skills| skills.unwrap_or_default())
            .boxed()
            .shared();
        entry.in_flight = Some(InFlight {
            generation,
            id: load_id,
            load: load.clone(),
        });
        drop(state);

        let catalog = self.clone();
        let native = self.has_native_commands(context.harness);
        let work = async move {
            let loaded = catalog.load_catalog(context).await;
            let now = catalog.inner.sources.now_ms();
            let skills = {
                let mut state = catalog.inner.state.lock();
                let current = state
                    .entries
                    .get_mut(&key)
                    .filter(|entry| entry.id == entry_id && entry.generation == generation);
                let skills = match (current, loaded) {
                    (None, _) => state
                        .entries
                        .get(&key)
                        .and_then(|entry| entry.skills.clone())
                        .unwrap_or_default(),
                    (Some(entry), Ok(skills)) => {
                        entry.skills = Some(skills.clone());
                        entry.loaded_at = now;
                        entry.retry_at = 0;
                        skills
                    }
                    (Some(entry), Err(_)) if native => {
                        entry.retry_at = now + NATIVE_SKILL_RETRY_MS;
                        entry.skills.clone().unwrap_or_default()
                    }
                    (Some(entry), Err(_)) => {
                        let fallback = merge_catalog(&[]);
                        entry.skills = Some(fallback.clone());
                        entry.loaded_at = now;
                        fallback
                    }
                };
                if let Some(entry) = state.entries.get_mut(&key)
                    && entry.id == entry_id
                    && entry.generation == generation
                    && entry
                        .in_flight
                        .as_ref()
                        .is_some_and(|flight| flight.id == load_id)
                {
                    entry.in_flight = None;
                }
                skills
            };
            let _ = done.send(skills);
        };
        self.inner.spawner.spawn(work.boxed());
        load
    }

    async fn load_catalog(&self, context: SkillCatalogContext) -> Result<Vec<Skill>, String> {
        if let Some(provider) = self.provider(context.harness) {
            let commands = provider
                .discover(command_context(&context))
                .await
                .map_err(|error| error.to_string())?;
            return Ok(commands.into_iter().map(Skill::Native).collect());
        }
        let disabled_paths = self.load_disabled_skill_paths();
        let discovered = self
            .inner
            .sources
            .list_skills(context.cwd.clone(), disabled_paths)
            .await?;
        let disabled: HashSet<String> = self.load_disabled_skill_paths().into_iter().collect();
        let enabled: Vec<DiscoveredSkill> = discovered
            .into_iter()
            .filter(|skill| !disabled.contains(&skill.path))
            .collect();
        Ok(merge_catalog(&enabled))
    }

    /// `applySkillsToTurn`: prefix the bodies of the file and built-in
    /// skills the text invokes.
    pub async fn apply_skills_to_turn(&self, text: &str, context: &SkillCatalogContext) -> String {
        if self.has_native_commands(context.harness) {
            return text.to_string();
        }
        let names = skill_names_in_text(text);
        if names.is_empty() {
            return text.to_string();
        }
        let catalog = self.load_skills(context, false).await;
        let mut picked: Vec<Skill> = Vec::new();
        for name in &names {
            if let Some(skill) = catalog.iter().find(|item| item.name() == name)
                && matches!(skill, Skill::File(_) | Skill::Builtin(_))
            {
                picked.push(skill.clone());
            }
        }
        if picked.is_empty() {
            return text.to_string();
        }
        let reads = picked.iter().map(|skill| self.read_skill_body(skill));
        let bodies: HashMap<String, String> = picked
            .iter()
            .map(|skill| skill.name().to_string())
            .zip(futures::future::join_all(reads).await)
            .collect();
        inject_skill_prompt(text, &picked, &bodies)
    }

    /// `warmNativeSkills`.
    pub fn warm_native_skills(&self, context: &SkillCatalogContext) {
        if !self.has_native_commands(context.harness) {
            return;
        }
        // The load runs on the spawner; nobody needs its result now.
        drop(self.load_skills(context, false));
    }

    /// `readSkillBody`.
    pub async fn read_skill_body(&self, skill: &Skill) -> String {
        match skill {
            Skill::Builtin(_) => CREATE_SKILL_BODY.to_string(),
            Skill::File(file) => match self.inner.sources.read_text_file(file.path.clone()).await {
                Ok(body) => body,
                Err(_) => format!(
                    "Skill \"{}\" could not be read from {}.",
                    file.name, file.path
                ),
            },
            Skill::Native(_) => String::new(),
        }
    }

    /// `createBlankSkill`. Returns the new SKILL.md path. The caller nudges
    /// the project's file index (`invalidateProjectFiles`).
    pub async fn create_blank_skill(
        &self,
        cwd: &str,
        name: &str,
        scope: FileSkillScope,
    ) -> Result<String, String> {
        let name = slug_skill_name(name);
        if !is_valid_skill_name(&name) {
            return Err("Use a lowercase name with letters, numbers, and hyphens.".into());
        }
        let root = if scope == FileSkillScope::User || !is_local_project(cwd) {
            self.inner.sources.home_dir().await?
        } else {
            cwd.to_string()
        };
        let relative = format!(".agents/skills/{name}");
        self.inner
            .sources
            .create_path(root.clone(), relative.clone(), true)
            .await?;
        let path = join_path(&root, &format!("{relative}/SKILL.md"));
        self.inner
            .sources
            .write_text_file(path.clone(), blank_skill_markdown(&name))
            .await?;
        self.invalidate_skills(Some(cwd));
        Ok(path)
    }
}

fn command_context(context: &SkillCatalogContext) -> CommandContext {
    CommandContext {
        cwd: context.cwd.clone(),
        session_id: context.session_id.clone(),
    }
}

/// `/^\s*\/[^\s/\\]+(?=\s|$)/`.
fn leading_native_command(text: &str) -> bool {
    let trimmed = text.trim_start_matches(js::is_space);
    let Some(rest) = trimmed.strip_prefix('/') else {
        return false;
    };
    let end = rest
        .char_indices()
        .find(|(_, c)| js::is_space(*c) || *c == '/' || *c == '\\')
        .map_or(rest.len(), |(index, _)| index);
    if end == 0 {
        return false;
    }
    rest[end..].chars().next().is_none_or(js::is_space)
}

/// `mergeCatalog`: `.agents` skills win, then the bundled create-skill,
/// then provider folders.
pub fn merge_catalog(discovered: &[DiscoveredSkill]) -> Vec<Skill> {
    let mut out: Vec<Skill> = Vec::new();
    let mut add = |skill: Skill| {
        if skill.name().is_empty() || out.iter().any(|item| item.name() == skill.name()) {
            return;
        }
        out.push(skill);
    };
    for skill in discovered {
        if skill.source == "agents" {
            add(as_skill(skill));
        }
    }
    add(Skill::Builtin(BUILTIN_CREATE_SKILL));
    for skill in discovered {
        if skill.source != "agents" {
            add(as_skill(skill));
        }
    }
    out
}

fn as_skill(skill: &DiscoveredSkill) -> Skill {
    Skill::File(FileSkill {
        name: skill.name.clone(),
        description: skill.description.clone(),
        invocation: skill.name.clone(),
        path: skill.path.clone(),
        scope: if skill.scope == "user" {
            FileSkillScope::User
        } else {
            FileSkillScope::Project
        },
        source: skill.source.clone(),
    })
}

/// One `SKILL_TOKEN_RE` match: the name and the byte offset of its slash.
struct SkillToken<'a> {
    name: &'a str,
    start: usize,
    end: usize,
}

/// `[a-z0-9]+(?:-[a-z0-9]+)*`, returning the length matched.
fn skill_segment(text: &str) -> usize {
    let bytes = text.as_bytes();
    let word = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    let mut index = 0;
    while index < bytes.len() && word(bytes[index]) {
        index += 1;
    }
    if index == 0 {
        return 0;
    }
    while index + 1 < bytes.len() && bytes[index] == b'-' && word(bytes[index + 1]) {
        index += 1;
        while index < bytes.len() && word(bytes[index]) {
            index += 1;
        }
    }
    index
}

/// Every `SKILL_TOKEN_RE` match in order:
/// `/(^|\s)\/(name(?::name)?)(?=\s|$)/g`.
fn skill_tokens(text: &str) -> Vec<SkillToken<'_>> {
    let mut tokens = Vec::new();
    let mut search = 0;
    while search < text.len() {
        let Some(offset) = text[search..].find('/') else {
            break;
        };
        let slash = search + offset;
        let lead_ok = slash == 0 || text[..slash].chars().next_back().is_some_and(js::is_space);
        let rest = &text[slash + 1..];
        let mut len = skill_segment(rest);
        if len > 0 && rest[len..].starts_with(':') {
            let tail = skill_segment(&rest[len + 1..]);
            if tail > 0 {
                len += 1 + tail;
            }
        }
        let end = slash + 1 + len;
        let boundary = text[end..].chars().next().is_none_or(js::is_space);
        if lead_ok && len > 0 && boundary {
            tokens.push(SkillToken {
                name: &text[slash + 1..end],
                start: slash,
                end,
            });
            search = end;
        } else {
            search = slash + 1;
        }
    }
    tokens
}

/// `skillNamesInText`.
pub fn skill_names_in_text(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for token in skill_tokens(text) {
        if names.iter().any(|name| name == token.name)
            || is_markdown_blockquote_position(text, token.start)
        {
            continue;
        }
        names.push(token.name.to_string());
    }
    names
}

/// `SkillTextPart`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillTextPart {
    pub text: String,
    pub skill: bool,
}

/// `skillTextParts`: split composer text so known `/skill` tokens can be
/// highlighted.
pub fn skill_text_parts(text: &str, names: &HashSet<String>) -> Vec<SkillTextPart> {
    if text.is_empty() {
        return Vec::new();
    }
    if names.is_empty() {
        return vec![SkillTextPart {
            text: text.to_string(),
            skill: false,
        }];
    }
    let mut parts: Vec<SkillTextPart> = Vec::new();
    let mut push = |value: &str, skill: bool| {
        if value.is_empty() {
            return;
        }
        if let Some(last) = parts.last_mut()
            && last.skill == skill
        {
            last.text.push_str(value);
            return;
        }
        parts.push(SkillTextPart {
            text: value.to_string(),
            skill,
        });
    };
    let mut cursor = 0;
    for token in skill_tokens(text) {
        if !names.contains(token.name) || is_markdown_blockquote_position(text, token.start) {
            continue;
        }
        push(&text[cursor..token.start], false);
        push(&text[token.start..token.end], true);
        cursor = token.end;
    }
    push(&text[cursor..], false);
    parts
}

/// `injectSkillPrompt`.
pub fn inject_skill_prompt(
    text: &str,
    skills: &[Skill],
    bodies: &HashMap<String, String>,
) -> String {
    let mut blocks: Vec<String> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for skill in skills {
        if !seen.insert(skill.name()) {
            continue;
        }
        let Some(body) = bodies
            .get(skill.name())
            .map(|body| js::trim(body))
            .filter(|body| !body.is_empty())
        else {
            continue;
        };
        blocks.push(format!("## /{}\n\n{body}", skill.name()));
    }
    if blocks.is_empty() {
        return text.to_string();
    }
    [
        "The user invoked skill(s) with /name. Follow every instruction in each skill body.",
        "",
        &blocks.join("\n\n"),
        "",
        "---",
        "",
        text,
    ]
    .join("\n")
}

/// `slugSkillName`.
pub fn slug_skill_name(raw: &str) -> String {
    let lower = js::trim(raw).to_lowercase();
    let mut slug = String::new();
    let mut dash = false;
    for c in lower.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
            dash = false;
        } else if !dash {
            slug.push('-');
            dash = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    let capped: String = trimmed.chars().take(64).collect();
    capped.trim_end_matches('-').to_string()
}

/// `isValidSkillName`: `/^[a-z0-9]+(?:-[a-z0-9]+)*$/` and at most 64 characters.
pub fn is_valid_skill_name(name: &str) -> bool {
    !name.is_empty() && skill_segment(name) == name.len() && name.len() <= 64
}

/// `titleFromSkillName`.
pub fn title_from_skill_name(name: &str) -> String {
    name.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `blankSkillMarkdown`.
pub fn blank_skill_markdown(name: &str) -> String {
    let title = title_from_skill_name(name);
    let words = name.replace('-', " ");
    format!(
        "---\nname: {name}\ndescription: {title}. Use when the user asks to {words}.\n---\n\n# {title}\n\n## Instructions\n\n"
    )
}

#[cfg(test)]
mod tests;
