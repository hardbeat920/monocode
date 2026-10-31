//! The session menu's GitHub link picker: the session repository's issues and
//! pull requests, searchable by title or number, or any pasted github.com URL.
use crate::{input, popover, theme::*};
use gpui::{
    AnyElement, Context, Entity, EventEmitter, FocusHandle, Focusable, Hsla, ListAlignment,
    ListState, Role, Subscription, Task, Window, div, list, prelude::*, px, rgb,
};
use monocode_core::{
    scm::{self, GithubWorkItem},
    session_groups::{WorkItem, parse_github_work_item},
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

const SEARCH_DELAY: Duration = Duration::from_millis(250);
const ROW_HEIGHT: f32 = 36.;

type FetchWorkItems = fn(&Path, &str, &str) -> Result<Vec<GithubWorkItem>, String>;

pub(crate) enum Picked {
    Item(WorkItem),
    Remove,
    Dismissed,
}
#[derive(Clone)]
enum Choice {
    Found(GithubWorkItem),
    Url(WorkItem),
}
pub(crate) struct WorkItemPicker {
    cwd: PathBuf,
    current: Option<WorkItem>,
    /// None while `gh` resolves the session folder's repository.
    repo: Option<Result<String, String>>,
    recent: Vec<GithubWorkItem>,
    /// Results for the query they were fetched for.
    found: Option<(String, Vec<GithubWorkItem>)>,
    searching: bool,
    error: Option<String>,
    filtered: Vec<Choice>,
    list: ListState,
    search: Entity<input::Input>,
    active: usize,
    lookup: Option<Task<()>>,
    /// Bumped whenever a lookup starts or is cancelled; a response applies
    /// only while its id is still current.
    lookup_id: u64,
    fetch: FetchWorkItems,
    _changed: Subscription,
}
impl EventEmitter<Picked> for WorkItemPicker {}
impl WorkItemPicker {
    pub(crate) fn new(cwd: PathBuf, current: Option<WorkItem>, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            input::Input::field(
                "Search issues and pull requests",
                "Search, #number or paste a URL…",
                content(0.4),
                cx,
            )
        });
        let changed = cx.subscribe(&search, |this, _, _: &input::Changed, cx| {
            this.active = 0;
            this.schedule_lookup(cx);
            this.refresh_choices(cx);
        });
        let load_cwd = cwd.clone();
        let work = cx.background_executor().spawn(async move {
            if !load_cwd.is_dir() {
                return (
                    Err("This session's folder isn't available.".to_string()),
                    Ok(Vec::new()),
                );
            }
            let repo = scm::github_repo(&load_cwd);
            let recent = match &repo {
                Ok(repo) => scm::github_work_items(&load_cwd, repo, ""),
                Err(_) => Ok(Vec::new()),
            };
            (repo, recent)
        });
        cx.spawn(async move |this, cx| {
            let (repo, recent) = work.await;
            let _ = this.update(cx, |this, cx| {
                match recent {
                    Ok(recent) => this.recent = recent,
                    Err(error) => this.error = Some(error),
                }
                this.repo = Some(repo);
                this.schedule_lookup(cx);
                this.refresh_choices(cx);
            });
        })
        .detach();
        let mut this = Self {
            cwd,
            current,
            repo: None,
            recent: Vec::new(),
            found: None,
            searching: false,
            error: None,
            filtered: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, px(200.)),
            search,
            active: 0,
            lookup: None,
            lookup_id: 0,
            fetch: scm::github_work_items,
            _changed: changed,
        };
        this.refresh_choices(cx);
        this
    }
    pub(crate) fn focus(&self, cx: &gpui::App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
    #[cfg(test)]
    pub(crate) fn search_for(&mut self, text: &str, cx: &mut Context<Self>) {
        self.search.update(cx, |i, cx| i.set_text(text.into(), cx));
        self.refresh_choices(cx);
    }
    fn query(&self, cx: &gpui::App) -> String {
        self.search.read(cx).content.trim().to_owned()
    }
    /// Asks GitHub for the typed number or title once typing pauses; the
    /// recent list filters locally meanwhile.
    fn schedule_lookup(&mut self, cx: &mut Context<Self>) {
        let query = self.query(cx);
        let Some(Ok(repo)) = self.repo.clone() else {
            return;
        };
        if query.is_empty()
            || parse_github_work_item(&query).is_some()
            || self.found.as_ref().is_some_and(|(q, _)| *q == query)
        {
            self.cancel_lookup();
            return;
        }
        self.lookup_id += 1;
        let id = self.lookup_id;
        self.searching = true;
        let cwd = self.cwd.clone();
        let fetch = self.fetch;
        self.lookup = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DELAY).await;
            let lookup_query = query.clone();
            let result = cx
                .background_executor()
                .spawn(async move { fetch(&cwd, &repo, &lookup_query) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.lookup_id != id {
                    return;
                }
                this.searching = false;
                match result {
                    Ok(items) => {
                        this.error = None;
                        this.found = Some((query, items));
                    }
                    Err(error) => this.error = Some(error),
                }
                this.refresh_choices(cx);
            });
        }));
    }
    fn cancel_lookup(&mut self) {
        self.lookup = None;
        self.lookup_id += 1;
        self.searching = false;
    }
    fn refresh_choices(&mut self, cx: &mut Context<Self>) {
        self.filtered = self.choices(cx);
        self.active = self.active.min(self.filtered.len().saturating_sub(1));
        self.list
            .reset_with_uniform_height(self.filtered.len(), px(ROW_HEIGHT));
        cx.notify();
    }
    fn choices(&self, cx: &gpui::App) -> Vec<Choice> {
        let query = self.query(cx);
        if let Some(item) = parse_github_work_item(&query) {
            return vec![Choice::Url(item)];
        }
        let needle = query.to_lowercase();
        let digits = needle.strip_prefix('#').unwrap_or(&needle);
        let numeric = !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit());
        let mut choices: Vec<GithubWorkItem> = self
            .recent
            .iter()
            .filter(|found| {
                if numeric {
                    found.item.number.to_string().starts_with(digits)
                } else {
                    found.title.to_lowercase().contains(&needle)
                }
            })
            .cloned()
            .collect();
        if let Some((_, items)) = self.found.as_ref().filter(|(q, _)| *q == query) {
            for found in items.iter().rev() {
                choices.retain(|c| c.item.url != found.item.url);
                // An exact number goes first; title matches follow the
                // locally filtered ones.
                if numeric {
                    choices.insert(0, found.clone());
                } else {
                    choices.push(found.clone());
                }
            }
        }
        choices.into_iter().map(Choice::Found).collect()
    }
    fn row(&self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let (item, title, state) = match &self.filtered[ix] {
            Choice::Found(found) => (
                found.item.clone(),
                found.title.clone(),
                Some(found.state.as_str()),
            ),
            Choice::Url(item) => (item.clone(), format!("Link {}", item.url), None),
        };
        let selected = self.current.as_ref().is_some_and(|c| c.url == item.url);
        let (glyph, color): (&str, Hsla) = match (item.kind.as_str(), state) {
            ("pr", Some("merged")) => ("git-merge", rgb(0xa78bfa).into()),
            ("pr", Some("closed")) => ("git-pull-request-closed", rgb(0xf87171).into()),
            ("pr", Some("draft")) => ("git-pull-request-draft", content(0.4)),
            ("pr", Some(_)) => ("git-pull-request", rgb(0x4ade80).into()),
            ("issue", Some("closed")) => ("check-circle", rgb(0xa78bfa).into()),
            ("issue", Some(_)) => ("circle-dot", rgb(0x4ade80).into()),
            _ => ("external-link", content(0.5)),
        };
        let label = format!(
            "{} #{} {title}",
            if item.kind == "pr" {
                "Pull request"
            } else {
                "Issue"
            },
            item.number
        );
        div()
            .id(("work-item-option", ix))
            .debug_selector(move || format!("work-item-option-{ix}"))
            .role(Role::ListBoxOption)
            .aria_label(label)
            .aria_selected(selected)
            .w_full()
            .h(px(ROW_HEIGHT))
            .flex_shrink_0()
            .px(px(8.))
            .rounded(px(6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .text_size(px(12.))
            .text_color(content(0.9))
            .when(ix == self.active, |d| d.bg(content(0.08)))
            .child(icon(glyph, 14.).text_color(color))
            .when(state.is_some(), |d| {
                d.child(
                    div()
                        .flex_shrink_0()
                        .text_color(content(0.45))
                        .child(format!("#{}", item.number)),
                )
            })
            .child(div().min_w_0().flex_1().truncate().child(title))
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
    pub(crate) fn pick(&mut self, ix: usize, cx: &mut Context<Self>) {
        match self.filtered.get(ix) {
            Some(Choice::Found(found)) => cx.emit(Picked::Item(found.item.clone())),
            Some(Choice::Url(item)) => cx.emit(Picked::Item(item.clone())),
            None => {}
        }
    }
    fn empty_label(&self, cx: &gpui::App) -> &'static str {
        let query = self.query(cx);
        match &self.repo {
            None => "Loading issues and pull requests…",
            Some(Err(_)) if query.is_empty() => "Paste a github.com issue or pull request URL.",
            Some(Err(_)) => "Not a GitHub issue or pull request URL.",
            Some(Ok(_)) if self.searching => "Searching…",
            Some(Ok(_)) if query.is_empty() => "No open issues or pull requests.",
            Some(Ok(_)) => "No matches.",
        }
    }
}
impl gpui::Render for WorkItemPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let empty = self.filtered.is_empty();
        let rows = self.filtered.len().max(1) as f32;
        let hint = match &self.repo {
            Some(Ok(repo)) => format!("{repo} · search by title or #number, or paste a URL"),
            Some(Err(_)) => "No GitHub repository for this session's folder".into(),
            None => "Search by title or #number, or paste a URL".into(),
        };
        div()
            .id("work-item-picker")
            .debug_selector(|| "work-item-picker".into())
            .role(Role::Dialog)
            .aria_label(if self.current.is_some() {
                "Edit GitHub link"
            } else {
                "Link GitHub issue or PR"
            })
            .key_context("WorkItemPicker")
            .w_full()
            .flex()
            .flex_col()
            .overflow_hidden()
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
                    .h(px(40.))
                    .px(px(12.))
                    .border_b_1()
                    .border_color(content(0.07))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(icon("search", 14.).text_color(content(0.4)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .child(self.search.clone()),
                    )
                    .when(self.searching || self.repo.is_none(), |d| {
                        d.child(icon("loader", 13.).text_color(content(0.4)))
                    }),
            )
            .child(
                div()
                    .id("work-item-choices")
                    .role(Role::ListBox)
                    .aria_label("Issues and pull requests")
                    .h(px((rows * ROW_HEIGHT).min(ROW_HEIGHT * 8.) + 8.))
                    .p(px(4.))
                    .flex()
                    .flex_col()
                    .when(!empty, |d| {
                        d.child(
                            list(self.list.clone(), move |ix, _, cx| {
                                entity.update(cx, |view, cx| view.row(ix, cx))
                            })
                            .w_full()
                            .flex_1()
                            .min_h_0(),
                        )
                    })
                    .when(empty, |d| {
                        d.child(
                            div()
                                .px(px(8.))
                                .h(px(ROW_HEIGHT))
                                .flex()
                                .items_center()
                                .text_size(px(12.))
                                .text_color(content(0.45))
                                .child(self.empty_label(cx)),
                        )
                    }),
            )
            .when_some(self.error.clone(), |d, error| {
                d.child(
                    div()
                        .id("work-item-error")
                        .role(Role::Alert)
                        .px(px(12.))
                        .pb(px(6.))
                        .text_size(px(11.))
                        .text_color(rgb(0xf87171))
                        .truncate()
                        .child(error),
                )
            })
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(12.))
                    .py(px(8.))
                    .border_t_1()
                    .border_color(content(0.07))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(11.))
                    .text_color(content(0.4))
                    .child(div().min_w_0().flex_1().truncate().child(hint))
                    .when(self.current.is_some(), |d| {
                        d.child(
                            div()
                                .id("work-item-remove")
                                .debug_selector(|| "work-item-remove".into())
                                .role(Role::Button)
                                .cursor_pointer()
                                .flex_shrink_0()
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .text_color(rgb(0xf87171))
                                .hover(|s| s.bg(gpui::rgba(0xf871711a)))
                                .child("Remove link")
                                .on_click(cx.listener(|_, _, _, cx| cx.emit(Picked::Remove))),
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn found(kind: &str, number: u64, title: &str) -> GithubWorkItem {
        GithubWorkItem {
            item: WorkItem {
                kind: kind.into(),
                repo: "acme/app".into(),
                number,
                url: format!(
                    "https://github.com/acme/app/{}/{number}",
                    if kind == "pr" { "pull" } else { "issues" }
                ),
            },
            title: title.into(),
            state: "open".into(),
        }
    }

    #[gpui::test]
    fn filters_recent_items_by_title_or_number_and_accepts_urls(cx: &mut TestAppContext) {
        let picker = cx.new(|cx| WorkItemPicker::new("/nonexistent/monocode".into(), None, cx));
        cx.run_until_parked();
        let numbers = |cx: &mut TestAppContext, query: &str| {
            picker.update(cx, |view, cx| {
                view.recent = vec![
                    found("pr", 42, "Fix sidebar links"),
                    found("issue", 7, "Links are not clickable"),
                    found("issue", 420, "Crash on launch"),
                ];
                view.search.update(cx, |i, cx| i.set_text(query.into(), cx));
                view.choices(cx)
                    .into_iter()
                    .map(|c| match c {
                        Choice::Found(f) => f.item.number,
                        Choice::Url(item) => item.number + 1_000,
                    })
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(numbers(cx, ""), [42, 7, 420]);
        assert_eq!(numbers(cx, "links"), [42, 7]);
        assert_eq!(numbers(cx, "#42"), [42, 420]);
        assert_eq!(numbers(cx, "https://github.com/other/repo/pull/5"), [1_005]);
        picker.update(cx, |view, _| {
            assert!(
                matches!(view.repo, Some(Err(_))),
                "no folder means no gh lookup"
            );
        });
    }

    fn fetch_by_query(_: &Path, _: &str, query: &str) -> Result<Vec<GithubWorkItem>, String> {
        let number = if query == "alpha" { 100 } else { 200 };
        Ok(vec![found("issue", number, query)])
    }

    #[gpui::test]
    fn returning_to_a_cached_search_cancels_the_pending_one(cx: &mut TestAppContext) {
        let picker = cx.new(|cx| WorkItemPicker::new("/nonexistent/monocode".into(), None, cx));
        cx.run_until_parked();
        picker.update(cx, |view, _| {
            view.repo = Some(Ok("acme/app".into()));
            view.fetch = fetch_by_query;
        });
        let numbers = |cx: &mut TestAppContext| {
            picker.update(cx, |view, _| {
                view.filtered
                    .iter()
                    .map(|c| match c {
                        Choice::Found(f) => f.item.number,
                        Choice::Url(item) => item.number,
                    })
                    .collect::<Vec<_>>()
            })
        };

        picker.update(cx, |view, cx| view.search_for("alpha", cx));
        cx.executor().advance_clock(SEARCH_DELAY);
        cx.run_until_parked();
        assert_eq!(numbers(cx), [100]);

        picker.update(cx, |view, cx| view.search_for("beta", cx));
        cx.run_until_parked();
        picker.update(cx, |view, cx| {
            assert!(view.searching);
            view.search_for("alpha", cx);
        });
        cx.run_until_parked();
        picker.update(cx, |view, _| {
            assert!(!view.searching, "the cached query isn't searching");
            assert!(view.lookup.is_none(), "the beta lookup was cancelled");
        });

        cx.executor().advance_clock(SEARCH_DELAY * 2);
        cx.run_until_parked();
        assert_eq!(numbers(cx), [100]);
        picker.update(cx, |view, _| {
            assert_eq!(view.found.as_ref().map(|(q, _)| q.as_str()), Some("alpha"));
        });
    }
}
