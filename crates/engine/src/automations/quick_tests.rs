//! Ports of the quick composer model tests (quickComposer, quickAttachments,
//! quickComposerDefaults, quickComposerShortcut, quickWorkspace,
//! launchDelivery) and quickLaunchSession.test.ts, plus `QuickLaunch`
//! entity tests.

use std::cell::{Cell, RefCell};
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;
use std::time::Duration;

use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use gpui::{App, AppContext, Task, TestAppContext};
use monocode_core::block::{BlockRole, TurnIntent};
use monocode_core::models::{
    AgentModel, HarnessAvailability, LastModelChoice, ModelCatalog, ModelEnv, ModelPrefs,
    ModelProvider,
};
use monocode_core::session::WorkspaceMode;
use monocode_core::shortcut::{
    Modifiers, ShortcutEvent, is_global_shortcut, is_shortcut, quick_composer_shortcut_label,
    quick_composer_shortcut_preview, shortcut_from_key_event,
};
use monocode_core::{
    Attachment, AttachmentKind, HarnessId, Platform, ProjectProviders, RUNTIME_MODES, Session,
};
use monocode_layout::{LayoutNode, SplitDir, leaf_ids};
use monocode_settings::Kv;
use serde_json::{Value, json};

use super::host::SessionPlacement;
use super::launch_delivery::{
    Accepted, Accepting, Delivery, INITIAL_RETRY_MS, LaunchError, LaunchReceiver, ReceiverOptions,
};
use super::quick_composer::*;
use super::quick_launch::QuickLaunch;
use super::quick_launch_session::accept_quick_launch;
use super::testing::*;
use crate::projects::backend::{Worktree, Worktrees};
use crate::runtime::Engine;
use crate::runtime::testing::init_test_engine;
use crate::submit::acceptance::{ProjectLocationSync, submit_after_project_sync};
use crate::submit::attachments::{AttachmentIo, PathInfo};
use crate::submit::{SubmissionAcceptance, SubmitError};

fn base() -> Value {
    json!({ "prompt": "hi", "cwd": "/Users/me/code/app", "harness": "claude" })
}

fn with(mut value: Value, extra: Value) -> Value {
    for (key, entry) in extra.as_object().unwrap() {
        value[key] = entry.clone();
    }
    value
}

// quickComposer.test.ts: parseQuickLaunch

#[test]
fn accepts_a_complete_launch() {
    assert_eq!(
        parse_quick_launch(&json!({
            "prompt": "fix the flaky test",
            "cwd": "/Users/me/code/app",
            "harness": "claude",
            "reveal": true,
        })),
        Some(QuickLaunchRequest::new(
            "fix the flaky test",
            "/Users/me/code/app",
            HarnessId::Claude,
            true
        ))
    );
}

#[test]
fn carries_the_chosen_model_and_leaves_it_to_the_workspace_when_blank() {
    let codex = with(base(), json!({ "harness": "codex" }));
    assert_eq!(
        parse_quick_launch(&with(codex.clone(), json!({ "model": "gpt-6" })))
            .unwrap()
            .model
            .as_deref(),
        Some("gpt-6")
    );
    assert_eq!(
        parse_quick_launch(&with(codex.clone(), json!({ "model": "" })))
            .unwrap()
            .model,
        None
    );
    assert_eq!(
        parse_quick_launch(&with(codex, json!({ "model": 7 })))
            .unwrap()
            .model,
        None
    );
}

#[test]
fn carries_effort_and_other_model_settings_across_the_launch_boundary() {
    let settings = parse_quick_launch(&with(
        base(),
        json!({ "modelSettings": { "effort": "high", "fast": "true", "invalid": 7 } }),
    ))
    .unwrap()
    .model_settings
    .unwrap();
    assert_eq!(settings.len(), 2);
    assert_eq!(settings["effort"], "high");
    assert_eq!(settings["fast"], "true");
    assert_eq!(
        parse_quick_launch(&with(base(), json!({ "modelSettings": ["high"] })))
            .unwrap()
            .model_settings,
        None
    );
    assert_eq!(parse_quick_launch(&base()).unwrap().model_settings, None);
}

#[test]
fn carries_the_selected_permissions_into_the_session() {
    for mode in RUNTIME_MODES {
        let launch = parse_quick_launch(&json!({
            "prompt": "hi", "cwd": "/tmp/project", "harness": "codex", "runtimeMode": mode.as_str(),
        }))
        .unwrap();
        assert_eq!(launch.runtime_mode, Some(mode));
    }
}

#[test]
fn leaves_missing_or_invalid_permissions_at_the_sessions_supervised_default() {
    let base = json!({ "prompt": "hi", "cwd": "/tmp/project", "harness": "codex" });
    assert_eq!(parse_quick_launch(&base).unwrap().runtime_mode, None);
    assert_eq!(
        parse_quick_launch(&with(base, json!({ "runtimeMode": "unknown" })))
            .unwrap()
            .runtime_mode,
        None
    );
}

#[test]
fn starts_quietly_unless_reveal_is_exactly_true() {
    assert!(
        !parse_quick_launch(&with(
            base(),
            json!({ "harness": "codex", "reveal": "yes" })
        ))
        .unwrap()
        .reveal
    );
}

#[test]
fn drops_blank_prompts_missing_projects_and_unknown_harnesses() {
    assert_eq!(
        parse_quick_launch(&with(base(), json!({ "prompt": "   " }))),
        None
    );
    assert_eq!(
        parse_quick_launch(&with(base(), json!({ "cwd": "" }))),
        None
    );
    assert_eq!(
        parse_quick_launch(&with(base(), json!({ "harness": "gemini" }))),
        None
    );
    assert_eq!(parse_quick_launch(&Value::Null), None);
}

#[test]
fn drops_draft_and_intent_as_the_typescript_parser_did() {
    let launch =
        parse_quick_launch(&with(base(), json!({ "draft": true, "intent": "plan" }))).unwrap();
    assert_eq!(launch.draft, None);
    assert_eq!(launch.intent, None);
}

// orderQuickProjects

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[test]
fn puts_recents_first_and_fills_in_the_rest_of_the_rail_once() {
    assert_eq!(
        order_quick_projects(
            &strings(&["/Users/me/code/b", "/Users/me/code/a/"]),
            &strings(&["/Users/me/code/c"]),
            &strings(&["/Users/me/code/a", "/Users/me/code/d"]),
            &[],
        ),
        strings(&[
            "/Users/me/code/b",
            "/Users/me/code/a",
            "/Users/me/code/c",
            "/Users/me/code/d"
        ])
    );
}

#[test]
fn leaves_out_archived_projects_and_non_projects() {
    assert_eq!(
        order_quick_projects(
            &strings(&["/Users/me/code/a", "~", "/", "/Users/me/code/old"]),
            &[],
            &[],
            &strings(&["/Users/me/code/old"]),
        ),
        strings(&["/Users/me/code/a"])
    );
}

// filterQuickProjects

#[test]
fn matches_the_project_name_or_its_parent_folder() {
    let projects = strings(&["/Users/me/code/monocode", "/Users/me/work/api"]);
    assert_eq!(
        filter_quick_projects(&projects, "mono"),
        strings(&["/Users/me/code/monocode"])
    );
    assert_eq!(
        filter_quick_projects(&projects, "work"),
        strings(&["/Users/me/work/api"])
    );
}

#[test]
fn returns_everything_for_an_empty_query() {
    let projects = strings(&["/Users/me/code/monocode", "/Users/me/work/api"]);
    assert_eq!(filter_quick_projects(&projects, "  "), projects);
}

// filterQuickModels

#[test]
fn matches_the_model_name_its_provider_or_the_harness() {
    let mut kimi = AgentModel::new("opencode/kimi", HarnessId::Opencode, "Kimi K3");
    kimi.provider = Some(ModelProvider {
        id: "moonshot".into(),
        name: "Moonshot".into(),
    });
    let models = vec![
        AgentModel::new("claude-opus", HarnessId::Claude, "Claude Opus"),
        kimi,
    ];
    let ids = |query: &str| -> Vec<String> {
        filter_quick_models(&models, query)
            .into_iter()
            .map(|model| model.id)
            .collect()
    };
    assert_eq!(ids("opus"), ["claude-opus"]);
    assert_eq!(ids("moonshot"), ["opencode/kimi"]);
    assert_eq!(ids("opencode"), ["opencode/kimi"]);
}

#[test]
fn remembers_the_last_project_while_it_is_still_offered() {
    let kv = Kv::in_memory();
    let projects = strings(&["/Users/me/code/a", "/Users/me/code/b"]);
    assert_eq!(
        initial_quick_project(&kv, &projects).as_deref(),
        Some("/Users/me/code/a")
    );
    remember_quick_project(&kv, "/Users/me/code/b");
    assert_eq!(
        initial_quick_project(&kv, &projects).as_deref(),
        Some("/Users/me/code/b")
    );
    remember_quick_project(&kv, "/Users/me/code/gone");
    assert_eq!(
        initial_quick_project(&kv, &projects).as_deref(),
        Some("/Users/me/code/a")
    );
}

#[test]
fn sends_only_live_catalogs_and_applies_them_on_the_other_side() {
    let mut live = ModelCatalog::new();
    live.set_harness_models(
        HarnessId::Codex,
        vec![AgentModel::new("codex:live", HarnessId::Codex, "Live")],
    );
    let sent = live_quick_catalog(&live, |harness| harness == HarnessId::Codex);
    assert_eq!(sent.models.len(), 1);
    assert_eq!(sent.available_harnesses, [HarnessId::Codex]);
    let mut panel = ModelCatalog::new();
    let available = apply_quick_catalog(&mut panel, &serde_json::to_value(&sent).unwrap());
    assert_eq!(available, Some(vec![HarnessId::Codex]));
    assert!(panel.has_live_catalog(HarnessId::Codex));
    assert_eq!(panel.models_for(HarnessId::Codex)[0].id, "codex:live");
    assert_eq!(
        apply_quick_catalog(&mut panel, &json!({ "models": {} })),
        None
    );
}

// quickAttachments.test.ts

#[derive(Default)]
struct FakeIo {
    writes: std::sync::Mutex<Vec<(String, String)>>,
}

impl AttachmentIo for FakeIo {
    fn inspect_paths(
        &self,
        _paths: Vec<String>,
    ) -> BoxFuture<'static, Result<Vec<PathInfo>, String>> {
        futures::future::ready(Ok(Vec::new())).boxed()
    }

    fn read_file_base64(&self, _path: String) -> BoxFuture<'static, Result<String, String>> {
        futures::future::ready(Err("unreadable".into())).boxed()
    }

    fn write_attachment(
        &self,
        name: String,
        data: String,
    ) -> BoxFuture<'static, Result<String, String>> {
        self.writes.lock().unwrap().push((name.clone(), data));
        futures::future::ready(Ok(format!("/tmp/{name}"))).boxed()
    }
}

fn screenshot() -> Attachment {
    Attachment {
        id: "shot".into(),
        name: "Screenshot.png".into(),
        kind: AttachmentKind::Image,
        mime_type: "image/png".into(),
        size: 4,
        data: Some("dGVzdA==".into()),
        preview_url: Some("blob:local".into()),
        ..Attachment::default()
    }
}

fn attachment_base() -> Value {
    json!({ "prompt": "", "cwd": "/tmp/project", "harness": "codex", "reveal": false })
}

#[test]
fn stores_pasted_images_and_sends_only_portable_metadata_across_windows() {
    let io = FakeIo::default();
    let stored =
        futures::executor::block_on(store_quick_attachments(&io, vec![screenshot()])).unwrap();
    assert_eq!(
        *io.writes.lock().unwrap(),
        [("Screenshot.png".to_string(), "dGVzdA==".to_string())]
    );
    let attachments = quick_launch_attachments(&stored).unwrap();
    assert_eq!(
        serde_json::to_value(&attachments).unwrap(),
        json!([{
            "id": "shot", "name": "Screenshot.png", "kind": "image",
            "mimeType": "image/png", "size": 4, "path": "/tmp/Screenshot.png",
        }])
    );
    let launch = parse_quick_launch(&with(
        attachment_base(),
        json!({ "attachments": serde_json::to_value(&attachments).unwrap() }),
    ))
    .unwrap();
    assert_eq!(launch.attachments, Some(attachments));
}

#[test]
fn does_not_rewrite_files_that_already_live_on_disk() {
    let io = FakeIo::default();
    let file = Attachment {
        path: Some("/tmp/file.png".into()),
        ..screenshot()
    };
    let stored =
        futures::executor::block_on(store_quick_attachments(&io, vec![file.clone()])).unwrap();
    assert_eq!(stored, [file]);
    assert!(io.writes.lock().unwrap().is_empty());
}

#[test]
fn rejects_unreadable_or_malformed_attachments_instead_of_silently_losing_them() {
    let io = FakeIo::default();
    let unreadable = Attachment {
        data: None,
        ..screenshot()
    };
    let error =
        futures::executor::block_on(store_quick_attachments(&io, vec![unreadable])).unwrap_err();
    assert!(error.starts_with("Could not attach"));
    assert!(
        quick_launch_attachments(&[screenshot()])
            .unwrap_err()
            .starts_with("Could not attach")
    );
    let image = serde_json::to_value(screenshot()).unwrap();
    for attachment in [
        image.clone(),
        with(image.clone(), json!({ "path": "" })),
        with(image.clone(), json!({ "path": "/tmp/a", "size": -1 })),
        with(image, json!({ "path": "/tmp/a", "kind": "unknown" })),
    ] {
        assert_eq!(
            parse_quick_launch(&with(
                attachment_base(),
                json!({ "prompt": "Look", "attachments": [attachment] })
            )),
            None
        );
    }
}

#[test]
fn rejects_excess_attachments_and_unsupported_providers() {
    let file = with(
        serde_json::to_value(screenshot()).unwrap(),
        json!({ "path": "/tmp/image.png" }),
    );
    let many: Vec<Value> = (0..21).map(|_| file.clone()).collect();
    assert_eq!(
        parse_quick_launch(&with(attachment_base(), json!({ "attachments": many }))),
        None
    );
    assert_eq!(
        parse_quick_launch(&with(
            attachment_base(),
            json!({ "harness": "fx", "attachments": [file] })
        )),
        None
    );
}

// quickComposerDefaults.test.ts

struct Env {
    catalog: ModelCatalog,
    prefs: ModelPrefs,
    availability: HarnessAvailability,
    projects: ProjectProviders,
}

impl Env {
    fn new() -> Self {
        Self {
            catalog: ModelCatalog::new(),
            prefs: ModelPrefs::default(),
            availability: HarnessAvailability::default(),
            projects: ProjectProviders::default(),
        }
    }

    fn env(&self) -> ModelEnv<'_> {
        ModelEnv {
            catalog: &self.catalog,
            prefs: &self.prefs,
            availability: &self.availability,
            projects: &self.projects,
        }
    }
}

fn choice(harness: HarnessId, model: &str) -> LastModelChoice {
    LastModelChoice {
        harness,
        model: model.into(),
    }
}

#[test]
fn uses_the_configured_codex_default_instead_of_the_last_quick_composer_model() {
    let mut env = Env::new();
    env.prefs
        .save_last_model_choice(HarnessId::Codex, "codex:gpt-5.6-luna");
    assert_eq!(
        initial_quick_choice(&env.env()),
        choice(HarnessId::Codex, "codex:gpt-5.6-luna")
    );
}

#[test]
fn rereads_the_providers_default_when_opening_another_quick_session() {
    let mut env = Env::new();
    env.prefs
        .save_last_model_choice(HarnessId::Cursor, "cursor:composer-2.5");
    assert_eq!(initial_quick_choice(&env.env()).harness, HarnessId::Cursor);
    env.prefs
        .save_last_model_choice(HarnessId::Codex, "codex:gpt-5.6-luna");
    assert_eq!(
        initial_quick_choice(&env.env()),
        choice(HarnessId::Codex, "codex:gpt-5.6-luna")
    );
}

#[test]
fn preserves_a_live_only_model_until_its_provider_catalog_arrives() {
    let mut env = Env::new();
    env.prefs
        .save_last_model_choice(HarnessId::Codex, "codex:gpt-5.6-luna");
    let chosen = initial_quick_choice(&env.env());
    assert_eq!(resolve_quick_model(&env.catalog, &chosen), None);
    env.catalog.set_harness_models(
        HarnessId::Codex,
        vec![
            AgentModel::new("codex:other", HarnessId::Codex, "Another model"),
            AgentModel::new("codex:gpt-5.6-luna", HarnessId::Codex, "GPT-5.6-Luna"),
        ],
    );
    assert_eq!(
        resolve_quick_model(&env.catalog, &chosen).unwrap().id,
        "codex:gpt-5.6-luna"
    );
    assert_eq!(initial_quick_choice(&env.env()), chosen);
}

#[test]
fn uses_an_enabled_provider_while_the_configured_default_is_hidden() {
    let mut env = Env::new();
    env.prefs
        .save_last_model_choice(HarnessId::Codex, "codex:gpt-5.6-luna");
    env.prefs
        .set_picker_provider_visible(HarnessId::Codex, false);
    assert_eq!(
        initial_quick_choice(&env.env()),
        choice(
            HarnessId::Claude,
            &env.catalog.default_model_id(HarnessId::Claude)
        )
    );
    // Falling back must not overwrite the configured default.
    assert_eq!(
        env.prefs.last_model,
        Some(choice(HarnessId::Codex, "codex:gpt-5.6-luna"))
    );
    env.prefs
        .set_picker_provider_visible(HarnessId::Codex, true);
    assert_eq!(
        initial_quick_choice(&env.env()),
        choice(HarnessId::Codex, "codex:gpt-5.6-luna")
    );
}

// quickComposerShortcut.test.ts (the helpers live in monocode_core)

fn press(code: &str, meta: bool, ctrl: bool, alt: bool, shift: bool) -> ShortcutEvent {
    ShortcutEvent {
        code: code.into(),
        modifiers: Modifiers {
            meta_key: meta,
            ctrl_key: ctrl,
            alt_key: alt,
            shift_key: shift,
        },
    }
}

#[test]
fn records_physical_keys_and_displays_the_chosen_combination() {
    let shortcut = shortcut_from_key_event(&press("KeyK", true, false, true, false)).unwrap();
    assert_eq!(shortcut, "Command+Option+KeyK");
    assert_eq!(
        quick_composer_shortcut_label(&shortcut, Platform::Mac),
        "⌘⌥K"
    );
    let meta = Modifiers {
        meta_key: true,
        ..Modifiers::default()
    };
    assert_eq!(
        quick_composer_shortcut_preview(meta, None, None, Platform::Mac),
        "⌘"
    );
    assert_eq!(
        quick_composer_shortcut_preview(meta, Some("KeyK"), None, Platform::Mac),
        "⌘K"
    );
    assert_eq!(
        shortcut_from_key_event(&press("KeyK", true, false, false, false)).as_deref(),
        Some("Command+KeyK")
    );
    assert_eq!(
        shortcut_from_key_event(&press("Digit2", false, true, false, true)).as_deref(),
        Some("Control+Shift+Digit2")
    );
}

#[test]
fn rejects_plain_typing_modifier_keys_and_unsupported_codes() {
    assert_eq!(
        shortcut_from_key_event(&press("Space", false, false, false, true)),
        None
    );
    assert_eq!(
        shortcut_from_key_event(&press("MetaLeft", true, false, false, false)),
        None
    );
    assert!(!is_shortcut("Shift+Space"));
    assert!(!is_shortcut("Command+Command+Space"));
    assert!(is_shortcut("Command+KeyK"));
    assert!(is_shortcut("Control+KeyK"));
}

#[test]
fn records_option_chords_but_keeps_them_out_of_os_global_hotkeys() {
    assert_eq!(
        shortcut_from_key_event(&press("KeyK", false, false, true, false)).as_deref(),
        Some("Option+KeyK")
    );
    assert_eq!(
        shortcut_from_key_event(&press("KeyK", false, false, true, true)).as_deref(),
        Some("Option+Shift+KeyK")
    );
    assert_eq!(
        shortcut_from_key_event(&press("KeyK", false, false, false, true)),
        None
    );
    assert!(is_shortcut("Option+KeyK"));
    assert!(!is_global_shortcut("Option+KeyK"));
    assert!(is_global_shortcut("Control+Option+KeyK"));
}

// quickWorkspace.test.ts

fn feature_tree() -> Worktree {
    Worktree {
        head: "abc".into(),
        dirty: Some(false),
        unpushed: Some(0),
        ..Worktree::new("/tmp/project-feature", Some("feature"))
    }
}

fn workspace_base() -> Value {
    json!({ "prompt": "fix it", "cwd": "/tmp/project", "harness": "codex", "reveal": false })
}

fn launch_with(fields: QuickWorkspaceFields) -> Value {
    let mut launch = parse_quick_launch(&workspace_base()).unwrap();
    fields.apply(&mut launch);
    serde_json::to_value(launch).unwrap()
}

#[test]
fn defers_new_worktree_creation_and_carries_the_chosen_base_into_the_session() {
    let listed = Cell::new(0);
    let fields = futures::executor::block_on(quick_workspace_launch(
        &QuickWorkspace {
            cwd: Some("/tmp/project".into()),
            mode: WorkspaceMode::Worktree,
            base: Some("origin/develop".into()),
            tree: None,
        },
        |_| {
            listed.set(listed.get() + 1);
            async { Ok(Worktrees::default()) }
        },
    ))
    .unwrap();
    assert_eq!(listed.get(), 0);
    let launch = parse_quick_launch(&launch_with(fields)).unwrap();
    let session = apply_quick_workspace(
        Session::blank("s", HarnessId::Codex, "codex:model", "/tmp/project"),
        &launch,
    );
    assert_eq!(session.workspace_mode, Some(WorkspaceMode::Worktree));
    assert_eq!(session.worktree_base.as_deref(), Some("origin/develop"));
    assert_eq!(session.cwd, "/tmp/project");
    assert_eq!(session.worktree_cwd, None);
}

#[test]
fn revalidates_an_existing_worktree_and_starts_in_it_while_keeping_the_project_identity() {
    let asked = RefCell::new(Vec::new());
    let fields = futures::executor::block_on(quick_workspace_launch(
        &QuickWorkspace {
            cwd: Some("/tmp/project".into()),
            mode: WorkspaceMode::Current,
            base: None,
            tree: Some(feature_tree()),
        },
        |cwd| {
            asked.borrow_mut().push(cwd);
            async {
                Ok(Worktrees {
                    worktrees: vec![feature_tree()],
                    default_root: String::new(),
                })
            }
        },
    ))
    .unwrap();
    assert_eq!(*asked.borrow(), ["/tmp/project"]);
    let launch = parse_quick_launch(&launch_with(fields)).unwrap();
    let session = apply_quick_workspace(
        Session::blank("s", HarnessId::Codex, "codex:model", "/tmp/project"),
        &launch,
    );
    assert_eq!(session.cwd, "/tmp/project");
    assert_eq!(
        session.worktree_cwd.as_deref(),
        Some("/tmp/project-feature")
    );
}

#[test]
fn rejects_worktrees_that_were_removed_after_selecting_them() {
    let error = futures::executor::block_on(quick_workspace_launch(
        &QuickWorkspace {
            cwd: Some("/tmp/project".into()),
            mode: WorkspaceMode::Current,
            base: None,
            tree: Some(feature_tree()),
        },
        |_| async {
            Ok(Worktrees {
                worktrees: vec![Worktree {
                    missing: true,
                    ..feature_tree()
                }],
                default_root: String::new(),
            })
        },
    ))
    .unwrap_err();
    assert!(error.contains("no longer available"));
}

#[test]
fn resets_the_base_and_existing_worktree_when_the_project_changes() {
    assert_eq!(
        workspace_for_project(
            &QuickWorkspace {
                cwd: Some("/tmp/project".into()),
                mode: WorkspaceMode::Worktree,
                base: Some("feature".into()),
                tree: Some(feature_tree()),
            },
            Some("/tmp/other"),
        ),
        QuickWorkspace {
            cwd: Some("/tmp/other".into()),
            mode: WorkspaceMode::Current,
            base: None,
            tree: None,
        }
    );
}

#[test]
fn keeps_ordinary_launches_compatible_and_rejects_conflicting_workspace_fields() {
    assert_eq!(
        serde_json::to_value(parse_quick_launch(&workspace_base()).unwrap()).unwrap(),
        workspace_base()
    );
    for fields in [
        json!({ "workspaceMode": "unknown" }),
        json!({ "workspaceMode": "worktree", "worktreeCwd": "/tmp/project-feature" }),
        json!({ "worktreeBase": "main" }),
        json!({ "workspaceMode": "worktree", "worktreeBase": "" }),
        json!({ "worktreeCwd": 42 }),
    ] {
        assert_eq!(parse_quick_launch(&with(workspace_base(), fields)), None);
    }
}

// launchDelivery.test.ts

type Step<T> = Result<T, LaunchError>;

/// The queue, `take`, `accept`, and `ack` of the TypeScript `setup()`.
#[derive(Default)]
struct Line {
    queue: RefCell<Vec<Value>>,
    takes: Cell<usize>,
    take_script: RefCell<VecDeque<Step<Option<Value>>>>,
    take_always: RefCell<Option<Value>>,
    /// Set `disposed` during the next take.
    dispose_on_take: Cell<bool>,
    accepts: RefCell<Vec<String>>,
    accept_script: RefCell<VecDeque<Step<()>>>,
    accept_fails: Cell<bool>,
    accept_gate: RefCell<Option<oneshot::Receiver<()>>>,
    /// Set `disposed` and fail during the next accept.
    dispose_on_accept: Cell<bool>,
    acks: RefCell<Vec<String>>,
    ack_script: RefCell<VecDeque<Step<()>>>,
    disposed: Cell<bool>,
}

fn request() -> Value {
    json!({ "prompt": "go", "cwd": "/repo", "harness": "codex", "reveal": false })
}

fn not_ready() -> LaunchError {
    LaunchError::Failed("not ready".into())
}

fn interrupted() -> LaunchError {
    LaunchError::Failed("IPC interrupted".into())
}

fn line() -> Rc<Line> {
    let line = Line::default();
    *line.queue.borrow_mut() = vec![
        json!({ "id": "first", "request": request() }),
        json!({ "id": "second", "request": request() }),
    ];
    Rc::new(line)
}

fn options(line: &Rc<Line>, accepted: Accepted, accepting: Accepting) -> ReceiverOptions {
    let take_line = line.clone();
    let accept_line = line.clone();
    let ack_line = line.clone();
    let disposed_line = line.clone();
    ReceiverOptions {
        take: Rc::new(move |_cx: &mut App| {
            let line = &take_line;
            line.takes.set(line.takes.get() + 1);
            if line.dispose_on_take.replace(false) {
                line.disposed.set(true);
            }
            if let Some(step) = line.take_script.borrow_mut().pop_front() {
                return Task::ready(step);
            }
            if let Some(value) = line.take_always.borrow().clone() {
                return Task::ready(Ok(Some(value)));
            }
            Task::ready(Ok(line.queue.borrow().first().cloned()))
        }),
        accept: Rc::new(move |_launch, id, cx: &mut App| {
            let line = accept_line.clone();
            line.accepts.borrow_mut().push(id);
            if line.dispose_on_accept.replace(false) {
                line.disposed.set(true);
                return Task::ready(Err(not_ready()));
            }
            if let Some(gate) = line.accept_gate.borrow_mut().take() {
                return cx.spawn(async move |_| {
                    let _ = gate.await;
                    Ok(())
                });
            }
            if let Some(step) = line.accept_script.borrow_mut().pop_front() {
                return Task::ready(step);
            }
            if line.accept_fails.get() {
                return Task::ready(Err(not_ready()));
            }
            Task::ready(Ok(()))
        }),
        ack: Rc::new(move |id: String, _cx: &mut App| {
            let line = &ack_line;
            line.acks.borrow_mut().push(id.clone());
            if let Some(step) = line.ack_script.borrow_mut().pop_front() {
                return Task::ready(step);
            }
            line.queue
                .borrow_mut()
                .retain(|entry| entry["id"].as_str() != Some(id.as_str()));
            Task::ready(Ok(()))
        }),
        disposed: Rc::new(move || disposed_line.disposed.get()),
        accepted,
        accepting,
    }
}

fn receiver(line: &Rc<Line>) -> (LaunchReceiver, Accepted) {
    let accepted = Accepted::default();
    (
        LaunchReceiver::new(options(line, accepted.clone(), Accepting::default())),
        accepted,
    )
}

fn result(delivery: Delivery) -> Result<(), LaunchError> {
    delivery.now_or_never().expect("delivery finished")
}

fn advance(cx: &mut TestAppContext, millis: u64) {
    cx.executor().advance_clock(Duration::from_millis(millis));
    cx.run_until_parked();
}

#[gpui::test]
fn serializes_mount_focus_event_triggers_and_acknowledges_each_accepted_launch(
    cx: &mut TestAppContext,
) {
    let line = line();
    let (receiver, _) = receiver(&line);
    let deliveries: Vec<Delivery> = (0..3)
        .map(|_| cx.update(|cx| receiver.receive(cx)))
        .collect();
    cx.run_until_parked();
    for delivery in deliveries {
        assert_eq!(result(delivery), Ok(()));
    }
    assert_eq!(*line.accepts.borrow(), ["first", "second"]);
    assert!(line.queue.borrow().is_empty());
}

#[gpui::test]
fn retries_a_failed_acceptance_without_another_event(cx: &mut TestAppContext) {
    let line = line();
    line.accept_script.borrow_mut().push_back(Err(not_ready()));
    let (receiver, _) = receiver(&line);
    let delivery = cx.update(|cx| receiver.receive(cx));
    cx.run_until_parked();
    assert_eq!(result(delivery), Err(not_ready()));
    assert!(line.acks.borrow().is_empty());
    assert_eq!(line.queue.borrow().len(), 2);
    advance(cx, 250);
    assert_eq!(*line.accepts.borrow(), ["first", "first", "second"]);
    assert!(line.queue.borrow().is_empty());
    assert!(!receiver.has_pending_retry());
}

#[gpui::test]
fn retries_a_lost_ack_without_another_event_or_accepting_the_launch_twice(cx: &mut TestAppContext) {
    let line = line();
    line.queue.borrow_mut().truncate(1);
    line.ack_script.borrow_mut().push_back(Err(interrupted()));
    let (receiver, accepted) = receiver(&line);
    let delivery = cx.update(|cx| receiver.receive(cx));
    cx.run_until_parked();
    assert_eq!(result(delivery), Err(interrupted()));
    assert_eq!(line.queue.borrow().len(), 1);
    assert!(accepted.borrow().contains("first"));
    advance(cx, 250);
    assert_eq!(line.accepts.borrow().len(), 1);
    assert_eq!(line.acks.borrow().len(), 2);
    assert!(line.queue.borrow().is_empty());
    assert!(accepted.borrow().is_empty());
    assert!(!receiver.has_pending_retry());
}

#[gpui::test]
fn does_not_automatically_retry_or_acknowledge_invalid_payloads(cx: &mut TestAppContext) {
    for value in [
        json!({ "id": "first", "request": { "prompt": "malformed" } }),
        json!({ "id": 123, "request": request() }),
        json!({ "id": "", "request": request() }),
        json!("invalid envelope"),
    ] {
        let line = line();
        *line.take_always.borrow_mut() = Some(value);
        let (receiver, _) = receiver(&line);
        let delivery = cx.update(|cx| receiver.receive(cx));
        cx.run_until_parked();
        let error = result(delivery).unwrap_err();
        assert!(matches!(error, LaunchError::Invalid(_)));
        assert!(error.message().contains("Invalid queued session"));
        advance(cx, 60_000);
        assert_eq!(line.takes.get(), 1);
        assert!(line.accepts.borrow().is_empty());
        assert!(line.acks.borrow().is_empty());
        assert!(!receiver.has_pending_retry());
    }
}

#[gpui::test]
fn stops_automatic_retries_if_a_subsequent_take_returns_an_invalid_payload(
    cx: &mut TestAppContext,
) {
    let line = line();
    line.take_script.borrow_mut().push_back(Err(interrupted()));
    *line.take_always.borrow_mut() = Some(json!({ "id": "first", "request": {} }));
    let (receiver, _) = receiver(&line);
    let delivery = cx.update(|cx| receiver.receive(cx));
    cx.run_until_parked();
    assert_eq!(result(delivery), Err(interrupted()));
    advance(cx, 60_000);
    assert_eq!(line.takes.get(), 2);
    assert!(line.accepts.borrow().is_empty());
    assert!(line.acks.borrow().is_empty());
    assert!(!receiver.has_pending_retry());
}

#[gpui::test]
fn caps_exponential_backoff_and_resets_it_after_successful_delivery(cx: &mut TestAppContext) {
    let line = line();
    line.accept_fails.set(true);
    let (receiver, _) = receiver(&line);
    let delivery = cx.update(|cx| receiver.receive(cx));
    cx.run_until_parked();
    assert_eq!(result(delivery), Err(not_ready()));
    let mut attempts = 1;
    for delay in [250, 500, 1000, 2000, 4000, 8000, 16000, 30000, 30000] {
        advance(cx, delay - 1);
        assert_eq!(line.accepts.borrow().len(), attempts);
        advance(cx, 1);
        attempts += 1;
        assert_eq!(line.accepts.borrow().len(), attempts);
        assert!(receiver.has_pending_retry());
    }
    line.accept_fails.set(false);
    advance(cx, 30_000);
    assert!(line.queue.borrow().is_empty());
    assert!(!receiver.has_pending_retry());
    assert_eq!(receiver.retry_delay_ms(), INITIAL_RETRY_MS);

    line.queue
        .borrow_mut()
        .push(json!({ "id": "third", "request": request() }));
    line.accept_script.borrow_mut().push_back(Err(not_ready()));
    let delivery = cx.update(|cx| receiver.receive(cx));
    cx.run_until_parked();
    assert_eq!(result(delivery), Err(not_ready()));
    advance(cx, 250);
    assert!(line.queue.borrow().is_empty());
    assert!(!receiver.has_pending_retry());
}

#[gpui::test]
fn lets_an_event_retry_immediately_without_leaving_a_redundant_timer(cx: &mut TestAppContext) {
    let line = line();
    line.take_script.borrow_mut().push_back(Err(interrupted()));
    let (receiver, _) = receiver(&line);
    let delivery = cx.update(|cx| receiver.receive(cx));
    cx.run_until_parked();
    assert_eq!(result(delivery), Err(interrupted()));
    assert!(receiver.has_pending_retry());
    let again: Vec<Delivery> = (0..2)
        .map(|_| cx.update(|cx| receiver.receive(cx)))
        .collect();
    cx.run_until_parked();
    for delivery in again {
        assert_eq!(result(delivery), Ok(()));
    }
    assert!(line.queue.borrow().is_empty());
    assert!(!receiver.has_pending_retry());
    assert_eq!(line.accepts.borrow().len(), 2);
}

#[gpui::test]
fn cancels_pending_retries_when_disposed(cx: &mut TestAppContext) {
    let line = line();
    line.accept_script.borrow_mut().push_back(Err(not_ready()));
    let (receiver, _) = receiver(&line);
    let delivery = cx.update(|cx| receiver.receive(cx));
    cx.run_until_parked();
    assert_eq!(result(delivery), Err(not_ready()));
    receiver.dispose();
    assert!(!receiver.has_pending_retry());
    advance(cx, 60_000);
    let delivery = cx.update(|cx| receiver.receive(cx));
    cx.run_until_parked();
    assert_eq!(result(delivery), Ok(()));
    assert_eq!(line.takes.get(), 1);
}

#[gpui::test]
fn does_not_schedule_retries_for_an_in_flight_failure_after_disposal(cx: &mut TestAppContext) {
    let line = line();
    line.dispose_on_accept.set(true);
    let (receiver, _) = receiver(&line);
    let delivery = cx.update(|cx| receiver.receive(cx));
    cx.run_until_parked();
    assert_eq!(result(delivery), Err(not_ready()));
    assert!(!receiver.has_pending_retry());
}

#[gpui::test]
fn keeps_the_launch_queued_if_the_owner_goes_away_during_a_take(cx: &mut TestAppContext) {
    let line = line();
    line.dispose_on_take.set(true);
    let (receiver, _) = receiver(&line);
    let delivery = cx.update(|cx| receiver.receive(cx));
    cx.run_until_parked();
    assert_eq!(result(delivery), Ok(()));
    assert!(line.accepts.borrow().is_empty());
    assert!(line.acks.borrow().is_empty());
    assert_eq!(line.queue.borrow().len(), 2);
}

#[gpui::test]
fn shares_an_in_flight_acceptance_across_remounts(cx: &mut TestAppContext) {
    let line = line();
    let (release, gate) = oneshot::channel();
    *line.accept_gate.borrow_mut() = Some(gate);
    let accepted = Accepted::default();
    let accepting = Accepting::default();
    let first_receiver = LaunchReceiver::new(options(&line, accepted.clone(), accepting.clone()));
    let first = cx.update(|cx| first_receiver.receive(cx));
    cx.run_until_parked();
    let second_receiver = LaunchReceiver::new(options(&line, accepted, accepting));
    let second = cx.update(|cx| second_receiver.receive(cx));
    cx.run_until_parked();
    assert!(line.acks.borrow().is_empty());
    release.send(()).unwrap();
    cx.run_until_parked();
    assert_eq!(result(first), Ok(()));
    assert_eq!(result(second), Ok(()));
    assert_eq!(line.accepts.borrow().len(), 2);
    assert!(line.queue.borrow().is_empty());
}

// quickLaunchSession.test.ts

struct Session2 {
    host: Rc<FakeHost>,
    old_session: String,
    old_tab: String,
    line: Rc<Line>,
    receiver: LaunchReceiver,
}

fn launch_request(reveal: bool) -> QuickLaunchRequest {
    QuickLaunchRequest::new(
        "hello from the floating composer",
        "/new-project",
        HarnessId::Codex,
        reveal,
    )
}

/// The TypeScript `setup()`: an old session in its tab, and a receiver
/// whose `accept` is `acceptQuickLaunch` over the fake workspace.
fn session_setup(cx: &mut TestAppContext, reveal: bool) -> Session2 {
    let host = FakeHost::new();
    init_engine_with_hosts(cx, std::slice::from_ref(&host));
    let old = cx.update(|cx| open_session("old-session", "/old-project", cx));
    let old_tab = monocode_layout::new_tab(&old.id);
    let old_tab_id = old_tab.id.clone();
    host.tabs.borrow_mut().push(old_tab);
    *host.project_cwd.borrow_mut() = "/old-project".into();
    let line = Rc::new(Line::default());
    *line.queue.borrow_mut() =
        vec![json!({ "id": "quick-session", "request": launch_request(reveal) })];
    let accept_host = host.clone();
    let mut options = options(&line, Accepted::default(), Accepting::default());
    options.accept = Rc::new(move |launch, id, cx: &mut App| {
        let host = accept_host.clone();
        cx.spawn(async move |cx| accept_quick_launch(launch, id, host, None, cx).await)
    });
    Session2 {
        host,
        old_session: old.id,
        old_tab: old_tab_id,
        line,
        receiver: LaunchReceiver::new(options),
    }
}

fn accept_now(
    setup: &Session2,
    launch: QuickLaunchRequest,
    id: &str,
    placement: Option<SessionPlacement>,
    cx: &mut TestAppContext,
) -> Result<(), LaunchError> {
    let host = setup.host.clone();
    let id = id.to_string();
    let task = cx.update(|cx| {
        cx.spawn(async move |cx| accept_quick_launch(launch, id, host, placement, cx).await)
    });
    cx.run_until_parked();
    task.now_or_never().expect("accepted")
}

fn open(cx: &mut TestAppContext, id: &str) -> Option<Session> {
    cx.update(|cx| Engine::sessions(cx).read(cx).get(id).cloned())
}

#[gpui::test]
fn switches_project_and_recents_before_revealing_a_cross_project_quick_session(
    cx: &mut TestAppContext,
) {
    let setup = session_setup(cx, true);
    let delivery = cx.update(|cx| setup.receiver.receive(cx));
    cx.run_until_parked();
    assert_eq!(result(delivery), Ok(()));
    let log = setup.host.log();
    let tab = setup.host.tabs.borrow()[1].id.clone();
    assert_eq!(
        log,
        [
            format!("append:{tab}:/new-project"),
            "project:/new-project".to_string(),
            "remember:/new-project".to_string(),
            format!("reveal:{tab}:/new-project"),
        ]
    );
    assert_eq!(*setup.host.project_cwd.borrow(), "/new-project");
}

#[gpui::test]
fn leaves_the_selected_project_recents_and_active_tab_unchanged_for_background_launches(
    cx: &mut TestAppContext,
) {
    let setup = session_setup(cx, false);
    let delivery = cx.update(|cx| setup.receiver.receive(cx));
    cx.run_until_parked();
    assert_eq!(result(delivery), Ok(()));
    assert_eq!(*setup.host.project_cwd.borrow(), "/old-project");
    let log = setup.host.log();
    assert_eq!(log.len(), 1);
    assert!(log[0].starts_with("append:"));
    assert_eq!(setup.line.acks.borrow().len(), 1);
}

#[gpui::test]
fn starts_the_first_turn_in_the_mode_picked_in_the_floating_composer(cx: &mut TestAppContext) {
    let setup = session_setup(cx, false);
    let mut launch = launch_request(false);
    launch.intent = Some(QuickIntent::Orchestrate);
    accept_now(&setup, launch, "quick-session", None, cx).unwrap();
    let submit = setup.host.submits.borrow()[0].clone();
    assert_eq!(submit.session_id, "quick-session");
    assert_eq!(submit.text, "hello from the floating composer");
    assert!(submit.attachments.is_empty());
    assert_eq!(submit.intent, Some(TurnIntent::Orchestrate));
}

#[gpui::test]
fn creates_a_draft_only_session_without_submitting_an_agent_turn(cx: &mut TestAppContext) {
    let setup = session_setup(cx, false);
    let mut launch = launch_request(false);
    launch.draft = Some(true);
    accept_now(&setup, launch.clone(), "quick-session", None, cx).unwrap();
    assert!(setup.host.submits.borrow().is_empty());
    assert_eq!(
        *setup.host.drafts.borrow(),
        [(
            "quick-session".to_string(),
            launch.prompt.clone(),
            "quick-session".to_string()
        )]
    );
    let session = open(cx, "quick-session").unwrap();
    assert_eq!(session.quick_launch_accepted, Some(true));
    assert_eq!(session.blocks.len(), 1);
    assert!(session.blocks[0].is_draft());
    assert_eq!(
        session.blocks[0].app_request_id.as_deref(),
        Some("quick-session")
    );
    accept_now(&setup, launch.clone(), "quick-session", None, cx).unwrap();
    assert_eq!(setup.host.drafts.borrow().len(), 1);
    cx.update(|cx| {
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.update("quick-session", cx, |session| {
                session.quick_launch_accepted = None;
                for block in &mut session.blocks {
                    block.draft = Some(false);
                }
            });
        })
    });
    accept_now(&setup, launch, "quick-session", None, cx).unwrap();
    assert_eq!(setup.host.drafts.borrow().len(), 1);
}

#[gpui::test]
fn places_successive_sessions_in_right_and_down_splits_of_the_same_tab(cx: &mut TestAppContext) {
    let setup = session_setup(cx, true);
    let mut launch = launch_request(true);
    launch.cwd = "/old-project".into();
    launch.draft = Some(true);
    accept_now(
        &setup,
        launch.clone(),
        "right-session",
        Some(SessionPlacement {
            direction: SplitDir::Right,
            beside_session_id: setup.old_session.clone(),
        }),
        cx,
    )
    .unwrap();
    accept_now(
        &setup,
        launch,
        "down-session",
        Some(SessionPlacement {
            direction: SplitDir::Down,
            beside_session_id: "right-session".into(),
        }),
        cx,
    )
    .unwrap();
    let tabs = setup.host.tabs.borrow();
    assert_eq!(tabs.len(), 1);
    assert_eq!(tabs[0].id, setup.old_tab);
    let LayoutNode::Split(outer) = &tabs[0].layout else {
        panic!("a split");
    };
    assert_eq!(outer.dir, SplitDir::Right);
    assert_eq!(
        leaf_ids(&outer.children[0]),
        std::slice::from_ref(&setup.old_session)
    );
    let LayoutNode::Split(inner) = &outer.children[1] else {
        panic!("a nested split");
    };
    assert_eq!(inner.dir, SplitDir::Down);
    assert_eq!(
        leaf_ids(&outer.children[1]),
        ["right-session", "down-session"]
    );
    let log = setup.host.log();
    assert!(!log.iter().any(|entry| entry.starts_with("append:")));
    assert_eq!(
        log.last().unwrap(),
        &format!("reveal:{}:/old-project", setup.old_tab)
    );
    assert!(setup.host.submits.borrow().is_empty());
}

#[gpui::test]
fn rejects_a_missing_pane_before_adding_or_submitting_a_session(cx: &mut TestAppContext) {
    let setup = session_setup(cx, false);
    let error = accept_now(
        &setup,
        launch_request(false),
        "lost-session",
        Some(SessionPlacement {
            direction: SplitDir::Right,
            beside_session_id: "missing".into(),
        }),
        cx,
    )
    .unwrap_err();
    assert_eq!(error.message(), "Target pane unavailable");
    assert!(open(cx, "lost-session").is_none());
    assert!(setup.host.submits.borrow().is_empty());
}

/// `submitAfterProjectSync` behind a deferred acceptance, committing the
/// turn once the project check passes.
fn deferred_submit(
    sync: impl std::future::Future<Output = Result<Option<ProjectLocationSync>, String>> + 'static,
    submit: Result<bool, SubmitError>,
    errors: Rc<Cell<usize>>,
    session_id: &str,
    text: &str,
    cx: &mut App,
) -> SubmissionAcceptance {
    let (sender, acceptance) = SubmissionAcceptance::deferred();
    let (session_id, text) = (session_id.to_string(), text.to_string());
    cx.spawn(async move |cx| {
        let result = submit_after_project_sync(
            "/new-project",
            sync,
            |_, _| async { Ok(()) },
            || async move { submit },
            |_| errors.set(errors.get() + 1),
        )
        .await;
        if result == Ok(true) {
            cx.update(|cx| commit_turn(&session_id, &text, cx));
        }
        let _ = sender.send(result);
    })
    .detach();
    acceptance
}

fn located() -> ProjectLocationSync {
    ProjectLocationSync {
        path: "/new-project".into(),
        identity: "repo".into(),
        moved: false,
    }
}

fn user_turns(session: &Session) -> usize {
    session
        .blocks
        .iter()
        .filter(|block| block.role == BlockRole::User)
        .count()
}

#[gpui::test]
fn does_not_acknowledge_or_mark_accepted_while_project_synchronization_is_pending(
    cx: &mut TestAppContext,
) {
    let setup = session_setup(cx, false);
    let (resolve, gate) = oneshot::channel::<()>();
    let gate = RefCell::new(Some(gate));
    let errors = Rc::new(Cell::new(0));
    setup.host.on_submit(move |session_id, text, cx| {
        let gate = gate.borrow_mut().take().expect("one deferred submit");
        let sync = async move {
            gate.await.map_err(|error| error.to_string())?;
            Ok(Some(located()))
        };
        deferred_submit(sync, Ok(true), errors.clone(), session_id, text, cx)
    });
    let delivery = cx.update(|cx| setup.receiver.receive(cx));
    cx.run_until_parked();
    assert_eq!(setup.host.submits.borrow().len(), 1);
    assert!(setup.line.acks.borrow().is_empty());
    let session = open(cx, "quick-session").unwrap();
    assert_eq!(user_turns(&session), 0);
    assert_eq!(session.quick_launch_accepted, None);
    resolve.send(()).unwrap();
    cx.run_until_parked();
    assert_eq!(result(delivery), Ok(()));
    let session = open(cx, "quick-session").unwrap();
    assert_eq!(user_turns(&session), 1);
    assert_eq!(setup.line.acks.borrow().len(), 1);
    assert_eq!(session.quick_launch_accepted, Some(true));
}

#[gpui::test]
fn retains_the_prompt_after_a_failure_and_retries_the_same_session(cx: &mut TestAppContext) {
    for failure in ["sync failure", "deferred rejection", "deferred exception"] {
        let setup = session_setup(cx, false);
        let errors = Rc::new(Cell::new(0));
        let calls = Rc::new(Cell::new(0));
        let handler_errors = errors.clone();
        let handler_calls = calls.clone();
        setup.host.on_submit(move |session_id, text, cx| {
            handler_calls.set(handler_calls.get() + 1);
            if handler_calls.get() > 1 {
                return SubmissionAcceptance::Ready(commit_turn(session_id, text, cx));
            }
            let (sync, submit): (Result<Option<ProjectLocationSync>, String>, _) = match failure {
                "sync failure" => (Err("disk unavailable".into()), Ok(true)),
                "deferred rejection" => (Ok(Some(located())), Ok(false)),
                _ => (
                    Ok(Some(located())),
                    Err(SubmitError::message("submission failed")),
                ),
            };
            deferred_submit(
                async move { sync },
                submit,
                handler_errors.clone(),
                session_id,
                text,
                cx,
            )
        });
        let delivery = cx.update(|cx| setup.receiver.receive(cx));
        cx.run_until_parked();
        let error = result(delivery).unwrap_err();
        assert!(error.message().contains("could not accept"), "{failure}");
        assert!(setup.line.acks.borrow().is_empty());
        assert_eq!(setup.line.queue.borrow().len(), 1);
        let session = open(cx, "quick-session").unwrap();
        assert_eq!(session.quick_launch_accepted, None);
        assert_eq!(user_turns(&session), 0);
        assert_eq!(
            errors.get(),
            if failure == "deferred rejection" {
                0
            } else {
                1
            },
            "{failure}"
        );

        advance(cx, 250);
        assert!(setup.line.queue.borrow().is_empty(), "{failure}");
        assert_eq!(setup.line.acks.borrow().len(), 1);
        let session = open(cx, "quick-session").unwrap();
        assert_eq!(user_turns(&session), 1);
        assert_eq!(session.quick_launch_accepted, Some(true));
        let appended = setup
            .host
            .log()
            .iter()
            .filter(|entry| entry.starts_with("append:"))
            .count();
        assert_eq!(appended, 1, "{failure}");
    }
}

#[gpui::test]
fn retains_a_missing_projects_prompt_without_automatic_retries_then_accepts_an_explicit_retry(
    cx: &mut TestAppContext,
) {
    let setup = session_setup(cx, false);
    let connected = Rc::new(Cell::new(false));
    let errors = Rc::new(Cell::new(0));
    let handler_connected = connected.clone();
    let handler_errors = errors.clone();
    setup.host.on_submit(move |session_id, text, cx| {
        let location = handler_connected.get().then(located);
        deferred_submit(
            async move { Ok(location) },
            Ok(true),
            handler_errors.clone(),
            session_id,
            text,
            cx,
        )
    });
    let delivery = cx.update(|cx| setup.receiver.receive(cx));
    cx.run_until_parked();
    assert!(matches!(
        result(delivery),
        Err(LaunchError::ProjectNotFound(_))
    ));
    advance(cx, 300_000);
    assert!(!setup.receiver.has_pending_retry());
    assert_eq!(setup.host.submits.borrow().len(), 1);
    assert_eq!(errors.get(), 1);
    assert!(setup.line.acks.borrow().is_empty());
    assert_eq!(setup.line.queue.borrow().len(), 1);
    assert_eq!(
        open(cx, "quick-session").unwrap().quick_launch_accepted,
        None
    );

    connected.set(true);
    let delivery = cx.update(|cx| setup.receiver.receive(cx));
    cx.run_until_parked();
    assert_eq!(result(delivery), Ok(()));
    assert!(setup.line.queue.borrow().is_empty());
    assert_eq!(setup.line.acks.borrow().len(), 1);
    let appended = setup
        .host
        .log()
        .iter()
        .filter(|entry| entry.starts_with("append:"))
        .count();
    assert_eq!(appended, 1);
}

#[gpui::test]
fn does_not_submit_an_accepted_prompt_again_after_a_lost_ack(cx: &mut TestAppContext) {
    let setup = session_setup(cx, false);
    setup
        .line
        .ack_script
        .borrow_mut()
        .push_back(Err(interrupted()));
    let delivery = cx.update(|cx| setup.receiver.receive(cx));
    cx.run_until_parked();
    assert_eq!(result(delivery), Err(interrupted()));
    advance(cx, 250);
    assert_eq!(setup.host.submits.borrow().len(), 1);
    assert!(setup.line.queue.borrow().is_empty());
}

// The QuickLaunch entity.

fn quick_launch(cx: &mut TestAppContext) -> gpui::Entity<QuickLaunch> {
    init_test_engine(cx);
    cx.update(|cx| cx.new(|cx| QuickLaunch::new(Kv::in_memory(), Platform::Mac, cx)))
}

#[gpui::test]
fn submitting_hands_the_launch_to_the_focused_window(cx: &mut TestAppContext) {
    let entity = quick_launch(cx);
    let main = FakeHost::new();
    let second = FakeHost::new();
    second.focused.set(true);
    entity.update(cx, |quick, cx| {
        quick.attach_window("main", main.clone(), cx);
        quick.attach_window("window-2", second.clone(), cx);
    });
    cx.run_until_parked();
    let mut request = QuickLaunchRequest::new("Ship it", "/tmp/project", HarnessId::Codex, true);
    request.model = Some("codex:gpt-5.5".into());
    let id = entity
        .update(cx, |quick, cx| quick.submit(request, cx))
        .unwrap();
    cx.run_until_parked();
    assert!(main.submits.borrow().is_empty());
    assert_eq!(second.submits.borrow()[0].session_id, id);
    assert_eq!(second.forward.get(), 1);
    entity.read_with(cx, |quick, _| assert!(quick.queue().is_empty()));
    let session = open(cx, &id).unwrap();
    assert_eq!(session.quick_launch_accepted, Some(true));
    assert_eq!(session.cwd, "/tmp/project");
}

#[gpui::test]
fn submitting_checks_the_launch_first(cx: &mut TestAppContext) {
    let entity = quick_launch(cx);
    entity.update(cx, |quick, cx| {
        quick.attach_window("main", FakeHost::new(), cx)
    });
    let submit = |request: QuickLaunchRequest, cx: &mut TestAppContext| {
        entity.update(cx, |quick, cx| quick.submit(request, cx))
    };
    let request =
        |prompt: &str, cwd: &str| QuickLaunchRequest::new(prompt, cwd, HarnessId::Codex, false);
    assert_eq!(
        submit(request("  ", "/tmp/project"), cx),
        Err("Write a prompt first.".into())
    );
    assert_eq!(
        submit(request(&"x".repeat(256 * 1024 + 1), "/tmp/project"), cx),
        Err("That prompt is too long for the quick composer.".into())
    );
    assert_eq!(
        submit(request("hi", " "), cx),
        Err("Pick a project first.".into())
    );
    let mut based = request("hi", "/tmp/project");
    based.worktree_base = Some("main".into());
    assert_eq!(
        submit(based, cx),
        Err("Select a valid workspace for this session.".into())
    );
    let mut gone = request("hi", "/tmp/project");
    gone.worktree_cwd = Some("/definitely/not/a/worktree".into());
    assert_eq!(
        submit(gone, cx),
        Err("This worktree is no longer available. Select another working copy.".into())
    );
    let mut attached = request("hi", "/tmp/project");
    attached.attachments = Some(vec![Attachment {
        path: Some("/definitely/missing.png".into()),
        ..screenshot()
    }]);
    assert_eq!(
        submit(attached, cx),
        Err("Could not read attachment: Screenshot.png".into())
    );
}

#[gpui::test]
fn a_closed_window_hands_its_launch_to_the_next_window_that_asks(cx: &mut TestAppContext) {
    let entity = quick_launch(cx);
    let refusing = FakeHost::new();
    refusing.on_submit(|_, _, _| SubmissionAcceptance::Ready(false));
    entity.update(cx, |quick, cx| {
        quick.attach_window("main", refusing.clone(), cx)
    });
    let id = entity
        .update(cx, |quick, cx| {
            quick.submit(
                QuickLaunchRequest::new("hello", "/tmp/project", HarnessId::Codex, false),
                cx,
            )
        })
        .unwrap();
    cx.run_until_parked();
    entity.read_with(cx, |quick, _| {
        assert_eq!(quick.queue().owner_of(&id), Some("main"));
    });
    entity.update(cx, |quick, _| quick.detach_window("main"));
    let next = FakeHost::new();
    entity.update(cx, |quick, cx| {
        quick.attach_window("window-2", next.clone(), cx)
    });
    cx.run_until_parked();
    assert_eq!(next.submits.borrow().len(), 1);
    entity.read_with(cx, |quick, _| assert!(quick.queue().is_empty()));
    assert_eq!(open(cx, &id).unwrap().quick_launch_accepted, Some(true));
    // The refusing window's retry timer stopped with it.
    advance(cx, 60_000);
    assert_eq!(refusing.submits.borrow().len(), 1);
}

#[gpui::test]
fn with_no_window_the_app_must_open_one(cx: &mut TestAppContext) {
    let entity = quick_launch(cx);
    let result = entity.update(cx, |quick, cx| {
        quick.submit(
            QuickLaunchRequest::new("hello", "/tmp/project", HarnessId::Codex, false),
            cx,
        )
    });
    assert_eq!(result, Err("No workspace window is open.".into()));
    entity.read_with(cx, |quick, _| assert!(quick.queue().is_empty()));
    let _unused: HashSet<String> = HashSet::new();
}
