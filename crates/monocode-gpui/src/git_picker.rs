//! The composer's searchable BranchPicker and existing-worktree list.
use crate::{input, popover, theme::*};
use gpui::{
    AnyElement, Context, Entity, EventEmitter, FocusHandle, Focusable, ListAlignment, ListState,
    Role, Subscription, Window, div, list, prelude::*, px,
};
use monocode_core::git::{self, Snapshot};
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Branch,
    Base,
    Worktree,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[gpui::test]
    fn large_branch_picker_renders_the_viewport_and_reveals_keyboard_selection(
        cx: &mut TestAppContext,
    ) {
        let cwd = "/nonexistent/monocode-large-branch-picker";
        crate::git_cache::remember(
            cwd,
            &Snapshot {
                branches: (0..2_000)
                    .map(|i| git::Branch {
                        name: format!("branch-{i}"),
                        remote: None,
                    })
                    .collect(),
                ..Default::default()
            },
        );
        let window = cx.update(|cx| {
            cx.open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                        gpui::point(px(0.), px(0.)),
                        gpui::size(px(400.), px(400.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|cx| GitPicker::new(cwd.into(), Mode::Base, None, cx)),
            )
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        assert!(visual.debug_bounds("git-option-0").is_some());
        assert!(visual.debug_bounds("git-option-1999").is_none());
        window
            .update(visual, |view, _, cx| view.step(1_999, cx))
            .unwrap();
        visual.run_until_parked();
        assert!(visual.debug_bounds("git-option-1999").is_some());
        assert!(visual.debug_bounds("git-option-0").is_none());
    }

    #[gpui::test]
    fn refreshing_choices_keeps_the_keyboard_selection_in_view(cx: &mut TestAppContext) {
        let cwd = "/nonexistent/monocode-refresh-branch-picker";
        crate::git_cache::remember(
            cwd,
            &Snapshot {
                branches: (0..2_000)
                    .map(|i| git::Branch {
                        name: format!("branch-{i}"),
                        remote: None,
                    })
                    .collect(),
                ..Default::default()
            },
        );
        let window = cx.update(|cx| {
            cx.open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                        gpui::point(px(0.), px(0.)),
                        gpui::size(px(400.), px(400.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| cx.new(|cx| GitPicker::new(cwd.into(), Mode::Base, None, cx)),
            )
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        window
            .update(visual, |view, _, cx| view.step(1_500, cx))
            .unwrap();
        visual.run_until_parked();
        assert!(visual.debug_bounds("git-option-1500").is_some());
        // A finished background load rebuilds the list under the selection.
        window
            .update(visual, |view, _, cx| view.refresh_choices(cx))
            .unwrap();
        visual.run_until_parked();
        assert!(visual.debug_bounds("git-option-1500").is_some());
        assert!(visual.debug_bounds("git-option-0").is_none());
    }

    #[gpui::test]
    fn all_picker_modes_start_with_cached_choices_and_keep_them_after_refresh_failure(
        cx: &mut TestAppContext,
    ) {
        let cwd = "/nonexistent/monocode-cached-git-picker";
        crate::git_cache::remember(
            cwd,
            &Snapshot {
                current: Some("main".into()),
                branches: vec![
                    git::Branch {
                        name: "main".into(),
                        remote: None,
                    },
                    git::Branch {
                        name: "feature".into(),
                        remote: None,
                    },
                ],
                worktrees: vec![git::Worktree {
                    path: cwd.into(),
                    branch: Some("main".into()),
                    main: true,
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        let pickers: Vec<_> = [Mode::Branch, Mode::Base, Mode::Worktree]
            .into_iter()
            .map(|mode| {
                let picker = cx.new(|cx| GitPicker::new(cwd.into(), mode, None, cx));
                picker.update(cx, |view, cx| {
                    assert!(
                        view.state.is_some(),
                        "choices are available before any background work"
                    );
                    assert_eq!(
                        view.choices(cx).len(),
                        if mode == Mode::Worktree { 1 } else { 2 }
                    );
                });
                picker
            })
            .collect();
        cx.run_until_parked();
        for picker in pickers {
            picker.update(cx, |view, cx| {
                assert!(view.error.is_some());
                assert!(
                    !view.choices(cx).is_empty(),
                    "a failed refresh retains usable choices"
                );
            });
        }
        assert!(crate::git_cache::get(cwd).is_some());
    }
}
pub(crate) enum Picked {
    Changed,
    Base(String),
    Worktree(String),
    Dismissed,
}
pub(crate) struct GitPicker {
    cwd: PathBuf,
    mode: Mode,
    selected: Option<String>,
    state: Option<Arc<Snapshot>>,
    filtered: Vec<Choice>,
    list: ListState,
    search: Entity<input::Input>,
    active: usize,
    busy: bool,
    existing_only: bool,
    error: Option<String>,
    _changed: Subscription,
}
#[derive(Clone)]
enum Choice {
    Branch(git::Branch),
    Create(String),
    Worktree(git::Worktree),
}
impl EventEmitter<Picked> for GitPicker {}
impl GitPicker {
    pub(crate) fn new(
        cwd: PathBuf,
        mode: Mode,
        selected: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| {
            input::Input::field(
                if mode == Mode::Worktree {
                    "Search working copies"
                } else {
                    "Search branches"
                },
                if mode == Mode::Worktree {
                    "Search working copies…"
                } else {
                    "Search branches…"
                },
                content(0.4),
                cx,
            )
        });
        let changed = cx.subscribe(&search, |this, _, _: &input::Changed, cx| {
            this.active = 0;
            this.refresh_choices(cx);
        });
        let state = crate::git_cache::get(&cwd.to_string_lossy());
        let load_cwd = cwd.clone();
        let work = cx
            .background_executor()
            .spawn(async move { crate::git_cache::load(&load_cwd) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(state) => this.state = Some(state),
                    Err(error) => this.error = Some(error),
                }
                this.refresh_choices(cx);
            });
        })
        .detach();
        let mut this = Self {
            cwd,
            mode,
            selected,
            state,
            filtered: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, px(200.)),
            search,
            active: 0,
            busy: false,
            existing_only: false,
            error: None,
            _changed: changed,
        };
        this.refresh_choices(cx);
        this
    }
    pub(crate) fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
    pub(crate) fn mode(&self) -> Mode {
        self.mode
    }
    /// WorkspacePicker's submenu contains available linked copies; the parent
    /// already offers the project folder through Current checkout.
    pub(crate) fn existing_worktrees(mut self) -> Self {
        self.existing_only = true;
        self.filtered.retain(
            |choice| matches!(choice, Choice::Worktree(tree) if !tree.main && !tree.missing),
        );
        self.list
            .reset_with_uniform_height(self.filtered.len(), px(44.));
        self
    }
    fn refresh_choices(&mut self, cx: &mut Context<Self>) {
        self.filtered = self.choices(cx);
        self.active = self.active.min(self.filtered.len().saturating_sub(1));
        self.list
            .reset_with_uniform_height(self.filtered.len(), px(44.));
        // The reset scrolls to the top; keep the kept selection on screen.
        self.list.scroll_to_reveal_item(self.active);
        cx.notify();
    }
    fn choices(&self, cx: &gpui::App) -> Vec<Choice> {
        let query = self.search.read(cx).content.trim().to_owned();
        let needle = query.to_lowercase();
        let Some(state) = &self.state else {
            return Vec::new();
        };
        if self.mode == Mode::Worktree {
            return state
                .worktrees
                .iter()
                .filter(|tree| !self.existing_only || (!tree.main && !tree.missing))
                .filter(|tree| {
                    format!(
                        "{} {}",
                        tree.branch.as_deref().unwrap_or("detached"),
                        tree.path
                    )
                    .to_lowercase()
                    .contains(&needle)
                })
                .cloned()
                .map(Choice::Worktree)
                .collect();
        }
        let mut choices: Vec<_> = state
            .branches
            .iter()
            .filter(|branch| {
                format!("{} {}", branch.name, branch.remote.as_deref().unwrap_or(""))
                    .to_lowercase()
                    .contains(&needle)
            })
            .cloned()
            .map(Choice::Branch)
            .collect();
        if self.mode == Mode::Branch
            && !query.is_empty()
            && !state
                .branches
                .iter()
                .any(|b| b.remote.is_none() && b.name == query)
        {
            choices.push(Choice::Create(query));
        }
        choices
    }
    fn row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let choice = self.filtered[ix].clone();
        let (label, detail, selected, disabled, path) = match choice {
            Choice::Branch(branch) => {
                let full = branch
                    .remote
                    .as_ref()
                    .map_or(branch.name.clone(), |r| format!("{r}/{}", branch.name));
                let selected = if self.mode == Mode::Base {
                    self.selected.as_ref() == Some(&full)
                } else {
                    branch.remote.is_none()
                        && self.state.as_ref().and_then(|s| s.current.as_ref())
                            == Some(&branch.name)
                };
                (branch.name, branch.remote, selected, false, "git-branch")
            }
            Choice::Create(name) => (
                format!("Create branch ‘{name}’"),
                None,
                false,
                false,
                "plus",
            ),
            Choice::Worktree(tree) => {
                let selected = if self.existing_only {
                    self.selected.as_deref().is_some_and(|path| {
                        monocode_core::files::path_key(path)
                            == monocode_core::files::path_key(&tree.path)
                    })
                } else {
                    std::fs::canonicalize(&tree.path)
                        .ok()
                        .is_some_and(|path| Some(path) == std::fs::canonicalize(&self.cwd).ok())
                };
                (
                    tree.branch.unwrap_or_else(|| {
                        format!("Detached {}", tree.head.chars().take(7).collect::<String>())
                    }),
                    Some(if tree.main {
                        "Project folder".into()
                    } else {
                        tree.path
                    }),
                    selected,
                    tree.missing,
                    if tree.main {
                        "git-branch"
                    } else {
                        "folder-tree"
                    },
                )
            }
        };
        div()
            .id(("git-option", ix))
            .debug_selector(move || format!("git-option-{ix}"))
            .role(Role::ListBoxOption)
            .aria_label(label.clone())
            .aria_selected(selected)
            .flex_shrink_0()
            .min_h(px(32.))
            .px(px(8.))
            .py(px(6.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .text_color(content(if disabled || self.busy { 0.35 } else { 0.9 }))
            .when(ix == self.active, |d| d.bg(content(0.08)))
            .child(icon(path, 14.).text_color(content(0.5)))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(div().truncate().text_size(px(12.)).child(label))
                    .when_some(detail, |d, detail| {
                        d.child(
                            div()
                                .truncate()
                                .text_size(px(10.))
                                .text_color(content(0.4))
                                .child(detail),
                        )
                    }),
            )
            .when(selected, |d| d.child(icon("check-2", 14.)))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.active = ix;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.pick(ix, cx)))
            .into_any_element()
    }
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        self.active = self
            .active
            .saturating_add_signed(delta)
            .min(self.filtered.len().saturating_sub(1));
        self.list.scroll_to_reveal_item(self.active);
        cx.notify();
    }
    fn pick(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(choice) = self.filtered.get(ix).cloned() else {
            return;
        };
        match choice {
            Choice::Worktree(tree) => {
                if !tree.missing {
                    cx.emit(Picked::Worktree(tree.path));
                }
            }
            Choice::Branch(branch) if self.mode == Mode::Base => {
                cx.emit(Picked::Base(
                    branch.remote.map_or(branch.name.clone(), |remote| {
                        format!("{remote}/{}", branch.name)
                    }),
                ));
            }
            Choice::Branch(branch)
                if branch.remote.is_none()
                    && self.state.as_ref().and_then(|s| s.current.as_ref())
                        == Some(&branch.name) =>
            {
                cx.emit(Picked::Dismissed)
            }
            choice => {
                self.busy = true;
                self.error = None;
                let cwd = self.cwd.clone();
                let work = cx.background_executor().spawn(async move {
                    match choice {
                        Choice::Branch(branch) => git::checkout(&cwd, &branch),
                        Choice::Create(name) => git::create_branch(&cwd, &name),
                        _ => unreachable!(),
                    }
                });
                cx.spawn(async move |this, cx| {
                    let result = work.await;
                    let _ = this.update(cx, |this, cx| {
                        this.busy = false;
                        match result {
                            Ok(()) => cx.emit(Picked::Changed),
                            Err(error) => this.error = Some(error),
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        cx.notify();
    }
}
impl gpui::Render for GitPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = self.filtered.is_empty();
        let entity = cx.entity();
        let existing_only = self.existing_only;
        div()
            .id("git-picker")
            .debug_selector(move || {
                if existing_only {
                    "existing-worktrees-submenu".into()
                } else {
                    "git-picker".into()
                }
            })
            .role(Role::Dialog)
            .aria_label(if existing_only {
                "Existing worktrees"
            } else if self.mode == Mode::Worktree {
                "Working copies"
            } else if self.mode == Mode::Base {
                "Worktree base branch"
            } else {
                "Branches"
            })
            .key_context("GitPicker")
            .w_full()
            .when(!existing_only, |d| d.h_full())
            .when(existing_only, |d| d.max_h(px(320.)))
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .capture_action(cx.listener(|this, _: &input::Left, window, cx| {
                if this.existing_only && this.search.read(cx).content.is_empty() {
                    window.dispatch_action(Box::new(popover::CloseSubmenu), cx);
                    cx.stop_propagation();
                }
            }))
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
            .on_action(cx.listener(|_, _: &popover::Cancel, _, cx| {
                cx.emit(Picked::Dismissed);
                cx.stop_propagation();
            }))
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(Picked::Dismissed);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex_shrink_0()
                    .h(px(36.))
                    .px(px(12.))
                    .border_b_1()
                    .border_color(content(0.07))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon("search", 14.).text_color(content(0.4)))
                    .child(div().flex_1().min_w_0().child(self.search.clone())),
            )
            .child(
                div()
                    .id("git-choices")
                    .role(Role::ListBox)
                    .aria_label("Working copy options")
                    .when(!existing_only, |d| d.flex_1())
                    .when(existing_only, |d| {
                        d.h(px((self.filtered.len().max(1) as f32 * 48.).min(284.)))
                    })
                    .min_h_0()
                    .p(px(4.))
                    .flex()
                    .flex_col()
                    .when(!empty, |d| {
                        d.child(
                            list(self.list.clone(), move |ix, _, cx| {
                                entity.update(cx, |view, cx| view.row(ix, cx))
                            })
                            .flex_1()
                            .min_h_0(),
                        )
                    })
                    .when(empty, |d| {
                        d.child(
                            div()
                                .p(px(12.))
                                .text_size(px(12.))
                                .text_color(content(0.45))
                                .child(if self.state.is_none() && self.error.is_none() {
                                    "Loading…"
                                } else if existing_only
                                    && self.search.read(cx).content.trim().is_empty()
                                {
                                    "No existing worktrees"
                                } else {
                                    "No matches"
                                }),
                        )
                    }),
            )
            .when(self.mode == Mode::Branch, |d| {
                d.child(
                    div()
                        .px(px(12.))
                        .py(px(6.))
                        .text_size(px(10.))
                        .text_color(content(0.4))
                        .child("Type a new name to create a branch"),
                )
            })
            .when_some(self.error.clone(), |d, error| {
                d.child(
                    div()
                        .p(px(10.))
                        .text_size(px(11.))
                        .text_color(gpui::rgb(0xf87171))
                        .child(error),
                )
            })
    }
}
