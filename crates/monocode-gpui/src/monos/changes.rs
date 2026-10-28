//! MonoChangesView: assigned-project tabs with a retained commit/review surface
//! per checkout. Only the selected checkout owns a live Git poller.
use super::*;
use crate::popover::{MenuAction, MenuItem};
use std::{path::Path, time::Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Checkout {
    root: String,
    count: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Project {
    path: String,
    checkouts: Vec<Checkout>,
}
impl Project {
    fn count(&self) -> usize {
        self.checkouts.iter().map(|c| c.count).sum()
    }
}
#[derive(Clone, PartialEq, Eq)]
struct Source {
    session: Option<i64>,
    cwd: String,
    updated: u64,
    projects: Vec<String>,
}
#[derive(Default)]
struct Summary {
    source: Option<Source>,
    projects: Vec<Project>,
    error: Option<String>,
    loaded: Option<Instant>,
    loading: bool,
    revision: u64,
}
struct Surface {
    git: Entity<crate::git_status::ProjectGit>,
    changes: Entity<crate::changes::Changes>,
    review: Option<Entity<crate::review::ReviewPane>>,
    enabled: bool,
    _subscriptions: Vec<Subscription>,
}
#[derive(Default)]
pub(super) struct ProjectChanges {
    summaries: HashMap<String, Summary>,
    selected: HashMap<String, String>,
    last_roots: HashMap<(String, String), String>,
    surfaces: HashMap<(String, String), Surface>,
}
impl ProjectChanges {
    pub(super) fn count(&self, mono: &str) -> usize {
        self.summaries
            .get(mono)
            .map_or(0, |s| s.projects.iter().map(Project::count).sum())
    }
    fn projects(&self, mono: &Mono) -> Vec<Project> {
        let saved = self.summaries.get(&mono.id).map(|s| &s.projects);
        mono.projects
            .iter()
            .map(|path| {
                saved
                    .and_then(|p| p.iter().find(|p| &p.path == path))
                    .cloned()
                    .unwrap_or_else(|| Project {
                        path: path.clone(),
                        checkouts: vec![Checkout {
                            root: path.clone(),
                            count: 0,
                        }],
                    })
            })
            .collect()
    }
    fn selected_root(&self, mono: &Mono, projects: &[Project]) -> Option<String> {
        selected_root(self.selected.get(&mono.id).map(String::as_str), projects)
    }
}

fn selected_root(selected: Option<&str>, projects: &[Project]) -> Option<String> {
    selected
        .and_then(|selected| {
            projects
                .iter()
                .flat_map(|p| &p.checkouts)
                .find(|c| c.root == selected)
        })
        .or_else(|| projects.first().and_then(|p| p.checkouts.first()))
        .map(|c| c.root.clone())
}

/// Locate a file by walking directory metadata, without reading its contents or
/// diffing the repository. Linked worktrees point back through `commondir`.
fn repository(path: &Path) -> Option<(String, String)> {
    for folder in path.ancestors() {
        let marker = folder.join(".git");
        if marker.is_dir() {
            let root = folder.to_string_lossy().into_owned();
            return Some((root.clone(), root));
        }
        if marker.is_file() {
            let text = std::fs::read_to_string(&marker).ok()?;
            let gitdir = PathBuf::from(text.trim().strip_prefix("gitdir:")?.trim());
            let gitdir = if gitdir.is_absolute() {
                gitdir
            } else {
                folder.join(gitdir)
            };
            let main = std::fs::read_to_string(gitdir.join("commondir"))
                .ok()
                .and_then(|common| gitdir.join(common.trim()).canonicalize().ok())
                .filter(|common| common.file_name().is_some_and(|n| n == ".git"))
                .and_then(|common| common.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| folder.to_path_buf());
            return Some((
                folder.to_string_lossy().into_owned(),
                main.to_string_lossy().into_owned(),
            ));
        }
    }
    None
}

fn group_files(
    attached: &[String],
    files: &[monocode_core::checkpoint::CheckpointFile],
    previous: &[Project],
    mut locate: impl FnMut(&Path) -> Option<(String, String)>,
) -> Vec<Project> {
    let locations: Vec<_> = attached.iter().map(|p| locate(Path::new(p))).collect();
    let mut projects: Vec<_> = attached
        .iter()
        .map(|path| {
            let mut checkouts = previous
                .iter()
                .find(|p| &p.path == path)
                .map(|p| {
                    p.checkouts
                        .iter()
                        .map(|c| Checkout {
                            root: c.root.clone(),
                            count: 0,
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if !checkouts.iter().any(|c| &c.root == path) {
                checkouts.insert(
                    0,
                    Checkout {
                        root: path.clone(),
                        count: 0,
                    },
                );
            }
            Project {
                path: path.clone(),
                checkouts,
            }
        })
        .collect();
    let mut seen = HashSet::new();
    for file in files {
        if !seen.insert(&file.path) {
            continue;
        }
        let Some((root, main)) = locate(Path::new(&file.path)) else {
            continue;
        };
        // Prefer the most specific assigned folder, then its main repository.
        let index = attached
            .iter()
            .enumerate()
            .filter(|(_, path)| Path::new(&file.path).starts_with(Path::new(path)))
            .max_by_key(|(_, path)| path.len())
            .map(|(i, _)| i)
            .or_else(|| {
                locations.iter().position(|location| {
                    location
                        .as_ref()
                        .is_some_and(|(_, assigned_main)| assigned_main == &main)
                })
            });
        let Some(index) = index else {
            continue;
        };
        let project = &mut projects[index];
        let target = if Path::new(&file.path).starts_with(Path::new(&project.path)) {
            project.path.clone()
        } else {
            root
        };
        if let Some(checkout) = project.checkouts.iter_mut().find(|c| c.root == target) {
            checkout.count += 1;
        } else {
            project.checkouts.push(Checkout {
                root: target,
                count: 1,
            });
        }
    }
    projects
}

impl Monos {
    /// The badge reads one session recording, not the full indexes of all its
    /// assigned repositories. Coalesced, cached and always off the UI thread.
    pub(super) fn ensure_changes_summary(&mut self, mono: &Mono, cx: &mut Context<Self>) {
        let chat = self.chats.get(&mono.id).map(|v| v.read(cx));
        let source = Source {
            session: chat.as_ref().map(|v| v.summary.id).or(mono.session_id),
            cwd: chat
                .as_ref()
                .map(|v| {
                    v.settings
                        .worktree_cwd
                        .as_ref()
                        .unwrap_or(&v.summary.cwd)
                        .clone()
                })
                .unwrap_or_else(|| mono.projects.first().cloned().unwrap_or_default()),
            updated: chat.as_ref().map_or(0, |v| v.summary.updated_at),
            projects: mono.projects.clone(),
        };
        let summary = self
            .project_changes
            .summaries
            .entry(mono.id.clone())
            .or_default();
        if summary.source.as_ref() != Some(&source) {
            if summary
                .source
                .as_ref()
                .is_some_and(|old| old.session != source.session || old.cwd != source.cwd)
            {
                summary.projects.clear();
            } else {
                summary
                    .projects
                    .retain(|p| source.projects.contains(&p.path));
            }
            summary.source = Some(source.clone());
            summary.loaded = None;
            summary.revision += 1;
        }
        if summary.loading
            || summary
                .loaded
                .is_some_and(|at| at.elapsed() < Duration::from_secs(5))
        {
            return;
        }
        summary.loading = true;
        let revision = summary.revision;
        let previous = summary.projects.clone();
        let id = mono.id.clone();
        let work_source = source.clone();
        let database = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            let files = match (work_source.session, monocode_core::checkpoint::store()) {
                (Some(session), Some(store))
                    if !work_source.cwd.is_empty() && work_source.cwd != "~" =>
                {
                    let mut aliases = vec![session.to_string()];
                    if let Ok(persisted) =
                        locked(&database).and_then(|s| s.persisted_session_id(session))
                        && !aliases.contains(&persisted)
                    {
                        aliases.push(persisted);
                    }
                    store
                        .exclusive(|s| s.status_readonly_aliases(&aliases, &work_source.cwd))?
                        .files
                }
                _ => Vec::new(),
            };
            Ok::<_, String>(group_files(
                &work_source.projects,
                &files,
                &previous,
                repository,
            ))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                let Some(summary) = this.project_changes.summaries.get_mut(&id) else {
                    return;
                };
                summary.loading = false;
                if summary.revision != revision || summary.source.as_ref() != Some(&source) {
                    cx.notify();
                    return;
                }
                summary.loaded = Some(Instant::now());
                match result {
                    Ok(projects) => {
                        summary.projects = projects;
                        summary.error = None;
                    }
                    Err(error) => summary.error = Some(error),
                }
                this.changed(cx);
            });
        })
        .detach();
    }

    pub(super) fn pause_changes(&mut self, cx: &mut Context<Self>) {
        for surface in self.project_changes.surfaces.values_mut() {
            if surface.enabled {
                surface.enabled = false;
                surface.git.update(cx, |v, cx| v.set_polling(false, cx));
                surface.changes.update(cx, |v, cx| v.set_enabled(false, cx));
            }
        }
    }

    pub(super) fn ensure_changes(
        &mut self,
        mono: &Mono,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let projects = self.project_changes.projects(mono);
        let Some(root) = self.project_changes.selected_root(mono, &projects) else {
            self.pause_changes(cx);
            return;
        };
        self.project_changes
            .selected
            .insert(mono.id.clone(), root.clone());
        let key = (mono.id.clone(), root.clone());
        for (other, surface) in &mut self.project_changes.surfaces {
            if other != &key && surface.enabled {
                surface.enabled = false;
                surface.git.update(cx, |v, cx| v.set_polling(false, cx));
                surface.changes.update(cx, |v, cx| v.set_enabled(false, cx));
            }
        }
        if !self.project_changes.surfaces.contains_key(&key) {
            let git = cx.new(|cx| crate::git_status::ProjectGit::new(root, cx));
            let provider = self
                .chat()
                .map(|v| Kind::from_id(&v.read(cx).summary.provider))
                .unwrap_or(Kind::Codex);
            let changes = cx.new(|cx| {
                let mut view = crate::changes::Changes::new(git.clone(), window, cx);
                view.text_provider = provider;
                view
            });
            let event_key = key.clone();
            let changes_event = cx.subscribe(
                &changes,
                move |this, view, event: &crate::changes::ChangesEvent, cx| match event {
                    crate::changes::ChangesEvent::View(mode) => {
                        view.update(cx, |v, cx| v.set_view(*mode, cx))
                    }
                    crate::changes::ChangesEvent::OpenDiff { cwd, path, staged } => {
                        this.open_change_review(&event_key, cwd, path.as_deref(), *staged, cx)
                    }
                },
            );
            let summary_id = mono.id.clone();
            let git_event = cx.subscribe(
                &git,
                move |this, _, _: &crate::git_status::GitChanged, cx| {
                    if let Some(summary) = this.project_changes.summaries.get_mut(&summary_id) {
                        summary.loaded = None;
                        summary.revision += 1;
                    }
                    if let Some(mono) = this.roster.get(&summary_id).cloned() {
                        this.ensure_changes_summary(&mono, cx);
                    }
                    cx.notify();
                },
            );
            self.project_changes.surfaces.insert(
                key.clone(),
                Surface {
                    git,
                    changes,
                    review: None,
                    enabled: false,
                    _subscriptions: vec![changes_event, git_event],
                },
            );
        }
        let surface = self.project_changes.surfaces.get_mut(&key).unwrap();
        if !surface.enabled {
            surface.enabled = true;
            surface.changes.update(cx, |v, cx| v.set_enabled(true, cx));
            surface.git.update(cx, |v, cx| v.set_polling(true, cx));
        }
    }

    fn open_change_review(
        &mut self,
        key: &(String, String),
        cwd: &str,
        path: Option<&str>,
        staged: bool,
        cx: &mut Context<Self>,
    ) {
        // A retained surface never follows another Mono's current selection.
        if self.active.as_ref() != Some(&key.0)
            || self.project_changes.selected.get(&key.0) != Some(&key.1)
        {
            return;
        }
        let Some(surface) = self.project_changes.surfaces.get(key) else {
            return;
        };
        let review = surface.review.clone().unwrap_or_else(|| {
            cx.new(|cx| {
                crate::review::ReviewPane::new(
                    surface.git.clone(),
                    crate::review::ReviewState::default(),
                    cx,
                )
            })
        });
        if surface.review.is_none() {
            let review_key = key.clone();
            let event = cx.subscribe(
                &review,
                move |this, _, event: &crate::review::ReviewEvent, cx| {
                    match event {
                        crate::review::ReviewEvent::Empty => {
                            if let Some(surface) =
                                this.project_changes.surfaces.get_mut(&review_key)
                            {
                                surface.review = None;
                            }
                            if this.active.as_ref() == Some(&review_key.0)
                                && this.project_changes.selected.get(&review_key.0)
                                    == Some(&review_key.1)
                            {
                                this.review = None;
                                this.route = None;
                            }
                        }
                        crate::review::ReviewEvent::Comment(text) => {
                            this.enqueue(&review_key.0, text.clone(), Vec::new(), false, cx)
                        }
                        _ => (),
                    }
                    cx.notify();
                },
            );
            self.project_changes
                .surfaces
                .get_mut(key)
                .unwrap()
                ._subscriptions
                .push(event);
        }
        review.update(cx, |view, cx| {
            view.open(
                crate::review::ReviewTab::Changes {
                    cwd: cwd.into(),
                    path: path.map(str::to_owned),
                    staged: Some(staged),
                },
                cx,
            )
        });
        self.project_changes.surfaces.get_mut(key).unwrap().review = Some(review.clone());
        self.review = Some(review);
        self.route = Some(Route::Review);
        cx.notify();
    }

    fn select_change_project(
        &mut self,
        mono: &str,
        project: &str,
        root: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(m) = self.roster.get(mono).cloned() else {
            return;
        };
        let projects = self.project_changes.projects(&m);
        if !projects
            .iter()
            .any(|p| p.path == project && p.checkouts.iter().any(|c| c.root == root))
        {
            return;
        }
        self.project_changes
            .selected
            .insert(mono.into(), root.into());
        self.project_changes
            .last_roots
            .insert((mono.into(), project.into()), root.into());
        self.ensure_changes(&m, window, cx);
        cx.notify();
    }

    fn pick_change_checkout(
        &mut self,
        mono: String,
        project: String,
        checkouts: Vec<Checkout>,
        anchor: AnchorSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selected = self.project_changes.selected.get(&mono);
        let items: Vec<MenuItem> = checkouts
            .iter()
            .enumerate()
            .map(|(ix, checkout)| {
                MenuAction::new(
                    format!("checkout:{ix}"),
                    Path::new(&checkout.root).file_name().map_or_else(
                        || checkout.root.clone(),
                        |n| n.to_string_lossy().into_owned(),
                    ),
                )
                .description(checkout.root.clone())
                .checked(selected == Some(&checkout.root))
                .badge(if checkout.count > 0 {
                    checkout.count.to_string()
                } else {
                    String::new()
                })
                .into()
            })
            .collect();
        let menu = cx.new(|cx| Menu::new("Working copies", items, window, cx).width(280.));
        let event = cx.subscribe(&menu, move |this, _, event: &MenuEvent, cx| {
            this.picker = None;
            if let MenuEvent::Picked(action, _) = event
                && let Some(checkout) = action
                    .strip_prefix("checkout:")
                    .and_then(|n| n.parse::<usize>().ok())
                    .and_then(|i| checkouts.get(i))
            {
                // Entity creation needs a Window; defer selection to the next
                // overview render, which also activates exactly one poller.
                if this
                    .roster
                    .get(&mono)
                    .is_some_and(|m| m.projects.contains(&project))
                {
                    this.project_changes
                        .selected
                        .insert(mono.clone(), checkout.root.clone());
                    this.project_changes
                        .last_roots
                        .insert((mono.clone(), project.clone()), checkout.root.clone());
                    this.pause_changes(cx);
                }
            }
            cx.notify();
        });
        self.picker = Some(panels::OpenPicker {
            menu,
            anchor: Anchor::Slot(anchor),
            width: 280.,
            align: crate::popover::Align::Start,
            _events: vec![event],
        });
        cx.notify();
    }

    pub(super) fn changes_list(&self, mono: &Mono, cx: &mut Context<Self>) -> AnyElement {
        let projects = self.project_changes.projects(mono);
        if projects.is_empty() {
            return div()
                .px_4()
                .py_4()
                .text_size(px(12.))
                .text_color(content(0.45))
                .child(format!(
                    "Give {} a project to work on, and its changes show up here to commit.",
                    mono.label()
                ))
                .into_any_element();
        }
        let selected = self.project_changes.selected_root(mono, &projects);
        let tabs = div()
            .id("mono-changes-projects")
            .debug_selector(|| "mono-changes-projects".into())
            .role(Role::TabList)
            .aria_label("Projects")
            .h(px(36.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(1.))
            .px_2()
            .overflow_x_scroll()
            .border_b_1()
            .border_color(content(0.08))
            .children(projects.into_iter().enumerate().map(|(ix, project)| {
                let active = project
                    .checkouts
                    .iter()
                    .any(|c| selected.as_ref() == Some(&c.root));
                let appearance = self
                    .owner
                    .upgrade()
                    .and_then(|o| {
                        o.read(cx)
                            .workspace
                            .project_appearance
                            .get(&project.path)
                            .cloned()
                    })
                    .unwrap_or_default();
                let label = appearance.label(&project.path);
                let count = project.count();
                let remembered = self
                    .project_changes
                    .last_roots
                    .get(&(mono.id.clone(), project.path.clone()));
                let target = project
                    .checkouts
                    .iter()
                    .find(|c| {
                        if active {
                            selected.as_ref() == Some(&c.root)
                        } else {
                            remembered == Some(&c.root)
                        }
                    })
                    .unwrap_or(&project.checkouts[0])
                    .root
                    .clone();
                let mono_id = mono.id.clone();
                let path = project.path.clone();
                let title = format!(
                    "{label}, {count} pending changes{}",
                    if active { ", selected" } else { "" }
                );
                let checkouts = project.checkouts.clone();
                let picker_mono = mono.id.clone();
                let picker_project = project.path.clone();
                let anchor = AnchorSlot::default();
                div()
                    .h(px(24.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .rounded_md()
                    .when(active, |d| d.bg(content(0.08)))
                    .text_size(px(12.))
                    .text_color(content(if active { 1. } else { 0.5 }))
                    .child(
                        div()
                            .id(("mono-changes-project", ix))
                            .debug_selector(move || format!("mono-changes-project-{ix}"))
                            .role(Role::Tab)
                            .aria_selected(active)
                            .aria_label(title)
                            .cursor_pointer()
                            .h_full()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .px_2()
                            .rounded_md()
                            .hover(|s| s.bg(content(0.05)))
                            .child(crate::project_settings::project_icon(
                                &appearance,
                                &project.path,
                                14.,
                            ))
                            .child(div().max_w(px(160.)).truncate().child(label))
                            .when(count > 0, |d| {
                                d.child(div().text_color(content(0.4)).child(count.to_string()))
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_change_project(&mono_id, &path, &target, window, cx)
                            })),
                    )
                    .when(project.checkouts.len() > 1, |d| {
                        d.child(
                            div()
                                .id(("mono-changes-checkouts", ix))
                                .role(Role::Button)
                                .aria_label("Switch project working copy")
                                .size(px(18.))
                                .mr_1()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_sm()
                                .hover(|s| s.bg(content(0.08)))
                                .child(anchor.probe())
                                .child(icon("chevron-down", 12.))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.pick_change_checkout(
                                        picker_mono.clone(),
                                        picker_project.clone(),
                                        checkouts.clone(),
                                        anchor.clone(),
                                        window,
                                        cx,
                                    )
                                })),
                        )
                    })
            }));
        div()
            .id("mono-changes")
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(tabs)
            .when_some(
                selected
                    .and_then(|root| self.project_changes.surfaces.get(&(mono.id.clone(), root))),
                |d, surface| d.child(div().flex_1().min_h_0().child(surface.changes.clone())),
            )
            .when_some(
                self.project_changes
                    .summaries
                    .get(&mono.id)
                    .and_then(|s| s.error.clone()),
                |d, error| {
                    d.child(
                        div()
                            .px_3()
                            .py_2()
                            .text_size(px(12.))
                            .text_color(rgb(0xff6467))
                            .child(format!("Couldn’t load repositories: {error}")),
                    )
                },
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn file(path: &str) -> monocode_core::checkpoint::CheckpointFile {
        monocode_core::checkpoint::CheckpointFile {
            path: path.into(),
            relative: path.into(),
            status: "modified".into(),
            additions: 1,
            deletions: 0,
            exact: true,
            undoable: true,
        }
    }
    fn locate(path: &Path) -> Option<(String, String)> {
        for (root, main) in [("/a-wt", "/a"), ("/a", "/a"), ("/ab", "/ab"), ("/b", "/b")] {
            if path.starts_with(root) {
                return Some((root.into(), main.into()));
            }
        }
        None
    }
    #[test]
    fn all_assignments_have_tabs_and_recorded_worktrees_stay_with_their_project() {
        let assigned = vec!["/a".into(), "/b".into(), "/empty".into()];
        let grouped = group_files(
            &assigned,
            &[
                file("/a/one"),
                file("/a-wt/two"),
                file("/a-wt/two"),
                file("/b/one"),
                file("/ab/outside"),
            ],
            &[],
            locate,
        );
        assert_eq!(
            grouped.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(),
            ["/a", "/b", "/empty"]
        );
        assert_eq!(
            grouped.iter().map(Project::count).collect::<Vec<_>>(),
            [2, 1, 0]
        );
        assert_eq!(
            grouped[0]
                .checkouts
                .iter()
                .map(|c| c.root.as_str())
                .collect::<Vec<_>>(),
            ["/a", "/a-wt"]
        );
        let committed = group_files(&assigned, &[], &grouped, locate);
        assert_eq!(
            committed[0].checkouts.len(),
            2,
            "checkout survives commit so it can still be pushed"
        );
        assert_eq!(committed.iter().map(Project::count).sum::<usize>(), 0);
    }
    #[test]
    fn selection_survives_refresh_and_falls_back_when_assignment_is_removed() {
        let projects = group_files(
            &["/a".into(), "/b".into()],
            &[file("/a-wt/one")],
            &[],
            locate,
        );
        assert_eq!(
            selected_root(Some("/a-wt"), &projects).as_deref(),
            Some("/a-wt")
        );
        assert_eq!(
            selected_root(Some("/removed"), &projects).as_deref(),
            Some("/a")
        );
        assert_eq!(
            selected_root(Some("/a-wt"), &projects[1..]).as_deref(),
            Some("/b")
        );
        assert_eq!(selected_root(None, &[]), None);
    }
    #[test]
    fn a_monos_selection_and_badge_do_not_leak_to_another_mono() {
        let mut state = ProjectChanges::default();
        let a = Mono {
            id: "first".into(),
            projects: vec!["/a".into(), "/b".into()],
            ..Default::default()
        };
        let b = Mono {
            id: "second".into(),
            projects: a.projects.clone(),
            ..Default::default()
        };
        state.selected.insert(a.id.clone(), "/b".into());
        state.summaries.insert(
            a.id.clone(),
            Summary {
                projects: group_files(&a.projects, &[file("/a/one")], &[], locate),
                ..Default::default()
            },
        );
        assert_eq!(
            state.selected_root(&a, &state.projects(&a)).as_deref(),
            Some("/b")
        );
        assert_eq!(
            state.selected_root(&b, &state.projects(&b)).as_deref(),
            Some("/a")
        );
        assert_eq!((state.count(&a.id), state.count(&b.id)), (1, 0));
    }
}
