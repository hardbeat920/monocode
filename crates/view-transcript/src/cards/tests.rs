//! Card behavior in test windows: keys, hover timers, focus, and the events
//! each card reports. Ports of QuestionForm.test.ts, ToolDiffPreview.test.ts,
//! ApprovalToasts.test.ts, UserLinkPreview.test.ts, and the React behavior
//! of TranscriptFind and PromptOutline.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    AppContext as _, Context, Entity, EventEmitter, Focusable as _, IntoElement,
    ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window, div,
};
use monocode_core::block::{ToolPreview, ToolPreviewKind, ToolPreviewLine, ToolPreviewLineKind};
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::transcript::ToolCallState;
use monocode_core::user_question::{
    QuestionAnswers, UserQuestion, UserQuestionOption, UserQuestionPrompt, UserQuestionReply,
};
use monocode_core::{Block, BlockRole, HarnessId};

use super::approval_toasts::{ApprovalNotice, ApprovalToastEvent, ApprovalToasts, NoticeKind};
use super::find_bar::{TranscriptFind, TranscriptFindEvent};
use super::generated_image::{GeneratedImage, GeneratedImageState};
use super::link_preview::{
    LinkPreviewRequest, LinkPreviews, LinkWorkItem, LinkWorkItemDetails, LoadState,
    UserLinkPreview, UserLinkPreviewEvent, WorkItemLabel, WorkItemPerson, work_item_key,
};
use super::markdown_document::MarkdownDocumentPreview;
use super::prompt_outline::{PromptOutline, PromptOutlineEvent};
use super::prompt_outline_model::{OutlineAnchor, OutlineBand};
use super::question_form::{QuestionForm, QuestionFormEvent};
use super::tool_diff::{ToolDiffEvent, ToolDiffPreview};
use crate::transcript::model::link::parse_user_message_link;

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(monocode_ui::AppearanceSettings::default(), cx);
        crate::transcript::init(cx);
    });
}

/// Collects an entity's events.
fn record<T, E>(entity: &Entity<T>, cx: &mut VisualTestContext) -> Rc<RefCell<Vec<E>>>
where
    T: EventEmitter<E> + 'static,
    E: Clone + 'static,
{
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(entity, move |_, event: &E, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach();
    });
    events
}

/// Draw a frame, which delivers focus changes, then settle.
fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.activate_window();
        window.refresh();
        window.draw(cx).clear();
    });
    cx.run_until_parked();
}

fn advance(cx: &mut VisualTestContext, ms: u64) {
    cx.executor().advance_clock(Duration::from_millis(ms));
    cx.run_until_parked();
}

// QuestionForm.test.ts

fn colour_prompt(multi_select: bool) -> UserQuestionPrompt {
    UserQuestionPrompt {
        request_id: 7,
        title: None,
        auto_resolve_at: None,
        questions: vec![UserQuestion {
            id: "colour".into(),
            header: None,
            prompt: "Pick a colour".into(),
            multi_select,
            allow_custom: false,
            options: ["red", "green", "blue"]
                .iter()
                .map(|id| UserQuestionOption {
                    id: id.to_string(),
                    label: id[..1].to_uppercase() + &id[1..],
                    description: None,
                })
                .collect(),
        }],
    }
}

fn question_form(
    multi_select: bool,
    cx: &mut TestAppContext,
) -> (Entity<QuestionForm>, &mut VisualTestContext) {
    init(cx);
    let (form, cx) =
        cx.add_window_view(|window, cx| QuestionForm::new(colour_prompt(multi_select), window, cx));
    cx.run_until_parked();
    // Focus the option group, as tabbing into the form does.
    form.update_in(cx, |form, window, cx| {
        form.focus_handle(cx).focus(window, cx)
    });
    cx.run_until_parked();
    (form, cx)
}

fn selected(form: &Entity<QuestionForm>, cx: &mut VisualTestContext) -> Vec<String> {
    form.read_with(cx, |form, _| form.selected("colour"))
}

#[gpui::test]
fn moves_the_highlighted_option_with_arrow_keys_and_selects_it_with_enter(cx: &mut TestAppContext) {
    let (form, cx) = question_form(false, cx);
    let events = record::<_, QuestionFormEvent>(&form, cx);
    assert_eq!(form.read_with(cx, |form, _| form.highlighted()), 0);
    cx.simulate_keystrokes("down");
    assert_eq!(form.read_with(cx, |form, _| form.highlighted()), 1);
    cx.simulate_keystrokes("enter");
    assert_eq!(selected(&form, cx), ["green"]);
    form.update_in(cx, |form, window, cx| form.continue_current(window, cx));
    let mut answers = QuestionAnswers::new();
    answers.insert("colour".into(), vec!["green".into()]);
    assert_eq!(
        events.borrow().as_slice(),
        [QuestionFormEvent::Reply {
            request_id: 7,
            reply: UserQuestionReply::Answered {
                answers,
                custom: None
            }
        }]
    );
}

#[gpui::test]
fn wraps_arrow_navigation_and_supports_home_and_end(cx: &mut TestAppContext) {
    let (form, cx) = question_form(false, cx);
    let highlighted = |cx: &mut VisualTestContext| form.read_with(cx, |form, _| form.highlighted());
    cx.simulate_keystrokes("up");
    assert_eq!(highlighted(cx), 2);
    cx.simulate_keystrokes("home");
    assert_eq!(highlighted(cx), 0);
    cx.simulate_keystrokes("end");
    assert_eq!(highlighted(cx), 2);
}

#[gpui::test]
fn uses_number_keys_to_focus_and_select_the_matching_option(cx: &mut TestAppContext) {
    let (form, cx) = question_form(false, cx);
    cx.simulate_keystrokes("3");
    assert_eq!(form.read_with(cx, |form, _| form.highlighted()), 2);
    assert_eq!(selected(&form, cx), ["blue"]);
    // Modified digits are left alone.
    cx.simulate_keystrokes("cmd-1");
    assert_eq!(selected(&form, cx), ["blue"]);
}

#[gpui::test]
fn toggles_highlighted_options_for_multi_select_questions(cx: &mut TestAppContext) {
    let (form, cx) = question_form(true, cx);
    cx.simulate_keystrokes("enter down space");
    assert_eq!(selected(&form, cx), ["red", "green"]);
    cx.simulate_keystrokes("space");
    assert_eq!(selected(&form, cx), ["red"]);
}

#[gpui::test]
fn skipping_the_only_question_skips_the_prompt(cx: &mut TestAppContext) {
    let (form, cx) = question_form(false, cx);
    let events = record::<_, QuestionFormEvent>(&form, cx);
    form.update_in(cx, |form, window, cx| form.skip_current(window, cx));
    assert_eq!(
        events.borrow().as_slice(),
        [QuestionFormEvent::Reply {
            request_id: 7,
            reply: UserQuestionReply::Skipped
        }]
    );
}

#[gpui::test]
fn a_question_with_a_deadline_reports_interaction(cx: &mut TestAppContext) {
    let (form, cx) = question_form(false, cx);
    form.update_in(cx, |form, window, cx| {
        let mut prompt = colour_prompt(false);
        prompt.request_id = 8;
        prompt.auto_resolve_at = Some(super::util::now_ms() + 30_000);
        form.set_prompt(prompt, window, cx);
        form.focus_handle(cx).focus(window, cx);
    });
    let events = record::<_, QuestionFormEvent>(&form, cx);
    cx.simulate_keystrokes("down");
    assert!(
        events
            .borrow()
            .contains(&QuestionFormEvent::Interaction { request_id: 8 })
    );
}

// ToolDiffPreview.test.ts

fn edit_preview() -> ToolPreview {
    let mut preview = ToolPreview::new(ToolPreviewKind::Write);
    preview.path = Some("/Users/me/Documents/notes.md".into());
    preview.additions = Some(1);
    preview.deletions = Some(1);
    let line = |kind, text: &str, number| ToolPreviewLine {
        kind,
        text: text.into(),
        number: Some(number),
        extra: Default::default(),
    };
    preview.lines = Some(vec![
        line(ToolPreviewLineKind::Del, "  before", 1),
        line(ToolPreviewLineKind::Add, "  after", 1),
        line(ToolPreviewLineKind::Context, "keep", 2),
    ]);
    preview
}

fn diff_preview(cx: &mut TestAppContext) -> (Entity<ToolDiffPreview>, &mut VisualTestContext) {
    init(cx);
    let (preview, cx) = cx.add_window_view(|window, cx| {
        ToolDiffPreview::new(
            edit_preview(),
            "notes.md",
            ToolCallState::Accepted,
            None,
            true,
            |_, _| div().child("notes.md").into_any_element(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    (preview, cx)
}

fn diff_open(preview: &Entity<ToolDiffPreview>, cx: &mut VisualTestContext) -> bool {
    preview.read_with(cx, |preview, _| preview.is_open())
}

#[gpui::test]
fn waits_for_hover_stays_open_across_the_gap_and_closes_after_leaving(cx: &mut TestAppContext) {
    let (preview, cx) = diff_preview(cx);
    preview.update(cx, |preview, cx| preview.hover_trigger(true, cx));
    advance(cx, 299);
    assert!(!diff_open(&preview, cx));
    advance(cx, 1);
    assert!(diff_open(&preview, cx));
    let lines: Vec<String> = preview.read_with(cx, |preview, cx| {
        preview
            .popover()
            .read(cx)
            .lines()
            .iter()
            .map(|line| line.text.clone())
            .collect()
    });
    assert_eq!(lines, ["  before", "  after", "keep"]);
    preview.update(cx, |preview, cx| preview.hover_trigger(false, cx));
    advance(cx, 100);
    preview.update(cx, |preview, cx| preview.hover_surface(true, cx));
    advance(cx, 500);
    assert!(diff_open(&preview, cx));
    preview.update(cx, |preview, cx| preview.hover_surface(false, cx));
    advance(cx, 200);
    assert!(!diff_open(&preview, cx));
}

#[gpui::test]
fn does_not_flash_for_a_passing_pointer(cx: &mut TestAppContext) {
    let (preview, cx) = diff_preview(cx);
    preview.update(cx, |preview, cx| preview.hover_trigger(true, cx));
    advance(cx, 100);
    preview.update(cx, |preview, cx| preview.hover_trigger(false, cx));
    advance(cx, 500);
    assert!(!diff_open(&preview, cx));
}

#[gpui::test]
fn preserves_the_link_action_and_opens_files_from_the_preview(cx: &mut TestAppContext) {
    let (preview, cx) = diff_preview(cx);
    let events = record::<_, ToolDiffEvent>(&preview, cx);
    preview.update(cx, |preview, cx| {
        preview.hover_trigger(true, cx);
        preview.click(cx);
    });
    advance(cx, 1000);
    assert!(!diff_open(&preview, cx));
    assert_eq!(events.borrow().as_slice(), [ToolDiffEvent::Open]);
    preview.update(cx, |preview, cx| preview.hover_trigger(true, cx));
    advance(cx, 300);
    assert!(diff_open(&preview, cx));
    let popover = preview.read_with(cx, |preview, _| preview.popover().clone());
    popover.update_in(cx, |popover, window, cx| popover.open_file(window, cx));
    cx.run_until_parked();
    assert_eq!(
        events.borrow().last(),
        Some(&ToolDiffEvent::OpenFile {
            path: "/Users/me/Documents/notes.md".into()
        })
    );
    assert!(!diff_open(&preview, cx));
}

#[gpui::test]
fn opens_on_focus_and_closes_on_escape_from_the_card(cx: &mut TestAppContext) {
    let (preview, cx) = diff_preview(cx);
    preview.update_in(cx, |preview, window, cx| {
        preview.focus_handle(cx).focus(window, cx)
    });
    draw(cx);
    assert!(diff_open(&preview, cx));
    // Down moves focus into the card; Escape there closes it and refocuses
    // the chip without reopening the card.
    cx.simulate_keystrokes("down");
    draw(cx);
    let surface = preview.read_with(cx, |preview, cx| {
        preview.popover().read(cx).focus_handle(cx)
    });
    assert!(cx.update(|window, _| surface.is_focused(window)));
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!diff_open(&preview, cx));
    advance(cx, 1000);
    assert!(!diff_open(&preview, cx));
    let trigger = preview.read_with(cx, |preview, cx| preview.focus_handle(cx));
    assert!(cx.update(|window, _| trigger.is_focused(window)));
}

// ApprovalToasts.test.ts

/// The engine's rule: a project's `after` time is when it was resumed or
/// its category turned back on; anything that happened before stays quiet.
#[derive(Default)]
struct Preferences {
    muted: HashMap<String, bool>,
    disabled: HashMap<String, bool>,
    after: HashMap<String, i64>,
}

struct ToastHost {
    toasts: Entity<ApprovalToasts>,
}

impl Render for ToastHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.toasts.clone())
    }
}

fn toast_notice(id: &str, kind: NoticeKind, request_id: i64) -> ApprovalNotice {
    ApprovalNotice {
        session_id: id.into(),
        request_id,
        label: format!("Request {id}"),
        kind,
        session_title: id.into(),
        harness: HarnessId::Codex,
        cwd: format!("/projects/{id}"),
    }
}

type Clock = Rc<RefCell<i64>>;

fn toasts(
    cx: &mut TestAppContext,
) -> (
    Entity<ApprovalToasts>,
    Rc<RefCell<Preferences>>,
    Clock,
    &mut VisualTestContext,
) {
    init(cx);
    let prefs = Rc::new(RefCell::new(Preferences::default()));
    let clock: Clock = Rc::new(RefCell::new(1_800_000_000_000));
    let (host, cx) = cx.add_window_view(|_, cx| {
        let toasts = cx.new(ApprovalToasts::new);
        ToastHost { toasts }
    });
    let toasts = host.read_with(cx, |host, _| host.toasts.clone());
    let gate_prefs = prefs.clone();
    let gate_clock = clock.clone();
    let clock_for_toasts = clock.clone();
    toasts.update(cx, |toasts, cx| {
        toasts.set_clock(move || *clock_for_toasts.borrow());
        toasts.set_gate(
            move |notice, occurred_at, _| {
                let prefs = gate_prefs.borrow();
                let now = *gate_clock.borrow();
                let project = &notice.cwd;
                let after = prefs.after.get(project).copied().unwrap_or(0);
                !prefs.muted.get(project).copied().unwrap_or(false)
                    && !prefs.disabled.get(project).copied().unwrap_or(false)
                    && now > after
                    && occurred_at > after
            },
            cx,
        );
    });
    (toasts, prefs, clock, cx)
}

fn visible_requests(toasts: &Entity<ApprovalToasts>, cx: &mut VisualTestContext) -> Vec<String> {
    toasts.read_with(cx, |toasts, cx| {
        toasts
            .visible(cx)
            .into_iter()
            .map(|notice| notice.label)
            .collect()
    })
}

#[gpui::test]
fn hides_muted_project_popups_while_another_projects_controls_remain_usable(
    cx: &mut TestAppContext,
) {
    let (toasts, prefs, _, cx) = toasts(cx);
    prefs
        .borrow_mut()
        .muted
        .insert("/projects/private".into(), true);
    let events = record::<_, ApprovalToastEvent>(&toasts, cx);
    toasts.update(cx, |toasts, cx| {
        toasts.set_notices(
            vec![
                toast_notice("private", NoticeKind::Approval, 1),
                toast_notice("work", NoticeKind::Approval, 1),
            ],
            cx,
        )
    });
    cx.run_until_parked();
    assert_eq!(visible_requests(&toasts, cx), ["Request work"]);
    toasts.update(cx, |toasts, cx| {
        toasts.decide("work", 1, ApprovalDecision::Allow, cx)
    });
    assert_eq!(
        events.borrow().as_slice(),
        [ApprovalToastEvent::Approval {
            session_id: "work".into(),
            request_id: 1,
            decision: ApprovalDecision::Allow
        }]
    );
}

#[gpui::test]
fn immediately_hides_an_existing_question_when_its_category_is_disabled(cx: &mut TestAppContext) {
    let (toasts, prefs, _, cx) = toasts(cx);
    toasts.update(cx, |toasts, cx| {
        toasts.set_notices(vec![toast_notice("work", NoticeKind::Question, 1)], cx)
    });
    assert_eq!(visible_requests(&toasts, cx).len(), 1);
    prefs
        .borrow_mut()
        .disabled
        .insert("/projects/work".into(), true);
    assert!(visible_requests(&toasts, cx).is_empty());
}

#[gpui::test]
fn does_not_replay_a_mounted_approval_after_its_project_is_resumed(cx: &mut TestAppContext) {
    let (toasts, prefs, clock, cx) = toasts(cx);
    prefs
        .borrow_mut()
        .muted
        .insert("/projects/work".into(), true);
    toasts.update(cx, |toasts, cx| {
        toasts.set_notices(vec![toast_notice("work", NoticeKind::Approval, 1)], cx)
    });
    assert!(visible_requests(&toasts, cx).is_empty());
    *clock.borrow_mut() += 1000;
    {
        let mut prefs = prefs.borrow_mut();
        prefs.muted.insert("/projects/work".into(), false);
        prefs.after.insert("/projects/work".into(), *clock.borrow());
    }
    assert!(visible_requests(&toasts, cx).is_empty());
    *clock.borrow_mut() += 1;
    toasts.update(cx, |toasts, cx| {
        toasts.set_notices(vec![toast_notice("work", NoticeKind::Approval, 2)], cx)
    });
    assert_eq!(visible_requests(&toasts, cx).len(), 1);
}

#[gpui::test]
fn a_click_on_a_card_opens_its_session(cx: &mut TestAppContext) {
    let (toasts, _, _, cx) = toasts(cx);
    let events = record::<_, ApprovalToastEvent>(&toasts, cx);
    toasts.update(cx, |toasts, cx| toasts.focus_session("work", cx));
    assert_eq!(
        events.borrow().as_slice(),
        [ApprovalToastEvent::FocusSession {
            session_id: "work".into()
        }]
    );
}

// UserLinkPreview.test.ts

fn github_link() -> crate::transcript::model::link::UserLink {
    parse_user_message_link("https://github.com/acme/widgets/pull/73")
        .unwrap()
        .link
}

type Requests = Rc<RefCell<Vec<LinkPreviewRequest>>>;

fn link_preview(
    cx: &mut TestAppContext,
) -> (Entity<UserLinkPreview>, Requests, &mut VisualTestContext) {
    init(cx);
    let requests: Requests = Rc::default();
    let sink = requests.clone();
    cx.update(|cx| LinkPreviews::set_loader(cx, move |request, _| sink.borrow_mut().push(request)));
    let (preview, cx) = cx.add_window_view(|window, cx| {
        UserLinkPreview::new(
            github_link(),
            Some("/workspace/widgets".into()),
            true,
            window,
            cx,
        )
    });
    cx.run_until_parked();
    (preview, requests, cx)
}

#[gpui::test]
fn loads_a_rich_popover_when_the_chip_receives_focus(cx: &mut TestAppContext) {
    let (preview, requests, cx) = link_preview(cx);
    preview.update_in(cx, |preview, window, cx| {
        preview.focus_handle(cx).focus(window, cx)
    });
    draw(cx);
    assert!(preview.read_with(cx, |preview, _| preview.is_open()));
    assert_eq!(
        requests.borrow().as_slice(),
        [LinkPreviewRequest::WorkItem {
            cwd: "/workspace/widgets".into(),
            repo: "acme/widgets".into(),
            pull_request: true,
            number: 73,
        }]
    );
    assert_eq!(
        preview.read_with(cx, |preview, _| preview.load_state()),
        LoadState::Loading
    );
    let key = work_item_key(github_link().github_work_item.as_ref().unwrap());
    cx.update(|_, cx| {
        LinkPreviews::resolve_work_item(
            &key,
            Ok(LinkWorkItem {
                title: "Make work item links easier to scan".into(),
                state: "open".into(),
                draft: false,
                updated: Some("2 hours ago".into()),
                labels: vec![WorkItemLabel {
                    name: "enhancement".into(),
                    color: "8b5cf6".into(),
                }],
                assignees: vec![WorkItemPerson {
                    login: "grace".into(),
                    avatar_url: None,
                }],
            }),
            Ok(LinkWorkItemDetails {
                body: "Adds **compact chips** and a useful hover preview.".into(),
                author: "ada".into(),
                author_avatar_url: None,
                base_ref_name: Some("main".into()),
                head_ref_name: Some("link-chips".into()),
            }),
            cx,
        )
    });
    cx.run_until_parked();
    assert_eq!(
        preview.read_with(cx, |preview, _| preview.load_state()),
        LoadState::Ready
    );
    let (item, details) = preview.read_with(cx, |preview, cx| preview.work_item(cx));
    assert_eq!(item.unwrap().title, "Make work item links easier to scan");
    assert_eq!(details.unwrap().author, "ada");
    // Blur closes it.
    cx.update(|window, _| window.blur());
    draw(cx);
    assert!(!preview.read_with(cx, |preview, _| preview.is_open()));
}

#[gpui::test]
fn opens_after_a_short_hover_delay(cx: &mut TestAppContext) {
    let (preview, _, cx) = link_preview(cx);
    preview.update(cx, |preview, cx| preview.show_after_delay(cx));
    advance(cx, 219);
    assert!(!preview.read_with(cx, |preview, _| preview.is_open()));
    advance(cx, 1);
    assert!(preview.read_with(cx, |preview, _| preview.is_open()));
    preview.update(cx, |preview, cx| preview.hide_after_delay(cx));
    advance(cx, 99);
    assert!(preview.read_with(cx, |preview, _| preview.is_open()));
    advance(cx, 1);
    assert!(!preview.read_with(cx, |preview, _| preview.is_open()));
}

#[gpui::test]
fn keeps_the_chip_clickable_and_opens_the_original_url(cx: &mut TestAppContext) {
    let (preview, _, cx) = link_preview(cx);
    let events = record::<_, UserLinkPreviewEvent>(&preview, cx);
    preview.update(cx, |preview, cx| preview.click(cx));
    assert_eq!(
        events.borrow().as_slice(),
        [UserLinkPreviewEvent::Open {
            url: "https://github.com/acme/widgets/pull/73".into()
        }]
    );
}

#[gpui::test]
fn a_failed_lookup_reads_as_unavailable(cx: &mut TestAppContext) {
    let (preview, _, cx) = link_preview(cx);
    preview.update(cx, |preview, cx| preview.show_now(cx));
    let key = work_item_key(github_link().github_work_item.as_ref().unwrap());
    cx.update(|_, cx| {
        LinkPreviews::resolve_work_item(&key, Err("gh".into()), Err("gh".into()), cx)
    });
    cx.run_until_parked();
    assert_eq!(
        preview.read_with(cx, |preview, _| preview.load_state()),
        LoadState::Unavailable
    );
}

// TranscriptFind.tsx

fn blocks(texts: &[(&str, BlockRole, &str)]) -> Vec<Arc<Block>> {
    texts
        .iter()
        .map(|(id, role, text)| Arc::new(Block::new(*id, *role, *text)))
        .collect()
}

/// A find bar that is not drawn: test windows cannot draw a focused text
/// field, which asks the platform window for its native view.
fn find_bar(cx: &mut TestAppContext) -> (Entity<TranscriptFind>, &mut VisualTestContext) {
    init(cx);
    let cx = cx.add_empty_window();
    let find = cx.update(|window, cx| cx.new(|cx| TranscriptFind::new(window, cx)));
    (find, cx)
}

#[gpui::test]
fn find_steps_through_matches_and_clears_on_close(cx: &mut TestAppContext) {
    let (find, cx) = find_bar(cx);
    let events = record::<_, TranscriptFindEvent>(&find, cx);
    find.update_in(cx, |find, window, cx| {
        find.set_blocks(
            blocks(&[
                ("u1", BlockRole::User, "Find the cat"),
                ("a1", BlockRole::Assistant, "A cat sat"),
                ("r1", BlockRole::Reasoning, "cat thoughts"),
                ("a2", BlockRole::Assistant, "No match"),
            ]),
            cx,
        );
        let _ = window;
        find.open(cx);
        find.set_query("CAT".into(), cx);
    });
    let navigate = |id: Option<&str>, query: &str| TranscriptFindEvent::Navigate {
        block_id: id.map(str::to_string),
        query: query.into(),
    };
    assert_eq!(events.borrow().last(), Some(&navigate(Some("u1"), "CAT")));
    find.update(cx, |find, cx| find.step(1, cx));
    assert_eq!(events.borrow().last(), Some(&navigate(Some("a1"), "CAT")));
    find.update(cx, |find, cx| find.step(1, cx));
    assert_eq!(events.borrow().last(), Some(&navigate(Some("u1"), "CAT")));
    find.update(cx, |find, cx| find.step(-1, cx));
    assert_eq!(events.borrow().last(), Some(&navigate(Some("a1"), "CAT")));
    find.update(cx, |find, cx| find.close_find(cx));
    assert_eq!(events.borrow().last(), Some(&navigate(None, "")));
}

#[gpui::test]
fn find_keys_open_step_and_close(cx: &mut TestAppContext) {
    let (find, cx) = find_bar(cx);
    find.update(cx, |find, cx| {
        find.set_blocks(
            blocks(&[
                ("a1", BlockRole::Assistant, "one cat"),
                ("a2", BlockRole::Assistant, "two cats"),
            ]),
            cx,
        );
    });
    let key = |text: &str| gpui::Keystroke::parse(text).unwrap();
    let used = find.update_in(cx, |find, window, cx| {
        find.handle_key(&key("cmd-f"), true, None, window, cx)
    });
    assert!(used);
    assert!(find.read_with(cx, |find, _| find.is_open()));
    find.update(cx, |find, cx| find.set_query("cat".into(), cx));
    find.update_in(cx, |find, window, cx| {
        find.handle_key(&key("cmd-g"), true, None, window, cx)
    });
    assert_eq!(
        find.read_with(cx, |find, _| find.selected().map(str::to_string)),
        Some("a2".into())
    );
    find.update_in(cx, |find, window, cx| {
        find.handle_key(&key("shift-f3"), true, None, window, cx)
    });
    assert_eq!(
        find.read_with(cx, |find, _| find.selected().map(str::to_string)),
        Some("a1".into())
    );
    // An unfocused pane ignores the keys.
    let ignored = find.update_in(cx, |find, window, cx| {
        find.handle_key(&key("escape"), false, None, window, cx)
    });
    assert!(!ignored);
    find.update_in(cx, |find, window, cx| {
        find.handle_key(&key("escape"), true, None, window, cx)
    });
    assert!(!find.read_with(cx, |find, _| find.is_open()));
    // A custom binding replaces Cmd-F, which is still swallowed.
    let custom = key("ctrl-alt-f");
    let swallowed = find.update_in(cx, |find, window, cx| {
        find.handle_key(&key("cmd-f"), true, Some(&custom), window, cx)
    });
    assert!(swallowed);
    assert!(!find.read_with(cx, |find, _| find.is_open()));
    find.update_in(cx, |find, window, cx| {
        find.handle_key(&custom, true, Some(&custom), window, cx)
    });
    assert!(find.read_with(cx, |find, _| find.is_open()));
}

// PromptOutline.tsx

#[gpui::test]
fn the_outline_marks_hovers_and_jumps(cx: &mut TestAppContext) {
    init(cx);
    let (outline, cx) = cx.add_window_view(|_, cx| PromptOutline::new(cx));
    let events = record::<_, PromptOutlineEvent>(&outline, cx);
    outline.update(cx, |outline, cx| {
        outline.set_blocks(
            blocks(&[
                ("u1", BlockRole::User, "First ask"),
                ("a1", BlockRole::Assistant, "Yes."),
                ("u2", BlockRole::User, "Second ask"),
                ("a2", BlockRole::Assistant, "Done."),
                ("u3", BlockRole::User, "Third ask"),
            ]),
            cx,
        );
        let anchors = [
            OutlineAnchor {
                id: "u1".into(),
                top: 0.,
                bottom: 40.,
            },
            OutlineAnchor {
                id: "u2".into(),
                top: 150.,
                bottom: 190.,
            },
            OutlineAnchor {
                id: "u3".into(),
                top: 700.,
                bottom: 740.,
            },
        ];
        let viewport = OutlineBand {
            top: 100.,
            bottom: 500.,
        };
        outline.set_viewport(viewport, &anchors, 300., cx);
    });
    assert_eq!(
        outline.read_with(cx, |outline, _| outline.active_id().map(str::to_string)),
        Some("u2".into())
    );
    // At the end, the last prompt is marked.
    outline.update(cx, |outline, cx| {
        outline.set_viewport(
            OutlineBand {
                top: 100.,
                bottom: 500.,
            },
            &[],
            0.,
            cx,
        )
    });
    assert_eq!(
        outline.read_with(cx, |outline, _| outline.active_id().map(str::to_string)),
        Some("u3".into())
    );
    // A hidden tab keeps the mark.
    outline.update(cx, |outline, cx| {
        outline.set_viewport(
            OutlineBand {
                top: 0.,
                bottom: 0.,
            },
            &[],
            900.,
            cx,
        )
    });
    assert_eq!(
        outline.read_with(cx, |outline, _| outline.active_id().map(str::to_string)),
        Some("u3".into())
    );
    outline.update(cx, |outline, cx| outline.hover_bar("u1", cx));
    assert!(!outline.read_with(cx, |outline, _| outline.is_open()));
    advance(cx, 25);
    assert!(outline.read_with(cx, |outline, _| outline.is_open()));
    outline.update(cx, |outline, cx| outline.jump_to("u1", cx));
    assert_eq!(
        events.borrow().as_slice(),
        [PromptOutlineEvent::Jump {
            block_id: "u1".into()
        }]
    );
    outline.update(cx, |outline, cx| outline.close(cx));
    assert!(!outline.read_with(cx, |outline, _| outline.is_open()));
}

#[gpui::test]
fn jumps_a_transcript_to_a_prompts_turn(cx: &mut TestAppContext) {
    init(cx);
    let (transcript, cx) = cx.add_window_view(|_, cx| {
        let mut view = crate::transcript::TranscriptView::new(cx);
        let mut session =
            monocode_core::Session::blank("s", HarnessId::Claude, "claude:opus-4.6", "/repo");
        session.blocks = ["u1", "a1", "u2", "a2"]
            .iter()
            .map(|id| {
                let role = if id.starts_with('u') {
                    BlockRole::User
                } else {
                    BlockRole::Assistant
                };
                Block::new(*id, role, format!("text {id}"))
            })
            .collect();
        view.set_session(Arc::new(session), cx);
        view
    });
    cx.run_until_parked();
    let found = transcript.update(cx, |transcript, cx| {
        super::prompt_outline::jump_to_prompt(transcript, "u2", cx)
    });
    assert!(found);
    assert!(transcript.read_with(cx, |transcript, _| transcript.is_scrolled_away()));
    let missing = transcript.update(cx, |transcript, cx| {
        super::prompt_outline::jump_to_prompt(transcript, "nope", cx)
    });
    assert!(!missing);
}

// GeneratedImage.tsx

const PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xf8, 0xcf, 0xc0, 0xf0,
    0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

#[gpui::test]
fn reads_a_generated_image_and_opens_it_full_screen(cx: &mut TestAppContext) {
    init(cx);
    let dir = std::env::temp_dir().join(format!("mc-cards-image-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cat.png");
    std::fs::write(&path, PIXEL_PNG).unwrap();
    let meta = monocode_core::block::GeneratedImageMeta {
        path: path.to_string_lossy().into_owned(),
        name: "cat.png".into(),
        mime_type: "image/png".into(),
        size: PIXEL_PNG.len() as i64,
        alt: None,
        extra: Default::default(),
    };
    let (image, cx) = cx.add_window_view(|_, cx| GeneratedImage::new(meta, cx));
    // The file is read on a background thread.
    for _ in 0..50 {
        cx.run_until_parked();
        if !matches!(
            image.read_with(cx, |image, _| image.state().clone()),
            GeneratedImageState::Loading
        ) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    match image.read_with(cx, |image, _| image.state().clone()) {
        GeneratedImageState::Ready { size, .. } => assert_eq!(size, PIXEL_PNG.len() as i64),
        _ => panic!("the image should load"),
    }
    image.update_in(cx, |image, window, cx| image.open(window, cx));
    assert!(image.read_with(cx, |image, _| image.is_open()));
    cx.simulate_keystrokes("escape");
    assert!(!image.read_with(cx, |image, _| image.is_open()));
    let _ = std::fs::remove_dir_all(dir);
}

// MarkdownDocumentPreview.tsx

#[gpui::test]
fn a_remote_attachment_thumbnail_opens_and_closes_full_screen(cx: &mut TestAppContext) {
    use base64::Engine as _;
    init(cx);
    let file = monocode_core::Attachment {
        id: "remote-image".into(),
        name: "pixel.png".into(),
        kind: monocode_core::AttachmentKind::Image,
        mime_type: "image/png".into(),
        path: Some("/missing/on-this-desktop/pixel.png".into()),
        data: Some(base64::engine::general_purpose::STANDARD.encode(PIXEL_PNG)),
        ..monocode_core::Attachment::default()
    };
    let (preview, cx) = cx.add_window_view(|_, cx| GeneratedImage::new_attachment(file, cx));
    for _ in 0..50 {
        cx.run_until_parked();
        if !matches!(
            preview.read_with(cx, |preview, _| preview.state().clone()),
            GeneratedImageState::Loading
        ) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(matches!(
        preview.read_with(cx, |preview, _| preview.state().clone()),
        GeneratedImageState::Ready {
            dimensions: Some((1, 1)),
            ..
        }
    ));
    draw(cx);
    let bounds = cx
        .debug_bounds("attachment-image")
        .expect("thumbnail rendered");
    let previous_focus = cx.update(|window, cx| {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        focus
    });
    cx.simulate_click(bounds.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(preview.read_with(cx, |preview, _| preview.is_open()));
    draw(cx);
    let image_bounds = cx
        .debug_bounds("image-lightbox-image")
        .expect("lightbox image rendered");
    cx.simulate_click(image_bounds.center(), gpui::Modifiers::default());
    assert!(preview.read_with(cx, |preview, _| preview.is_open()));
    cx.simulate_keystrokes("escape");
    assert!(!preview.read_with(cx, |preview, _| preview.is_open()));
    assert!(cx.update(|window, _| previous_focus.is_focused(window)));
}

#[gpui::test]
fn folds_frontmatter_into_a_disclosure(cx: &mut TestAppContext) {
    init(cx);
    let (preview, cx) = cx.add_window_view(|_, cx| {
        MarkdownDocumentPreview::new("---\ntitle: Example\n---\n\n# Heading", "Properties", cx)
    });
    cx.run_until_parked();
    preview.read_with(cx, |preview, cx| {
        assert_eq!(preview.metadata(), Some("title: Example"));
        assert!(!preview.is_metadata_open());
        assert_eq!(preview.body().read(cx).text().trim(), "# Heading");
    });
    preview.update(cx, |preview, cx| preview.toggle_metadata(cx));
    assert!(preview.read_with(cx, |preview, _| preview.is_metadata_open()));
    preview.update(cx, |preview, cx| preview.set_text("# Plain", cx));
    assert!(!preview.read_with(cx, |preview, _| preview.has_metadata()));
}

// TranscriptSelectionMenu inside the transcript

#[gpui::test]
fn selected_reply_text_goes_to_the_chat_and_to_notes(cx: &mut TestAppContext) {
    use crate::cards::TranscriptCardEvent;
    use crate::cards::selection_menu::SelectionAction;
    use crate::transcript::{TranscriptConfig, TranscriptEvent, TranscriptView};

    init(cx);
    let (transcript, cx) = cx.add_window_view(|_, cx| {
        let mut view = TranscriptView::new(cx);
        view.set_config(
            TranscriptConfig {
                can_add_to_chat: true,
                can_save_notes: true,
                ..Default::default()
            },
            cx,
        );
        let mut session =
            monocode_core::Session::blank("s", HarnessId::Claude, "claude:opus-4.6", "/repo");
        session.blocks = vec![
            Block::new("u1", BlockRole::User, "How should bubbles measure text?"),
            Block::new("a1", BlockRole::Assistant, "Measure with the real font."),
        ];
        view.set_session(Arc::new(session), cx);
        view
    });
    draw(cx);
    let cards = record::<_, TranscriptCardEvent>(&transcript, cx);
    let events = record::<_, TranscriptEvent>(&transcript, cx);
    let reply = transcript
        .read_with(cx, |view, _| view.markdown_for("a1"))
        .expect("the reply draws through a markdown view");
    reply.update_in(cx, |view, window, cx| {
        view.focus_handle(cx).focus(window, cx)
    });
    draw(cx);
    cx.simulate_keystrokes("cmd-a");
    // The menu opens where a drag over the reply ends.
    let bounds = reply.read_with(cx, |view, _| view.rendered_text()[0].bounds);
    cx.simulate_mouse_move(bounds.center(), None, gpui::Modifiers::none());
    cx.simulate_mouse_up(
        bounds.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::none(),
    );
    let menu = transcript.read_with(cx, |view, _| view.selection_menu().clone());
    let selected = menu.read_with(cx, |menu, _| menu.selection().map(|s| s.text.clone()));
    assert_eq!(selected.as_deref(), Some("Measure with the real font."));
    menu.update(cx, |menu, cx| menu.pick(SelectionAction::AddToChat, cx));
    assert_eq!(
        cards.borrow().as_slice(),
        [TranscriptCardEvent::AddToChat {
            text: "Measure with the real font.".into()
        }]
    );
    assert!(menu.read_with(cx, |menu, _| menu.selection().is_none()));
    // Picking cleared the reply's selection.
    assert_eq!(reply.read_with(cx, |view, _| view.selected_text()), None);
    // Notes go out as `SaveNote`.
    reply.update_in(cx, |view, window, cx| {
        view.focus_handle(cx).focus(window, cx)
    });
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_mouse_move(bounds.center(), None, gpui::Modifiers::none());
    cx.simulate_mouse_up(
        bounds.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::none(),
    );
    menu.update(cx, |menu, cx| menu.pick(SelectionAction::AddToNotes, cx));
    assert!(events.borrow().contains(&TranscriptEvent::SaveNote {
        text: "Measure with the real font.".into()
    }));
    assert!(menu.read_with(cx, |menu, _| menu.selection().is_none()));
}
