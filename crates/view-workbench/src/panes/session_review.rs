//! Port of src/features/sessions/ui/SessionReview.tsx: the card under a
//! finished turn that lists the files the session changed, with Undo, Keep,
//! and Review.
//!
//! The checkpoint calls (`sessionCheckpointStatus`, `keepSessionChanges`,
//! `undoSessionChanges`) go through a [`SessionReviewHost`]. The owner tells
//! the card about outside changes with [`SessionReview::review_changed`],
//! [`SessionReview::git_changed`], and [`SessionReview::resumed`], which
//! replace the subscriptions and the window focus listener.
//!
//! The transcript draws the same card inline (view-transcript's changes
//! card); this entity owns the loading and the actions either way, and its
//! [`SessionReview::files`] can feed that card.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, Context, ElementId, EventEmitter, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Task, Window, div,
};
use monocode_core::paths::basename;
use monocode_ui::styled::format_integer;
use monocode_ui::widgets::tooltip;
use monocode_ui::{IconName, Theme, UiStyled as _, file_type_icon, icon, u};

/// How long the card waits before reloading after a change notice.
const REFRESH_DEBOUNCE: Duration = Duration::from_millis(200);
/// Files shown before "Show N more files".
const COLLAPSED_FILES: usize = 3;

/// `CheckpointFile`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointFile {
    pub path: String,
    pub relative: String,
    pub additions: i64,
    pub deletions: i64,
    /// The counts are exact; otherwise the file mixes session and outside
    /// changes.
    pub exact: bool,
    pub undoable: bool,
}

/// The checkpoint store behind the card.
pub trait SessionReviewHost {
    /// `sessionCheckpointStatus`.
    fn status(
        &self,
        session_id: &str,
        cwd: &str,
        cx: &mut App,
    ) -> Task<Result<Vec<CheckpointFile>, String>>;
    /// `keepSessionChanges`.
    fn keep(
        &self,
        session_id: &str,
        cwd: &str,
        cx: &mut App,
    ) -> Task<Result<Vec<CheckpointFile>, String>>;
    /// `undoSessionChanges`.
    fn undo(
        &self,
        session_id: &str,
        cwd: &str,
        cx: &mut App,
    ) -> Task<Result<Vec<CheckpointFile>, String>>;
    /// After a Keep or Undo: `notifyGitChanged`, `invalidateWatchedFiles`,
    /// and `invalidateProjectFiles`.
    fn changed(&self, _previous: &[String], _cwd: &str, _cx: &mut App) {}
}

/// What the card shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionReviewProps {
    pub session_id: String,
    pub cwd: String,
    /// Off while the session is not on screen.
    pub enabled: bool,
    pub busy: bool,
    /// Another session runs in this project, so Undo is unsafe.
    pub undo_locked: bool,
}

/// `"keep" | "undo"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewAction {
    Keep,
    Undo,
}

/// What the card reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionReviewEvent {
    /// `onOpenDiff(path, { sessionId, cwd })`: Review opens every file.
    OpenDiff {
        path: Option<String>,
        session_id: String,
        cwd: String,
    },
}

/// The Undo button's tooltip.
pub fn undo_title(can_undo_all: bool, undo_locked: bool) -> &'static str {
    if can_undo_all {
        "Undo all session changes"
    } else if undo_locked {
        "Undo is unavailable while another session is running in this project"
    } else {
        "Undo is unavailable because a file changed outside this session"
    }
}

/// The session review card.
pub struct SessionReview {
    props: SessionReviewProps,
    host: Rc<dyn SessionReviewHost>,
    files: Vec<CheckpointFile>,
    expanded: bool,
    acting: Option<ReviewAction>,
    load: Option<Task<()>>,
    refresh: Option<Task<()>>,
    action: Option<Task<()>>,
}

impl EventEmitter<SessionReviewEvent> for SessionReview {}

impl SessionReview {
    pub fn new(
        props: SessionReviewProps,
        host: Rc<dyn SessionReviewHost>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut review = Self {
            props: SessionReviewProps::default(),
            host,
            files: Vec::new(),
            expanded: false,
            acting: None,
            load: None,
            refresh: None,
            action: None,
        };
        review.set_props(props, cx);
        review
    }

    /// New props run the effects: a busy session clears the card, and an
    /// enabled idle one reloads.
    pub fn set_props(&mut self, props: SessionReviewProps, cx: &mut Context<Self>) {
        let reload = props.enabled
            && !props.busy
            && (props.session_id != self.props.session_id
                || props.cwd != self.props.cwd
                || props.enabled != self.props.enabled
                || props.busy != self.props.busy);
        self.props = props;
        if self.props.busy || !self.props.enabled {
            self.refresh = None;
        }
        if self.props.busy {
            self.set_files(Vec::new());
        }
        if reload {
            self.load(cx);
        }
        cx.notify();
    }

    pub fn files(&self) -> &[CheckpointFile] {
        &self.files
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    pub fn acting(&self) -> Option<ReviewAction> {
        self.acting
    }

    /// The card shows only for an idle session with changed files.
    pub fn is_visible(&self) -> bool {
        !self.props.busy && !self.files.is_empty()
    }

    /// `canUndoAll`.
    pub fn can_undo_all(&self) -> bool {
        !self.props.undo_locked && self.files.iter().all(|file| file.undoable)
    }

    fn listening(&self) -> bool {
        self.props.enabled && !self.props.busy
    }

    fn set_files(&mut self, files: Vec<CheckpointFile>) {
        self.files = files;
        if self.files.len() <= COLLAPSED_FILES {
            self.expanded = false;
        }
    }

    /// `load`.
    pub fn load(&mut self, cx: &mut Context<Self>) {
        let cwd = self.props.cwd.clone();
        if cwd.is_empty() || cwd == "~" {
            self.set_files(Vec::new());
            cx.notify();
            return;
        }
        let status = self.host.status(&self.props.session_id, &cwd, cx);
        self.load = Some(cx.spawn(async move |this, cx| {
            let result = status.await;
            this.update(cx, |this, cx| {
                this.set_files(result.unwrap_or_default());
                cx.notify();
            })
            .ok();
        }));
    }

    fn schedule(&mut self, cx: &mut Context<Self>) {
        if !self.listening() {
            return;
        }
        self.refresh = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(REFRESH_DEBOUNCE).await;
            this.update(cx, |this, cx| {
                this.refresh = None;
                this.load(cx);
            })
            .ok();
        }));
    }

    /// `subscribeReviewChanged`: a checkpoint changed for `session_id`, or
    /// for every session when `None`.
    pub fn review_changed(&mut self, session_id: Option<&str>, cx: &mut Context<Self>) {
        if session_id.is_none_or(|id| id == self.props.session_id) {
            self.schedule(cx);
        }
    }

    /// `subscribeGitChanged`.
    pub fn git_changed(&mut self, cx: &mut Context<Self>) {
        if !self.files.is_empty() {
            self.schedule(cx);
        }
    }

    /// The window regained focus or became visible.
    pub fn resumed(&mut self, cx: &mut Context<Self>) {
        if !self.files.is_empty() {
            self.schedule(cx);
        }
    }

    /// `run`: Keep or Undo every session change.
    pub fn run(&mut self, action: ReviewAction, cx: &mut Context<Self>) {
        if self.acting.is_some() {
            return;
        }
        self.acting = Some(action);
        let (session_id, cwd) = (self.props.session_id.clone(), self.props.cwd.clone());
        let task = match action {
            ReviewAction::Keep => self.host.keep(&session_id, &cwd, cx),
            ReviewAction::Undo => self.host.undo(&session_id, &cwd, cx),
        };
        let previous: Vec<String> = self.files.iter().map(|file| file.path.clone()).collect();
        self.action = Some(cx.spawn(async move |this, cx| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(files) => {
                        this.set_files(files);
                        let host = this.host.clone();
                        host.changed(&previous, &cwd, cx);
                    }
                    Err(_) => this.load(cx),
                }
                this.acting = None;
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn open_diff(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        cx.emit(SessionReviewEvent::OpenDiff {
            path,
            session_id: self.props.session_id.clone(),
            cwd: self.props.cwd.clone(),
        });
    }

    fn render_file(
        &self,
        file: &CheckpointFile,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let name = basename(&file.relative);
        let path = file.path.clone();
        let hover = theme.content(0.05);
        let ink = theme.colors.content;
        let counts: AnyElement = if file.exact {
            div()
                .flex()
                .flex_none()
                .gap(u(8.))
                .text_px(11.)
                .semibold()
                .tabular()
                .child(
                    div()
                        .text_color(theme.colors.success)
                        .child(format!("+{}", format_integer(file.additions))),
                )
                .child(
                    div()
                        .text_color(theme.colors.danger)
                        .child(format!("-{}", format_integer(file.deletions))),
                )
                .into_any_element()
        } else {
            div()
                .flex_none()
                .text_px(11.)
                .medium()
                .text_color(monocode_ui::color::with_alpha(
                    monocode_ui::color::hex(0xffd230),
                    0.8,
                ))
                .child("Mixed changes")
                .into_any_element()
        };
        div()
            .id(ElementId::Name(
                format!("review-file:{}", file.relative).into(),
            ))
            .debug_selector({
                let relative = file.relative.clone();
                move || format!("review-file:{relative}")
            })
            .flex()
            .h(u(32.))
            .w_full()
            .min_w_0()
            .items_center()
            .gap(u(8.))
            .px(u(12.))
            .text_color(theme.content(0.65))
            .hover(move |s| s.bg(hover).text_color(ink))
            .tooltip(tooltip(file.relative.clone()))
            .on_click(cx.listener(move |this, _, _, cx| this.open_diff(Some(path.clone()), cx)))
            .child(file_type_icon(name).size(15.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(theme.fonts.mono.clone())
                    .text_px(12.)
                    .child(file.relative.clone()),
            )
            .child(counts)
            .into_any_element()
    }
}

fn action_button(
    id: &'static str,
    label: &'static str,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let hover = theme.content(0.08);
    let ink = theme.colors.content;
    div()
        .id(id)
        .debug_selector(move || id.into())
        .flex()
        .h(u(28.))
        .items_center()
        .rounded(u(theme.radius.md))
        .px(u(10.))
        .text_px(11.)
        .text_color(theme.content(0.50))
        .hover(move |s| s.bg(hover).text_color(ink))
        .child(label)
}

impl Render for SessionReview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div().id("session-review-shell");
        // The card is the result of a turn. It stays out of the live turn,
        // and comes back once the turn settles and the files reload.
        if !self.is_visible() {
            return root;
        }
        let theme = Theme::of(cx).clone();
        let disabled = self.acting.is_some();
        let can_undo_all = self.can_undo_all();
        let visible: Vec<CheckpointFile> = if self.expanded {
            self.files.clone()
        } else {
            self.files.iter().take(COLLAPSED_FILES).cloned().collect()
        };
        let hidden = self.files.len() - visible.len();
        let (additions, deletions) = self.files.iter().fold((0, 0), |(a, d), file| {
            (a + file.additions, d + file.deletions)
        });
        let count = self.files.len();

        let mut undo = action_button("review-undo", "Undo", &theme)
            .tooltip(tooltip(undo_title(can_undo_all, self.props.undo_locked)));
        undo = if disabled || !can_undo_all {
            undo.opacity(0.35)
        } else {
            undo.on_click(cx.listener(|this, _, _, cx| this.run(ReviewAction::Undo, cx)))
        };
        let mut keep = action_button("review-keep", "Keep", &theme)
            .tooltip(tooltip("Keep all session changes and dismiss this card"));
        keep = if disabled {
            keep.opacity(0.35)
        } else {
            keep.on_click(cx.listener(|this, _, _, cx| this.run(ReviewAction::Keep, cx)))
        };
        let review_hover = theme.content(0.12);
        let ink = theme.colors.content;
        let review = div()
            .id("review-open")
            .debug_selector(|| "review-open".into())
            .flex()
            .h(u(28.))
            .items_center()
            .rounded(u(theme.radius.md))
            .border_1()
            .border_color(theme.content(0.12))
            .bg(theme.content(0.08))
            .px(u(10.))
            .text_px(11.)
            .medium()
            .text_color(theme.content(0.75))
            .hover(move |s| s.bg(review_hover).text_color(ink))
            .tooltip(tooltip("Review changes"))
            .on_click(cx.listener(|this, _, _, cx| this.open_diff(None, cx)))
            .child("Review");

        let header = div()
            .flex()
            .min_w_0()
            .items_center()
            .gap(u(10.))
            .px(u(12.))
            .py(u(10.))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .size(u(32.))
                    .items_center()
                    .justify_center()
                    .rounded(u(theme.radius.lg))
                    .bg(theme.content(0.08))
                    .text_color(theme.content(0.55))
                    .child(
                        icon(IconName::FileDiff)
                            .size(u(16.))
                            .text_color(theme.content(0.55)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_px(12.)
                            .medium()
                            .text_color(theme.content(0.80))
                            .child(format!(
                                "Changed {count} {}",
                                if count == 1 { "file" } else { "files" }
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(u(6.))
                            .mt(u(-2.))
                            .text_px(11.)
                            .semibold()
                            .tabular()
                            .child(
                                div()
                                    .text_color(theme.colors.success)
                                    .child(format!("+{}", format_integer(additions))),
                            )
                            .child(
                                div()
                                    .text_color(theme.colors.danger)
                                    .child(format!("-{}", format_integer(deletions))),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(u(2.))
                    .child(undo)
                    .child(keep)
                    .child(review),
            );

        let mut list = div()
            .id("review-files")
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(theme.colors.stroke)
            .py(u(4.));
        if self.expanded {
            list = list.max_h(u(256.)).overflow_y_scroll();
        }
        for file in &visible {
            list = list.child(self.render_file(file, &theme, cx));
        }

        let mut card = div()
            .debug_selector(|| "session-review".into())
            .overflow_hidden()
            .rounded(u(theme.radius.xl))
            .border_1()
            .border_color(theme.content(0.12))
            .bg(theme.content(0.03))
            .child(header)
            .child(list);
        if count > COLLAPSED_FILES {
            let hover = theme.content(0.05);
            let hover_ink = theme.content(0.70);
            let expanded = self.expanded;
            card = card.child(
                div()
                    .id("review-expand")
                    .group("review-expand")
                    .debug_selector(|| "review-expand".into())
                    .flex()
                    .h(u(32.))
                    .w_full()
                    .items_center()
                    .gap(u(6.))
                    .border_t_1()
                    .border_color(theme.colors.stroke)
                    .px(u(12.))
                    .text_px(11.)
                    .text_color(theme.content(0.45))
                    .hover(move |s| s.bg(hover).text_color(hover_ink))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.expanded = !this.expanded;
                        cx.notify();
                    }))
                    .child(
                        icon(if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size(u(14.))
                        .text_color(theme.content(0.45))
                        .group_hover("review-expand", move |s| s.text_color(hover_ink)),
                    )
                    .child(if expanded {
                        "Show fewer files".to_string()
                    } else {
                        format!(
                            "Show {hidden} more {}",
                            if hidden == 1 { "file" } else { "files" }
                        )
                    }),
            );
        }
        root.px(u(16.)).pt(u(4.)).pb(u(8.)).child(card)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

    use super::*;
    use crate::panes::test_support::{draw, init};

    fn file(name: &str, undoable: bool) -> CheckpointFile {
        CheckpointFile {
            path: format!("/repo/{name}"),
            relative: name.into(),
            additions: 12,
            deletions: 3,
            exact: true,
            undoable,
        }
    }

    #[derive(Default)]
    struct FakeHost {
        files: RefCell<Vec<CheckpointFile>>,
        calls: RefCell<Vec<&'static str>>,
    }

    impl SessionReviewHost for FakeHost {
        fn status(
            &self,
            _: &str,
            _: &str,
            _: &mut App,
        ) -> Task<Result<Vec<CheckpointFile>, String>> {
            self.calls.borrow_mut().push("status");
            Task::ready(Ok(self.files.borrow().clone()))
        }

        fn keep(&self, _: &str, _: &str, _: &mut App) -> Task<Result<Vec<CheckpointFile>, String>> {
            self.calls.borrow_mut().push("keep");
            self.files.borrow_mut().clear();
            Task::ready(Ok(Vec::new()))
        }

        fn undo(&self, _: &str, _: &str, _: &mut App) -> Task<Result<Vec<CheckpointFile>, String>> {
            self.calls.borrow_mut().push("undo");
            Task::ready(Err("conflict".into()))
        }

        fn changed(&self, _: &[String], _: &str, _: &mut App) {
            self.calls.borrow_mut().push("changed");
        }
    }

    fn props(busy: bool) -> SessionReviewProps {
        SessionReviewProps {
            session_id: "s1".into(),
            cwd: "/repo".into(),
            enabled: true,
            busy,
            undo_locked: false,
        }
    }

    fn mount(
        files: Vec<CheckpointFile>,
        cx: &mut TestAppContext,
    ) -> (Entity<SessionReview>, Rc<FakeHost>, &mut VisualTestContext) {
        cx.update(init);
        let host = Rc::new(FakeHost::default());
        *host.files.borrow_mut() = files;
        let review_host: Rc<dyn SessionReviewHost> = host.clone();
        let (review, cx) =
            cx.add_window_view(move |_, cx| SessionReview::new(props(false), review_host, cx));
        draw(cx);
        (review, host, cx)
    }

    #[gpui::test]
    fn loads_the_changed_files_and_hides_during_a_turn(cx: &mut TestAppContext) {
        let (review, _, cx) = mount(vec![file("a.ts", true), file("b.ts", true)], cx);
        assert!(cx.debug_bounds("session-review").is_some());
        assert_eq!(review.read_with(cx, |review, _| review.files().len()), 2);
        review.update(cx, |review, cx| review.set_props(props(true), cx));
        draw(cx);
        assert!(cx.debug_bounds("session-review").is_none());
        assert!(review.read_with(cx, |review, _| review.files().is_empty()));
    }

    #[gpui::test]
    fn expands_past_three_files_and_opens_diffs(cx: &mut TestAppContext) {
        let files = ["a.ts", "b.ts", "c.ts", "d.ts", "e.ts"]
            .map(|name| file(name, true))
            .to_vec();
        let (review, _, cx) = mount(files, cx);
        assert!(cx.debug_bounds("review-file:d.ts").is_none());
        let expand = cx.debug_bounds("review-expand").unwrap().center();
        cx.simulate_click(expand, Modifiers::none());
        draw(cx);
        assert!(cx.debug_bounds("review-file:e.ts").is_some());

        let events: Rc<RefCell<Vec<SessionReviewEvent>>> = Rc::default();
        let sink = events.clone();
        cx.update(|_, cx| {
            cx.subscribe(&review, move |_, event: &SessionReviewEvent, _| {
                sink.borrow_mut().push(event.clone())
            })
            .detach();
        });
        let row = cx.debug_bounds("review-file:b.ts").unwrap().center();
        cx.simulate_click(row, Modifiers::none());
        let open = cx.debug_bounds("review-open").unwrap().center();
        cx.simulate_click(open, Modifiers::none());
        assert_eq!(
            events.borrow().as_slice(),
            [
                SessionReviewEvent::OpenDiff {
                    path: Some("/repo/b.ts".into()),
                    session_id: "s1".into(),
                    cwd: "/repo".into()
                },
                SessionReviewEvent::OpenDiff {
                    path: None,
                    session_id: "s1".into(),
                    cwd: "/repo".into()
                },
            ]
        );
    }

    #[gpui::test]
    fn keeps_changes_and_reloads_after_a_failed_undo(cx: &mut TestAppContext) {
        let (review, host, cx) = mount(vec![file("a.ts", true)], cx);
        review.update(cx, |review, cx| review.run(ReviewAction::Undo, cx));
        draw(cx);
        assert_eq!(host.calls.borrow().as_slice(), ["status", "undo", "status"]);
        let keep = cx.debug_bounds("review-keep").unwrap().center();
        cx.simulate_click(keep, Modifiers::none());
        draw(cx);
        assert_eq!(
            host.calls.borrow().as_slice(),
            ["status", "undo", "status", "keep", "changed"]
        );
        assert!(cx.debug_bounds("session-review").is_none());
    }

    #[test]
    fn explains_why_undo_is_unavailable() {
        assert_eq!(undo_title(true, false), "Undo all session changes");
        assert!(undo_title(false, true).contains("another session"));
        assert!(undo_title(false, false).contains("outside this session"));
    }
}
