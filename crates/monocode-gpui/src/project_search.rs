//! ProjectSearch.tsx: content search inside the Explorer, scoped to its checkout.
use crate::{
    file_icons::file_type_icon,
    input::{self, Input},
    right_panel::{RED_400, spinner, stroke, tabular},
    theme::*,
};
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, HighlightStyle,
    ListAlignment, ListState, Role, StyledText, Subscription, Task, Window, div, list, prelude::*,
    px, rgb,
};
use monocode_core::search::project::{self, SearchMatch, SearchOptions};
use std::{
    collections::HashMap,
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

fn accent(alpha: f32) -> gpui::Hsla {
    let mut color = accent_color();
    color.a = alpha;
    color
}

pub(crate) enum SearchEvent {
    Open {
        cwd: String,
        matched: SearchMatch,
        pin: bool,
    },
    Close,
}

struct Match {
    source: SearchMatch,
    highlight: Option<Range<usize>>,
}

struct Group {
    relative: String,
    name: String,
    matches: Vec<Match>,
}

pub(crate) struct ProjectSearch {
    cwd: String,
    active: bool,
    query: Entity<Input>,
    include: Entity<Input>,
    exclude: Entity<Input>,
    case_sensitive: bool,
    whole_word: bool,
    regex: bool,
    loading: bool,
    error: Option<String>,
    groups: Vec<Group>,
    count: usize,
    truncated: bool,
    cancel: Arc<AtomicBool>,
    debounce: Option<Task<()>>,
    rows: Vec<(usize, Option<usize>)>,
    list: ListState,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SearchEvent> for ProjectSearch {}

impl Focusable for ProjectSearch {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.query.focus_handle(cx)
    }
}

impl Drop for ProjectSearch {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

impl ProjectSearch {
    pub(crate) fn new(cwd: String, active: bool, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| {
            let mut input = Input::field("Search", "Search", content(0.35), cx);
            input.set_font_size(12.);
            input
        });
        let include = cx.new(|cx| {
            let mut input = Input::field("files to include", "files to include", content(0.35), cx);
            input.set_font_size(11.);
            input
        });
        let exclude = cx.new(|cx| {
            let mut input = Input::field("files to exclude", "files to exclude", content(0.35), cx);
            input.set_font_size(11.);
            input
        });
        let subscriptions = [&query, &include, &exclude]
            .into_iter()
            .map(|input| cx.subscribe(input, |this, _, _: &input::Changed, cx| this.refresh(cx)))
            .collect();
        Self {
            cwd,
            active,
            query,
            include,
            exclude,
            case_sensitive: false,
            whole_word: false,
            regex: false,
            loading: false,
            error: None,
            groups: Vec::new(),
            count: 0,
            truncated: false,
            cancel: Arc::new(AtomicBool::new(false)),
            debounce: None,
            rows: Vec::new(),
            list: ListState::new(0, ListAlignment::Top, px(200.)),
            _subscriptions: subscriptions,
        }
    }

    pub(crate) fn focus(&self, window: &mut Window, cx: &mut App) {
        self.query.update(cx, |input, cx| {
            let length = input.content.len();
            input.select_range(0..length, cx);
        });
        window.focus(&self.query.focus_handle(cx), cx);
    }

    pub(crate) fn is_composing(&self, cx: &App) -> bool {
        [&self.query, &self.include, &self.exclude]
            .into_iter()
            .any(|input| input.read(cx).is_composing())
    }

    pub(crate) fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active != active {
            self.active = active;
            self.refresh(cx);
        }
    }

    pub(crate) fn set_root(&mut self, cwd: String, cx: &mut Context<Self>) {
        if self.cwd != cwd {
            self.cwd = cwd;
            self.groups.clear();
            self.rows.clear();
            self.list.reset(0);
            self.count = 0;
            self.refresh(cx);
        }
    }

    fn options(&self, cx: &App) -> SearchOptions {
        let filter = |input: &Entity<Input>| {
            let text = input.read(cx).content.trim();
            (!text.is_empty()).then(|| text.to_owned())
        };
        SearchOptions {
            cwd: self.cwd.clone(),
            query: self.query.read(cx).content.trim().to_owned(),
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
            regex: self.regex,
            include: filter(&self.include),
            exclude: filter(&self.exclude),
            search_id: String::new(),
        }
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.cancel.store(true, Ordering::Release);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.debounce = None;
        self.loading = false;
        self.error = None;
        self.truncated = false;
        let options = self.options(cx);
        if options.query.is_empty() || self.cwd.is_empty() || self.cwd == "~" {
            self.groups.clear();
            self.count = 0;
            self.rows.clear();
            self.list.reset(0);
            cx.notify();
            return;
        }
        if self.active {
            let check = self.cancel.clone();
            self.debounce = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(200))
                    .await;
                if !check.load(Ordering::Acquire) {
                    let _ = this.update(cx, |this, cx| this.start(options, cx));
                }
            }));
        }
        cx.notify();
    }

    fn start(&mut self, options: SearchOptions, cx: &mut Context<Self>) {
        self.loading = true;
        let flag = self.cancel.clone();
        let check = flag.clone();
        let work = cx.background_executor().spawn(async move {
            let result =
                project::search_project(std::path::Path::new(&options.cwd), &options, &flag)?;
            let ranges = project::preview_ranges(&result.matches, &options);
            let mut groups: Vec<Group> = Vec::new();
            let mut by_path = HashMap::new();
            let count = result.matches.len();
            for (source, highlight) in result.matches.into_iter().zip(ranges) {
                let index = *by_path.entry(source.path.clone()).or_insert_with(|| {
                    let index = groups.len();
                    groups.push(Group {
                        name: source
                            .relative
                            .rsplit('/')
                            .next()
                            .unwrap_or(&source.relative)
                            .to_owned(),
                        relative: source.relative.clone(),
                        matches: Vec::new(),
                    });
                    index
                });
                groups[index].matches.push(Match { source, highlight });
            }
            let rows: Vec<_> = groups
                .iter()
                .enumerate()
                .flat_map(|(group, data)| {
                    std::iter::once((group, None))
                        .chain((0..data.matches.len()).map(move |i| (group, Some(i))))
                })
                .collect();
            Ok::<_, String>((groups, count, result.truncated, rows))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            if check.load(Ordering::Acquire) {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok((groups, count, truncated, rows)) => {
                        this.groups = groups;
                        this.rows = rows;
                        this.list
                            .reset_with_uniform_height(this.rows.len(), px(28.));
                        this.count = count;
                        this.truncated = truncated;
                    }
                    Err(error) => {
                        this.groups.clear();
                        this.rows.clear();
                        this.list.reset(0);
                        this.count = 0;
                        this.error = Some(error);
                    }
                }
                this.list.scroll_to(gpui::ListOffset {
                    item_ix: 0,
                    offset_in_item: px(0.),
                });
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn pick(&self, group: usize, index: usize, pin: bool, cx: &mut Context<Self>) {
        if let Some(matched) = self
            .groups
            .get(group)
            .and_then(|group| group.matches.get(index))
        {
            cx.emit(SearchEvent::Open {
                cwd: self.cwd.clone(),
                matched: matched.source.clone(),
                pin,
            });
        }
    }

    fn row(&self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let (group_index, matched) = self.rows[row];
        let group = &self.groups[group_index];
        let Some(index) = matched else {
            return div()
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .child(
                    div()
                        .flex()
                        .min_w_0()
                        .items_center()
                        .gap(px(6.))
                        .px_2()
                        .py(px(6.))
                        .child(file_type_icon(&group.name, false, false, 16.))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .text_size(px(12.))
                                .child(group.name.clone()),
                        )
                        .child(
                            div()
                                .rounded_full()
                                .bg(accent(0.2))
                                .px(px(6.))
                                .py(px(2.))
                                .text_size(px(10.))
                                .font_features(tabular())
                                .text_color(accent_color())
                                .child(group.matches.len().to_string()),
                        ),
                )
                .child(
                    div()
                        .truncate()
                        .px_2()
                        .pb_1()
                        .text_size(px(10.))
                        .text_color(content(0.4))
                        .child(group.relative.clone()),
                )
                .into_any_element();
        };
        let matched = &group.matches[index];
        let text = StyledText::new(matched.source.preview.trim_end().to_owned()).with_highlights(
            matched.highlight.clone().map(|range| {
                (
                    range,
                    HighlightStyle {
                        background_color: Some(accent(0.35)),
                        color: Some(content(1.)),
                        ..Default::default()
                    },
                )
            }),
        );
        div()
            .id(("project-search-match", group_index * 501 + index))
            .debug_selector(move || format!("project-search-match-{}", group_index * 501 + index))
            .role(Role::Button)
            .aria_label(format!(
                "{}:{} {}",
                group.relative, matched.source.line, matched.source.preview
            ))
            .w_full()
            .min_w_0()
            .flex()
            .items_start()
            .gap_2()
            .px_2()
            .py_1()
            .hover(|s| s.bg(content(0.05)))
            .when(index + 1 == group.matches.len(), |d| {
                d.border_b_1().border_color(stroke())
            })
            .child(
                div()
                    .w(px(28.))
                    .flex_shrink_0()
                    .pt(px(1.))
                    .text_align(gpui::TextAlign::Right)
                    .font_family("monospace")
                    .font_features(tabular())
                    .text_size(px(11.))
                    .text_color(content(0.35))
                    .child(matched.source.line.to_string()),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .truncate()
                    .font_family("monospace")
                    .text_size(px(11.))
                    .line_height(px(20.))
                    .text_color(content(0.8))
                    .child(text),
            )
            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                this.pick(group_index, index, event.click_count() >= 2, cx);
            }))
            .into_any_element()
    }

    fn toggle(
        id: &'static str,
        label: &'static str,
        path: &'static str,
        active: bool,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .aria_selected(active)
            .size(px(24.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(2.))
            .text_color(content(if active { 1. } else { 0.4 }))
            .when(active, |d| d.bg(content(0.1)))
            .hover(|s| s.bg(content(0.1)).text_color(content(0.7)))
            .child(icon(path, 14.))
    }

    fn filter(input: Entity<Input>) -> AnyElement {
        div()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .flex()
            .rounded(px(6.))
            .border_1()
            .border_color(content(0.1))
            .bg(content(0.05))
            .px_2()
            .py(px(6.))
            .child(div().min_w_0().flex_1().child(input))
            .into_any_element()
    }
}

impl Render for ProjectSearch {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.cwd.is_empty() || self.cwd == "~" {
            return div()
                .px_3()
                .py_2()
                .text_size(px(12.))
                .text_color(content(0.5))
                .child("No project folder")
                .into_any_element();
        }
        let status = if self.loading {
            "Searching…".to_owned()
        } else if let Some(error) = &self.error {
            error.clone()
        } else if self.query.read(cx).content.trim().is_empty() {
            "Type to search across the project".to_owned()
        } else if self.count == 0 {
            "No results".to_owned()
        } else {
            format!(
                "{} result{} in {} file{}{}",
                self.count,
                if self.count == 1 { "" } else { "s" },
                self.groups.len(),
                if self.groups.len() == 1 { "" } else { "s" },
                if self.truncated { " (limited)" } else { "" }
            )
        };
        let entity = cx.entity();
        div()
            .id("explorer-project-search")
            .debug_selector(|| "explorer-project-search".into())
            .role(Role::TabPanel)
            .aria_label("Search in files")
            .size_full()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .text_color(content(1.))
            .capture_action(cx.listener(|this, _: &input::Submit, window, cx| {
                if !this.query.read(cx).is_composing()
                    && this.query.focus_handle(cx).is_focused(window)
                {
                    this.pick(0, 0, false, cx);
                    cx.stop_propagation();
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &crate::shell::DismissMenu, _, cx| {
                if !this.is_composing(cx) {
                    cx.emit(SearchEvent::Close);
                }
                cx.stop_propagation();
            }))
            .child(
                div()
                    .flex_shrink_0()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px(px(6.))
                    .py_1()
                    .border_b_1()
                    .border_color(stroke())
                    .child(
                        div()
                            .id("project-search-back")
                            .role(Role::Button)
                            .aria_label("Back to files")
                            .size(px(28.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .text_color(content(0.5))
                            .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
                            .child(icon("chevron-left", 16.))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(SearchEvent::Close))),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(content(0.55))
                            .child("Search in files"),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .border_b_1()
                    .border_color(stroke())
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_1()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(content(0.1))
                            .bg(content(0.05))
                            .pl_2()
                            .pr_1()
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .py(px(6.))
                                    .child(self.query.clone()),
                            )
                            .child(
                                Self::toggle(
                                    "project-search-case",
                                    "Match case",
                                    "case-sensitive",
                                    self.case_sensitive,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.case_sensitive = !this.case_sensitive;
                                        this.refresh(cx);
                                    },
                                )),
                            )
                            .child(
                                Self::toggle(
                                    "project-search-word",
                                    "Match whole word",
                                    "whole-word",
                                    self.whole_word,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.whole_word = !this.whole_word;
                                        this.refresh(cx);
                                    },
                                )),
                            )
                            .child(
                                Self::toggle(
                                    "project-search-regex",
                                    "Use regular expression",
                                    "regex",
                                    self.regex,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.regex = !this.regex;
                                        this.refresh(cx);
                                    },
                                )),
                            ),
                    )
                    .child(Self::filter(self.include.clone()))
                    .child(Self::filter(self.exclude.clone())),
            )
            .child(
                div()
                    .min_h(px(32.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py(px(6.))
                    .text_size(px(11.))
                    .text_color(if self.error.is_some() {
                        rgb(RED_400).into()
                    } else {
                        content(0.45)
                    })
                    .when(self.loading, |d| {
                        d.child(spinner("project-search-spinner", "loader", 12.))
                    })
                    .child(div().min_w_0().flex_1().child(status)),
            )
            .child(
                div()
                    .id("project-search-results")
                    .min_h_0()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(
                        list(self.list.clone(), move |row, _, cx| {
                            entity.update(cx, |view, cx| view.row(row, cx))
                        })
                        .flex_1()
                        .min_h_0(),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[gpui::test]
    fn large_results_render_only_the_viewport_and_can_reach_the_last_match(
        cx: &mut gpui::TestAppContext,
    ) {
        let window = cx.update(|cx| {
            cx.open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::new(
                        gpui::point(px(0.), px(0.)),
                        gpui::size(px(400.), px(400.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|cx| {
                        let mut view = ProjectSearch::new("/repo".into(), false, cx);
                        view.groups.push(Group {
                            relative: "src/file.rs".into(),
                            name: "file.rs".into(),
                            matches: (0..500)
                                .map(|i| Match {
                                    source: SearchMatch {
                                        path: "/repo/src/file.rs".into(),
                                        relative: "src/file.rs".into(),
                                        line: i + 1,
                                        column: 1,
                                        preview: format!("match {i}"),
                                    },
                                    highlight: None,
                                })
                                .collect(),
                        });
                        view.rows = std::iter::once((0, None))
                            .chain((0..500).map(|i| (0, Some(i))))
                            .collect();
                        view.list
                            .reset_with_uniform_height(view.rows.len(), px(28.));
                        view.count = 500;
                        view
                    })
                },
            )
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        assert!(visual.debug_bounds("project-search-match-0").is_some());
        assert!(visual.debug_bounds("project-search-match-499").is_none());
        window
            .update(visual, |view, _, cx| {
                view.list.scroll_to(gpui::ListOffset {
                    item_ix: 500,
                    offset_in_item: px(0.),
                });
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
        assert!(visual.debug_bounds("project-search-match-499").is_some());
        assert!(visual.debug_bounds("project-search-match-0").is_none());
    }
}
