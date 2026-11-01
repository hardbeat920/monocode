use super::panels::{button, empty_state, matches_query, search_bar};
use super::*;
use crate::tooltip::Tip;
use monocode_core::mono::habits::{Run, Schedule};
use monocode_core::mono::heartbeat;
use monocode_core::time::now_ms;

enum Claimed {
    Habit(Box<Habit>),
    Heartbeat(String),
}
/// What a worker needs to decide whether a Mono's heartbeat is due.
struct HeartbeatCandidate {
    mono: Mono,
    /// The Mono's own chat, habit runs and other app-hidden sessions.
    skip: HashSet<i64>,
}
/// The user's inbox across the Mono's projects. Fetched without holding the
/// database: providers are network calls.
fn fetch_inbox(store: &Database, mono: &Mono) -> Vec<monocode_core::inbox::Item> {
    let Some(cwd) = mono.projects.first() else {
        return Vec::new();
    };
    let Some((data_dir, state)) = locked(store).ok().and_then(|db| {
        Some((
            db.data_dir()?.to_path_buf(),
            monocode_core::inbox::ViewState::load(&db),
        ))
    }) else {
        return Vec::new();
    };
    let service = monocode_core::inbox::Service {
        data_dir,
        cwd: cwd.into(),
    };
    let projects: Vec<std::path::PathBuf> = mono.projects.iter().map(Into::into).collect();
    service.list(&projects, &state).items
}
/// Runs on a worker. Advances the cursor whenever a heartbeat is due, even when
/// nothing changed, so a quiet Mono costs no model call.
fn prepare_heartbeat(
    store: &Database,
    root: &std::path::Path,
    beat: &HeartbeatCandidate,
    now: u64,
    inbox: fn(&Database, &Mono) -> Vec<monocode_core::inbox::Item>,
) -> Result<Option<String>, String> {
    let mono = &beat.mono;
    let state = heartbeat::load(root, &mono.id)?;
    if state.last_run_at == 0 {
        // Enabling heartbeats or a first launch is not inactivity.
        heartbeat::update(root, &mono.id, |s| s.last_run_at = now)?;
        return Ok(None);
    }
    let (rows, sessions, notes) = {
        let db = locked(store)?;
        let chat = mono
            .session_id
            .map(|id| db.mono_session(id))
            .transpose()?
            .flatten();
        let activity = chat.as_ref().map_or(0, |s| s.updated_at);
        if !heartbeat::due(mono.heartbeat_minutes, &state, activity, now) {
            return Ok(None);
        }
        let rows = chat
            .map(|s| db.load(s.id))
            .transpose()?
            .map(|t| t.rows)
            .unwrap_or_default();
        if activity > state.last_run_at {
            heartbeat::update(root, &mono.id, |s| s.quiet_streak = 0)?;
        }
        let all = db.sessions()?;
        let since = state.last_run_at;
        let mut sessions = Vec::new();
        for s in heartbeat::idle_sessions(
            &all,
            &mono.projects,
            |id| beat.skip.contains(&id),
            since,
            now,
        )
        .into_iter()
        .take(5)
        {
            let (user, agent) = heartbeat::last_exchange(&db.load(s.id)?.rows);
            sessions.push(heartbeat::SessionLead {
                id: s.id,
                title: s.title.clone(),
                idle_ms: now.saturating_sub(s.updated_at),
                user,
                agent,
            });
        }
        let notes = heartbeat::changed_notes(&db.list_notes()?, &mono.projects, since, now);
        (rows, sessions, notes)
    };
    let since = state.last_run_at;
    heartbeat::update(root, &mono.id, |s| s.last_run_at = now)?;
    let inbox = heartbeat::changed_inbox(&inbox(store, mono), &mono.projects, since, now);
    let files = mono::load_files(root, mono)?;
    let standing = files
        .soul
        .as_deref()
        .and_then(heartbeat::standing_instructions);
    let digest = heartbeat::digest(sessions, notes, inbox);
    if digest.is_empty() && standing.is_none() {
        return Ok(None);
    }
    let brief = heartbeat::brief(&rows);
    Ok(Some(heartbeat::message(
        standing.as_deref(),
        &digest,
        &state.recent,
        &brief,
        since,
        now,
    )))
}

#[derive(Default)]
pub(super) struct HabitUi {
    saving: bool,
    revision: u64,
    error: Option<String>,
    picker_field: Option<&'static str>,
    expanded: HashSet<(String, u64)>,
}

fn duration(ms: u64) -> String {
    let seconds = (ms.saturating_add(500) / 1000).max(1);
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds.is_multiple_of(60) {
        format!("{}m", seconds / 60)
    } else {
        format!("{}m {}s", seconds / 60, seconds % 60)
    }
}
fn when(at: u64, now: u64) -> String {
    let Some(at) = chrono::DateTime::from_timestamp_millis(at as i64) else {
        return String::new();
    };
    let at = at.with_timezone(&chrono::Local);
    let today = chrono::DateTime::from_timestamp_millis(now as i64)
        .map(|d| d.with_timezone(&chrono::Local).date_naive());
    if Some(at.date_naive()) == today {
        format!("Today {}", at.format("%H:%M"))
    } else if now.saturating_sub(at.timestamp_millis().max(0) as u64) < 6 * 24 * 60 * 60 * 1000 {
        at.format("%a %H:%M").to_string()
    } else {
        at.format("%b %-d, %H:%M").to_string()
    }
}
fn same_day(a: u64, b: u64) -> bool {
    let day = |at| {
        chrono::DateTime::from_timestamp_millis(at as i64)
            .map(|d| d.with_timezone(&chrono::Local).date_naive())
    };
    day(a).is_some() && day(a) == day(b)
}
/// One row per habit, following HabitsList's today/upcoming/done/later order.
fn grouped_habits(
    habits: &[Habit],
    now: u64,
    running: impl Fn(&Habit) -> Option<u64>,
) -> [Vec<usize>; 3] {
    let mut upcoming = Vec::new();
    let mut done = Vec::new();
    let mut later = Vec::new();
    for (ix, h) in habits.iter().enumerate() {
        if let Some(since) = running(h) {
            upcoming.push((0, since, ix));
        } else if h.run_requested || h.enabled && h.next_run_at <= now {
            upcoming.push((1, now, ix));
        } else if h.enabled && same_day(h.next_run_at, now) {
            upcoming.push((2, h.next_run_at, ix));
        } else if let Some(last) = h
            .runs
            .first()
            .map(|r| r.at)
            .or(h.last_run_at)
            .filter(|at| same_day(*at, now))
        {
            done.push((last, ix));
        } else {
            later.push((!h.enabled, h.next_run_at, ix));
        }
    }
    upcoming.sort_by_key(|item| (item.0, item.1));
    done.sort_by_key(|item| item.0);
    later.sort_by_key(|item| (item.0, item.1));
    [
        upcoming.into_iter().map(|item| item.2).collect(),
        done.into_iter().map(|item| item.1).collect(),
        later.into_iter().map(|item| item.2).collect(),
    ]
}
fn section(title: &'static str) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .border_t_1()
        .border_color(content(0.1))
        .px_2()
        .pt_3()
        .pb_4()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .px_2()
                .text_size(px(11.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(content(0.4))
                .child(title),
        )
}
fn property(label: &'static str, value: impl IntoElement) -> gpui::Div {
    div()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .flex()
        .items_start()
        .gap_2()
        .text_size(px(12.))
        .line_height(px(28.))
        .child(
            div()
                .w(px(80.))
                .flex_shrink_0()
                .text_color(content(0.45))
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_color(content(0.85))
                .child(value),
        )
}
/// Fixed controls must never be compressed by a large, scrollable draft.
fn habit_field(field: Entity<Input>) -> gpui::Div {
    div()
        .px_2()
        .py_1()
        .rounded_md()
        .hover(|s| s.bg(content(0.05)))
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .overflow_hidden()
        .child(field)
}
fn habit_icon_button(
    id: SharedString,
    label: &'static str,
    glyph: &'static str,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .tip(label)
        .flex_shrink_0()
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .text_color(content(0.5))
        .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
        .child(icon(glyph, 14.))
}
impl Monos {
    pub(super) fn attention_views(&self, id: &str, cx: &App) -> Vec<Entity<Conversation>> {
        let mut views = Vec::new();
        for (&session, run) in self.running.iter().filter(|(_, r)| r.mono == id) {
            if run.view.read(cx).needs_approval() {
                views.push(run.view.clone());
            }
            if let Some(owner) = self.owner.upgrade() {
                for launch in self
                    .roster
                    .monos
                    .iter()
                    .flat_map(|m| m.launches.iter())
                    .filter(|l| {
                        (l.origin_turn >> 32) as i64 == session && !l.completed && !l.dismissed
                    })
                {
                    if let Some(view) = owner
                        .read(cx)
                        .mono_session_entity(launch.session_id)
                        .filter(|v| v.read(cx).needs_approval())
                    {
                        views.push(view);
                    }
                }
            }
        }
        views
    }

    pub(super) fn habit_running_since(&self, habit: &Habit) -> Option<u64> {
        self.running
            .values()
            .find(|r| r.habit == habit.id && self.active.as_deref() == Some(r.mono.as_str()))
            .map(|r| r.launched)
            .or(habit.running_since)
    }
    fn habit_busy(&self, habit: &Habit) -> bool {
        self.habit_running_since(habit).is_some()
            || habit.run_requested
            || habit.enabled && habit.next_run_at <= now_ms()
    }
    pub(super) fn habits_list(&mut self, mono: &Mono, cx: &mut Context<Self>) -> AnyElement {
        let filter = self.field(
            "mono-habit-search",
            "Search habits…",
            String::new(),
            false,
            cx,
        );
        let query = filter.read(cx).content.to_string();
        let groups =
            grouped_habits(&self.habits, now_ms(), |h| self.habit_running_since(h)).map(|items| {
                items
                    .into_iter()
                    .filter(|ix| {
                        let h = &self.habits[*ix];
                        matches_query(
                            &query,
                            &format!("{} {} {}", h.name, h.instructions, h.schedule.label()),
                        )
                    })
                    .collect::<Vec<_>>()
            });
        let no_matches = !self.habits.is_empty() && groups.iter().all(|items| items.is_empty());
        let list = div().id("mono-habits-list").debug_selector(|| "mono-habits-list".into())
            .flex_1().min_h_0().min_w_0().overflow_y_scroll().p_2().flex().flex_col().gap_4()
            .when(no_matches, |d| d.child(empty_state("search-01", "No habits match this search.")))
            .when(self.habits.is_empty(), |d| d.child(
                div().flex_1().min_h(px(160.)).flex().flex_col().items_center().justify_center()
                    .gap_3().px_6().py_10().text_center().text_size(px(12.)).line_height(px(18.))
                    .text_color(content(0.55)).child(icon("clock", 18.))
                    .child(format!("Ask {} to put recurring work on a schedule, like a morning digest or a weekly report.", mono.label()))))
            .children(["Up next today", "Done today", "Later"].into_iter().zip(groups).filter(|(_, items)| !items.is_empty()).map(|(title, items)| {
                div().flex_shrink_0().w_full().min_w_0().flex().flex_col().gap_1p5()
                    .child(div().px_2().text_size(px(11.)).text_color(content(0.4)).child(title))
                    .children(items.into_iter().map(|ix| self.habit_row(&self.habits[ix], mono, cx)))
            }));
        div()
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .when(!self.habits.is_empty(), |d| d.child(search_bar(filter)))
            .child(list)
            .into_any_element()
    }
    fn habit_row(&self, h: &Habit, mono: &Mono, cx: &mut Context<Self>) -> AnyElement {
        let id = h.id.clone();
        let since = self.habit_running_since(h);
        let subtitle = if let Some(since) = since {
            format!("Running now · {}", duration(now_ms().saturating_sub(since)))
        } else if self.habit_busy(h) {
            "Starting…".into()
        } else if h.enabled {
            h.schedule.label()
        } else {
            format!("Paused · {}", h.schedule.label())
        };
        super::panels::outlined(div().id(SharedString::from(format!("mono-habit-{id}"))))
            .debug_selector({
                let id = id.clone();
                move || format!("mono-habit-{id}")
            })
            .group("mono-habit")
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .flex()
            .items_center()
            .child(
                div()
                    .id(SharedString::from(format!("mono-habit-open-{id}")))
                    .role(Role::Button)
                    .aria_label(h.name.clone())
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .px(px(10.))
                    .py_2()
                    .child(
                        div()
                            .size(px(28.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .bg(content(0.06))
                            .child(if since.is_some() {
                                div()
                                    .size(px(8.))
                                    .rounded_full()
                                    .bg(rgb(mono.color))
                                    .into_any_element()
                            } else {
                                icon("clock", 14.)
                                    .text_color(content(if h.enabled { 0.6 } else { 0.3 }))
                                    .into_any_element()
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(20.))
                                    .truncate()
                                    .text_color(content(if h.enabled || since.is_some() {
                                        0.85
                                    } else {
                                        0.45
                                    }))
                                    .child(h.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .line_height(px(16.))
                                    .truncate()
                                    .text_color(content(0.4))
                                    .child(subtitle),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.route = Some(Route::Habit(id.clone()));
                        this.clear_habit_fields();
                        cx.notify();
                    })),
            )
            .child(self.habit_actions(h, false, cx).pr_2())
            .into_any_element()
    }
    fn habit_actions(&self, h: &Habit, page: bool, cx: &mut Context<Self>) -> gpui::Div {
        let busy = self.habit_busy(h);
        let running = self.habit_running_since(h).is_some();
        let run = h.id.clone();
        let toggle = h.id.clone();
        let remove = h.id.clone();
        let suffix = if page {
            String::new()
        } else {
            format!("-{}", h.id)
        };
        div()
            .when(!page, |d| {
                d.opacity(0.).group_hover("mono-habit", |s| s.opacity(1.))
            })
            .flex_shrink_0()
            .flex()
            .items_center()
            .child(
                habit_icon_button(
                    SharedString::from(format!("mono-habit-run{suffix}")),
                    if running {
                        "Already running"
                    } else if busy {
                        "Starting"
                    } else {
                        "Run now"
                    },
                    "zap",
                )
                .debug_selector({
                    let suffix = suffix.clone();
                    move || format!("mono-habit-run{suffix}")
                })
                .when(busy, |d| d.opacity(0.35))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !busy {
                        this.request_habit(&run, cx);
                    }
                })),
            )
            .child(
                habit_icon_button(
                    SharedString::from(format!("mono-habit-toggle{suffix}")),
                    if h.enabled { "Pause" } else { "Resume" },
                    if h.enabled {
                        "queue-pause"
                    } else {
                        "queue-play"
                    },
                )
                .debug_selector({
                    let suffix = suffix.clone();
                    move || format!("mono-habit-toggle{suffix}")
                })
                .on_click(cx.listener(move |this, _, _, cx| this.habit_toggle(&toggle, cx))),
            )
            .child(
                habit_icon_button(
                    SharedString::from(format!("mono-habit-delete{suffix}")),
                    "Remove",
                    "trash",
                )
                .debug_selector(move || format!("mono-habit-delete{suffix}"))
                .on_click(cx.listener(move |this, _, _, cx| this.remove_habit(&remove, cx))),
            )
    }
    pub(super) fn clear_habit_fields(&mut self) {
        self.fields.retain(|key, _| !key.starts_with("habit-"));
        self.habit_ui.error = None;
        self.habit_ui.revision = self.habit_ui.revision.wrapping_add(1);
    }
    pub(super) fn habit_toggle(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(mono) = self.active.clone() else {
            return;
        };
        let root = self.root.clone();
        let target = mono.clone();
        let id = id.to_string();
        self.file_work(
            mono,
            move || {
                habits::update(&root, &target, |all| {
                    let h = all
                        .iter_mut()
                        .find(|h| h.id == id)
                        .ok_or("Habit not found")?;
                    h.enabled = !h.enabled;
                    if h.enabled {
                        h.next_run_at = h.schedule.next(now_ms())?;
                    }
                    Ok(())
                })
            },
            cx,
        );
    }
    pub(super) fn habit_page(
        &mut self,
        mono: &Mono,
        id: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(id) = id {
            let Some(h) = self.habits.iter().find(|h| h.id == id).cloned() else {
                return div()
                    .size_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(self.page_header("Habit", cx))
                    .child(
                        div()
                            .p_4()
                            .text_size(px(12.))
                            .text_color(content(0.45))
                            .child("This habit is no longer available."),
                    )
                    .into_any_element();
            };
            return self.habit_detail(mono, h, cx);
        }
        self.new_habit_page(window, cx)
    }
    fn habit_detail(&self, mono: &Mono, h: Habit, cx: &mut Context<Self>) -> AnyElement {
        let since = self.habit_running_since(&h);
        let mut info = div()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .px_4()
            .py_3()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(property(
                "Runs",
                if h.enabled {
                    h.schedule.label()
                } else {
                    "Paused".into()
                },
            ));
        if since.is_none() && self.habit_busy(&h) {
            info = info.child(property("Next", "Starting…"));
        } else if h.enabled {
            info = info.child(property("Next", when(h.next_run_at, now_ms())));
        }
        let mut history = section("Recent runs");
        if let Some(since) = since {
            history = history.child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .px_2()
                    .py_2()
                    .text_size(px(12.))
                    .child(
                        div()
                            .size(px(8.))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(rgb(mono.color)),
                    )
                    .child(div().flex_1().min_w_0().child("Running now"))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_color(content(0.4))
                            .child(duration(now_ms().saturating_sub(since))),
                    ),
            );
        }
        if h.runs.is_empty() && since.is_none() {
            history = history.child(
                div()
                    .px_2()
                    .py_3()
                    .text_size(px(12.))
                    .text_color(content(0.4))
                    .child("It hasn't run yet."),
            );
        }
        for run in &h.runs {
            history = history.child(self.habit_run_row(mono, &h, run, cx));
        }
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                self.page_header(&h.name, cx)
                    .child(self.habit_actions(&h, true, cx)),
            )
            .child(
                div()
                    .id("mono-habit-editor")
                    .debug_selector(|| "mono-habit-editor".into())
                    .w_full()
                    .min_w_0()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(info)
                    .child(
                        section("What it does").child(
                            div()
                                .id("mono-habit-instructions")
                                .debug_selector(|| "mono-habit-instructions".into())
                                .w_full()
                                .min_w_0()
                                .flex_shrink_0()
                                .px_2()
                                .text_size(px(12.))
                                .line_height(px(20.))
                                .text_color(content(0.75))
                                .child(h.instructions.clone()),
                        ),
                    )
                    .child(history),
            )
            .into_any_element()
    }
    fn habit_run_row(
        &self,
        mono: &Mono,
        h: &Habit,
        run: &Run,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = (h.id.clone(), run.at);
        let expanded = self.habit_ui.expanded.contains(&key);
        let detail = run
            .report
            .as_ref()
            .or(run.error.as_ref())
            .is_some_and(|text| !text.is_empty());
        let (label, color) = match run.outcome.as_str() {
            "posted" => ("Messaged you", rgb(mono.color).into()),
            "quiet" => ("Nothing to report", content(0.25)),
            _ => ("Couldn't finish", rgb(0xf87171).into()),
        };
        let time = format!(
            "{}{}",
            when(run.at, now_ms()),
            if run.duration_ms > 0 {
                format!(" · {}", duration(run.duration_ms))
            } else {
                String::new()
            }
        );
        let mut card = div()
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .rounded_md()
            .when(expanded && detail, |d| d.bg(content(0.05)))
            .child(
                div()
                    .id(SharedString::from(format!(
                        "mono-habit-history-{}-{}",
                        h.id, run.at
                    )))
                    .debug_selector({
                        let id = h.id.clone();
                        let at = run.at;
                        move || format!("mono-habit-history-{id}-{at}")
                    })
                    .role(Role::Button)
                    .aria_label(format!("{label}, {time}"))
                    .w_full()
                    .min_w_0()
                    .px_2()
                    .py_2()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2p5()
                    .text_size(px(12.))
                    .when(detail, |d| {
                        d.aria_expanded(expanded).hover(|s| s.bg(content(0.05)))
                    })
                    .child(div().size(px(8.)).flex_shrink_0().rounded_full().bg(color))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(72.))
                            .truncate()
                            .text_color(content(0.85))
                            .child(label),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(content(0.4))
                            .child(time),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if detail {
                            if !this.habit_ui.expanded.remove(&key) {
                                this.habit_ui.expanded.insert(key.clone());
                            }
                            cx.notify();
                        }
                    })),
            );
        if expanded && detail {
            let mut body = div()
                .id(SharedString::from(format!(
                    "mono-habit-report-{}-{}",
                    h.id, run.at
                )))
                .debug_selector({
                    let id = h.id.clone();
                    let at = run.at;
                    move || format!("mono-habit-report-{id}-{at}")
                })
                .w_full()
                .min_w_0()
                .flex_shrink_0()
                .overflow_hidden()
                .pl(px(26.))
                .pr_3()
                .pb_3()
                .text_size(px(12.))
                .line_height(px(20.));
            if let Some(report) = &run.report {
                for (ix, mut row) in crate::markdown_model::parse_full(report)
                    .blocks
                    .iter()
                    .flat_map(|b| crate::markdown::rows(&b.block))
                    .enumerate()
                {
                    crate::markdown::resolve_file_links(
                        &mut row,
                        &mono.projects.first().cloned().unwrap_or_default(),
                    );
                    body = body.child(div().w_full().min_w_0().flex_shrink_0().child(
                        crate::markdown::body(
                            &row,
                            format!("habit-report-{}-{}-{ix}", h.id, run.at).into(),
                            crate::markdown::Tone::Normal,
                            None,
                            std::time::Instant::now(),
                        ),
                    ));
                }
            } else if let Some(error) = &run.error {
                body = body.child(div().text_color(rgb(0xf87171)).child(error.clone()));
            }
            card = card.child(body);
        }
        card.into_any_element()
    }
    fn new_habit_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let fresh = !self.fields.contains_key("habit-name");
        let name = self.field("habit-name", "Name", String::new(), false, cx);
        let instructions = if let Some(field) = self.fields.get("habit-instructions") {
            field.clone()
        } else {
            let field = cx.new(Input::habit_instructions);
            self.observations
                .push(cx.subscribe(&field, |_, _, _: &input::Changed, cx| cx.notify()));
            self.fields
                .insert("habit-instructions".into(), field.clone());
            field
        };
        if fresh {
            // Views are rendered lazily during prepaint. The chat composer may
            // already have claimed this frame's accessibility focus, so moving
            // focus here would let the new field claim it a second time.
            let manager = cx.weak_entity();
            let focus = name.focus_handle(cx);
            let field_id = name.entity_id();
            let revision = self.habit_ui.revision;
            window.on_next_frame(move |window, cx| {
                let _ = manager.update(cx, |this, cx| {
                    if this.route == Some(Route::NewHabit)
                        && this.habit_ui.revision == revision
                        && this
                            .fields
                            .get("habit-name")
                            .is_some_and(|field| field.entity_id() == field_id)
                    {
                        window.focus(&focus, cx);
                    }
                });
            });
            name.update(cx, |f, _| f.set_max_chars(80));
            instructions.update(cx, |f, _| {
                f.set_max_chars(4000);
                f.set_font_size(12.);
                f.set_line_height(20.);
            });
            for field in [&name, &instructions] {
                self.observations
                    .push(cx.subscribe(field, |this, _, _: &input::Submitted, cx| {
                        this.save_habit(cx)
                    }));
            }
        }
        for (key, text) in [
            ("habit-kind", "daily"),
            ("habit-time", "09:00"),
            ("habit-minute", "0"),
            ("habit-weekday", "1"),
        ] {
            self.field(key, key, text.into(), false, cx);
        }
        self.fields["habit-time"].update(cx, |f, _| f.set_max_chars(5));
        let ready = !self.habit_ui.saving
            && !self.value("habit-name", cx).trim().is_empty()
            && !self.value("habit-instructions", cx).trim().is_empty();
        let next = self
            .habit_schedule(cx)
            .next(now_ms())
            .map(|at| when(at, now_ms()))
            .unwrap_or_else(|_| "Choose a valid time (HH:MM)".into());
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                self.page_header("New habit", cx).child(
                    button(
                        "mono-habit-save",
                        if self.habit_ui.saving {
                            "Creating…"
                        } else {
                            "Create"
                        },
                    )
                    .debug_selector(|| "mono-habit-save".into())
                    .flex_shrink_0()
                    .h(px(24.))
                    .py_0()
                    .px_2()
                    .bg(content(0.1))
                    .when(!ready, |d| d.opacity(0.4))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if ready {
                            this.save_habit(cx);
                        }
                    })),
                ),
            )
            .child(
                div()
                    .id("mono-habit-editor")
                    .debug_selector(|| "mono-habit-editor".into())
                    .w_full()
                    .min_w_0()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("mono-habit-name-field")
                            .debug_selector(|| "mono-habit-name-field".into())
                            .w_full()
                            .min_w_0()
                            .flex_shrink_0()
                            .px_2()
                            .pt_3()
                            .child(
                                habit_field(name)
                                    .h(px(32.))
                                    .py_1p5()
                                    .font_weight(gpui::FontWeight::MEDIUM),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex_shrink_0()
                            .px_4()
                            .py_3()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(property("Runs", self.schedule_controls(cx)))
                            .child(property("Next", next)),
                    )
                    .child(
                        section("What it does")
                            .child(
                                div()
                                    .id("mono-habit-prompt-field")
                                    .debug_selector(|| "mono-habit-prompt-field".into())
                                    .w_full()
                                    .min_w_0()
                                    .flex_shrink_0()
                                    .child(habit_field(instructions)),
                            )
                            .when_some(self.habit_ui.error.clone(), |d, error| {
                                d.child(
                                    div()
                                        .px_2()
                                        .pt_2()
                                        .text_size(px(12.))
                                        .text_color(rgb(0xf87171))
                                        .child(error),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }
    fn habit_schedule(&self, cx: &App) -> Schedule {
        Schedule {
            schedule_kind: self.value("habit-kind", cx),
            time: self.value("habit-time", cx).trim().into(),
            minute: self.value("habit-minute", cx).parse().unwrap_or(60),
            day_of_week: self.value("habit-weekday", cx).parse().unwrap_or(7),
        }
    }
    fn schedule_picker(
        &self,
        field: &'static str,
        label: &'static str,
        current: String,
        choices: Vec<(String, String)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = choices
            .iter()
            .find(|(value, _)| value == &current)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| current.clone());
        let anchor = AnchorSlot::default();
        let focus = self.fields[field].focus_handle(cx);
        let keyboard_choices = choices.clone();
        let keyboard_anchor = anchor.clone();
        let expanded = self.picker.is_some() && self.habit_ui.picker_field == Some(field);
        div()
            .id(field)
            .debug_selector(move || format!("mono-{field}-picker"))
            .role(Role::ComboBox)
            .aria_label(format!("{label}: {selected}"))
            .aria_expanded(expanded)
            .track_focus(&focus)
            .tab_index(0)
            .relative()
            .h(px(28.))
            .max_w_full()
            .flex_shrink_0()
            .px_2()
            .rounded_md()
            .bg(content(0.1))
            .hover(|s| s.bg(content(0.14)))
            .flex()
            .items_center()
            .gap_1()
            .text_size(px(12.))
            .line_height(px(16.))
            .child(anchor.probe())
            .child(div().min_w_0().truncate().child(selected))
            .child(icon("chevron-down", 12.))
            .on_key_down(
                cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                    if matches!(
                        event.keystroke.key.as_str(),
                        "enter" | "space" | "down" | "up"
                    ) {
                        this.open_schedule_picker(
                            field,
                            label,
                            keyboard_choices.clone(),
                            keyboard_anchor.clone(),
                            window,
                            cx,
                        );
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                window.focus(&this.fields[field].focus_handle(cx), cx);
                this.open_schedule_picker(
                    field,
                    label,
                    choices.clone(),
                    anchor.clone(),
                    window,
                    cx,
                );
            }))
            .into_any_element()
    }
    fn open_schedule_picker(
        &mut self,
        field: &'static str,
        label: &'static str,
        choices: Vec<(String, String)>,
        anchor: AnchorSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self.value(field, cx);
        let items = choices
            .into_iter()
            .map(|(value, label)| {
                crate::popover::MenuAction::new(value.clone(), label)
                    .checked(value == current)
                    .into()
            })
            .collect();
        let menu = cx.new(|cx| {
            Menu::new(label, items, window, cx)
                .width(240.)
                .select_value(&current)
        });
        let picked = cx.subscribe(&menu, move |this, _, event: &MenuEvent, cx| {
            if let MenuEvent::Picked(value, _) = event
                && let Some(input) = this.fields.get(field)
            {
                input.update(cx, |input, cx| input.set_text(value.to_string(), cx));
            }
            this.picker = None;
            this.habit_ui.picker_field = None;
            cx.notify();
        });
        self.habit_ui.picker_field = Some(field);
        self.picker = Some(OpenPicker {
            menu,
            anchor: Anchor::Slot(anchor),
            width: 240.,
            align: crate::popover::Align::Start,
            _events: vec![picked],
        });
        cx.notify();
    }
    fn schedule_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let kind = self.value("habit-kind", cx);
        let choices = |items: &[(&str, &str)]| {
            items
                .iter()
                .map(|(value, label)| (value.to_string(), label.to_string()))
                .collect()
        };
        let days = [
            "Sunday",
            "Monday",
            "Tuesday",
            "Wednesday",
            "Thursday",
            "Friday",
            "Saturday",
        ];
        div()
            .id("mono-habit-schedule")
            .debug_selector(|| "mono-habit-schedule".into())
            .w_full()
            .min_w_0()
            .flex_shrink_0()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1p5()
            .py_0p5()
            .child(self.schedule_picker(
                "habit-kind",
                "Repeats",
                kind.clone(),
                choices(&[
                    ("daily", "Daily"),
                    ("weekdays", "Weekdays"),
                    ("weekly", "Weekly"),
                    ("hourly", "Hourly"),
                ]),
                cx,
            ))
            .when(kind == "weekly", |d| {
                d.child(div().text_color(content(0.45)).child("on")).child(
                    self.schedule_picker(
                        "habit-weekday",
                        "Day",
                        self.value("habit-weekday", cx),
                        days.into_iter()
                            .enumerate()
                            .map(|(ix, label)| (ix.to_string(), label.into()))
                            .collect(),
                        cx,
                    ),
                )
            })
            .child(div().text_color(content(0.45)).child("at"))
            .when(kind == "hourly", |d| {
                d.child(
                    self.schedule_picker(
                        "habit-minute",
                        "Minute",
                        self.value("habit-minute", cx),
                        [0, 15, 30, 45]
                            .into_iter()
                            .map(|minute| (minute.to_string(), format!(":{minute:02}")))
                            .collect(),
                        cx,
                    ),
                )
            })
            .when(kind != "hourly", |d| {
                d.child(
                    habit_field(self.fields["habit-time"].clone())
                        .w(px(84.))
                        .h(px(28.))
                        .bg(content(0.1)),
                )
            })
            .into_any_element()
    }
    fn save_habit(&mut self, cx: &mut Context<Self>) {
        if self.habit_ui.saving || self.route != Some(Route::NewHabit) {
            return;
        }
        let Some(mono) = self.active.clone() else {
            return;
        };
        let name = self
            .value("habit-name", cx)
            .trim()
            .chars()
            .take(80)
            .collect();
        let instructions = self
            .value("habit-instructions", cx)
            .trim()
            .chars()
            .take(4000)
            .collect();
        let habit = match Habit::new(name, instructions, self.habit_schedule(cx), now_ms()) {
            Ok(h) => h,
            Err(error) => {
                self.habit_ui.error = Some(error);
                cx.notify();
                return;
            }
        };
        self.habit_ui.saving = true;
        self.habit_ui.error = None;
        let root = self.root.clone();
        let target = mono.clone();
        let revision = self.habit_ui.revision;
        let work = cx.background_executor().spawn(async move {
            habits::update(&root, &target, |all| {
                all.push(habit.clone());
                Ok(())
            })
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.habit_ui.saving = false;
                match result {
                    Ok(()) => {
                        this.reload_files(&mono, cx);
                        if this.active.as_deref() == Some(&mono)
                            && this.route == Some(Route::NewHabit)
                            && this.habit_ui.revision == revision
                        {
                            this.route = None;
                            this.clear_habit_fields();
                        }
                    }
                    Err(error) => {
                        if this.active.as_deref() == Some(&mono)
                            && this.habit_ui.revision == revision
                        {
                            this.habit_ui.error = Some(error);
                        }
                    }
                }
                this.changed(cx);
            });
        })
        .detach();
        cx.notify();
    }
    fn request_habit(&mut self, id: &str, cx: &mut Context<Self>) {
        if self
            .habits
            .iter()
            .find(|h| h.id == id)
            .is_none_or(|h| self.habit_busy(h))
        {
            return;
        }
        let Some(mono) = self.active.clone() else {
            return;
        };
        let root = self.root.clone();
        let target = mono.clone();
        let id = id.to_string();
        if let Some(h) = self.habits.iter_mut().find(|h| h.id == id) {
            h.run_requested = true;
        }
        let work = cx.background_executor().spawn(async move {
            habits::update(&root, &target, |all| {
                all.iter_mut()
                    .find(|h| h.id == id)
                    .ok_or("Habit not found")?
                    .run_requested = true;
                Ok(())
            })
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.error = Some(error);
                }
                this.reload_files(&mono, cx);
                this.check_habits(cx);
                this.changed(cx);
            });
        })
        .detach();
        cx.notify();
    }
    fn remove_habit(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(mono) = self.active.clone() else {
            return;
        };
        let root = self.root.clone();
        let target = mono.clone();
        let id = id.to_string();
        let removed = id.clone();
        let work = cx.background_executor().spawn(async move {
            habits::update(&root, &target, |all| {
                all.retain(|h| h.id != id);
                Ok(())
            })
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.reload_files(&mono, cx);
                        if this.active.as_deref() == Some(&mono)
                            && this.route == Some(Route::Habit(removed.clone()))
                        {
                            this.route = None;
                            this.clear_habit_fields();
                        }
                    }
                    Err(error) => this.error = Some(error),
                }
                this.changed(cx);
            });
        })
        .detach();
    }
    /// Monos whose chat is idle with nothing queued; habits run first.
    fn heartbeat_candidates(&self, cx: &App) -> Vec<HeartbeatCandidate> {
        let mut skip: HashSet<i64> = self.roster.hidden_sessions.iter().copied().collect();
        for m in &self.roster.monos {
            skip.extend(m.session_id);
            skip.extend(m.habit_sessions.iter().copied());
        }
        self.roster
            .monos
            .iter()
            .filter(|m| m.heartbeat_minutes > 0 && m.session_id.is_some() && m.outbox.is_empty())
            .filter(|m| !self.chats.get(&m.id).is_some_and(|v| v.read(cx).busy))
            .filter(|m| !self.inflight.contains_key(&m.id))
            .map(|m| HeartbeatCandidate {
                mono: m.clone(),
                skip: skip.clone(),
            })
            .collect()
    }
    /// A quiet heartbeat leaves nothing behind: no hidden session, no history.
    fn discard_quiet_heartbeat(&mut self, mono: &str, session: i64, cx: &mut Context<Self>) {
        if let Some(m) = self.roster.get_mut(mono) {
            m.habit_sessions.retain(|&s| s != session);
        }
        self.roster.hidden_sessions.retain(|&s| s != session);
        self.save(cx);
        let store = self.store.clone();
        cx.background_executor()
            .spawn(async move {
                if let Ok(mut db) = locked(&store) {
                    let _ = db.delete(session);
                }
            })
            .detach();
    }
    pub(super) fn tick(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.check_habits(cx);
        cx.notify();
    }
    fn check_habits(&mut self, cx: &mut Context<Self>) {
        let now = now_ms();
        if let Some(id) = self.active.clone() {
            self.refresh_work(&id, cx);
        }
        self.refresh_cards(cx);
        let waiting_monos = self
            .running
            .values()
            .filter(|r| !self.attention_views(&r.mono, cx).is_empty())
            .map(|r| r.mono.clone())
            .collect::<HashSet<_>>();
        let timed_out: Vec<_> = self
            .running
            .iter_mut()
            .filter_map(|(&id, run)| {
                let waiting = waiting_monos.contains(&run.mono);
                if waiting {
                    let since = *run.waiting_since.get_or_insert(now);
                    (now.saturating_sub(since) > habits::APPROVAL_WAIT_MS).then_some(id)
                } else {
                    if let Some(since) = run.waiting_since.take() {
                        run.started += now.saturating_sub(since);
                    }
                    (now.saturating_sub(run.started) > habits::TIMEOUT_MS).then_some(id)
                }
            })
            .collect();
        for id in timed_out {
            if let Some(run) = self.running.get_mut(&id) {
                run.failure = Some("Habit timed out while waiting for work or an answer".into());
                run.mailbox.clear();
                run.view.update(cx, |v, cx| v.mono_stop(cx));
            }
            let children = self
                .roster
                .monos
                .iter_mut()
                .flat_map(|m| m.launches.iter_mut())
                .filter(|l| (l.origin_turn >> 32) as i64 == id && !l.completed && !l.dismissed)
                .map(|l| {
                    l.dismissed = true;
                    l.session_id
                })
                .collect::<Vec<_>>();
            if let Some(owner) = self.owner.upgrade() {
                for child in children {
                    if let Some(view) = owner.read(cx).mono_session_entity(child) {
                        view.update(cx, |v, cx| v.stop_if_busy(cx));
                    }
                }
            }
            self.habit_changed(id, cx);
        }
        self.check_completions(cx);
        if self.scheduling || self.active_run.is_some() {
            return;
        }
        self.scheduling = true;
        let root = self.root.clone();
        let ids: Vec<_> = self.roster.monos.iter().map(|m| m.id.clone()).collect();
        let beats = self.heartbeat_candidates(cx);
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            for id in ids {
                let claimed = habits::update(&root, &id, |all| {
                    // A process restart cannot resume an old provider turn.
                    // Record the interruption instead of leaving it claimed forever.
                    for h in all.iter_mut() {
                        if let Some(start) = h.running_since {
                            h.finish(Run {
                                at: start,
                                duration_ms: now.saturating_sub(start),
                                outcome: "interrupted".into(),
                                report: None,
                                error: Some("The app closed before this run finished".into()),
                            })?;
                        }
                        if h.enabled
                            && h.next_run_at < now
                            && now - h.next_run_at > habits::GRACE_MS
                        {
                            h.next_run_at = h.schedule.next(now)?;
                        }
                    }
                    if let Some(h) = all.iter_mut().find(|h| h.due(now)) {
                        h.running_since = Some(now);
                        let manual = std::mem::take(&mut h.run_requested);
                        if !manual || h.next_run_at <= now {
                            h.next_run_at = h.schedule.next(now)?;
                        }
                        return Ok(Some(h.clone()));
                    }
                    Ok(None)
                })?;
                if let Some(h) = claimed {
                    return Ok::<_, String>(Some((id, Claimed::Habit(Box::new(h)))));
                }
            }
            for beat in beats {
                if let Some(text) = prepare_heartbeat(&store, &root, &beat, now, fetch_inbox)? {
                    return Ok(Some((beat.mono.id.clone(), Claimed::Heartbeat(text))));
                }
            }
            Ok(None)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.scheduling = false;
                match result {
                    Ok(Some((id, Claimed::Habit(habit)))) => this.start_habit(&id, *habit, cx),
                    Ok(Some((id, Claimed::Heartbeat(text)))) => this.start_heartbeat(&id, text, cx),
                    Ok(None) => (),
                    Err(e) => {
                        this.error = Some(e);
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }
    fn start_habit(&mut self, id: &str, habit: Habit, cx: &mut Context<Self>) {
        let text = format!(
            "[Scheduled habit: {}]\n{}\n\nDo this scheduled work independently. Return a concise report; if nothing requires attention, reply exactly NOTHING_TO_REPORT. Any session you start belongs to this habit. Do not change the habit unless asked.",
            habit.name, habit.instructions
        );
        self.start_run(id, habit.id, habit.name, text, false, cx);
    }
    pub(super) fn start_heartbeat(&mut self, id: &str, text: String, cx: &mut Context<Self>) {
        self.start_run(
            id,
            heartbeat::RUN_ID.into(),
            "Heartbeat".into(),
            text,
            true,
            cx,
        );
    }
    /// Habits and heartbeats run alike: a hidden side session that reports
    /// back into the Mono's chat only when it has something to say.
    fn start_run(
        &mut self,
        id: &str,
        run_id: String,
        name: String,
        text: String,
        triage: bool,
        cx: &mut Context<Self>,
    ) {
        self.active_run = Some(-1);
        let Some(mono) = self.roster.get(id).cloned() else {
            return;
        };
        self.ensure_chat(id, cx);
        let mut inherited = self
            .chats
            .get(id)
            .map(|v| v.read(cx).settings.clone())
            .unwrap_or_default();
        let provider = self
            .chats
            .get(id)
            .map(|v| v.read(cx).summary.provider.clone())
            .unwrap_or_else(|| "codex".into());
        let store = self.store.clone();
        let target = id.to_string();
        let work = cx.background_executor().spawn(async move {
            let db = locked(&store)?;
            let cwd = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            let mut s = db.create(&provider, &cwd)?;
            s.title = format!("{} · {}", mono.label(), name);
            db.title(s.id, &s.title)?;
            drop(db);
            if triage && inherited.usage_limit.is_none() {
                // The catalog is cached for a few minutes; a miss keeps the Mono's model.
                let models = Kind::parse(&provider)
                    .and_then(|kind| {
                        monocode_core::catalog::models_for(kind, &inherited.account).ok()
                    })
                    .unwrap_or_default();
                if let Some((model, effort)) =
                    heartbeat::model(&provider, inherited.model.as_deref(), &models)
                {
                    inherited.model = Some(model);
                    inherited.effort = effort;
                    inherited.model_settings.clear();
                    inherited.fast = false;
                    inherited.service_tier = None;
                }
            }
            Ok::<_, String>((s, mono, inherited))
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Err(e) => {
                        this.active_run = None;
                        this.error = Some(e);
                    }
                    Ok((summary, mono, mut settings)) => {
                        let session = summary.id;
                        let view = cx.new(|cx| Conversation::new(summary, this.store.clone(), cx));
                        let root = this.root.clone();
                        let env = this
                            .bridge
                            .as_ref()
                            .map(|b| b.environment(session))
                            .unwrap_or_default();
                        if !triage {
                            settings.access = AccessMode::FullAccess;
                        }
                        settings.internal_message = true;
                        settings.mono = None;
                        settings.environment.clear();
                        view.update(cx, |v, cx| {
                            v.install_transfer(settings, None, cx);
                            v.configure_mono(mono, root, env, cx);
                        });
                        this.observations.push(
                            cx.subscribe(&view, move |this, _, _: &SessionChanged, cx| {
                                this.habit_changed(session, cx)
                            }),
                        );
                        this.running.insert(
                            session,
                            RunningHabit {
                                mono: target.clone(),
                                habit: run_id,
                                view: view.clone(),
                                started: now_ms(),
                                launched: now_ms(),
                                failure: None,
                                waiting_since: None,
                                cards: Vec::new(),
                                mailbox: Vec::new(),
                            },
                        );
                        this.active_run = Some(session);
                        this.roster.hidden_sessions.push(session);
                        if let Some(m) = this.roster.get_mut(&target) {
                            m.habit_sessions.push(session);
                        }
                        this.save(cx);
                        let message = Message {
                            sent_at: Some(monocode_core::time::now_ms()),
                            id: mono::new_id(),
                            text,
                            internal: true,
                            error: None,
                            attachments: Vec::new(),
                        };
                        view.update(cx, |v, cx| v.mono_dispatch(&message, cx));
                    }
                }
                this.changed(cx);
            });
        })
        .detach();
    }
    pub(super) fn habit_changed(&mut self, session: i64, cx: &mut Context<Self>) {
        cx.emit(NotificationChanged);
        let Some(run) = self.running.get(&session) else {
            return;
        };
        if run.view.read(cx).busy {
            self.changed(cx);
            return;
        }
        if let Some(text) = self.running.get_mut(&session).and_then(|r| {
            if r.mailbox.is_empty() {
                None
            } else {
                Some(r.mailbox.remove(0))
            }
        }) {
            let view = self.running[&session].view.clone();
            let message = Message {
                id: mono::new_id(),
                text,
                sent_at: Some(monocode_core::time::now_ms()),
                internal: true,
                error: None,
                attachments: Vec::new(),
            };
            view.update(cx, |v, cx| v.mono_dispatch(&message, cx));
            return;
        }
        if self
            .roster
            .monos
            .iter()
            .flat_map(|m| m.launches.iter())
            .any(|l| {
                (l.origin_turn >> 32) as i64 == session
                    && l.notify_on_complete
                    && !l.completed
                    && !l.dismissed
            })
        {
            self.changed(cx);
            return;
        }
        let run = self.running.remove(&session).unwrap();
        self.active_run = None;
        let completion = {
            let view = run.view.read(cx);
            NotificationFinished {
                summary: view.summary.clone(),
                body: view.notification_reply_body(cx),
                habit: true,
                was_cancelled: view.notification_was_cancelled(),
            }
        };
        cx.emit(completion);
        let (mut ok, mut text) = run.view.read(cx).external_outcome(cx);
        if let Some(error) = run.failure {
            ok = false;
            text = error;
        }
        let report = ok
            .then(|| {
                habits::report(&monocode_core::response::final_prose(
                    run.view.read(cx).mono_rows(cx),
                ))
            })
            .flatten();
        let root = self.root.clone();
        let id = run.mono.clone();
        let habit = run.habit.clone();
        let at = run.launched;
        let duration = now_ms().saturating_sub(at);
        let error = (!ok).then_some(text);
        let stored = report.clone();
        let result = Run {
            at,
            duration_ms: duration,
            outcome: if ok {
                if report.is_some() { "posted" } else { "quiet" }
            } else {
                "failed"
            }
            .into(),
            report: stored,
            error,
        };
        let beat = habit == heartbeat::RUN_ID;
        if beat {
            let remembered = report.clone();
            let quiet = ok && report.is_none();
            self.file_work(
                id.clone(),
                move || {
                    heartbeat::update(&root, &id, |state| {
                        if let Some(report) = &remembered {
                            state.remember(report);
                            state.quiet_streak = 0;
                        } else if quiet {
                            state.quiet_streak = state.quiet_streak.saturating_add(1);
                        }
                    })
                },
                cx,
            );
            if ok && report.is_none() && run.cards.is_empty() {
                self.discard_quiet_heartbeat(&run.mono, session, cx);
            }
        } else {
            self.file_work(
                id.clone(),
                move || {
                    habits::update(&root, &id, |all| {
                        if let Some(h) = all.iter_mut().find(|h| h.id == habit) {
                            h.finish(result.clone())?;
                        }
                        Ok(())
                    })
                },
                cx,
            );
        }
        if let Some(report) = report {
            if let Some(m) = self.roster.get_mut(&run.mono) {
                m.pending_reports.push(if beat {
                    report
                } else {
                    format!("**Habit report**\n\n{report}")
                });
                for card in &run.cards {
                    m.pending_reports.push(format!("mono-card:{card}"));
                }
            }
            self.ensure_chat(&run.mono, cx);
            self.save(cx);
            self.dispatch(&run.mono, cx);
        }
        self.changed(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn heartbeat_seeds_waits_for_quiet_then_raises_a_left_session_once() {
        let dir = std::env::temp_dir().join(format!("mono-heartbeat-{}", mono::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let project = dir.to_string_lossy().to_string();
        let store: Database = std::sync::Arc::new(std::sync::Mutex::new(
            monocode_core::store::Store::open(&dir.join("native.sqlite")).unwrap(),
        ));
        let root = dir.join("monos");
        let (chat, left) = {
            let mut db = locked(&store).unwrap();
            let chat = db.create("codex", &project).unwrap();
            let left = db.create("codex", &project).unwrap();
            db.title(left.id, "Hermes effort").unwrap();
            let mut t = monocode_core::transcript::Transcript::default();
            t.begin("user");
            t.append("lets do option B");
            t.begin("assistant");
            t.append("Plan ready. Want me to start?");
            db.write_rows(left.id, 0, &t.rows).unwrap();
            (chat.id, left.id)
        };
        let beat = HeartbeatCandidate {
            mono: Mono {
                session_id: Some(chat),
                projects: vec![project.clone()],
                ..Default::default()
            },
            skip: HashSet::from([chat]),
        };
        let no_inbox: fn(&Database, &Mono) -> Vec<monocode_core::inbox::Item> = |_, _| Vec::new();
        let t = now_ms();
        let min = 60_000;
        let prepare = |at| prepare_heartbeat(&store, &root, &beat, at, no_inbox).unwrap();
        assert_eq!(prepare(t), None, "the first sight only seeds the cursor");
        assert_eq!(prepare(t + 10 * min), None, "not quiet for long enough");
        let text =
            prepare(t + 2 * heartbeat::IDLE_MS).expect("a left session is worth a heartbeat");
        assert!(text.contains(&format!("\"Hermes effort\" (sessionId {left})")));
        assert!(text.contains("lets do option B"));
        assert!(text.contains("Want me to start?"));
        assert_eq!(
            prepare(t + 2 * heartbeat::IDLE_MS + 31 * min),
            None,
            "nothing new and no standing instructions costs no model call"
        );
        assert_eq!(
            heartbeat::load(&root, &beat.mono.id).unwrap().last_run_at,
            t + 2 * heartbeat::IDLE_MS + 31 * min
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn habits_list_groups_today_with_running_first_and_paused_last() {
        let at = |day, hour| {
            chrono::Local
                .with_ymd_and_hms(2026, 10, day, hour, 0, 0)
                .single()
                .unwrap()
                .timestamp_millis() as u64
        };
        let now = at(12, 12);
        let make = |name: &str, next_run_at| {
            let mut h = Habit::new(
                name.into(),
                "Check updates".into(),
                Schedule::default(),
                now,
            )
            .unwrap();
            h.id = name.into();
            h.next_run_at = next_run_at;
            h
        };
        let mut morning = make("Morning", at(13, 9));
        morning.last_run_at = Some(at(12, 8));
        let mut paused = make("Paused", at(13, 7));
        paused.enabled = false;
        let mut starting = make("Starting", at(13, 9));
        starting.run_requested = true;
        let habits = [
            make("Weekly", at(16, 9)),
            make("Evening", at(12, 18)),
            morning,
            paused,
            starting,
            make("Running", at(13, 9)),
        ];
        assert_eq!(
            grouped_habits(&habits, now, |h| (h.id == "Running").then_some(at(12, 11))),
            [vec![5, 4, 1], vec![2], vec![0, 3]]
        );
    }

    #[test]
    fn a_habit_that_ran_and_is_due_again_today_is_only_upcoming() {
        let now = chrono::Local
            .with_ymd_and_hms(2026, 10, 12, 12, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis() as u64;
        let mut h = Habit::new(
            "Hourly".into(),
            "Check builds".into(),
            Schedule {
                schedule_kind: "hourly".into(),
                ..Default::default()
            },
            now,
        )
        .unwrap();
        h.last_run_at = Some(now - 60_000);
        assert_eq!(
            grouped_habits(&[h], now, |_| None),
            [vec![0], vec![], vec![]]
        );
        assert_eq!(duration(72_000), "1m 12s");
        assert_eq!(duration(60_000), "1m");
    }
}
