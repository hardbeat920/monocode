//! Cmd+P's project-file search and `>` command palette.
use crate::{input, theme::*};
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable, Role, Subscription, Window, div,
    prelude::*, px,
};
use monocode_core::{composer, files::ProjectFile};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy)]
pub(crate) enum Command {
    New,
    Project,
    Model,
    Settings,
    Sidebar,
    Panel,
    Terminal,
    Layout,
    WorkspaceStyle,
    Plan,
    Operator,
    Orchestrator,
}
const COMMANDS: &[(Command, &str, &str)] = &[
    (Command::New, "New conversation", "⌘N"),
    (Command::Project, "Open project", "⌘O"),
    (Command::Model, "Switch model", "⌘."),
    (Command::Settings, "Open settings", "⌘,"),
    (Command::Sidebar, "Toggle sidebar", "⌘B"),
    (Command::Panel, "Toggle files and changes", "⌘⇧B"),
    (Command::Terminal, "Toggle terminal", "⌘J"),
    (Command::Layout, "Switch workspace layout", "⌘⇧L"),
    (
        Command::WorkspaceStyle,
        "Switch rounded or boxy workspace",
        "⌘⌥L",
    ),
    (Command::Plan, "Toggle Plan mode", ""),
    (Command::Operator, "Toggle Operator", ""),
    (Command::Orchestrator, "Toggle Orchestrator", ""),
];
pub(crate) enum Picked {
    File(String),
    Command(Command),
    Dismiss,
}
#[derive(Clone)]
struct Row {
    label: String,
    detail: String,
    path: Option<String>,
    command: Option<Command>,
}
pub(crate) struct QuickOpen {
    pub(crate) search: Entity<input::Input>,
    files: Vec<ProjectFile>,
    rows: Vec<Row>,
    active: usize,
    scroll: gpui::ScrollHandle,
    loading: bool,
    error: Option<String>,
    cancel: Arc<AtomicBool>,
    _changed: Subscription,
}
impl EventEmitter<Picked> for QuickOpen {}
impl Focusable for QuickOpen {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}
impl Drop for QuickOpen {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl QuickOpen {
    pub(crate) fn new(cwd: String, palette: bool, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            input::Input::field(
                "Go to file or run a command",
                "Search files by name…  (> for commands)",
                content(0.35),
                cx,
            )
        });
        if palette {
            search.update(cx, |input, cx| input.set_text(">".into(), cx));
        }
        let changed = cx.subscribe(&search, |this, _, _: &input::Changed, cx| {
            this.active = 0;
            this.rank(cx);
            cx.notify();
        });
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let work = cx.background_executor().spawn(async move {
            monocode_core::files::list_project_files_sync_cancellable(&cwd, Some(&flag))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(files) => this.files = files,
                    Err(e) => this.error = Some(e),
                };
                this.rank(cx);
                cx.notify();
            });
        })
        .detach();
        let mut this = Self {
            search,
            files: Vec::new(),
            rows: Vec::new(),
            active: 0,
            scroll: gpui::ScrollHandle::new(),
            loading: true,
            error: None,
            cancel,
            _changed: changed,
        };
        this.rank(cx);
        this
    }
    fn rank(&mut self, cx: &gpui::App) {
        let query = self.search.read(cx).content.trim();
        self.rows = if let Some(query) = query.strip_prefix('>') {
            let mut commands: Vec<_> = COMMANDS
                .iter()
                .filter_map(|(command, label, hint)| {
                    composer::score(query.trim(), label).map(|score| {
                        (
                            score,
                            Row {
                                label: (*label).into(),
                                detail: if cfg!(target_os = "macos") {
                                    (*hint).into()
                                } else {
                                    hint.replace('⌘', "Ctrl+").replace('⇧', "Shift+")
                                },
                                path: None,
                                command: Some(*command),
                            },
                        )
                    })
                })
                .collect();
            commands.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
            commands.into_iter().map(|(_, row)| row).collect()
        } else {
            composer::rank_files(&self.files, query, 80)
                .into_iter()
                .map(|file| Row {
                    label: file.name.clone(),
                    detail: file.relative.clone(),
                    path: Some(file.path.clone()),
                    command: None,
                })
                .collect()
        };
        self.active = self.active.min(self.rows.len().saturating_sub(1));
        self.scroll.scroll_to_item(self.active);
    }
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        if !self.rows.is_empty() {
            self.active =
                (self.active as isize + delta).rem_euclid(self.rows.len() as isize) as usize;
            self.scroll.scroll_to_item(self.active);
            cx.notify();
        }
    }
    fn pick(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.search.read(cx).is_composing() {
            return;
        }
        if let Some(row) = self.rows.get(ix) {
            if let Some(path) = &row.path {
                cx.emit(Picked::File(path.clone()));
            } else if let Some(command) = row.command {
                cx.emit(Picked::Command(command));
            }
        }
    }
}
impl Render for QuickOpen {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .map(|(ix, row)| {
                div()
                    .id(("quick-open-row", ix))
                    .role(Role::ListItem)
                    .aria_label(row.detail.clone())
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .flex()
                    .gap_2p5()
                    .items_center()
                    .bg(if self.active == ix {
                        content(0.1)
                    } else {
                        content(0.)
                    })
                    .hover(|s| s.bg(content(0.07)))
                    .child(if row.path.is_some() {
                        crate::file_icons::file_type_icon(&row.label, false, false, 18.)
                    } else {
                        icon("sparkles", 16.).into_any_element()
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .child(div().truncate().child(row.label.clone()))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(content(0.4))
                                    .truncate()
                                    .child(row.detail.clone()),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.pick(ix, cx)))
                    .into_any_element()
            })
            .collect();
        div()
            .id("quick-open")
            .role(Role::Dialog)
            .aria_label("Go to file or run a command")
            .w_full()
            .flex()
            .flex_col()
            .capture_action(cx.listener(|this, _: &input::Up, _, cx| {
                this.step(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Down, _, cx| {
                this.step(1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Submit, _, cx| {
                this.pick(this.active, cx);
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|_, _: &crate::shell::DismissMenu, _, cx| {
                cx.emit(Picked::Dismiss);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(content(0.08))
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .child(icon("search", 16.).text_color(content(0.5)))
                    .child(div().flex_1().min_w_0().child(self.search.clone())),
            )
            .child(
                div()
                    .id("quick-open-results")
                    .role(Role::ListBox)
                    .aria_label("Results")
                    .p_1p5()
                    .max_h(px(380.))
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(rows)
                    .when(self.rows.is_empty(), |d| {
                        d.child(
                            div()
                                .px_3()
                                .py_6()
                                .text_size(px(13.))
                                .text_color(content(0.45))
                                .child(if self.loading {
                                    "Loading project files…".to_owned()
                                } else {
                                    self.error.clone().unwrap_or_else(|| "No results".into())
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .px_4()
                    .py_2()
                    .border_t_1()
                    .border_color(content(0.07))
                    .text_size(px(10.))
                    .text_color(content(0.35))
                    .child("↑ ↓ navigate    ↵ open    Esc close    > commands"),
            )
    }
}
