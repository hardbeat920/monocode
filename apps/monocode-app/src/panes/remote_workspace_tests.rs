//! Remote draft controls through the native workspace and a controlled host.

use super::*;
use gpui::FocusHandle;
use monocode_app::boot::{AppServices, BootOptions, boot};
use monocode_app::data_dir::{DataDir, DataDirSource};
use monocode_core::session::WorkspaceMode;
use monocode_engine::projects::{ProjectsConfig, ProjectsGlobal, testing::FakeBackend};
use monocode_engine::remote::remote_projects::remember_remote_project;
use monocode_engine::remote::testing::{FakeTransport, Reply, machine};
use monocode_engine::remote::{RemoteConfig, RemoteGlobal, RemoteTab, RemoteTurnOptions};
use monocode_engine::workspace::WorkspaceConfig;
use monocode_remote::host::protocol::HostProject;
use monocode_view_workbench::panes::workspace_picker::{
    ToggleWorkspaceMode, WorkspacePickerEvent, WorktreeEntry,
};
use serde_json::json;
use std::sync::Arc;

struct RemoteKeyboardRoot {
    area: Entity<WorkspaceArea>,
    pane: Entity<RemotePane>,
    focus: FocusHandle,
}

impl Render for RemoteKeyboardRoot {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .id("remote-workspace-keyboard-test")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &ToggleWorkspaceMode, window, cx| {
                this.area
                    .update(cx, |area, cx| area.toggle_workspace_mode(window, cx));
            }))
            .child(self.pane.clone())
    }
}

#[gpui::test]
fn remote_workspace_shortcut_preserves_selection_and_creates_the_host_worktree_before_sending(
    cx: &mut gpui::TestAppContext,
) {
    let directory = tempfile::tempdir().unwrap();
    let transport = FakeTransport::new();
    transport.set_machines(vec![machine("host", "env")]);
    transport.set_handler(|_, method, params| {
        Some(match method {
            "environment.describe" => Reply::Value(json!({
                "protocolVersion": 1, "environmentId": "env", "name": "Home",
                "providers": ["codex"], "capabilities": ["changes.wait"],
                "hostVersion": "0.6.0"
            })),
            "models.list" => Reply::Value(json!({"models": {
                "codex": [{"id":"codex:host-model", "harness":"codex", "name":"Host model"}]
            }, "errors": {}})),
            "sessions.list" => Reply::Value(json!([])),
            "workspace.run" => Reply::Value(match params["command"].as_str() {
                Some("git_branches") => json!({"current":"main", "detached":false, "branches": [
                    {"name":"main", "current":true, "remote":null},
                    {"name":"topic", "current":false, "remote":null}
                ]}),
                Some("git_worktrees") => json!({"defaultRoot":"/trees", "worktrees":[
                    {"path":"/repo", "branch":"main", "head":"main", "isMain":true, "missing":false},
                    {"path":"/trees/existing", "branch":"topic", "head":"topic", "isMain":false, "missing":false}
                ]}),
                _ => json!({}),
            }),
            "commands.dispatch" => Reply::Value(json!({
                "commandId":params["commandId"], "sessionId":"host-session", "revision":1
            })),
            "sessions.sync" => Reply::Value(json!({"kind":"snapshot", "value": {
                "projectId":"project", "revision":1, "status":"idle", "updatedAt":1,
                "session":{"id":"host-session", "harness":"codex", "model":"codex:host-model",
                    "modelSettings":{}, "runtimeMode":"supervised", "title":"Host work",
                    "cwd":"/trees/created", "blocks":[]}
            }})),
            _ => Reply::Hold,
        })
    });
    let local = FakeBackend::new();
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
        monocode_view_workbench::panes::workspace_picker::init(cx);
        // Boot only this disposable profile. No schedules, control server,
        // settings import, orphan cleanup, or provider turn runs here.
        boot(
            BootOptions {
                data_dir: DataDir {
                    path: directory.path().to_path_buf(),
                    source: DataDirSource::Flag,
                },
                import_webkit: false,
                sounds: false,
                reap_orphans: false,
                run_schedules: false,
                control_server: false,
            },
            cx,
        )
        .unwrap();
        let kv = AppServices::global(cx).kv.clone();
        remember_remote_project(
            &kv,
            "env",
            &HostProject {
                id: "project".into(),
                cwd: "/repo".into(),
                name: "repo".into(),
            },
        );
        RemoteGlobal::init(
            RemoteConfig {
                transport: Arc::new(transport.clone()),
                kv: kv.clone(),
                clock: Arc::new(|| 1_000),
            },
            cx,
        );
        ProjectsGlobal::init(
            ProjectsConfig {
                kv,
                backend: Arc::new(monocode_app::bridge::projects::AppProjectsBackend::new(
                    local.clone(),
                    RemoteGlobal::global(cx).client.clone(),
                )),
                clock: Arc::new(|| 1_000),
            },
            cx,
        );
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.insert(
                monocode_core::Session::blank(
                    "tab",
                    monocode_core::HarnessId::Codex,
                    "codex:host-model",
                    "remote://env/repo",
                ),
                cx,
            )
        });
    });
    cx.run_until_parked();
    let (root, cx) = cx.add_window_view(|window, cx| {
        let tab = monocode_layout::new_tab("tab");
        let workspace = cx.new(|cx| {
            Workspace::new(
                WorkspaceConfig {
                    active_tab_id: tab.id.clone(),
                    tabs: vec![tab],
                    autosave: false,
                    ..WorkspaceConfig::fresh(Some("remote://env/repo"))
                },
                cx,
            )
        });
        let area = cx.new(|_| {
            let mut area = WorkspaceArea::new();
            area.workspace = Some(workspace);
            area
        });
        let pane = area.update(cx, |area, cx| area.active_session_pane(window, cx));
        let Some(SessionView::Remote(pane)) = pane else {
            panic!("the workspace must mount a remote pane");
        };
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        RemoteKeyboardRoot { area, pane, focus }
    });
    cx.run_until_parked();
    let remote = cx.update(|_, cx| {
        let shell = Engine::sessions(cx).read(cx).get("tab").unwrap().clone();
        let RemoteTab::Connected(remote) =
            RemoteGlobal::sessions(cx).update(cx, |sessions, cx| sessions.open(&shell, true, cx))
        else {
            panic!("the paired host must be available");
        };
        remote
    });
    let picker = root.read_with(cx, |root, cx| {
        root.pane
            .read(cx)
            .toolbar()
            .read(cx)
            .workspace_picker()
            .unwrap()
            .clone()
    });
    cx.update(|window, cx| {
        window.draw(cx).clear();
        assert_eq!(picker.read(cx).props().mode, WorkspaceMode::Current);
        assert_eq!(picker.read(cx).props().base.as_deref(), Some("HEAD"));
    });
    assert!(cx.debug_bounds("workspace-mode-trigger").is_some());
    let shortcut = if cfg!(target_os = "macos") {
        "cmd-shift-g"
    } else {
        "ctrl-shift-g"
    };
    cx.simulate_keystrokes(shortcut);
    cx.run_until_parked();
    remote.read_with(cx, |remote, cx| {
        assert_eq!(
            remote.session(cx).workspace_mode,
            Some(WorkspaceMode::Worktree)
        );
    });
    cx.update(|_, cx| {
        picker.update(cx, |_, cx| {
            cx.emit(WorkspacePickerEvent::BaseChange("topic".into()));
            cx.emit(WorkspacePickerEvent::SelectWorktree(WorktreeEntry {
                path: "remote://env/trees/existing".into(),
                branch: Some("topic".into()),
                head: "topic".into(),
                is_main: false,
                missing: false,
            }));
        });
    });
    cx.run_until_parked();
    remote.read_with(cx, |remote, cx| {
        assert_eq!(remote.execution_cwd(), "/trees/existing");
        assert_eq!(remote.session(cx).worktree_base.as_deref(), Some("topic"));
    });
    // Toggling back and forth keeps the selected checkout and base.
    cx.simulate_keystrokes(shortcut);
    cx.run_until_parked();
    remote.read_with(cx, |remote, cx| {
        assert_eq!(
            remote.session(cx).workspace_mode,
            Some(WorkspaceMode::Current)
        );
    });
    cx.simulate_keystrokes(shortcut);
    cx.run_until_parked();
    transport.hold("git.worktreeCreate");
    cx.update(|_, cx| {
        assert!(RemoteGlobal::submit(
            "tab",
            "Build in the host worktree",
            &[],
            &RemoteTurnOptions::default(),
            cx
        ));
    });
    cx.run_until_parked();
    let creating = transport.calls_for("git.worktreeCreate");
    assert_eq!(creating.len(), 1);
    assert_eq!(creating[0]["projectId"], "project");
    assert_eq!(creating[0]["cwd"], "/trees/existing");
    assert_eq!(creating[0]["base"], "topic");
    assert_eq!(creating[0]["existing"], false);
    assert!(creating[0]["branch"].as_str().unwrap().starts_with("mc/"));
    assert!(transport.calls_for("commands.dispatch").is_empty());
    assert!(transport.release("git.worktreeCreate", Ok(json!({
        "path":"/trees/created", "branch":"mc/fixture", "head":"topic", "isMain":false, "missing":false,
    }))));
    cx.run_until_parked();
    let calls = transport.calls();
    let relevant: Vec<_> = calls
        .iter()
        .filter(|(_, method, _)| method == "git.worktreeCreate" || method == "commands.dispatch")
        .collect();
    assert_eq!(relevant.len(), 3);
    assert_eq!(relevant[0].1, "git.worktreeCreate");
    assert_eq!(relevant[1].2["type"], "create");
    assert_eq!(relevant[1].2["worktreeCwd"], "/trees/created");
    assert_eq!(relevant[1].2["autoWorktreeBranch"], "mc/fixture");
    assert_eq!(relevant[2].2["type"], "send");
    assert_eq!(relevant[2].2["sessionId"], "host-session");
    assert_eq!(relevant[2].2["text"], "Build in the host worktree");
    assert_eq!(relevant[1].2["runtimeMode"], "supervised");
    assert!(
        local.commands().is_empty(),
        "remote controls must not run local Git"
    );
    // A bound host session keeps its checkout fixed even before its first
    // transcript snapshot contains a user block.
    assert!(cx.debug_bounds("workspace-mode-trigger").is_none());
    cx.simulate_keystrokes(shortcut);
    cx.run_until_parked();
    assert_eq!(transport.calls_for("git.worktreeCreate").len(), 1);
    remote.read_with(cx, |remote, cx| {
        assert!(remote.started());
        assert_eq!(remote.execution_cwd(), "/trees/created");
        assert_eq!(remote.session(cx).workspace_mode, None);
    });
}
