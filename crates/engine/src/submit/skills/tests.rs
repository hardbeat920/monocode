//! Ports of skills.test.ts and skillCatalog.test.ts.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use monocode_core::HarnessId;
use monocode_harness::core::local_store::LocalStore;
use monocode_harness::core::native_commands::{
    CommandContext, CommandsListener, NativeCommand, NativeCommandProvider, Unsubscribe,
};
use monocode_harness::core::task::SmolSpawner;
use monocode_process::skills::DiscoveredSkill;
use parking_lot::Mutex;

use super::*;

type Listing = Result<Vec<DiscoveredSkill>, String>;
type Commands = Result<Vec<NativeCommand>, String>;

enum Reply<T> {
    Now(T),
    Later(oneshot::Receiver<T>),
}

impl<T: Send + 'static> Reply<T> {
    fn future(self, fallback: T) -> BoxFuture<'static, T>
    where
        T: Clone,
    {
        match self {
            Reply::Now(value) => futures::future::ready(value).boxed(),
            Reply::Later(receiver) => receiver.map(move |value| value.unwrap_or(fallback)).boxed(),
        }
    }
}

/// `deferred()`: a reply the test resolves later.
fn deferred<T>() -> (oneshot::Sender<T>, Reply<T>) {
    let (sender, receiver) = oneshot::channel();
    (sender, Reply::Later(receiver))
}

struct FakeProvider {
    raw: bool,
    live: bool,
    replies: Mutex<VecDeque<Reply<Commands>>>,
    default: Mutex<Commands>,
    calls: Mutex<Vec<CommandContext>>,
    listeners: Mutex<Vec<CommandsListener>>,
    unsubscribed: Arc<AtomicUsize>,
}

impl FakeProvider {
    fn new(raw: bool, live: bool, default: Vec<NativeCommand>) -> Arc<Self> {
        Arc::new(Self {
            raw,
            live,
            replies: Mutex::default(),
            default: Mutex::new(Ok(default)),
            calls: Mutex::default(),
            listeners: Mutex::default(),
            unsubscribed: Arc::default(),
        })
    }

    fn push(&self, reply: Reply<Commands>) {
        self.replies.lock().push_back(reply);
    }

    fn call_count(&self) -> usize {
        self.calls.lock().len()
    }
}

impl NativeCommandProvider for FakeProvider {
    fn discover(
        &self,
        context: CommandContext,
    ) -> BoxFuture<'_, anyhow::Result<Vec<NativeCommand>>> {
        self.calls.lock().push(context);
        let reply = self
            .replies
            .lock()
            .pop_front()
            .unwrap_or_else(|| Reply::Now(self.default.lock().clone()));
        reply
            .future(Err("dropped".into()))
            .map(|result| result.map_err(|error| anyhow::anyhow!(error)))
            .boxed()
    }

    fn subscribe(
        &self,
        _context: CommandContext,
        on_commands: CommandsListener,
    ) -> Option<Unsubscribe> {
        if !self.live {
            return None;
        }
        self.listeners.lock().push(on_commands);
        let unsubscribed = self.unsubscribed.clone();
        Some(Box::new(move || {
            unsubscribed.fetch_add(1, Ordering::SeqCst);
        }))
    }

    fn raw_slash_commands(&self) -> bool {
        self.raw
    }
}

type ListFn = Box<dyn Fn(&str, &[String]) -> Listing + Send + Sync>;

struct FakeSources {
    providers: HashMap<HarnessId, Arc<FakeProvider>>,
    list_replies: Mutex<VecDeque<Reply<Listing>>>,
    list_default: Mutex<ListFn>,
    list_calls: Mutex<Vec<(String, Vec<String>)>>,
    files: Mutex<HashMap<String, String>>,
    now: AtomicI64,
}

impl FakeSources {
    fn new() -> Arc<Self> {
        let pi = FakeProvider::new(false, false, vec![pi_skill("architect")]);
        let omp = FakeProvider::new(true, true, vec![command("workflow", HarnessId::Omp)]);
        Arc::new(Self {
            providers: HashMap::from([(HarnessId::Pi, pi), (HarnessId::Omp, omp)]),
            list_replies: Mutex::default(),
            list_default: Mutex::new(Box::new(|_, _| Ok(Vec::new()))),
            list_calls: Mutex::default(),
            files: Mutex::default(),
            // 2026-08-29T12:00:00Z
            now: AtomicI64::new(1_788_004_800_000),
        })
    }

    fn pi(&self) -> &Arc<FakeProvider> {
        &self.providers[&HarnessId::Pi]
    }

    fn omp(&self) -> &Arc<FakeProvider> {
        &self.providers[&HarnessId::Omp]
    }

    fn advance(&self, ms: i64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    fn set_listing(&self, list: impl Fn(&str, &[String]) -> Listing + Send + Sync + 'static) {
        *self.list_default.lock() = Box::new(list);
    }

    fn push_listing(&self, reply: Reply<Listing>) {
        self.list_replies.lock().push_back(reply);
    }
}

impl SkillSources for FakeSources {
    fn command_provider(&self, harness: HarnessId) -> Option<Arc<dyn NativeCommandProvider>> {
        self.providers
            .get(&harness)
            .map(|provider| provider.clone() as Arc<dyn NativeCommandProvider>)
    }

    fn list_skills(&self, cwd: String, disabled: Vec<String>) -> BoxFuture<'static, Listing> {
        self.list_calls.lock().push((cwd.clone(), disabled.clone()));
        let reply = self
            .list_replies
            .lock()
            .pop_front()
            .unwrap_or_else(|| Reply::Now((self.list_default.lock())(&cwd, &disabled)));
        reply.future(Err("dropped".into()))
    }

    fn read_text_file(&self, path: String) -> BoxFuture<'static, Result<String, String>> {
        let body = self.files.lock().get(&path).cloned().unwrap_or_default();
        futures::future::ready(Ok(body)).boxed()
    }

    fn home_dir(&self) -> BoxFuture<'static, Result<String, String>> {
        futures::future::ready(Ok("/home/user".to_string())).boxed()
    }

    fn create_path(
        &self,
        parent: String,
        name: String,
        _is_dir: bool,
    ) -> BoxFuture<'static, Result<String, String>> {
        futures::future::ready(Ok(format!("{parent}/{name}"))).boxed()
    }

    fn write_text_file(
        &self,
        path: String,
        content: String,
    ) -> BoxFuture<'static, Result<(), String>> {
        self.files.lock().insert(path, content);
        futures::future::ready(Ok(())).boxed()
    }

    fn now_ms(&self) -> i64 {
        self.now.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
struct TestStore {
    items: Mutex<HashMap<String, String>>,
    fail: AtomicBool,
}

impl LocalStore for TestStore {
    fn get_item(&self, key: &str) -> Option<String> {
        self.items.lock().get(key).cloned()
    }

    fn set_item(&self, key: &str, value: &str) -> Result<(), String> {
        if self.fail.load(Ordering::SeqCst) {
            return Err("quota".into());
        }
        self.items.lock().insert(key.into(), value.into());
        Ok(())
    }
}

struct Fixture {
    sources: Arc<FakeSources>,
    store: Arc<TestStore>,
    catalog: SkillCatalog,
}

fn fixture() -> Fixture {
    let sources = FakeSources::new();
    let store = Arc::new(TestStore::default());
    let catalog = SkillCatalog::new(sources.clone(), store.clone(), Arc::new(SmolSpawner));
    Fixture {
        sources,
        store,
        catalog,
    }
}

fn command(name: &str, source: HarnessId) -> NativeCommand {
    NativeCommand {
        name: name.into(),
        description: String::new(),
        invocation: name.into(),
        source,
        origin: None,
        aliases: None,
        input_hint: None,
        subcommands: None,
    }
}

fn pi_skill(name: &str) -> NativeCommand {
    NativeCommand {
        description: format!("{name} description"),
        invocation: format!("skill:{name}"),
        ..command(name, HarnessId::Pi)
    }
}

fn discovered(
    name: &str,
    description: &str,
    path: &str,
    scope: &str,
    source: &str,
) -> DiscoveredSkill {
    DiscoveredSkill {
        name: name.into(),
        description: description.into(),
        path: path.into(),
        scope: scope.into(),
        source: source.into(),
    }
}

fn names(skills: &[Skill]) -> Vec<&str> {
    skills.iter().map(Skill::name).collect()
}

/// `vi.waitFor`.
fn wait_for(check: impl Fn() -> bool) {
    for _ in 0..200 {
        if check() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("condition never held");
}

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    smol::block_on(future)
}

fn ctx(harness: HarnessId, cwd: &str) -> SkillCatalogContext {
    SkillCatalogContext::new(harness, cwd)
}

fn file_skill(
    name: &str,
    description: &str,
    path: &str,
    scope: FileSkillScope,
    source: &str,
) -> Skill {
    Skill::File(FileSkill {
        name: name.into(),
        description: description.into(),
        invocation: name.into(),
        path: path.into(),
        scope,
        source: source.into(),
    })
}

fn review() -> Skill {
    file_skill(
        "review-pr",
        "Review pull requests against team standards.",
        "/tmp/.agents/skills/review-pr/SKILL.md",
        FileSkillScope::Project,
        "agents",
    )
}

fn cursor_native() -> Skill {
    file_skill(
        "cursor-only",
        "Cursor native helper",
        "/tmp/.cursor/skills/cursor-only/SKILL.md",
        FileSkillScope::Project,
        "cursor",
    )
}

fn pi_native() -> Skill {
    Skill::Native(NativeCommand {
        description: "Design before implementation.".into(),
        ..pi_skill("architect")
    })
}

fn pi_file() -> Skill {
    file_skill(
        "pi-file",
        "A file discovered by the existing scanner.",
        "/tmp/.pi/skills/pi-file/SKILL.md",
        FileSkillScope::Project,
        "pi",
    )
}

fn create() -> Skill {
    Skill::Builtin(BUILTIN_CREATE_SKILL)
}

// skills.test.ts: native command composer behavior

#[test]
fn filters_commands_by_alias_and_inserts_their_invocation_with_arguments_intact() {
    let workflow = Skill::Native(NativeCommand {
        description: "Choose agents".into(),
        aliases: Some(vec!["review".into()]),
        ..command("orchestrate", HarnessId::Omp)
    });
    assert_eq!(
        rank_skills(std::slice::from_ref(&workflow), "review", MAX_PICKER),
        vec![workflow.clone()]
    );
    let text = "/rev foo";
    let token = slash_token_at(text, 4, true).unwrap();
    assert_eq!(
        replace_slash_token(text, &token, workflow.invocation()),
        "/orchestrate foo"
    );
    assert_eq!(
        slash_token_at("/Review_Code", 12, true).unwrap().query,
        "Review_Code"
    );
    assert_eq!(slash_token_at("/Review_Code", 12, false), None);
}

#[test]
fn only_treats_leading_command_tokens_as_native_invocations() {
    let fixture = fixture();
    let catalog = &fixture.catalog;
    assert!(catalog.is_native_command_prompt("/workflow foo @README.md", HarnessId::Omp));
    assert!(catalog.is_native_command_prompt("/omp:plan investigate", HarnessId::Omp));
    for text in [
        "Explain /workflow",
        "> /workflow",
        "/tmp/file.ts",
        "/tmp\\file.ts",
        "hello",
    ] {
        assert!(
            !catalog.is_native_command_prompt(text, HarnessId::Omp),
            "{text}"
        );
    }
    assert!(!catalog.is_native_command_prompt("/review foo", HarnessId::Claude));
}

// skillNamesInText

#[test]
fn collects_unique_skill_tokens() {
    assert_eq!(
        skill_names_in_text("/create-skill write a deploy skill"),
        ["create-skill"]
    );
    assert_eq!(
        skill_names_in_text("/review-pr /create-skill /review-pr"),
        ["review-pr", "create-skill"]
    );
    assert!(skill_names_in_text("path /tmp/foo").is_empty());
}

// skillTextParts

fn part(text: &str, skill: bool) -> SkillTextPart {
    SkillTextPart {
        text: text.into(),
        skill,
    }
}

fn known() -> HashSet<String> {
    ["review-pr", "create-skill", "skill:architect"]
        .into_iter()
        .map(String::from)
        .collect()
}

#[test]
fn marks_known_skill_tokens() {
    assert_eq!(
        skill_text_parts("/review-pr look at auth", &known()),
        vec![part("/review-pr", true), part(" look at auth", false)]
    );
}

#[test]
fn marks_a_known_namespaced_invocation() {
    assert_eq!(
        skill_text_parts("/skill:architect inspect this", &known()),
        vec![part("/skill:architect", true), part(" inspect this", false)]
    );
}

#[test]
fn leaves_unknown_tokens_as_plain_text() {
    assert_eq!(
        skill_text_parts("see /not-a-skill please", &known()),
        vec![part("see /not-a-skill please", false)]
    );
}

#[test]
fn splits_multiple_skills() {
    assert_eq!(
        skill_text_parts("/review-pr then /create-skill", &known()),
        vec![
            part("/review-pr", true),
            part(" then ", false),
            part("/create-skill", true)
        ]
    );
}

#[test]
fn ignores_skill_tokens_inside_markdown_blockquotes() {
    let text = "/review-pr\n> /create-skill\n  > /review-pr";
    assert_eq!(
        slash_token_at(text, text.find("/create-skill").unwrap() + 3, false),
        None
    );
    assert_eq!(skill_names_in_text(text), ["review-pr"]);
    let skills: Vec<String> = skill_text_parts(text, &known())
        .into_iter()
        .filter(|part| part.skill)
        .map(|part| part.text)
        .collect();
    assert_eq!(skills, ["/review-pr"]);
}

// applySkillsToTurn

#[test]
fn leaves_pi_native_skill_commands_unchanged() {
    let fixture = fixture();
    let text = "/skill:architect inspect this";
    assert_eq!(
        block_on(
            fixture
                .catalog
                .apply_skills_to_turn(text, &ctx(HarnessId::Pi, "/repo"))
        ),
        text
    );
}

// injectSkillPrompt

#[test]
fn prefixes_invoked_skill_bodies_and_keeps_the_user_text() {
    let bodies = HashMap::from([(
        "review-pr".to_string(),
        "# Review\n\nBe strict.".to_string(),
    )]);
    let out = inject_skill_prompt("/review-pr look at auth", &[review()], &bodies);
    assert!(out.contains("## /review-pr"));
    assert!(out.contains("Be strict."));
    assert!(out.ends_with("/review-pr look at auth"));
}

#[test]
fn returns_the_original_text_when_nothing_matches() {
    assert_eq!(inject_skill_prompt("hello", &[], &HashMap::new()), "hello");
}

// mergeCatalog

#[test]
fn lets_agents_win_then_monocode_create_skill_then_provider_skills() {
    let catalog = merge_catalog(&[
        discovered(
            "review-pr",
            "from agents",
            "/p/.agents/skills/review-pr/SKILL.md",
            "project",
            "agents",
        ),
        discovered(
            "review-pr",
            "from claude",
            "/p/.claude/skills/review-pr/SKILL.md",
            "project",
            "claude",
        ),
        discovered(
            "create-skill",
            "claude native",
            "/home/.claude/skills/create-skill/SKILL.md",
            "user",
            "claude",
        ),
        discovered(
            "cursor-only",
            "native",
            "/p/.cursor/skills/cursor-only/SKILL.md",
            "project",
            "cursor",
        ),
    ]);
    let find = |name: &str| catalog.iter().find(|skill| skill.name() == name).cloned();
    assert_eq!(find("review-pr").unwrap().description(), "from agents");
    assert_eq!(find("create-skill"), Some(create()));
    match find("cursor-only") {
        Some(Skill::File(file)) => assert_eq!(file.source, "cursor"),
        other => panic!("{other:?}"),
    }
}

// rankSkills

#[test]
fn puts_create_skill_first_when_the_query_is_empty() {
    let ranked = rank_skills(&[cursor_native(), review(), create()], "", MAX_PICKER);
    assert_eq!(names(&ranked), ["create-skill", "cursor-only", "review-pr"]);
}

#[test]
fn fuzzy_matches_names_ahead_of_descriptions() {
    let ranked = rank_skills(&[cursor_native(), review(), create()], "rev", MAX_PICKER);
    assert_eq!(ranked[0].name(), "review-pr");
}

#[test]
fn ranks_native_pi_rows_with_project_skills() {
    let ranked = rank_skills(&[review(), pi_native(), pi_file()], "", MAX_PICKER);
    assert_eq!(names(&ranked), ["architect", "pi-file", "review-pr"]);
}

#[test]
fn matches_the_displayed_invocation() {
    assert_eq!(
        rank_skills(&[review(), pi_native()], "skill", MAX_PICKER),
        vec![pi_native()]
    );
    assert_eq!(
        rank_skills(&[review(), pi_native()], "skill:arch", MAX_PICKER),
        vec![pi_native()]
    );
}

#[test]
fn gives_the_built_in_row_its_exact_invocation() {
    assert_eq!(BUILTIN_CREATE_SKILL.invocation, "create-skill");
}

#[test]
fn keeps_every_pi_result_when_the_composer_removes_the_default_cap() {
    let rows: Vec<Skill> = (0..75)
        .map(|index| {
            let name = format!("skill-{index:02}");
            Skill::Native(NativeCommand {
                description: "Pi skill".into(),
                invocation: format!("skill:{name}"),
                ..command(&name, HarnessId::Pi)
            })
        })
        .collect();
    assert_eq!(rank_skills(&rows, "", MAX_PICKER).len(), 50);
    assert_eq!(rank_skills(&rows, "", usize::MAX).len(), 75);
    assert_eq!(
        rank_skills(&rows, "skill-74", usize::MAX)[0].name(),
        "skill-74"
    );
}

// skill names

#[test]
fn slugs_and_validates() {
    assert_eq!(slug_skill_name("Review PR"), "review-pr");
    assert!(is_valid_skill_name("review-pr"));
    assert!(!is_valid_skill_name("Review"));
    assert!(!is_valid_skill_name("-nope"));
}

#[test]
fn writes_a_starter_skill_md() {
    let md = blank_skill_markdown("review-pr");
    assert!(md.contains("name: review-pr"));
    assert!(md.contains("# Review Pr"));
}

#[test]
fn creates_a_blank_skill_in_the_project_or_home() {
    let fixture = fixture();
    let path = block_on(fixture.catalog.create_blank_skill(
        "/Users/me/repo",
        "Deploy App",
        FileSkillScope::Project,
    ))
    .unwrap();
    assert_eq!(path, "/Users/me/repo/.agents/skills/deploy-app/SKILL.md");
    assert!(fixture.sources.files.lock()[&path].contains("name: deploy-app"));
    let home = block_on(fixture.catalog.create_blank_skill(
        "/Users/me/repo",
        "x",
        FileSkillScope::User,
    ))
    .unwrap();
    assert_eq!(home, "/home/user/.agents/skills/x/SKILL.md");
    assert_eq!(
        block_on(
            fixture
                .catalog
                .create_blank_skill("/repo", "!!!", FileSkillScope::Project)
        ),
        Err("Use a lowercase name with letters, numbers, and hyphens.".to_string())
    );
}

// skillCatalog.test.ts: provider-aware skill catalog

#[test]
fn uses_pi_discovery_without_adding_monocodes_built_in_row() {
    let fixture = fixture();
    let catalog = block_on(
        fixture
            .catalog
            .load_skills(&ctx(HarnessId::Pi, "/repo/"), false),
    );
    assert_eq!(fixture.sources.pi().calls.lock()[0].cwd, "/repo");
    assert_eq!(catalog, vec![Skill::Native(pi_skill("architect"))]);
    assert!(!catalog.contains(&create()));
}

#[test]
fn uses_omp_native_discovery_and_leaves_commands_and_arguments_out_of_skill_injection() {
    let fixture = fixture();
    let context = ctx(HarnessId::Omp, "/repo").with_session("thread");
    let skills = block_on(fixture.catalog.load_skills(&context, false));
    assert!(
        matches!(&skills[..], [Skill::Native(command)] if command.name == "workflow" && command.source == HarnessId::Omp)
    );
    assert_eq!(
        fixture.sources.omp().calls.lock()[0],
        CommandContext {
            cwd: "/repo".into(),
            session_id: Some("thread".into())
        }
    );
    assert!(fixture.sources.list_calls.lock().is_empty());
    assert_eq!(
        block_on(
            fixture
                .catalog
                .apply_skills_to_turn("/workflow foo /create-skill", &context)
        ),
        "/workflow foo /create-skill"
    );
}

#[test]
fn isolates_omp_sessions_while_retaining_pis_shared_project_cache() {
    let fixture = fixture();
    for harness in [HarnessId::Pi, HarnessId::Omp] {
        let a = fixture
            .catalog
            .skill_catalog_key(&ctx(harness, "/repo").with_session("a"));
        let b = fixture
            .catalog
            .skill_catalog_key(&ctx(harness, "/repo").with_session("b"));
        assert_eq!(a == b, harness == HarnessId::Pi);
    }
}

#[test]
fn a_live_command_update_supersedes_an_older_probe_and_refreshes_subscribers() {
    let fixture = fixture();
    let context = ctx(HarnessId::Omp, "/repo").with_session("thread");
    let (probe, reply) = deferred();
    fixture.sources.omp().push(reply);
    let pending = fixture.catalog.load_skills(&context, false);
    let seen: Arc<Mutex<Vec<Vec<Skill>>>> = Arc::default();
    let sink = seen.clone();
    let stop = fixture
        .catalog
        .subscribe_skills(&context, move |skills| sink.lock().push(skills));
    let listener = fixture.sources.omp().listeners.lock()[0].clone();
    listener(vec![command("new-workflow", HarnessId::Omp)]);
    probe
        .send(Ok(vec![command("old-workflow", HarnessId::Omp)]))
        .unwrap();
    assert_eq!(names(&block_on(pending)), ["new-workflow"]);
    assert!(
        seen.lock()[0]
            .iter()
            .any(|skill| matches!(skill, Skill::Native(command) if command.name == "new-workflow"))
    );
    stop();
    assert_eq!(fixture.sources.omp().unsubscribed.load(Ordering::SeqCst), 1);
}

#[test]
fn keeps_the_last_omp_inventory_on_discovery_failure_without_falling_back_to_injected_files() {
    let fixture = fixture();
    let context = ctx(HarnessId::Omp, "/repo");
    block_on(fixture.catalog.load_skills(&context, false));
    fixture.sources.advance(30_001);
    *fixture.sources.omp().default.lock() = Err("Unsupported command".into());
    assert_eq!(
        names(&block_on(fixture.catalog.load_skills(&context, false))),
        ["workflow"]
    );
    fixture.catalog.invalidate_skills(None);
    assert_eq!(
        block_on(fixture.catalog.load_skills(&context, false)),
        vec![]
    );
    assert!(fixture.sources.list_calls.lock().is_empty());
}

#[test]
fn keeps_filesystem_discovery_and_the_built_in_row_for_non_pi_providers() {
    let fixture = fixture();
    let catalog = block_on(
        fixture
            .catalog
            .load_skills(&ctx(HarnessId::Claude, "/repo"), false),
    );
    assert_eq!(
        fixture.sources.list_calls.lock()[0],
        ("/repo".to_string(), Vec::new())
    );
    assert!(catalog.contains(&create()));
    assert_eq!(fixture.sources.pi().call_count(), 0);
}

#[test]
fn separates_providers_and_coalesces_equivalent_pi_directories() {
    let fixture = fixture();
    let (pending, reply) = deferred();
    fixture.sources.pi().push(reply);
    let first = fixture
        .catalog
        .load_skills(&ctx(HarnessId::Pi, "/repo/"), false);
    let second = fixture
        .catalog
        .load_skills(&ctx(HarnessId::Pi, "/repo"), false);
    let claude = fixture
        .catalog
        .load_skills(&ctx(HarnessId::Claude, "/repo"), false);
    // The TypeScript called discover before its first await. Here the load
    // task calls it, so wait for that task to start.
    wait_for(|| fixture.sources.pi().call_count() == 1);
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert_eq!(fixture.sources.pi().call_count(), 1);
    assert_eq!(
        fixture
            .catalog
            .skill_catalog_key(&ctx(HarnessId::Pi, "/repo/")),
        fixture
            .catalog
            .skill_catalog_key(&ctx(HarnessId::Pi, "/repo"))
    );
    assert_ne!(
        fixture
            .catalog
            .skill_catalog_key(&ctx(HarnessId::Pi, "/repo")),
        fixture
            .catalog
            .skill_catalog_key(&ctx(HarnessId::Claude, "/repo"))
    );
    pending.send(Ok(vec![pi_skill("architect")])).unwrap();
    assert_eq!(block_on(first), block_on(second));
    block_on(claude);
}

#[test]
fn refreshes_stale_pi_data_and_retains_it_after_a_failed_refresh() {
    let fixture = fixture();
    let context = ctx(HarnessId::Pi, "/repo");
    block_on(fixture.catalog.load_skills(&context, false));
    fixture.sources.advance(30_001);
    let (refresh, reply) = deferred();
    fixture.sources.pi().push(reply);
    let loading = fixture.catalog.load_skills(&context, false);
    assert_eq!(
        fixture.catalog.peek_skills(&context).unwrap()[0].name(),
        "architect"
    );
    refresh.send(Ok(vec![pi_skill("new-skill")])).unwrap();
    assert_eq!(names(&block_on(loading)), ["new-skill"]);

    fixture.sources.advance(30_001);
    fixture.sources.pi().push(Reply::Now(Err("offline".into())));
    assert_eq!(
        names(&block_on(fixture.catalog.load_skills(&context, false))),
        ["new-skill"]
    );
    block_on(fixture.catalog.load_skills(&context, false));
    assert_eq!(fixture.sources.pi().call_count(), 3);

    fixture.sources.advance(5_001);
    block_on(fixture.catalog.load_skills(&context, false));
    assert_eq!(fixture.sources.pi().call_count(), 4);
}

#[test]
fn does_not_let_an_invalidated_request_replace_a_newer_generation() {
    let fixture = fixture();
    let context = ctx(HarnessId::Pi, "/repo");
    let (old, old_reply) = deferred();
    let (current, current_reply) = deferred();
    fixture.sources.pi().push(old_reply);
    fixture.sources.pi().push(current_reply);

    let old_load = fixture.catalog.load_skills(&context, false);
    fixture.catalog.invalidate_skills(Some("/repo"));
    let current_load = fixture.catalog.load_skills(&context, true);
    current.send(Ok(vec![pi_skill("current")])).unwrap();
    block_on(current_load);
    old.send(Ok(vec![pi_skill("old")])).unwrap();

    assert_eq!(names(&block_on(old_load)), ["current"]);
    assert_eq!(
        names(&fixture.catalog.peek_skills(&context).unwrap()),
        ["current"]
    );
}

#[test]
fn rejects_completions_captured_before_a_global_reset() {
    let fixture = fixture();
    let context = ctx(HarnessId::Pi, "/repo");
    let (old, old_reply) = deferred();
    fixture.sources.pi().push(old_reply);
    let old_load = fixture.catalog.load_skills(&context, false);

    fixture.catalog.invalidate_skills(None);
    fixture
        .sources
        .pi()
        .push(Reply::Now(Ok(vec![pi_skill("current")])));
    block_on(fixture.catalog.load_skills(&context, false));
    old.send(Ok(vec![pi_skill("old")])).unwrap();

    assert_eq!(names(&block_on(old_load)), ["current"]);
    assert_eq!(
        names(&fixture.catalog.peek_skills(&context).unwrap()),
        ["current"]
    );
}

// file skill visibility preferences

const REVIEW_PATH: &str = "/repo/.agents/skills/review/SKILL.md";

fn visibility_fixture() -> Fixture {
    let fixture = fixture();
    fixture.sources.set_listing(|_, _| {
        Ok(vec![discovered(
            "review",
            "Review changes",
            REVIEW_PATH,
            "project",
            "agents",
        )])
    });
    fixture
}

#[test]
fn removes_a_hidden_file_from_a_cached_catalog_and_restores_it() {
    let fixture = visibility_fixture();
    let context = ctx(HarnessId::Claude, "/repo");
    let has_review = |skills: &[Skill]| skills.iter().any(|skill| skill.name() == "review");
    assert!(has_review(&block_on(
        fixture.catalog.load_skills(&context, false)
    )));
    fixture
        .catalog
        .save_disabled_skill_paths(&[REVIEW_PATH.into()])
        .unwrap();
    assert_eq!(
        block_on(fixture.catalog.load_skills(&context, false)),
        vec![create()]
    );
    fixture.catalog.save_disabled_skill_paths(&[]).unwrap();
    assert!(has_review(&block_on(
        fixture.catalog.load_skills(&context, false)
    )));
}

#[test]
fn does_not_inject_hidden_skill_content_into_a_submitted_turn() {
    let fixture = visibility_fixture();
    fixture
        .catalog
        .save_disabled_skill_paths(&[REVIEW_PATH.into()])
        .unwrap();
    let result = block_on(
        fixture
            .catalog
            .apply_skills_to_turn("/review inspect this", &ctx(HarnessId::Claude, "/repo")),
    );
    assert_eq!(result, "/review inspect this");
}

#[test]
fn leaves_provider_owned_native_catalogs_intact() {
    let fixture = visibility_fixture();
    fixture
        .catalog
        .save_disabled_skill_paths(&[REVIEW_PATH.into()])
        .unwrap();
    let skills = block_on(
        fixture
            .catalog
            .load_skills(&ctx(HarnessId::Pi, "/repo"), false),
    );
    assert!(matches!(&skills[..], [Skill::Native(command)] if command.name == "architect"));
}

#[test]
fn notifies_open_views_only_after_persistence_succeeds() {
    let fixture = visibility_fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    fixture.catalog.subscribe_changes(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    });
    fixture
        .catalog
        .save_disabled_skill_paths(&[REVIEW_PATH.into()])
        .unwrap();
    assert_eq!(fixture.catalog.load_disabled_skill_paths(), [REVIEW_PATH]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    fixture.store.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        fixture.catalog.save_disabled_skill_paths(&[]),
        Err("Could not save skill preferences".to_string())
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn tolerates_malformed_and_mixed_stored_preferences() {
    let fixture = visibility_fixture();
    fixture
        .store
        .set_item(DISABLED_SKILL_PATHS_KEY, "invalid json")
        .unwrap();
    assert!(fixture.catalog.load_disabled_skill_paths().is_empty());
    fixture
        .store
        .set_item(
            DISABLED_SKILL_PATHS_KEY,
            &format!("[\"{REVIEW_PATH}\", null, 42]"),
        )
        .unwrap();
    assert_eq!(fixture.catalog.load_disabled_skill_paths(), [REVIEW_PATH]);
}

fn find_review(fixture: &Fixture, context: &SkillCatalogContext) -> Option<FileSkill> {
    fixture
        .catalog
        .peek_skills(context)?
        .into_iter()
        .find_map(|skill| match skill {
            Skill::File(file) if file.name == "review" => Some(file),
            _ => None,
        })
}

#[test]
fn does_not_restore_an_old_catalog_when_a_scan_finishes_after_hiding_a_skill() {
    let fixture = visibility_fixture();
    let project_path = "/repo/.agents/skills/review/SKILL.md";
    let personal_path = "/home/user/.agents/skills/review/SKILL.md";
    let project = discovered(
        "review",
        "Project review",
        project_path,
        "project",
        "agents",
    );
    let personal = discovered("review", "Personal review", personal_path, "user", "agents");
    let context = ctx(HarnessId::Claude, "/repo");

    // 1. Start discovery while the project skill is enabled.
    let (pending, reply) = deferred();
    fixture.sources.push_listing(reply);
    let old_scan = fixture.catalog.load_skills(&context, false);

    // 2. Disable the project winner during the scan.
    fixture
        .catalog
        .save_disabled_skill_paths(&[project_path.into()])
        .unwrap();

    // 3. Complete a fresh scan that selects the personal file fallback.
    fixture
        .sources
        .push_listing(Reply::Now(Ok(vec![personal.clone()])));
    block_on(fixture.catalog.load_skills(&context, false));
    let active = find_review(&fixture, &context).unwrap();
    assert_eq!(
        (active.path.as_str(), active.scope),
        (personal_path, FileSkillScope::User)
    );

    // 4. Resolve the older scan with the obsolete project candidate.
    pending.send(Ok(vec![project.clone()])).unwrap();
    block_on(old_scan);
    assert_eq!(find_review(&fixture, &context).unwrap().path, personal_path);

    // 5. Re-enable during a scan.
    let (re_enable, reply) = deferred();
    fixture.sources.push_listing(reply);
    let in_flight = fixture.catalog.load_skills(&context, true);
    fixture.catalog.save_disabled_skill_paths(&[]).unwrap();
    fixture.sources.push_listing(Reply::Now(Ok(vec![project])));
    block_on(fixture.catalog.load_skills(&context, false));
    let active = find_review(&fixture, &context).unwrap();
    assert_eq!(
        (active.path.as_str(), active.scope),
        (project_path, FileSkillScope::Project)
    );

    re_enable.send(Ok(vec![personal])).unwrap();
    block_on(in_flight);
    let active = find_review(&fixture, &context).unwrap();
    assert_eq!(
        (active.path.as_str(), active.scope),
        (project_path, FileSkillScope::Project)
    );
}

#[test]
fn falls_back_to_same_name_personal_skill_when_project_skill_is_disabled_and_injects_its_content() {
    let fixture = visibility_fixture();
    let project_path = "/repo/.agents/skills/review/SKILL.md";
    let personal_path = "/home/user/.agents/skills/review/SKILL.md";
    let context = ctx(HarnessId::Claude, "/repo");
    fixture.sources.set_listing(move |_, disabled| {
        let disabled: HashSet<&str> = disabled.iter().map(String::as_str).collect();
        if !disabled.contains(project_path) {
            return Ok(vec![discovered(
                "review",
                "Project review",
                project_path,
                "project",
                "agents",
            )]);
        }
        if !disabled.contains(personal_path) {
            return Ok(vec![discovered(
                "review",
                "Personal review",
                personal_path,
                "user",
                "agents",
            )]);
        }
        Ok(Vec::new())
    });
    fixture.sources.files.lock().extend([
        (
            project_path.to_string(),
            "Project review instructions".to_string(),
        ),
        (
            personal_path.to_string(),
            "Personal review instructions".to_string(),
        ),
    ]);
    let review_in = |skills: Vec<Skill>| {
        skills.into_iter().find_map(|skill| match skill {
            Skill::File(file) if file.name == "review" => Some(file),
            _ => None,
        })
    };
    let turn = || {
        block_on(
            fixture
                .catalog
                .apply_skills_to_turn("/review inspect this", &context),
        )
    };

    // 1. With neither disabled, the project file wins.
    let initial = review_in(block_on(fixture.catalog.load_skills(&context, false))).unwrap();
    assert_eq!(
        (initial.path.as_str(), initial.scope),
        (project_path, FileSkillScope::Project)
    );
    assert!(turn().contains("Project review instructions"));

    // 2. Disabling only the project file makes the personal file active.
    fixture
        .catalog
        .save_disabled_skill_paths(&[project_path.into()])
        .unwrap();
    let fallback = review_in(block_on(fixture.catalog.load_skills(&context, false))).unwrap();
    assert_eq!(
        (fallback.path.as_str(), fallback.scope),
        (personal_path, FileSkillScope::User)
    );
    let fallback_turn = turn();
    assert!(fallback_turn.contains("Personal review instructions"));
    assert!(!fallback_turn.contains("Project review instructions"));

    // 3. Re-enabling the project file restores it as the winner.
    fixture.catalog.save_disabled_skill_paths(&[]).unwrap();
    let restored = review_in(block_on(fixture.catalog.load_skills(&context, false))).unwrap();
    assert_eq!(restored.path, project_path);
    assert!(turn().contains("Project review instructions"));

    // 4. Disabling the lower-priority candidate does not affect the winner.
    fixture
        .catalog
        .save_disabled_skill_paths(&[personal_path.into()])
        .unwrap();
    let winner = review_in(block_on(fixture.catalog.load_skills(&context, false))).unwrap();
    assert_eq!(winner.path, project_path);

    // 5. Disabling both files removes that skill from the active catalog.
    fixture
        .catalog
        .save_disabled_skill_paths(&[project_path.into(), personal_path.into()])
        .unwrap();
    assert!(review_in(block_on(fixture.catalog.load_skills(&context, false))).is_none());
    assert_eq!(turn(), "/review inspect this");
}
