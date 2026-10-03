//! Renders the Connect views to PNGs, offscreen.
//!
//! ```sh
//! cargo run -p monocode-view-remote --example remote_gallery -- target/remote-gallery [scene]
//! ```
//!
//! Scenes: connections-paired, connections-ssh-password, connections-ssh-trust,
//! connections-pair-link, connections-remove, connections-empty,
//! remote-session, remote-session-new, remote-session-offline,
//! add-remote-project, and light variants of a few. Each scene opens a
//! headless window (it never takes focus) with the platform text system and
//! the headless renderer, waits for images and animations to settle, and
//! writes `<scene>.png` through `Window::render_to_image`.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyView, App, AppContext as _, Context, HeadlessAppContext, IntoElement, ParentElement as _,
    Render, Styled as _, Task, Window, WindowHandle, div, px, size,
};
use monocode_core::block::ToolPreviewLineKind;
use monocode_core::models::ModelCatalog;
use monocode_core::transcript::fixtures::{
    command, edit_with_diff, note, read, search, thought, timed_user,
};
use monocode_core::{Attachment, HarnessId, Session};
use monocode_remote::host::protocol::{
    HostProject, RemoteMachine, RemoteMachineSsh, SshSetup, SshSetupPrompt,
};
use monocode_ui::{AppearanceSettings, Theme, ThemePreference, u};
use monocode_view_composer::composer::RemoteFeatures;
use monocode_view_composer::pickers::{LocalModelSource, ModelSource};
use monocode_view_remote::connections::{AddMode, Field};
use monocode_view_remote::session::{FailedTurn, RemoteSessionStatus};
use monocode_view_remote::{
    AddRemoteProjectDialog, ConnectionsSettings, HostTask, RemoteHost, RemoteMachineState,
    RemoteSessionHost, RemoteSessionPane, RemoteSessionProps, SshBegin,
};
use serde_json::{Value, json};

/// Machines and answers for every scene.
struct GalleryHost {
    machines: Vec<RemoteMachine>,
    prompt: Option<SshSetupPrompt>,
}

fn studio() -> RemoteMachine {
    RemoteMachine {
        id: "studio".into(),
        name: "Studio".into(),
        endpoint: "10.0.0.2:3774".into(),
        endpoints: Some(vec![
            "https://10.0.0.2:3774".into(),
            "https://100.64.0.9:3774".into(),
        ]),
        environment_id: "env-studio".into(),
        ssh: None,
    }
}

fn mac_mini() -> RemoteMachine {
    RemoteMachine {
        id: "mini".into(),
        name: "Home Mac mini".into(),
        endpoint: "ssh://me@mini".into(),
        endpoints: Some(vec!["https://192.168.1.20:3774".into()]),
        environment_id: "env-mini".into(),
        ssh: Some(RemoteMachineSsh {
            target: "me@mini".into(),
            port: None,
            remote_port: 3774,
        }),
    }
}

fn build_box() -> RemoteMachine {
    RemoteMachine {
        id: "build".into(),
        name: "Build box".into(),
        endpoint: "ssh://ci@build".into(),
        endpoints: None,
        environment_id: "env-build".into(),
        ssh: Some(RemoteMachineSsh {
            target: "ci@build".into(),
            port: Some(2222),
            remote_port: 3774,
        }),
    }
}

impl RemoteHost for GalleryHost {
    fn machines(&self, _: &mut App) -> HostTask<Vec<RemoteMachine>> {
        Task::ready(Ok(self.machines.clone()))
    }

    fn request(
        &self,
        machine_id: &str,
        method: &str,
        params: Value,
        _: &mut App,
    ) -> HostTask<Value> {
        let answer = match (method, machine_id) {
            ("environment.describe", "studio") => Ok(json!({
                "protocolVersion": 1,
                "environmentId": "env-studio",
                "name": "studio",
                "providers": ["codex", "claude", "cursor"],
                "capabilities": ["changes.wait", "attachments.upload", "sessions.plan", "sessions.draft"],
                "hostVersion": "0.6.0",
            })),
            ("environment.describe", "mini") => Ok(json!({
                "protocolVersion": 1,
                "environmentId": "env-mini",
                "name": "mini",
                "providers": ["claude"],
                "capabilities": ["changes.wait"],
                "hostVersion": "0.5.2",
            })),
            ("environment.describe", _) => {
                Err("Machine is unreachable. ci@build did not answer over SSH".to_string())
            }
            ("projects.browse", _) => Ok(json!({
                "path": params.get("path").and_then(Value::as_str).unwrap_or("/home/me/code"),
                "parent": "/home/me",
                "entries": [
                    { "name": "agent-terminal", "path": "/home/me/code/agent-terminal" },
                    { "name": "arcade", "path": "/home/me/code/arcade" },
                    { "name": "dotfiles", "path": "/home/me/code/dotfiles" },
                    { "name": "monocode", "path": "/home/me/code/monocode" },
                    { "name": "website", "path": "/home/me/code/website" },
                ],
            })),
            _ => Ok(json!({})),
        };
        Task::ready(answer)
    }

    fn app_version(&self, _: &mut App) -> HostTask<String> {
        Task::ready(Ok("0.6.0".into()))
    }

    fn ssh_begin(&self, _: SshBegin, _: &mut App) -> HostTask<String> {
        Task::ready(Ok("job".into()))
    }

    fn ssh_poll(&self, job_id: &str, _: &mut App) -> HostTask<SshSetup> {
        Task::ready(Ok(SshSetup {
            id: job_id.into(),
            message: if self.prompt.as_ref().is_some_and(|prompt| prompt.confirm) {
                "Checking the host key…".into()
            } else {
                "Signing in to me@mini over SSH…".into()
            },
            prompt: self.prompt.clone(),
            done: false,
            error: None,
            machine: None,
        }))
    }

    fn remember_project(&self, environment_id: &str, project: &HostProject, _: &mut App) -> String {
        monocode_view_remote::remote_project_key(environment_id, &project.cwd)
    }
}

/// The composer side of a host session.
struct GallerySessionHost;

impl RemoteSessionHost for GallerySessionHost {
    fn submit(
        &self,
        _: String,
        _: Vec<Attachment>,
        _: monocode_core::session::ComposerTurnOptions,
        _: &mut Window,
        _: &mut App,
    ) -> bool {
        true
    }

    fn model_source(&self, _: &mut App) -> Option<Rc<dyn ModelSource>> {
        Some(Rc::new(LocalModelSource::new(
            ModelCatalog::new(),
            monocode_view_composer::pickers::model_source::all_available(),
        )))
    }
}

fn remote_session(busy: bool) -> Session {
    let mut session = Session::blank(
        "shell",
        HarnessId::Codex,
        "codex:gpt-5.5",
        "remote://env-studio/home/me/code/arcade",
    );
    session.title = "Finish screens".into();
    session.branch = Some("main".into());
    let root = "/home/me/code/arcade";
    let mut first = timed_user(
        "u1",
        "Make each arcade game finish with a WIN or LOSE screen.",
        1_700_000_000_000,
        142_000,
    );
    first.turn_model = Some(monocode_core::block::TurnModel {
        harness: HarnessId::Codex,
        id: "codex:gpt-5.5".into(),
        name: "GPT-5.5".into(),
        extra: Default::default(),
    });
    session.blocks = vec![
        first,
        thought(
            "r1",
            "**Finding the game loop**\n\nEach scene ends on a timer today.",
        ),
        note("a1", "I'll find where each game decides it is over."),
        search("s1", "isGameOver"),
        read("t1", &format!("{root}/src/surfaces/gridArcade.ts")),
        edit_with_diff(
            "e1",
            &format!("{root}/src/surfaces/gridArcade.ts"),
            &[
                (
                    ToolPreviewLineKind::Del,
                    1,
                    "export type ArcadeResult = \"win\";",
                ),
                (
                    ToolPreviewLineKind::Add,
                    1,
                    "export type ArcadeResult = \"win\" | \"lose\";",
                ),
            ],
        ),
        command("c1", "npm test -- --run src/surfaces", "completed"),
        note(
            "a2",
            "Each game now ends with a **WIN** or **LOSE** screen on the host, then the attract loop picks the next game.",
        ),
    ];
    if busy {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as i64)
            .unwrap_or_default();
        let mut running = timed_user(
            "u2",
            "Now run the full test suite on the host.",
            now - 42_000,
            0,
        );
        running.duration_ms = None;
        session.blocks.push(running);
        session
            .blocks
            .push(command("c2", "npm test", "in_progress"));
        session.busy = Some(true);
    }
    session
}

fn session_props(session: Session) -> RemoteSessionProps {
    RemoteSessionProps {
        machine_name: "Studio".into(),
        environment_id: "env-studio".into(),
        session: Some(Arc::new(session)),
        execution_cwd: "/home/me/code/arcade".into(),
        online: true,
        features: RemoteFeatures {
            attachments: true,
            plan: true,
            draft: true,
        },
        started: true,
        allowed_model_harnesses: vec![HarnessId::Codex],
        compact_supported: true,
        model_controls_beside: true,
        animate: false,
        ..RemoteSessionProps::default()
    }
}

/// A window body: the scene's view on the app background, inside a frame
/// like the settings page's content column when `padded`.
struct Stage {
    view: AnyView,
    padded: bool,
}

impl Render for Stage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let mut body = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.colors.background_base)
            .text_color(theme.colors.content);
        body = if self.padded {
            body.p(u(32.))
                .child(div().w_full().max_w(u(720.)).child(self.view.clone()))
        } else {
            body.child(self.view.clone())
        };
        body
    }
}

type Build = Box<dyn FnOnce(&mut Window, &mut App) -> AnyView>;
type After = Box<dyn FnOnce(&AnyView, &mut Window, &mut App)>;

struct Scene {
    name: &'static str,
    width: f32,
    height: f32,
    light: bool,
    padded: bool,
    build: Build,
    after: Option<After>,
}

fn connections(machines: Vec<RemoteMachine>, prompt: Option<SshSetupPrompt>) -> Build {
    Box::new(move |window, cx| {
        let host = Rc::new(GalleryHost { machines, prompt });
        cx.new(|cx| ConnectionsSettings::new(host, window, cx))
            .into()
    })
}

fn with_settings(
    f: impl FnOnce(&mut ConnectionsSettings, &mut Window, &mut Context<ConnectionsSettings>) + 'static,
) -> After {
    Box::new(move |view, window, cx| {
        let view = view
            .clone()
            .downcast::<ConnectionsSettings>()
            .expect("settings view");
        view.update(cx, |view, cx| f(view, window, cx));
    })
}

fn base_scenes() -> Vec<Scene> {
    let paired = || vec![studio(), mac_mini(), build_box()];
    let password = SshSetupPrompt {
        id: "password".into(),
        message: "me@mini's password:".into(),
        confirm: false,
    };
    let trust = SshSetupPrompt {
        id: "trust".into(),
        message: "The authenticity of host 'mini (192.168.1.20)' can't be established.\nED25519 key fingerprint is SHA256:Qm9vdGgtdGVzdC1maW5nZXJwcmludC1leGFtcGxl.\nThis key is not known by any other names."
            .into(),
        confirm: true,
    };
    let ssh_setup = |view: &mut ConnectionsSettings,
                     window: &mut Window,
                     cx: &mut Context<ConnectionsSettings>| {
        view.start_adding(cx);
        view.set_mode(AddMode::Ssh, cx);
        view.set_value(Field::Target, "me@mini", window, cx);
        view.set_value(Field::Name, "Home Mac mini", window, cx);
        view.begin(None, false, cx);
    };
    let list = vec![
        Scene {
            name: "connections-paired",
            width: 800.,
            height: 640.,
            light: false,
            padded: true,
            build: connections(paired(), None),
            after: None,
        },
        Scene {
            name: "connections-ssh-password",
            width: 800.,
            height: 1100.,
            light: false,
            padded: true,
            build: connections(vec![studio()], Some(password.clone())),
            after: Some(with_settings(ssh_setup)),
        },
        Scene {
            name: "connections-ssh-trust",
            width: 800.,
            height: 1100.,
            light: false,
            padded: true,
            build: connections(vec![studio()], Some(trust)),
            after: Some(with_settings(ssh_setup)),
        },
        Scene {
            name: "connections-pair-link",
            width: 800.,
            height: 960.,
            light: false,
            padded: true,
            build: connections(vec![studio()], None),
            after: Some(with_settings(|view, window, cx| {
                view.start_adding(cx);
                view.set_value(
                    Field::Link,
                    "monocode://pair?v=1&name=studio&id=env-studio",
                    window,
                    cx,
                );
            })),
        },
        Scene {
            name: "connections-remove",
            width: 800.,
            height: 760.,
            light: false,
            padded: true,
            build: connections(vec![studio(), mac_mini()], None),
            after: Some(Box::new(|view, window, cx| {
                // Clicking the trash button opens the confirmation; do it
                // through the same path a click takes.
                let view = view
                    .clone()
                    .downcast::<ConnectionsSettings>()
                    .expect("settings view");
                view.update(cx, |view, cx| view.confirm_remove("mini", cx));
                window.refresh();
            })),
        },
        Scene {
            name: "connections-empty",
            width: 800.,
            height: 360.,
            light: false,
            padded: true,
            build: connections(Vec::new(), None),
            after: None,
        },
        Scene {
            name: "remote-session",
            width: 1000.,
            height: 760.,
            light: false,
            padded: false,
            build: Box::new(|window, cx| {
                let mut props = session_props(remote_session(true));
                props.status = RemoteSessionStatus {
                    pending: true,
                    error: "Machine is unreachable. 10.0.0.2:3774 did not answer".into(),
                    ..RemoteSessionStatus::default()
                };
                props.online = false;
                cx.new(|cx| {
                    RemoteSessionPane::new(Rc::new(GallerySessionHost), props, None, window, cx)
                })
                .into()
            }),
            after: None,
        },
        Scene {
            name: "remote-session-idle",
            width: 1000.,
            height: 760.,
            light: false,
            padded: false,
            build: Box::new(|window, cx| {
                let props = session_props(remote_session(false));
                cx.new(|cx| {
                    RemoteSessionPane::new(Rc::new(GallerySessionHost), props, None, window, cx)
                })
                .into()
            }),
            after: None,
        },
        Scene {
            name: "remote-session-new",
            width: 1000.,
            height: 640.,
            light: false,
            padded: false,
            build: Box::new(|window, cx| {
                let mut session = remote_session(false);
                session.blocks.clear();
                let mut props = session_props(session);
                props.started = false;
                props.allowed_model_harnesses =
                    vec![HarnessId::Codex, HarnessId::Claude, HarnessId::Cursor];
                props.status = RemoteSessionStatus {
                    failed_turn: Some(FailedTurn { draft: false }),
                    error: "Host rejected request: the project folder no longer exists".into(),
                    ..RemoteSessionStatus::default()
                };
                cx.new(|cx| {
                    RemoteSessionPane::new(Rc::new(GallerySessionHost), props, None, window, cx)
                })
                .into()
            }),
            after: None,
        },
        Scene {
            name: "remote-session-offline",
            width: 1000.,
            height: 480.,
            light: false,
            padded: false,
            build: Box::new(|window, cx| {
                let mut props = session_props(remote_session(false));
                props.machine = RemoteMachineState::NotConnected;
                cx.new(|cx| {
                    RemoteSessionPane::new(Rc::new(GallerySessionHost), props, None, window, cx)
                })
                .into()
            }),
            after: None,
        },
        Scene {
            name: "add-remote-project",
            width: 900.,
            height: 640.,
            light: false,
            padded: false,
            build: Box::new(|window, cx| {
                let host = Rc::new(GalleryHost {
                    machines: vec![studio()],
                    prompt: None,
                });
                cx.new(|cx| AddRemoteProjectDialog::new(host, window, cx))
                    .into()
            }),
            after: None,
        },
        Scene {
            name: "add-remote-project-machines",
            width: 900.,
            height: 640.,
            light: false,
            padded: false,
            build: Box::new(|window, cx| {
                let host = Rc::new(GalleryHost {
                    machines: vec![studio(), mac_mini()],
                    prompt: None,
                });
                cx.new(|cx| AddRemoteProjectDialog::new(host, window, cx))
                    .into()
            }),
            after: None,
        },
    ];
    list
}

/// Every scene, plus light copies of a few.
fn scenes() -> Vec<Scene> {
    let mut list = base_scenes();
    for (name, base) in [
        ("connections-paired-light", "connections-paired"),
        ("remote-session-light", "remote-session-idle"),
        ("add-remote-project-light", "add-remote-project"),
    ] {
        let mut extra = base_scenes()
            .into_iter()
            .find(|scene| scene.name == base)
            .expect("scene");
        extra.name = name;
        extra.light = true;
        list.push(extra);
    }
    list
}

fn capture(cx: &mut HeadlessAppContext, scene: Scene, out: &Path) {
    let appearance = AppearanceSettings {
        theme_preference: if scene.light {
            ThemePreference::Light
        } else {
            ThemePreference::Dark
        },
        ..Default::default()
    };
    cx.update(|cx| monocode_ui::set_appearance(appearance, cx));
    let build = scene.build;
    let padded = scene.padded;
    let window: WindowHandle<Stage> = cx
        .open_window(
            size(px(scene.width), px(scene.height)),
            move |window, cx| {
                monocode_ui::sync_window(window, cx);
                let view = build(window, cx);
                cx.new(|_| Stage { view, padded })
            },
        )
        .expect("open window");
    let draw = |cx: &mut HeadlessAppContext| {
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear();
        })
        .expect("draw");
        cx.run_until_parked();
    };
    for _ in 0..3 {
        draw(cx);
    }
    if let Some(after) = scene.after {
        cx.update_window(window.into(), |root, window, cx| {
            let stage = root.downcast::<Stage>().expect("stage");
            let view = stage.read(cx).view.clone();
            after(&view, window, cx);
        })
        .expect("after");
    }
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(900) {
        draw(cx);
        std::thread::sleep(Duration::from_millis(16));
    }
    draw(cx);
    let image = cx.capture_screenshot(window.into()).expect("capture");
    let path = out.join(format!("{}.png", scene.name));
    image.save(&path).expect("save png");
    eprintln!(
        "wrote {} ({}x{})",
        path.display(),
        image.width(),
        image.height()
    );
    cx.update_window(window.into(), |_, window, _| window.remove_window())
        .ok();
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/remote-gallery"));
    let only = std::env::args().nth(2);
    std::fs::create_dir_all(&out).expect("create output dir");
    let platform = gpui_platform::current_platform(true);
    let mut cx = HeadlessAppContext::with_platform(
        platform.text_system(),
        Arc::new(monocode_ui::Assets),
        gpui_platform::current_headless_renderer,
    );
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(AppearanceSettings::default(), cx);
        monocode_view_transcript::transcript::init(cx);
        monocode_view_composer::composer::init(cx);
        monocode_view_composer::pickers::init(cx);
    });
    for scene in scenes() {
        if only.as_deref().is_some_and(|only| only != scene.name) {
            continue;
        }
        capture(&mut cx, scene, &out);
    }
}
