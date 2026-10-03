//! Behavior tests for the composer, ported from Composer.test.ts,
//! Composer.paste.test.ts, Composer.pasteImage.test.ts,
//! ComposerFileDrop.test.ts, and Composer.context.test.ts. A recording host
//! stands in for the engine.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui::{
    App, AppContext as _, ClipboardEntry, ClipboardItem, Context, Entity, ExternalPaths,
    Focusable as _, Image, ImageFormat, IntoElement, ParentElement as _, Render, Styled as _, Task,
    TestAppContext, VisualTestContext, Window, div, px,
};
use monocode_core::block::TurnIntent;
use monocode_core::session::EditedResendRejection;
use monocode_core::{Attachment, AttachmentKind, HarnessId};
use monocode_ui::AppearanceSettings;

use super::super::host::{
    ComposerHost, ComposerSubmission, FolderTarget, McpServers, SessionFolder, SkillContext,
};
use super::super::model::chat_context::ChatContextItem;
use super::super::model::clipboard::{ClipboardFile, CopiedFile, CopiedMessage};
use super::super::model::mcp::McpConnection;
use super::super::model::mentions::{ProjectFile, RankedFile};
use super::super::model::skills::Skill;
use super::{Composer, ComposerEvent, ComposerProps, LastTurnRecall};

#[derive(Default)]
struct Calls {
    submits: Vec<ComposerSubmission>,
    drafts: Vec<String>,
    btw: Vec<(String, bool)>,
    saved: Vec<(String, Vec<Attachment>)>,
    compacts: usize,
    stops: usize,
    folders: Vec<FolderTarget>,
    revoked: Vec<String>,
    paths: Vec<Vec<String>>,
    files: Vec<Vec<ClipboardFile>>,
}

struct TestHost {
    calls: Rc<RefCell<Calls>>,
    accept: Rc<Cell<bool>>,
    btw_accept: bool,
    compact_ok: bool,
}

impl TestHost {
    fn new() -> (Rc<Self>, Rc<RefCell<Calls>>, Rc<Cell<bool>>) {
        let calls = Rc::new(RefCell::new(Calls::default()));
        let accept = Rc::new(Cell::new(true));
        (
            Rc::new(Self {
                calls: calls.clone(),
                accept: accept.clone(),
                btw_accept: true,
                compact_ok: true,
            }),
            calls,
            accept,
        )
    }
}

fn file_attachment(path: &str) -> Attachment {
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    let mime = monocode_core::attachment::mime_from_name(&name);
    Attachment {
        id: path.into(),
        kind: monocode_core::attachment::kind_from_mime(&mime),
        mime_type: mime,
        name,
        path: Some(path.into()),
        ..Attachment::default()
    }
}

impl ComposerHost for TestHost {
    fn submit(&self, submission: ComposerSubmission, _: &mut Window, _: &mut App) -> bool {
        self.calls.borrow_mut().submits.push(submission);
        self.accept.get()
    }

    fn stop(&self, _: &mut Window, _: &mut App) {
        self.calls.borrow_mut().stops += 1;
    }

    fn save_draft(
        &self,
        text: String,
        attachments: Vec<Attachment>,
        _: &mut Window,
        _: &mut App,
    ) -> bool {
        self.calls.borrow_mut().saved.push((text, attachments));
        true
    }

    fn btw(&self, text: String, draft: bool, _: &mut Window, _: &mut App) -> bool {
        self.calls.borrow_mut().btw.push((text, draft));
        self.btw_accept
    }

    fn compact_context(&self, _: &mut Window, _: &mut App) -> bool {
        self.calls.borrow_mut().compacts += 1;
        self.compact_ok
    }

    fn place_in_folder(&self, target: FolderTarget, _: &mut Window, _: &mut App) {
        self.calls.borrow_mut().folders.push(target);
    }

    fn session_folders(&self, _: &str, _: &mut App) -> Vec<SessionFolder> {
        vec![SessionFolder {
            id: "f1".into(),
            name: "Arcade".into(),
            session_count: 2,
        }]
    }

    fn draft_changed(&self, text: &str, _: &mut App) {
        self.calls.borrow_mut().drafts.push(text.to_string());
    }

    fn attachments_from_paths(&self, paths: Vec<String>, _: &mut App) -> Task<Vec<Attachment>> {
        self.calls.borrow_mut().paths.push(paths.clone());
        Task::ready(
            paths
                .iter()
                .filter(|path| !path.contains("missing"))
                .map(|path| file_attachment(path))
                .collect(),
        )
    }

    fn attachments_from_files(
        &self,
        files: Vec<ClipboardFile>,
        _: &mut App,
    ) -> Task<Vec<Attachment>> {
        self.calls.borrow_mut().files.push(files.clone());
        Task::ready(
            files
                .into_iter()
                .enumerate()
                .map(|(index, file)| Attachment {
                    id: format!("file-{index}-{}", file.name),
                    kind: monocode_core::attachment::kind_from_mime(&file.mime_type),
                    mime_type: file.mime_type,
                    name: file.name,
                    size: file.bytes.len() as i64,
                    ..Attachment::default()
                })
                .collect(),
        )
    }

    fn pick_attachments(&self, _: &mut Window, _: &mut App) -> Task<Vec<Attachment>> {
        Task::ready(vec![file_attachment("/repo/picked.txt")])
    }

    fn revoke_attachment(&self, attachment: &Attachment, _: &mut App) {
        self.calls.borrow_mut().revoked.push(attachment.id.clone());
    }

    fn skills(&self, _: &SkillContext, _: &mut App) -> Vec<Skill> {
        vec![Skill::file(
            "review-pr",
            "Review pull requests against team standards.",
            "/repo/.agents/skills/review-pr/SKILL.md",
            "project",
            "agents",
        )]
    }

    fn mention_files(&self, _: &str, _: &mut App) -> Vec<ProjectFile> {
        vec![ProjectFile::new(
            "App.tsx",
            "/repo/src/App.tsx",
            "src/App.tsx",
        )]
    }

    fn rank_mentions(&self, cwd: &str, query: &str, cx: &mut App) -> Vec<RankedFile> {
        self.mention_files(cwd, cx)
            .into_iter()
            .filter(|file| file.relative.to_lowercase().contains(&query.to_lowercase()))
            .map(|file| RankedFile {
                file,
                ..RankedFile::default()
            })
            .collect()
    }

    fn mcp_servers(&self, _: &str, _: HarnessId, _: &mut App) -> McpServers {
        McpServers {
            servers: vec![McpConnection {
                provider: "claude".into(),
                name: "docs".into(),
                scope: "project".into(),
                config_path: "/repo/.mcp.json".into(),
                transport: "stdio".into(),
                enabled: None,
            }],
            ..McpServers::default()
        }
    }
}

struct Harness {
    composer: Entity<Composer>,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_end()
            .child(div().w(px(640.)).child(self.composer.clone()))
    }
}

fn props() -> ComposerProps {
    ComposerProps {
        focused: true,
        harness: HarnessId::Claude,
        model: "claude:sonnet-5".into(),
        cwd: "/Users/me/repo".into(),
        execution_cwd: "/Users/me/repo".into(),
        session_id: Some("s1".into()),
        hide_project_picker: true,
        hide_branch_picker: true,
        animate: false,
        runner_enabled: false,
        ..ComposerProps::default()
    }
}

struct Fixture<'a> {
    composer: Entity<Composer>,
    cx: &'a mut VisualTestContext,
}

fn mount<'a>(
    cx: &'a mut TestAppContext,
    host: Rc<TestHost>,
    props: ComposerProps,
    initial: Option<&str>,
) -> Fixture<'a> {
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(AppearanceSettings::default(), cx);
        super::super::init(cx);
        crate::pickers::init(cx);
    });
    let initial = initial.map(str::to_string);
    let (harness, cx) = cx.add_window_view(|window, cx| {
        let host: Rc<dyn ComposerHost> = host;
        let composer = cx.new(|cx| Composer::new(host, props, initial, window, cx));
        Harness { composer }
    });
    let composer = cx.update(|_, cx| harness.read(cx).composer.clone());
    cx.update(|window, cx| {
        let handle = composer.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    });
    let mut fixture = Fixture { composer, cx };
    fixture.draw();
    fixture
}

impl Fixture<'_> {
    fn draw(&mut self) {
        for _ in 0..2 {
            self.cx.update(|window, cx| {
                window.draw(cx).clear();
            });
            self.cx.run_until_parked();
        }
    }

    fn text(&mut self) -> String {
        self.cx
            .update(|_, cx| self.composer.read(cx).prompt.read(cx).text().to_string())
    }

    fn read<R>(&mut self, f: impl FnOnce(&Composer, &App) -> R) -> R {
        let composer = self.composer.clone();
        self.cx.update(|_, cx| f(composer.read(cx), cx))
    }

    fn update<R>(
        &mut self,
        f: impl FnOnce(&mut Composer, &mut Window, &mut Context<Composer>) -> R,
    ) -> R {
        let composer = self.composer.clone();
        let result = self
            .cx
            .update(|window, cx| composer.update(cx, |composer, cx| f(composer, window, cx)));
        self.draw();
        result
    }

    fn type_text(&mut self, text: &str) {
        self.cx.simulate_input(text);
        self.draw();
    }

    fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.draw();
    }

    fn set_props(&mut self, props: ComposerProps) {
        self.update(|composer, window, cx| composer.set_props(props, window, cx));
    }

    fn events(&mut self) -> Rc<RefCell<Vec<ComposerEvent>>> {
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        let composer = self.composer.clone();
        self.cx.update(|_, cx| {
            cx.subscribe(&composer, move |_, event: &ComposerEvent, _| {
                sink.borrow_mut().push(event.clone());
            })
            .detach();
        });
        events
    }
}

fn paste_keys() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-v"
    } else {
        "ctrl-v"
    }
}

// ComposerAction

#[gpui::test]
fn replaces_stop_with_send_when_typing_during_a_running_turn(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(
        cx,
        host,
        ComposerProps {
            busy: true,
            ..props()
        },
        None,
    );
    assert!(!f.read(|c, _| c.has_value()));
    f.type_text("follow up");
    assert!(f.read(|c, _| c.has_value()));
    f.keys("enter");
    assert_eq!(calls.borrow().submits.len(), 1);
    assert_eq!(calls.borrow().submits[0].text, "follow up");
}

#[gpui::test]
fn keeps_stop_while_busy_when_submitting_follow_up_text_is_disabled(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(
        cx,
        host,
        ComposerProps {
            busy: true,
            allow_busy_submit: false,
            ..props()
        },
        None,
    );
    f.type_text("follow up");
    f.update(|composer, window, cx| {
        let host = composer.host.clone();
        host.stop(window, cx);
    });
    assert_eq!(calls.borrow().stops, 1);
}

// Enter, Shift+Enter, and the slash picker.

#[gpui::test]
fn enter_submits_and_shift_enter_adds_a_line(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.type_text("first");
    f.keys("shift-enter");
    f.type_text("second");
    assert_eq!(f.text(), "first\nsecond");
    f.keys("enter");
    let calls = calls.borrow();
    assert_eq!(calls.submits.len(), 1);
    assert_eq!(calls.submits[0].text, "first\nsecond");
    assert_eq!(calls.submits[0].options.intent, Some(TurnIntent::Default));
    drop(calls);
    assert_eq!(f.text(), "");
}

#[gpui::test]
fn an_empty_draft_does_not_submit(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.keys("enter");
    assert!(calls.borrow().submits.is_empty());
}

#[gpui::test]
fn slash_opens_the_picker_and_tab_picks_the_active_row(cx: &mut TestAppContext) {
    let (host, _, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.type_text("/rev");
    assert!(f.read(|c, _| c.slash.is_some()));
    let first = f.read(|c, _| c.ranked_skills()[0].invocation.clone());
    assert_eq!(first, "review-pr");
    f.keys("tab");
    assert_eq!(f.text(), "/review-pr ");
    assert!(f.read(|c, _| c.slash.is_none()));
}

#[gpui::test]
fn arrows_move_through_the_slash_picker_and_escape_closes_it(cx: &mut TestAppContext) {
    let (host, _, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.type_text("/");
    let len = f.read(|c, _| c.ranked_skills().len());
    assert!(len > 2);
    f.keys("down down");
    assert_eq!(f.read(|c, _| c.skill_active), 2);
    f.keys("up");
    assert_eq!(f.read(|c, _| c.skill_active), 1);
    f.keys("up up");
    assert_eq!(f.read(|c, _| c.skill_active), len - 1);
    f.keys("escape");
    assert!(f.read(|c, _| c.slash.is_none()));
    assert_eq!(f.text(), "/");
}

#[gpui::test]
fn enter_on_a_bare_slash_does_nothing(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.type_text("/zzzz");
    f.keys("enter");
    // No row matches, so Enter closes the picker and sends the text.
    assert_eq!(calls.borrow().submits.len(), 1);
}

#[gpui::test]
fn offers_operator_in_the_slash_picker_and_submits_it_as_a_local_command(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.type_text("/operator");
    let operator = f.read(|c, _| {
        c.ranked_skills()
            .into_iter()
            .find(|skill| skill.invocation == "operator")
            .unwrap()
    });
    f.update(|composer, window, cx| composer.pick_skill(&operator, window, cx));
    assert_eq!(f.text(), "/operator ");
    assert!(f.read(|c, _| c.operator_active()));
    f.type_text("list my notes");
    f.update(|composer, window, cx| composer.submit(window, cx));
    let calls = calls.borrow();
    assert_eq!(calls.submits[0].text, "/operator list my notes");
    assert_eq!(calls.submits[0].options.intent, Some(TurnIntent::Default));
}

#[gpui::test]
fn keeps_plan_in_the_text_beside_its_pill_and_submits_with_the_plan_intent(
    cx: &mut TestAppContext,
) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.type_text("/pla");
    f.keys("enter");
    assert_eq!(f.text(), "/plan ");
    assert!(f.read(|c, _| c.plan_active()));
    f.type_text("sketch the refactor");
    f.keys("enter");
    let calls = calls.borrow();
    assert_eq!(calls.submits[0].text, "sketch the refactor");
    assert_eq!(calls.submits[0].options.intent, Some(TurnIntent::Plan));
}

#[gpui::test]
fn offers_orchestrator_and_submits_with_the_orchestrate_intent(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.type_text("/orch");
    f.keys("enter");
    assert_eq!(f.text(), "/orchestrator ");
    f.type_text("ship the release");
    f.keys("enter");
    f.type_text("/orchestrator fix the build");
    f.update(|composer, window, cx| composer.submit(window, cx));
    let calls = calls.borrow();
    assert_eq!(calls.submits[0].text, "ship the release");
    assert_eq!(
        calls.submits[0].options.intent,
        Some(TurnIntent::Orchestrate)
    );
    assert_eq!(calls.submits[1].text, "fix the build");
    assert_eq!(
        calls.submits[1].options.intent,
        Some(TurnIntent::Orchestrate)
    );
}

#[gpui::test]
fn the_plus_menu_operator_prefixes_the_sent_text(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.update(|composer, window, cx| composer.toggle_mode(super::Mode::Operator, window, cx));
    f.type_text("list sessions");
    f.keys("enter");
    assert_eq!(calls.borrow().submits[0].text, "/operator list sessions");
    // The mode is spent after a send.
    assert!(!f.read(|c, _| c.operator_active()));
}

#[gpui::test]
fn turning_a_mode_off_drops_its_leading_command(cx: &mut TestAppContext) {
    let (host, _, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.type_text("/plan do it");
    assert!(f.read(|c, _| c.plan_active()));
    f.update(|composer, window, cx| composer.clear_mode(super::Mode::Plan, window, cx));
    assert_eq!(f.text(), "do it");
    assert!(!f.read(|c, _| c.plan_active()));
}

// /btw

#[gpui::test]
fn opens_btw_as_soon_as_btw_is_typed_and_hands_over_the_rest(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(
        cx,
        host,
        ComposerProps {
            btw_enabled: true,
            ..props()
        },
        None,
    );
    f.type_text("/btw ");
    assert_eq!(calls.borrow().btw, vec![(String::new(), true)]);
    assert_eq!(f.text(), "");
}

#[gpui::test]
fn leaves_a_typed_btw_alone_when_btw_is_unavailable(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.type_text("/btw ");
    assert!(calls.borrow().btw.is_empty());
    assert_eq!(f.text(), "/btw ");
}

#[gpui::test]
fn keeps_the_draft_when_the_btw_command_is_rejected(cx: &mut TestAppContext) {
    let calls = Rc::new(RefCell::new(Calls::default()));
    let host = Rc::new(TestHost {
        calls: calls.clone(),
        accept: Rc::new(Cell::new(true)),
        btw_accept: false,
        compact_ok: true,
    });
    let mut f = mount(
        cx,
        host,
        ComposerProps {
            btw_enabled: true,
            ..props()
        },
        Some("/btw"),
    );
    f.keys("enter");
    assert_eq!(calls.borrow().btw, vec![(String::new(), false)]);
    assert!(calls.borrow().submits.is_empty());
    assert_eq!(f.text(), "/btw");
}

// Local commands.

#[gpui::test]
fn compact_runs_locally_and_clears_the_draft(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), Some("/compact"));
    f.keys("enter");
    assert_eq!(calls.borrow().compacts, 1);
    assert!(calls.borrow().submits.is_empty());
    assert_eq!(f.text(), "");
}

#[gpui::test]
fn mcp_opens_the_server_picker_and_tags_the_pick(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), Some("/mcp"));
    f.keys("enter");
    assert!(f.read(|c, _| c.mcp_picker_open));
    assert_eq!(f.text(), "");
    let docs = f.read(|c, _| c.mcp_servers.servers[0].clone());
    f.update(|composer, window, cx| composer.pick_mcp_server(&docs, window, cx));
    assert_eq!(f.text(), "@mcp/docs ");
    f.type_text("Find the docs");
    f.keys("enter");
    let text = calls.borrow().submits[0].text.clone();
    assert!(
        text.starts_with("MCP context: Use the configured server \"docs\" (claude)"),
        "{text}"
    );
    assert!(text.ends_with("@mcp/docs Find the docs"));
}

#[gpui::test]
fn removes_mcp_context_when_its_inline_tag_is_deleted(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), Some("/mcp"));
    f.keys("enter");
    let docs = f.read(|c, _| c.mcp_servers.servers[0].clone());
    f.update(|composer, window, cx| composer.pick_mcp_server(&docs, window, cx));
    f.update(|composer, _, cx| composer.set_text("Find the docs", cx));
    assert!(f.read(|c, _| c.selected_mcp().is_empty()));
    f.keys("enter");
    assert_eq!(calls.borrow().submits[0].text, "Find the docs");
}

#[gpui::test]
fn inserts_an_mcp_tag_beside_existing_composer_text(cx: &mut TestAppContext) {
    let (host, _, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.type_text("sad /mcp");
    let mcp = f.read(|c, _| {
        c.ranked_skills()
            .into_iter()
            .find(|skill| skill.invocation == "mcp")
            .unwrap()
    });
    f.update(|composer, window, cx| composer.pick_skill(&mcp, window, cx));
    let docs = f.read(|c, _| c.mcp_servers.servers[0].clone());
    f.update(|composer, window, cx| composer.pick_mcp_server(&docs, window, cx));
    assert_eq!(f.text(), "sad @mcp/docs ");
    let spans = f.read(|c, cx| c.decorations_for("sad @mcp/docs ", cx).spans.len());
    assert_eq!(spans, 1);
}

#[gpui::test]
fn add_to_folder_on_space_opens_the_folder_picker(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(
        cx,
        host,
        ComposerProps {
            folders_enabled: true,
            ..props()
        },
        None,
    );
    f.type_text("/add-to-folder");
    f.keys("escape");
    f.keys("space");
    assert!(f.read(|c, _| c.session_folder_open));
    assert_eq!(f.text(), "/add-to-folder");
    f.update(|composer, window, cx| {
        composer.pick_session_folder(
            FolderTarget::Existing {
                folder_id: "f1".into(),
            },
            window,
            cx,
        )
    });
    assert_eq!(f.text(), "/add-to-folder ");
    assert_eq!(calls.borrow().folders.len(), 1);
    f.type_text("Build the settings screen");
    f.keys("enter");
    assert_eq!(calls.borrow().submits[0].text, "Build the settings screen");
}

// Drafts.

#[gpui::test]
fn saves_a_new_message_as_a_draft_without_submitting_it(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(
        cx,
        host,
        ComposerProps {
            can_save_draft: true,
            ..props()
        },
        None,
    );
    f.update(|composer, window, cx| composer.toggle_mode(super::Mode::Draft, window, cx));
    f.type_text("Remember the arcade idea");
    f.keys("enter");
    let calls = calls.borrow();
    assert!(calls.submits.is_empty());
    assert_eq!(calls.saved[0].0, "Remember the arcade idea");
    drop(calls);
    assert_eq!(f.text(), "");
}

#[gpui::test]
fn saves_a_draft_command_message_as_a_draft(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(
        cx,
        host,
        ComposerProps {
            can_save_draft: true,
            ..props()
        },
        Some("/draft later"),
    );
    f.keys("enter");
    assert_eq!(calls.borrow().saved[0].0, "later");
    assert!(calls.borrow().submits.is_empty());
}

#[gpui::test]
fn clears_the_draft_when_the_reset_token_advances(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let props = ComposerProps {
        draft_reset_token: Some(1),
        ..props()
    };
    let mut f = mount(cx, host, props.clone(), Some("something here..."));
    assert_eq!(f.text(), "something here...");
    f.set_props(ComposerProps {
        draft_reset_token: Some(2),
        ..props
    });
    assert_eq!(f.text(), "");
    assert_eq!(calls.borrow().drafts.last().map(String::as_str), Some(""));
}

#[gpui::test]
fn keeps_drafts_and_blocks_sending_until_a_working_copy_is_selected(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(
        cx,
        host,
        ComposerProps {
            worktree_removed: true,
            ..props()
        },
        Some("Continue this feature"),
    );
    assert!(f.read(|c, _| c.placeholder_text().contains("Select a branch or worktree")));
    f.keys("enter");
    assert!(calls.borrow().submits.is_empty());
    assert_eq!(f.text(), "Continue this feature");
    f.set_props(props());
    f.update(|composer, window, cx| composer.submit(window, cx));
    assert_eq!(calls.borrow().submits[0].text, "Continue this feature");
}

#[gpui::test]
fn places_the_caret_at_the_end_of_an_initial_draft(cx: &mut TestAppContext) {
    let (host, _, _) = TestHost::new();
    let draft = "Comment on src/App.tsx:42\n\n";
    let mut f = mount(cx, host, props(), Some(draft));
    assert_eq!(f.text(), draft);
    let selection = f.read(|c, cx| c.prompt.read(cx).selection());
    assert_eq!(selection, draft.len()..draft.len());
}

#[gpui::test]
fn clears_the_parent_draft_before_submit(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), Some("Ship the empty-state fix"));
    calls.borrow_mut().drafts.clear();
    f.update(|composer, window, cx| composer.submit(window, cx));
    let calls = calls.borrow();
    assert_eq!(calls.submits[0].text, "Ship the empty-state fix");
    assert_eq!(calls.drafts.first().map(String::as_str), Some(""));
}

#[gpui::test]
fn restores_the_draft_when_submit_is_rejected(cx: &mut TestAppContext) {
    let (host, calls, accept) = TestHost::new();
    accept.set(false);
    let mut f = mount(
        cx,
        host,
        props(),
        Some("Blocked while orchestration is paused"),
    );
    f.update(|composer, window, cx| composer.submit(window, cx));
    assert_eq!(f.text(), "Blocked while orchestration is paused");
    assert_eq!(
        calls.borrow().drafts.last().map(String::as_str),
        Some("Blocked while orchestration is paused")
    );
}

// Last-turn recall.

fn recall_props() -> ComposerProps {
    ComposerProps {
        harness: HarnessId::Pi,
        edit_last_turn_supported: true,
        last_turn_recall: Some(LastTurnRecall {
            text: "Original prompt".into(),
            attachments: Vec::new(),
        }),
        ..props()
    }
}

#[gpui::test]
fn up_in_an_empty_composer_recalls_the_last_turn(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, recall_props(), None);
    let events = f.events();
    f.keys("up");
    assert_eq!(f.text(), "Original prompt");
    assert!(f.read(|c, _| c.is_editing_last_turn()));
    assert!(
        events
            .borrow()
            .contains(&ComposerEvent::EditingLastTurnChange(true))
    );
    f.keys("enter");
    let calls = calls.borrow();
    assert_eq!(calls.submits[0].options.resend_edited, Some(true));
    assert!(calls.submits[0].resend.is_some());
}

#[gpui::test]
fn does_not_restore_a_failed_resend_over_newer_composer_text(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, recall_props(), None);
    f.update(|composer, window, cx| composer.recall_last_turn(window, cx));
    f.update(|composer, window, cx| composer.submit(window, cx));
    let ticket = calls.borrow().submits[0].resend.unwrap();
    f.type_text("New prompt");
    f.update(|composer, window, cx| {
        composer.resend_rejected(
            ticket,
            EditedResendRejection {
                provider_rewound: false,
            },
            window,
            cx,
        )
    });
    assert_eq!(f.text(), "New prompt");
}

#[gpui::test]
fn retries_an_already_rewound_prompt_as_a_normal_submission(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(
        cx,
        host,
        ComposerProps {
            last_turn_recall: Some(LastTurnRecall {
                text: "Edited prompt".into(),
                attachments: Vec::new(),
            }),
            ..recall_props()
        },
        None,
    );
    f.update(|composer, window, cx| composer.recall_last_turn(window, cx));
    f.update(|composer, window, cx| composer.submit(window, cx));
    let ticket = calls.borrow().submits[0].resend.unwrap();
    f.update(|composer, window, cx| {
        composer.resend_rejected(
            ticket,
            EditedResendRejection {
                provider_rewound: true,
            },
            window,
            cx,
        )
    });
    assert_eq!(f.text(), "Edited prompt");
    assert!(!f.read(|c, _| c.is_editing_last_turn()));
    f.update(|composer, window, cx| composer.submit(window, cx));
    let calls = calls.borrow();
    assert_eq!(calls.submits.len(), 2);
    assert_eq!(calls.submits[1].options.resend_edited, None);
    assert_eq!(calls.submits[1].options.intent, Some(TurnIntent::Default));
}

#[gpui::test]
fn preserves_attachment_ownership_when_a_resend_is_restored(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let recalled = file_attachment("/repo/owned.png");
    let mut f = mount(
        cx,
        host,
        ComposerProps {
            last_turn_recall: Some(LastTurnRecall {
                text: "Look".into(),
                attachments: vec![recalled.clone()],
            }),
            ..recall_props()
        },
        None,
    );
    f.update(|composer, window, cx| composer.recall_last_turn(window, cx));
    // Cancelling the edit must not revoke files the transcript still owns.
    f.update(|composer, window, cx| composer.exit_edit_mode(window, cx));
    assert!(calls.borrow().revoked.is_empty());
    assert_eq!(f.text(), "");
}

// Context chips.

#[gpui::test]
fn a_trailing_context_block_becomes_chips_and_goes_back_on_send(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let item = ChatContextItem::Quote {
        text: "selected words".into(),
    };
    let draft = super::super::model::chat_context::compose_chat_context(
        "explain",
        std::slice::from_ref(&item),
    );
    let mut f = mount(cx, host, props(), Some(&draft));
    assert_eq!(f.text(), "explain");
    assert_eq!(
        f.read(|c, _| c.context_items().to_vec()),
        vec![item.clone()]
    );
    assert!(f.read(|c, _| c.placeholder_text() == "Add a message, or send…"));
    f.keys("enter");
    assert_eq!(calls.borrow().submits[0].text, draft);
}

#[gpui::test]
fn removing_a_chip_drops_it_from_the_draft(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let item = ChatContextItem::Code {
        path: "src/App.tsx".into(),
        start_line: 3,
        end_line: 9,
    };
    let draft =
        super::super::model::chat_context::compose_chat_context("", std::slice::from_ref(&item));
    let mut f = mount(cx, host, props(), Some(&draft));
    let key = super::super::model::chat_context::chat_context_key(&item);
    f.update(|composer, window, cx| composer.remove_context_item(&key, window, cx));
    assert!(f.read(|c, _| c.context_items().is_empty()));
    assert!(!f.read(|c, _| c.has_value()));
    assert_eq!(calls.borrow().drafts.last().map(String::as_str), Some(""));
}

// `@` mentions.

#[gpui::test]
fn at_opens_the_file_picker_and_enter_inserts_the_label(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.type_text("look at @App");
    assert!(f.read(|c, _| c.mention_open()));
    f.keys("enter");
    assert_eq!(f.text(), "look at @App.tsx ");
    assert!(calls.borrow().submits.is_empty());
    let hidden = f.read(|c, cx| c.decorations_for("look at @App.tsx ", cx).hidden);
    assert_eq!(hidden, vec![8..9]);
}

// Paste.

#[gpui::test]
fn plain_text_pastes_into_the_prompt(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.cx.write_to_clipboard(ClipboardItem::new_string("hello\r\nworld".into()));
    f.keys(paste_keys());
    assert_eq!(f.text(), "hello\nworld");
    assert!(calls.borrow().paths.is_empty());
}

#[gpui::test]
fn pasting_copied_files_attaches_them(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.cx.write_to_clipboard(ClipboardItem {
        entries: vec![ClipboardEntry::ExternalPaths(ExternalPaths(
            vec!["/repo/a.txt".into(), "/repo/b.png".into()].into(),
        ))],
    });
    f.keys(paste_keys());
    assert_eq!(f.text(), "");
    let names: Vec<String> =
        f.read(|c, _| c.attachments().iter().map(|a| a.name.clone()).collect());
    assert_eq!(names, ["a.txt", "b.png"]);
    assert_eq!(calls.borrow().paths.len(), 1);
}

#[gpui::test]
fn reports_copied_paths_that_no_longer_exist(cx: &mut TestAppContext) {
    let (host, _, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.cx.write_to_clipboard(ClipboardItem {
        entries: vec![ClipboardEntry::ExternalPaths(ExternalPaths(
            vec!["/repo/missing.txt".into()].into(),
        ))],
    });
    f.keys(paste_keys());
    let error = f.read(|c, _| c.paste_error().map(str::to_string));
    assert!(
        error
            .unwrap()
            .starts_with("Nothing to attach from that path")
    );
}

#[gpui::test]
fn caps_a_large_copy_at_one_turns_quota_and_warns(cx: &mut TestAppContext) {
    let (host, _, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    let paths: Vec<std::path::PathBuf> =
        (0..25).map(|i| format!("/repo/f{i}.txt").into()).collect();
    f.cx.write_to_clipboard(ClipboardItem {
        entries: vec![ClipboardEntry::ExternalPaths(ExternalPaths(paths.into()))],
    });
    f.keys(paste_keys());
    assert_eq!(f.read(|c, _| c.attachments().len()), 20);
    assert_eq!(
        f.read(|c, _| c.paste_error().map(str::to_string))
            .as_deref(),
        Some("Attached 20 of 25 copied files. A turn carries up to 20.")
    );
}

#[gpui::test]
fn pasting_an_image_attaches_it(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
        ImageFormat::Png,
        vec![137, 80, 78, 71],
    )));
    f.keys(paste_keys());
    assert_eq!(calls.borrow().files[0][0].name, "clipboard-image.png");
    assert_eq!(
        f.read(|c, _| c.attachments()[0].kind),
        AttachmentKind::Image
    );
}

#[gpui::test]
fn a_copied_monocode_message_pastes_its_text_and_files(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    let metadata = CopiedMessage {
        monocode_files: vec![CopiedFile {
            name: "notes.txt".into(),
            mime_type: "text/plain".into(),
            data: "aGk=".into(),
        }],
    };
    f.cx.write_to_clipboard(ClipboardItem::new_string_with_json_metadata(
        "Look at these".into(),
        metadata,
    ));
    f.keys(paste_keys());
    assert_eq!(f.text(), "Look at these");
    assert_eq!(calls.borrow().files[0][0].bytes, b"hi".to_vec());
    assert_eq!(f.read(|c, _| c.attachments().len()), 1);
}

#[gpui::test]
fn harnesses_without_attachments_ignore_pasted_files(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(
        cx,
        host,
        ComposerProps {
            harness: HarnessId::Fx,
            ..props()
        },
        None,
    );
    f.cx.write_to_clipboard(ClipboardItem {
        entries: vec![ClipboardEntry::ExternalPaths(ExternalPaths(
            vec!["/repo/a.txt".into()].into(),
        ))],
    });
    f.keys(paste_keys());
    assert!(calls.borrow().paths.is_empty());
    assert!(f.read(|c, _| c.attachments().is_empty()));
}

#[gpui::test]
fn send_waits_for_a_paste_and_a_reset_drops_a_late_one(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(
        cx,
        host,
        ComposerProps {
            draft_reset_token: Some(1),
            ..props()
        },
        Some("text"),
    );
    // A paste in flight holds Send until it lands.
    f.update(|composer, _, _| composer.pastes_in_flight = 1);
    f.update(|composer, window, cx| composer.submit(window, cx));
    assert!(calls.borrow().submits.is_empty());
    f.update(|composer, window, cx| composer.paste_settled(window, cx));
    assert_eq!(calls.borrow().submits.len(), 1);
    // After a reset, the waiting Send is dropped.
    f.type_text("next");
    f.update(|composer, _, _| composer.pastes_in_flight = 1);
    f.update(|composer, window, cx| composer.submit(window, cx));
    f.set_props(ComposerProps {
        draft_reset_token: Some(2),
        ..props()
    });
    f.update(|composer, window, cx| composer.paste_settled(window, cx));
    assert_eq!(calls.borrow().submits.len(), 1);
}

// Drop.

#[gpui::test]
fn dropping_files_attaches_them_and_clears_the_overlay(cx: &mut TestAppContext) {
    let (host, _, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    // The overlay shows while a drag is active; the test has none, so check
    // it before the next frame clears it.
    let composer = f.composer.clone();
    let shown = f.cx.update(|_, cx| {
        composer.update(cx, |composer, cx| {
            composer.set_file_drag(true, cx);
            composer.file_drag
        })
    });
    assert!(shown);
    f.update(|composer, window, cx| composer.drop_paths(vec!["/repo/drop.pdf".into()], window, cx));
    assert!(!f.read(|c, _| c.file_drag));
    assert_eq!(f.read(|c, _| c.attachments()[0].name.clone()), "drop.pdf");
}

#[gpui::test]
fn removing_an_attachment_revokes_it(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.update(|composer, window, cx| composer.drop_paths(vec!["/repo/drop.pdf".into()], window, cx));
    f.update(|composer, window, cx| composer.remove_attachment("/repo/drop.pdf", window, cx));
    assert!(f.read(|c, _| c.attachments().is_empty()));
    assert_eq!(calls.borrow().revoked, vec!["/repo/drop.pdf".to_string()]);
}

#[gpui::test]
fn an_attachment_only_message_can_be_sent(cx: &mut TestAppContext) {
    let (host, calls, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    f.update(|composer, window, cx| composer.drop_paths(vec!["/repo/drop.pdf".into()], window, cx));
    assert!(f.read(|c, _| c.has_value()));
    f.keys("enter");
    let calls = calls.borrow();
    assert_eq!(calls.submits[0].text, "");
    assert_eq!(calls.submits[0].attachments.len(), 1);
}

// Model and access events.

#[gpui::test]
fn the_access_picker_reports_mode_changes(cx: &mut TestAppContext) {
    let (host, _, _) = TestHost::new();
    let mut f = mount(cx, host, props(), None);
    let events = f.events();
    let access = f.read(|c, _| c.bar.access.clone());
    f.cx.update(|window, cx| {
        access.update(cx, |access, cx| {
            access.pick(monocode_core::RuntimeMode::FullAccess, window, cx)
        })
    });
    f.draw();
    assert!(events.borrow().contains(&ComposerEvent::RuntimeModeChange(
        monocode_core::RuntimeMode::FullAccess
    )));
}
