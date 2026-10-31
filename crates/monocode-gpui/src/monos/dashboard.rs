//! The Overview tab: who the Mono is and what it is doing, its code
//! contributions as a dithered calendar, today's numbers, what runs next and
//! what it made today.
use super::*;
use crate::right_panel::{DIFF_ADD, DIFF_DEL, format_integer, tabular};
use crate::runtime::{Database, locked};
use crate::visuals::dither::{
    Align, CELL, Grid, Label, Motion, glow, hoverable, inked, pointer, readout,
};
use chrono::{DateTime, Datelike, Duration as Days, Local, NaiveDate};
use gpui::{Bounds, Div, FontWeight, Hsla, canvas, point, size};
use monocode_core::contributions::{self, Contribution, Kind};
use monocode_core::time::{now_ms, relative};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

/// One day of a Mono's code contributions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Day {
    pub date: NaiveDate,
    pub commits: u32,
    pub reviews: u32,
    pub opened: u32,
    pub merged: u32,
    pub additions: u32,
    pub deletions: u32,
}
impl Day {
    fn empty(date: NaiveDate) -> Self {
        Self {
            date,
            commits: 0,
            reviews: 0,
            opened: 0,
            merged: 0,
            additions: 0,
            deletions: 0,
        }
    }
    /// Commits, reviews, and pull requests opened or merged each count once;
    /// lines never do.
    pub fn count(&self) -> u32 {
        self.commits + self.reviews + self.opened + self.merged
    }
}

/// Weeks of history the graph can show; a narrow panel keeps the latest.
const WEEKS: i64 = 26;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// How long loaded contributions are shown before they are read again.
const FRESH: std::time::Duration = std::time::Duration::from_secs(30);
/// How often opened pull requests are checked on GitHub for merging.
const LANDED_EVERY: std::time::Duration = std::time::Duration::from_secs(15 * 60);
/// The marker a session's saved history has been scanned under.
const SCANNED: &str = "contributions-scanned:";

/// A Mono's contributions as the Overview shows them.
pub(super) struct Loaded {
    at: Instant,
    days: Arc<Vec<Day>>,
    /// Pull requests merged today, by the Mono or after it opened them.
    merged_today: usize,
    landed_checked: Instant,
}

/// Every day from the graph's first Sunday through `today`, with what was
/// contributed on it.
pub(super) fn days_from(items: &[Contribution], today: NaiveDate) -> Vec<Day> {
    let start = first_day(today);
    let mut days: Vec<Day> = (0..=(today - start).num_days())
        .map(|i| Day::empty(start + Days::days(i)))
        .collect();
    for item in items {
        let Some(date) = local_day(item.at) else {
            continue;
        };
        let Some(day) = (date >= start)
            .then(|| (date - start).num_days() as usize)
            .and_then(|ix| days.get_mut(ix))
        else {
            continue;
        };
        match item.kind {
            Kind::Commit => {
                day.commits += 1;
                day.additions += item.additions as u32;
                day.deletions += item.deletions as u32;
            }
            Kind::Review => day.reviews += 1,
            Kind::Opened => day.opened += 1,
            Kind::Merged => day.merged += 1,
            Kind::Landed => {}
        }
    }
    days
}

/// Distinct pull requests merged on `day`, whether the Mono merged them or
/// they merged after it opened them.
pub(super) fn merged_on(items: &[Contribution], day: NaiveDate) -> usize {
    items
        .iter()
        .filter(|c| matches!(c.kind, Kind::Merged | Kind::Landed))
        .filter(|c| local_day(c.at) == Some(day))
        .map(|c| c.reference.as_str())
        .collect::<HashSet<_>>()
        .len()
}

/// What a saved tool call must mention to possibly be a contribution.
const MENTIONS: [&str; 4] = ["git commit", "git -C", "gh pr", "pull_request"];

/// Finds the contributions in a session's saved history, once. Later ones
/// are recorded as its turns run. Sessions that never ran a matching command
/// are ruled out by SQLite and never loaded.
fn scan(store: &Database, session: i64) {
    let key = format!("{SCANNED}{session}");
    let (rows, cwd, at) = {
        let Ok(store) = locked(store) else { return };
        if store.setting(&key).ok().flatten().is_some() {
            return;
        }
        if !store.session_mentions(session, &MENTIONS).unwrap_or(true) {
            let _ = store.save_setting(&key, "1");
            return;
        }
        let Some(summary) = store.mono_session(session).ok().flatten() else {
            let _ = store.save_setting(&key, "1");
            return;
        };
        let mut rows = store.load(session).map(|t| t.rows).unwrap_or_default();
        if let Ok(Some(before)) = store.history_before(session)
            && let Ok(page) = store.load_before(session, before, true)
        {
            rows.splice(0..0, page.rows);
        }
        (rows, summary.cwd, summary.updated_at)
    };
    // Git runs without the store lock.
    let resolved: Vec<_> = rows
        .iter()
        .filter(|row| row.role == "tool")
        .filter(|row| MENTIONS.iter().any(|w| row.text.contains(w)))
        .filter_map(|row| serde_json::from_str::<monocode_core::tool::ToolCall>(&row.text).ok())
        .flat_map(|tool| contributions::found(&tool))
        .map(|found| {
            let item = contributions::resolve(&found, session, std::path::Path::new(&cwd), at);
            (item, found.amend)
        })
        .collect();
    if let Ok(store) = locked(store) {
        for (item, amend) in resolved {
            let _ = store.record_contribution(&item, amend);
        }
        let _ = store.save_setting(&key, "1");
    }
}

/// Records pull requests the Mono opened that have merged since. Few, recent
/// and in the background: each asks GitHub.
fn check_landed(store: &Database, sessions: &[i64]) {
    let since = now_ms().saturating_sub(30 * 24 * 60 * 60 * 1000);
    let Ok(items) = locked(store).and_then(|s| s.contributions(sessions, since)) else {
        return;
    };
    let settled: HashSet<&str> = items
        .iter()
        .filter(|c| matches!(c.kind, Kind::Merged | Kind::Landed))
        .map(|c| c.reference.as_str())
        .collect();
    for opened in items
        .iter()
        .filter(|c| c.kind == Kind::Opened && !settled.contains(c.reference.as_str()))
        .take(10)
    {
        if let Ok(Some(at)) = contributions::merged_at(&opened.reference) {
            let landed = Contribution {
                kind: Kind::Landed,
                at,
                ..opened.clone()
            };
            if let Ok(store) = locked(store) {
                let _ = store.record_contribution(&landed, false);
            }
        }
    }
}

/// The Sunday that opens the graph's first week.
fn first_day(today: NaiveDate) -> NaiveDate {
    today - Days::days(today.weekday().num_days_from_sunday() as i64) - Days::weeks(WEEKS - 1)
}

/// Calendar intensity, 0 for an empty day and 1 to 4 by quartile of the
/// Mono's own active days, so a quiet Mono still shows contrast.
pub(super) fn levels(days: &[Day]) -> Vec<u8> {
    let mut active: Vec<u32> = days.iter().map(Day::count).filter(|c| *c > 0).collect();
    active.sort_unstable();
    let at = |q: f32| {
        active
            .get(((active.len() as f32 * q) as usize).min(active.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0)
    };
    let cuts = [at(0.25), at(0.5), at(0.75)];
    days.iter()
        .map(|d| match d.count() {
            0 => 0,
            c => 1 + cuts.iter().filter(|cut| c > **cut).count() as u8,
        })
        .collect()
}

/// Consecutive days with a contribution, ending today or yesterday.
pub(super) fn streak(days: &[Day]) -> usize {
    let mut tail = days.iter().rev().peekable();
    // Today still counts as open until it ends.
    if tail.peek().is_some_and(|d| d.count() == 0) {
        tail.next();
    }
    tail.take_while(|d| d.count() > 0).count()
}

fn local_day(ms: u64) -> Option<NaiveDate> {
    DateTime::from_timestamp_millis(ms as i64).map(|t| t.with_timezone(&Local).date_naive())
}

/// Dithered tone per level: a quarter of the cells up to every cell.
const TONES: [f32; 5] = [0., 0.25, 0.5, 0.75, 1.];
/// Each day is a square of 4×4 dither cells, a cell apart.
const SQUARE: i32 = 4;
const PITCH: i32 = 5;
const GUTTER: f32 = 28.;
const MONTHS: f32 = 18.;
const LEGEND: f32 = 22.;

/// The contribution calendar: weeks as columns, Sunday to Saturday down,
/// each day a Bayer-dithered square in the Mono's colour.
fn graph(days: Arc<Vec<Day>>, tint: Hsla, motion: Motion) -> AnyElement {
    let levels = levels(&days);
    let start = days.first().map(|d| d.date);
    let height = MONTHS + (7 * PITCH) as f32 * CELL + LEGEND;
    let map = canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let Some(start) = start else { return };
            let track = content(0.07);
            let pitch = PITCH as f32 * CELL;
            let square = SQUARE as f32 * CELL;
            let grid = Grid::new(
                bounds.origin + point(px(GUTTER), px(MONTHS)),
                bounds.size.width.as_f32() - GUTTER,
                7. * pitch,
            );
            let weeks = (days.len() as i32 + 6) / 7;
            // A narrow panel keeps the latest weeks, as calendars do.
            let fits = ((grid.cols + PITCH - SQUARE) / PITCH).clamp(1, weeks);
            let skip = weeks - fits;
            let day = |c: i32, r: i32| {
                let ix = ((c + skip) * 7 + r) as usize;
                (r < 7).then_some(ix).filter(|ix| *ix < days.len())
            };
            // Weeks thicken in left to right.
            let grown: Vec<f32> = (0..fits)
                .map(|c| motion.at(c as f32 * 12., 420., window, cx))
                .collect();
            let hovered = pointer(bounds, window).and_then(|(x, y)| {
                let (x, y) = (x - GUTTER, y - MONTHS);
                if x < 0. || y < 0. {
                    return None;
                }
                let (c, r) = ((x / pitch) as i32, (y / pitch) as i32);
                (c < fits).then_some(()).and(day(c, r)).map(|ix| (c, r, ix))
            });
            let today = days.len() - 1;
            if levels[today] > 0 {
                let c = (today / 7) as i32 - skip;
                let r = (today % 7) as f32;
                glow(
                    window,
                    Bounds::new(
                        grid.origin + point(px(c as f32 * pitch), px(r * pitch)),
                        size(px(square), px(square)),
                    ),
                    tint,
                    0.35,
                );
            }
            grid.paint(window, |col, row| {
                let (x, y) = (col % PITCH, row % PITCH);
                let (c, r) = (col / PITCH, row / PITCH);
                if x >= SQUARE || y >= SQUARE || c >= fits {
                    return None;
                }
                let ix = day(c, r)?;
                let level = levels[ix];
                if level == 0 {
                    return Some(track);
                }
                let mut tone = TONES[level as usize] * grown[c as usize];
                if hovered.is_some_and(|(hc, hr, _)| (hc, hr) == (c, r)) {
                    tone = (tone + 0.3).min(1.);
                }
                let tone = (tone * 16.).round() / 16.;
                Some(if inked(x, y, tone) { tint } else { track })
            });
            let axis = Label::mono(10., content(0.45));
            for (r, name) in [(1, "Mon"), (3, "Wed"), (5, "Fri")] {
                axis.paint(
                    name,
                    grid.origin + point(px(-GUTTER), px(r as f32 * pitch + square / 2.)),
                    Align::Left,
                    window,
                    cx,
                );
            }
            // A month is named over the first week that starts in it.
            let mut last_at = f32::MIN;
            for c in 0..fits {
                let week = start + Days::weeks((c + skip) as i64);
                let label_month = c == 0 || week.day() <= 7;
                let x = c as f32 * pitch;
                if !label_month || x - last_at < 30. {
                    continue;
                }
                last_at = x;
                axis.paint(
                    &week.format("%b").to_string(),
                    grid.origin + point(px(x), px(-MONTHS / 2.)),
                    Align::Left,
                    window,
                    cx,
                );
            }
            // Less ▫▪▪▪▪ More, flush right under the latest weeks.
            let legend_y = grid.origin.y.as_f32() + 7. * pitch + LEGEND / 2. + 2.;
            let more = Label::mono(10., content(0.4));
            let edge = (grid.origin.x.as_f32() + fits as f32 * pitch - CELL).round();
            more.paint(
                "More",
                point(px(edge), px(legend_y)),
                Align::Right,
                window,
                cx,
            );
            let right = edge - more.measure("More", window) - 6.;
            let swatches = (right - 5. * pitch + CELL).round();
            more.paint(
                "Less",
                point(px(swatches - 6.), px(legend_y)),
                Align::Right,
                window,
                cx,
            );
            let key = Grid::new(
                point(px(swatches), px(legend_y - square / 2.)),
                5. * pitch,
                square,
            );
            key.paint(window, |col, row| {
                let (x, level) = (col % PITCH, (col / PITCH) as usize);
                if x >= SQUARE || level > 4 {
                    return None;
                }
                Some(if inked(x, row, TONES[level]) {
                    tint
                } else {
                    track
                })
            });
            if let Some((c, r, ix)) = hovered {
                let d = &days[ix];
                let mut rows = vec![];
                if d.count() == 0 {
                    rows.push((track, "Contributions".into(), "None".into()));
                } else {
                    for (name, n) in [
                        ("Commits", d.commits),
                        ("Reviews", d.reviews),
                        ("PRs opened", d.opened),
                        ("PRs merged", d.merged),
                    ] {
                        if n > 0 {
                            rows.push((tint, name.into(), n.to_string()));
                        }
                    }
                    rows.push((
                        content(0.3),
                        "Lines".into(),
                        format!("+{} −{}", d.additions, d.deletions),
                    ));
                }
                readout(
                    &d.date.format("%a, %b %-d").to_string(),
                    &rows,
                    grid.origin + point(px(c as f32 * pitch + square), px(r as f32 * pitch)),
                    bounds,
                    window,
                    cx,
                );
            }
        },
    )
    .size_full();
    hoverable("mono-contributions-graph".into())
        .debug_selector(|| "mono-contributions-graph".into())
        .w_full()
        .h(px(height))
        .child(map)
        .into_any_element()
}

/// How far list rows reach past the section's edge: their hover fill gets
/// room around the content, which still lines up with the section labels.
const ROW_HANG: f32 = 6.;

fn section(title: &'static str) -> Div {
    div()
        .flex_shrink_0()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .px_1()
                .text_size(px(11.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(content(0.4))
                .child(title),
        )
}

fn tile(value: AnyElement, label: &'static str) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .px_3()
        .py(px(10.))
        .rounded_lg()
        .border_1()
        .border_color(content(0.05))
        .bg(content(0.03))
        .flex()
        .flex_col()
        .gap_0p5()
        .child(value)
        .child(
            div()
                .text_size(px(11.))
                .text_color(content(0.45))
                .truncate()
                .child(label),
        )
}

fn figure(text: String) -> Div {
    div()
        .text_size(px(20.))
        .line_height(px(26.))
        .font_weight(FontWeight::SEMIBOLD)
        .font_features(tabular())
        .text_color(content(0.95))
        .child(text)
}

impl Monos {
    /// The Mono's contributions as last loaded, reloading them in the
    /// background once they are stale. Drawing never touches the store.
    fn contributions_for(
        &mut self,
        mono: &Mono,
        today: NaiveDate,
        cx: &mut Context<Self>,
    ) -> (Arc<Vec<Day>>, usize) {
        let loaded = self.contributions.get(&mono.id);
        let stale = loaded.is_none_or(|l| l.at.elapsed() > FRESH);
        if stale && self.contributions_loading.insert(mono.id.clone()) {
            let landed = loaded.is_none_or(|l| l.landed_checked.elapsed() > LANDED_EVERY);
            let mut sessions: Vec<i64> = mono.session_id.into_iter().collect();
            sessions.extend(mono.launches.iter().map(|l| l.session_id));
            sessions.extend(mono.habit_sessions.iter().copied());
            sessions.sort_unstable();
            sessions.dedup();
            let store = self.store.clone();
            let id = mono.id.clone();
            let work = cx.background_executor().spawn(async move {
                for session in &sessions {
                    scan(&store, *session);
                }
                if landed {
                    check_landed(&store, &sessions);
                }
                let since = DateTime::<Local>::from_naive_utc_and_offset(
                    first_day(today).and_hms_opt(0, 0, 0).unwrap_or_default(),
                    *Local::now().offset(),
                )
                .timestamp_millis()
                .max(0) as u64;
                let items = locked(&store)
                    .and_then(|s| s.contributions(&sessions, since.saturating_sub(DAY_MS)))
                    .unwrap_or_default();
                (days_from(&items, today), merged_on(&items, today), landed)
            });
            cx.spawn(async move |this, cx| {
                let (days, merged_today, landed) = work.await;
                let _ = this.update(cx, |this, cx| {
                    this.contributions_loading.remove(&id);
                    let landed_checked = match this.contributions.get(&id) {
                        Some(old) if !landed => old.landed_checked,
                        _ => Instant::now(),
                    };
                    this.contributions.insert(
                        id,
                        Loaded {
                            at: Instant::now(),
                            days: Arc::new(days),
                            merged_today,
                            landed_checked,
                        },
                    );
                    cx.notify();
                });
            })
            .detach();
        }
        match self.contributions.get(&mono.id) {
            Some(l) => (l.days.clone(), l.merged_today),
            None => (Arc::new(days_from(&[], today)), 0),
        }
    }

    pub(super) fn dashboard(
        &mut self,
        mono: &Mono,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let shown = match &self.dashboard_shown {
            Some((id, at)) if *id == mono.id => *at,
            _ => {
                let at = Instant::now();
                self.dashboard_shown = Some((mono.id.clone(), at));
                at
            }
        };
        let now = now_ms();
        let today = Local::now().date_naive();
        let tint: Hsla = rgb(mono.color).into();
        let views = self
            .owner
            .upgrade()
            .map(|owner| owner.read(cx).mono_session_views(cx))
            .unwrap_or_default();
        let mine: Vec<_> = views
            .iter()
            .filter(|v| mono.launches.iter().any(|l| l.session_id == v.id))
            .collect();
        let state = self
            .chats
            .get(&mono.id)
            .map(|chat| chat.read(cx).mono_status())
            .unwrap_or("idle");
        let busy: Vec<_> = mine.iter().filter(|v| v.busy).collect();
        let state = match state {
            "idle" if !busy.is_empty() => "working",
            s => s,
        };

        // Identity, with a live line about what it is doing right now.
        let (live, ticker) = match (state, busy.first()) {
            ("needs-you", _) => (true, "Waiting for your input".to_string()),
            (_, Some(v)) if busy.len() > 1 => (
                true,
                format!("Working on {} +{} more", v.title, busy.len() - 1),
            ),
            (_, Some(v)) => (true, format!("Working on {}", v.title)),
            ("working", None) => (true, "Thinking…".to_string()),
            _ => match mine.iter().map(|v| v.updated_at).max().filter(|t| *t > 0) {
                Some(at) => (
                    false,
                    format!("Idle · last active {} ago", relative(at, now)),
                ),
                None => (false, "Idle".to_string()),
            },
        };
        let header = div()
            .flex_shrink_0()
            .w_full()
            .min_w_0()
            .px_4()
            .pt_4()
            .pb_3()
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .size(px(48.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(avatar::avatar(
                        &mono.mascot,
                        mono.color,
                        state,
                        44.,
                        state == "idle",
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(15.))
                            .line_height(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(content(1.))
                            .child(mono.label()),
                    )
                    .child(
                        div()
                            .debug_selector(|| "mono-dashboard-ticker".into())
                            .min_w_0()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_size(px(12.))
                            .text_color(if state == "needs-you" {
                                accent_color()
                            } else {
                                content(0.55)
                            })
                            .when(live, |d| {
                                d.child(div().size(px(6.)).flex_shrink_0().rounded_full().bg(
                                    if state == "needs-you" {
                                        accent_color()
                                    } else {
                                        tint
                                    },
                                ))
                            })
                            .child(div().min_w_0().truncate().child(ticker)),
                    ),
            );

        // Contributions: placeholder history until they are recorded.
        let (days, merged_today) = self.contributions_for(mono, today, cx);
        let total: u32 = days.iter().map(Day::count).sum();
        let streak = streak(&days);
        let today_stats = *days.last().expect("the graph always includes today");
        let contributions = section("Contributions")
            .child(
                div()
                    .px_1()
                    .flex()
                    .items_end()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .font_features(tabular())
                            .text_color(content(0.9))
                            .child(format_integer(total as i64)),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(content(0.5))
                            .child("in the last 6 months"),
                    )
                    .when(streak > 1, |d| {
                        d.child(
                            div()
                                .ml_auto()
                                .text_size(px(11.))
                                .font_features(tabular())
                                .text_color(content(0.5))
                                .child(format!("{streak}-day streak")),
                        )
                    }),
            )
            .child(graph(days, accent_color(), Motion(shown)));

        // Today's numbers: code from contributions, work from the Mono.
        let sessions_today = mine
            .iter()
            .filter(|v| local_day(v.updated_at) == Some(today))
            .count();
        let ran_today: usize = self
            .habits
            .iter()
            .map(|h| {
                h.runs
                    .iter()
                    .filter(|r| local_day(r.at) == Some(today))
                    .count()
            })
            .sum();
        let still_today = self
            .habits
            .iter()
            .filter(|h| h.enabled && h.next_run_at > now && local_day(h.next_run_at) == Some(today))
            .count();
        let lines = div()
            .flex()
            .items_end()
            .gap_1p5()
            .text_size(px(20.))
            .line_height(px(26.))
            .font_weight(FontWeight::SEMIBOLD)
            .font_features(tabular())
            .child(
                div()
                    .text_color(rgb(DIFF_ADD))
                    .child(format!("+{}", format_integer(today_stats.additions as i64))),
            )
            .child(
                div()
                    .text_color(rgb(DIFF_DEL))
                    .child(format!("−{}", format_integer(today_stats.deletions as i64))),
            );
        let today_tiles = section("Today")
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(tile(
                        figure(merged_today.to_string()).into_any_element(),
                        "PRs merged",
                    ))
                    .child(tile(lines.into_any_element(), "Lines changed")),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(tile(
                        figure(sessions_today.to_string()).into_any_element(),
                        "Sessions",
                    ))
                    .child(tile(
                        figure(format!("{ran_today}/{}", ran_today + still_today))
                            .into_any_element(),
                        "Habits run",
                    )),
            );

        // Up next: the soonest scheduled habits, running ones first.
        let mut upcoming: Vec<_> = self
            .habits
            .iter()
            .filter(|h| h.enabled || self.habit_running_since(h).is_some())
            .map(|h| (self.habit_running_since(h), h))
            .collect();
        upcoming.sort_by_key(|(running, h)| (running.is_none(), h.next_run_at));
        let up_next = section("Up next").children(upcoming.is_empty().then(|| {
            div()
                .px_1()
                .text_size(px(12.))
                .line_height(px(18.))
                .text_color(content(0.45))
                .child(format!(
                    "Nothing scheduled. Ask {} to put recurring work on a habit.",
                    mono.label()
                ))
                .into_any_element()
        }));
        let up_next = up_next.children(upcoming.into_iter().take(3).map(|(running, h)| {
            let id = h.id.clone();
            let at = local_day(h.next_run_at);
            // Today's runs give their time, later ones their weekday.
            let when = match running {
                Some(since) => format!("Running · {}", relative(since, now)),
                None => {
                    let day = DateTime::from_timestamp_millis(h.next_run_at as i64)
                        .map(|t| {
                            let t = t.with_timezone(&Local);
                            t.format(if at == Some(today) { "%H:%M" } else { "%a" })
                                .to_string()
                        })
                        .unwrap_or_default();
                    match relative(now, h.next_run_at.max(now)) {
                        soon if soon == "now" => format!("{day} · soon"),
                        later => format!("{day} · in {later}"),
                    }
                }
            };
            div()
                .id(SharedString::from(format!("mono-dashboard-habit-{id}")))
                .role(Role::Button)
                .aria_label(h.name.clone())
                .mx(px(-ROW_HANG))
                .px(px(ROW_HANG + 4.))
                .py_2()
                .rounded_md()
                .flex()
                .items_center()
                .gap_2()
                .hover(|s| s.bg(content(0.05)))
                .child(
                    div()
                        .size(px(28.))
                        .flex_shrink_0()
                        .rounded_md()
                        .bg(content(0.06))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(if running.is_some() {
                            div()
                                .size(px(8.))
                                .rounded_full()
                                .bg(tint)
                                .into_any_element()
                        } else {
                            icon("clock", 14.)
                                .text_color(content(0.6))
                                .into_any_element()
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(content(0.85))
                                .child(h.name.clone()),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(11.))
                                .text_color(content(0.45))
                                .child(h.schedule.label()),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(11.))
                        .font_features(tabular())
                        .text_color(if running.is_some() {
                            tint
                        } else {
                            content(0.4)
                        })
                        .child(when),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.route = Some(Route::Habit(id.clone()));
                    cx.notify();
                }))
        }));

        // New today: what the Mono wrote, newest first.
        let mut fresh: Vec<_> = mono
            .documents
            .iter()
            .filter(|d| local_day(d.created_at) == Some(today))
            .collect();
        fresh.sort_by_key(|d| std::cmp::Reverse(d.created_at));
        let new_today = section("New today").children(fresh.is_empty().then(|| {
            div()
                .px_1()
                .text_size(px(12.))
                .text_color(content(0.45))
                .child("Nothing new yet today.")
                .into_any_element()
        }));
        let new_today = new_today.children(fresh.into_iter().take(4).map(|doc| {
            let id = doc.id.clone();
            div()
                .id(SharedString::from(format!("mono-dashboard-doc-{id}")))
                .role(Role::Button)
                .aria_label(doc.title.clone())
                .mx(px(-ROW_HANG))
                .px(px(ROW_HANG + 4.))
                .py_2()
                .rounded_md()
                .flex()
                .items_center()
                .gap_2()
                .hover(|s| s.bg(content(0.05)))
                .child(
                    div()
                        .size(px(28.))
                        .flex_shrink_0()
                        .rounded_md()
                        .bg(content(0.06))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("file", 14.).text_color(content(0.6))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(content(0.85))
                                .child(doc.title.clone()),
                        )
                        .when(!doc.summary.trim().is_empty(), |d| {
                            d.child(
                                div()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(content(0.45))
                                    .child(doc.summary.clone()),
                            )
                        }),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(11.))
                        .text_color(content(0.4))
                        .child(relative(doc.created_at, now)),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.route = Some(Route::Document(id.clone()));
                    cx.notify();
                }))
        }));

        div()
            .id("mono-dashboard")
            .debug_selector(|| "mono-dashboard".into())
            .size_full()
            .min_w_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .child(header)
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .px_3()
                    .pt_2()
                    .pb_6()
                    .flex()
                    .flex_col()
                    .gap_6()
                    .child(contributions)
                    .child(today_tiles)
                    .child(up_next)
                    .child(new_today),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(count: u32) -> Day {
        Day {
            date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            commits: count,
            reviews: 0,
            opened: 0,
            merged: 0,
            additions: 0,
            deletions: 0,
        }
    }

    fn at(date: NaiveDate, hour: u32) -> u64 {
        date.and_hms_opt(hour, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .timestamp_millis() as u64
    }

    fn item(kind: Kind, reference: &str, at: u64) -> Contribution {
        Contribution {
            session: 1,
            kind,
            reference: reference.into(),
            at,
            additions: 10,
            deletions: 2,
        }
    }

    #[test]
    fn days_cover_whole_weeks_and_count_each_kind() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 29).unwrap();
        let yesterday = today - Days::days(1);
        let items = [
            item(Kind::Commit, "a", at(today, 9)),
            item(Kind::Commit, "b", at(today, 10)),
            item(Kind::Review, "acme/app#1@x", at(today, 11)),
            item(Kind::Opened, "acme/app#2", at(yesterday, 12)),
            item(Kind::Landed, "acme/app#2", at(today, 13)),
            item(Kind::Merged, "acme/app#3", at(today, 14)),
            // Older than the graph.
            item(Kind::Commit, "c", at(today - Days::weeks(40), 9)),
        ];
        let days = days_from(&items, today);
        assert_eq!(days.first().unwrap().date.weekday(), chrono::Weekday::Sun);
        assert_eq!(days.len() as i64, (WEEKS - 1) * 7 + 5);
        let last = days.last().unwrap();
        assert_eq!((last.commits, last.reviews, last.merged), (2, 1, 1));
        assert_eq!((last.additions, last.deletions), (20, 4));
        // Landing counts toward merged pull requests, not contributions.
        assert_eq!(last.count(), 4);
        assert_eq!(days[days.len() - 2].opened, 1);
        assert_eq!(days.iter().map(Day::count).sum::<u32>(), 5);
        assert_eq!(merged_on(&items, today), 2);
        assert_eq!(merged_on(&items, yesterday), 0);
    }

    #[test]
    fn levels_split_active_days_by_quartile() {
        let days: Vec<_> = [0, 1, 2, 3, 4, 5, 6, 7, 8].into_iter().map(day).collect();
        assert_eq!(levels(&days), [0, 1, 1, 1, 2, 2, 3, 3, 4]);
        assert_eq!(levels(&[day(0), day(0)]), [0, 0]);
        assert_eq!(levels(&[day(3)]), [1]);
    }

    #[test]
    fn streaks_count_back_from_today_or_yesterday() {
        let run = |counts: &[u32]| streak(&counts.iter().map(|c| day(*c)).collect::<Vec<_>>());
        assert_eq!(run(&[1, 0, 2, 3, 1]), 3);
        assert_eq!(run(&[1, 2, 3, 0]), 3);
        assert_eq!(run(&[1, 0, 0]), 0);
    }
}
