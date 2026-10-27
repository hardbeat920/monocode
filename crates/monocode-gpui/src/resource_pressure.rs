// ResourcePressureNotice.tsx: a footer chip while the machine is under
// pressure, opening the running work MonoCode owns so it can be stopped.
use crate::{popover::AnchorSlot, right_panel::spinner, theme::*};
use gpui::{
    AnyElement, Context, EventEmitter, FocusHandle, FontWeight, Role, ScrollHandle, SharedString,
    Task, Window, div, prelude::*, px, rgb,
};
use monocode_core::resources::{self, OwnedProcess, Pressure, Sample};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

const TICK: Duration = Duration::from_millis(500);
const POLL: Duration = Duration::from_secs(3);
const AMBER: u32 = 0xfbbf24;
const EMERALD: u32 = 0x10b981;
const RED: u32 = 0xf87171;

/// What the dialog shows for a session that owns running work.
pub(crate) struct SessionInfo {
    pub(crate) title: SharedString,
    pub(crate) provider: String,
}

/// Native turns run one provider process each; stopping it is stopping the turn.
pub(crate) struct StopSession(pub(crate) i64);

pub(crate) struct ResourceMonitor {
    pressure: Pressure,
    open: bool,
    classic: bool,
    processes: Vec<OwnedProcess>,
    loading: bool,
    error: Option<String>,
    stop_error: Option<String>,
    stopping: Option<String>,
    expanded: HashSet<i64>,
    trigger: FocusHandle,
    dialog: FocusHandle,
    previous_focus: Option<FocusHandle>,
    bounds: AnchorSlot,
    scroll: ScrollHandle,
    _sampler: Option<Task<()>>,
    refresh: Option<Task<()>>,
}
impl EventEmitter<StopSession> for ResourceMonitor {}
impl ResourceMonitor {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        // Tests drive `observe` directly rather than the machine's load.
        let sampler = (!cfg!(test)).then(|| {
            cx.spawn(async move |this, cx| {
                let mut last_tick = Instant::now();
                let mut last_poll = last_tick;
                let mut stalls = Duration::ZERO;
                loop {
                    cx.background_executor().timer(TICK).await;
                    let now = Instant::now();
                    let late = now
                        .saturating_duration_since(last_tick)
                        .saturating_sub(TICK);
                    last_tick = now;
                    // A hidden app's timers are throttled; that is not a stall.
                    if cx.update(|cx| cx.active_window().is_none()) {
                        stalls = Duration::ZERO;
                        last_poll = now;
                        if this.update(cx, |this, _| this.pressure.pause()).is_err() {
                            break;
                        }
                        continue;
                    }
                    if late > Duration::from_millis(100) {
                        stalls += late;
                    }
                    let elapsed = now.duration_since(last_poll);
                    if elapsed < POLL {
                        continue;
                    }
                    let stall_share = (stalls.as_secs_f64() / elapsed.as_secs_f64()).min(1.);
                    stalls = Duration::ZERO;
                    last_poll = now;
                    let usage = cx
                        .background_executor()
                        .spawn(async { resources::usage() })
                        .await;
                    last_tick = Instant::now();
                    let sample = Sample { usage, stall_share };
                    if this
                        .update(cx, |this, cx| this.observe(sample, now, cx))
                        .is_err()
                    {
                        break;
                    }
                }
            })
        });
        Self {
            pressure: Pressure::default(),
            open: false,
            classic: false,
            processes: Vec::new(),
            loading: false,
            error: None,
            stop_error: None,
            stopping: None,
            expanded: HashSet::new(),
            trigger: cx.focus_handle(),
            dialog: cx.focus_handle(),
            previous_focus: None,
            bounds: AnchorSlot::default(),
            scroll: ScrollHandle::new(),
            _sampler: sampler,
            refresh: None,
        }
    }
    fn observe(&mut self, sample: Sample, now: Instant, cx: &mut Context<Self>) {
        let before = (self.pressure.active, self.pressure.reasons.clone());
        self.pressure.sample(sample, now);
        if before != (self.pressure.active, self.pressure.reasons.clone()) {
            cx.notify();
        }
    }
    #[cfg(test)]
    pub(crate) fn saturate(&mut self, cx: &mut Context<Self>) {
        let high = Sample {
            usage: resources::Usage {
                cpu_percent: Some(97.),
                memory_pressure: Some(4),
            },
            stall_share: 0.,
        };
        let start = Instant::now();
        for s in 0..3 {
            self.observe(high, start + Duration::from_secs(s * 3), cx);
        }
    }
    pub(crate) fn set_classic(&mut self, classic: bool, cx: &mut Context<Self>) {
        if self.classic != classic {
            self.classic = classic;
            cx.notify();
        }
    }
    /// The chip shows only under pressure; the dialog stays until closed.
    pub(crate) fn chip_shown(&self) -> bool {
        self.pressure.active
    }
    pub(crate) fn is_open(&self) -> bool {
        self.open
    }
    pub(crate) fn measured_height(&self) -> Option<f32> {
        self.bounds.get().map(|b| b.size.height.as_f32())
    }
    fn reasons(&self) -> String {
        self.pressure
            .reasons
            .iter()
            .map(|r| r.label())
            .collect::<Vec<_>>()
            .join(" · ")
    }
    pub(crate) fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            return;
        }
        self.previous_focus = window.focused(cx).filter(|f| *f != self.trigger);
        self.open = true;
        self.loading = true;
        self.error = None;
        self.stop_error = None;
        window.focus(&self.dialog, cx);
        self.refresh(cx);
        cx.notify();
    }
    pub(crate) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        self.refresh = None;
        self.processes.clear();
        self.expanded.clear();
        let focus = self.previous_focus.take();
        if self.pressure.active {
            window.focus(focus.as_ref().unwrap_or(&self.trigger), cx);
        } else if let Some(focus) = focus {
            window.focus(&focus, cx);
        }
        cx.notify();
    }
    /// Lists owned processes now, then every 3s while the dialog is open.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh = Some(cx.spawn(async move |this, cx| {
            loop {
                let rows = cx
                    .background_executor()
                    .spawn(async { resources::processes() })
                    .await;
                if this
                    .update(cx, |this, cx| {
                        this.loading = false;
                        this.stopping = None;
                        match rows {
                            Ok(rows) => {
                                this.processes = stable_order(&this.processes, rows);
                                this.error = None;
                            }
                            Err(error) => this.error = Some(error),
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
                cx.background_executor().timer(POLL).await;
            }
        }));
    }
    fn stop_session(&mut self, id: i64, cx: &mut Context<Self>) {
        self.stop_error = None;
        self.stopping = Some(format!("session:{id}"));
        cx.emit(StopSession(id));
        self.refresh(cx);
        cx.notify();
    }
    fn stop_tool(&mut self, pid: u32, identity: String, cx: &mut Context<Self>) {
        self.stop_error = None;
        self.stopping = Some(identity.clone());
        // A user's stop request must not wait behind the next listing.
        let work = cx
            .background_executor()
            .spawn(async move { resources::stop_tool(pid, &identity) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.stopping = None;
                    this.stop_error = Some(error);
                }
                this.refresh(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// The dialog body, inside the shell's modal surface.
    pub(crate) fn dialog(
        &mut self,
        sessions: &HashMap<i64, SessionInfo>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.pressure.active;
        let reasons = self.reasons();
        let mut groups: Vec<(i64, Vec<&OwnedProcess>)> = Vec::new();
        for process in &self.processes {
            match groups.iter_mut().find(|(id, _)| *id == process.session) {
                Some((_, rows)) => rows.push(process),
                None => groups.push((process.session, vec![process])),
            }
        }
        let busy = self.stopping.is_some();
        let list_height = 320f32.min(window.viewport_size().height.as_f32() * 0.45);
        let error_line = |id: &'static str, text: &'static str| {
            div()
                .id(id)
                .role(Role::Alert)
                .mt_3()
                .text_size(px(12.))
                .text_color(rgb(RED))
                .child(text)
        };
        div()
            .id("resource-dialog")
            .debug_selector(|| "resource-dialog".into())
            .role(Role::Dialog)
            .aria_label("Running work")
            .track_focus(&self.dialog)
            .relative()
            .flex()
            .flex_col()
            .text_color(content(1.))
            .child(self.bounds.layout_probe())
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.close(window, cx);
                    window.prevent_default();
                    cx.stop_propagation();
                }
            }))
            // Modal.tsx's header.
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_start()
                    .gap_2()
                    .px_4()
                    .pt_3()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .pt(px(2.))
                            .text_size(px(16.))
                            .line_height(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Running work"),
                    )
                    .child(
                        icon_button("close-running-work", "Close", "close")
                            .size(px(28.))
                            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    ),
            )
            .child(
                div()
                    .p_4()
                    .text_size(px(13.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .gap(px(10.))
                            .rounded_lg()
                            .bg(content(0.04))
                            .px_3()
                            .py(px(10.))
                            .child(
                                div().mt(px(2.)).child(if active {
                                    icon("circle-alert", 16.).text_color(rgb(AMBER))
                                } else {
                                    icon("check", 16.).text_color(rgb(EMERALD))
                                }),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .text_size(px(12.))
                                    .child(div().font_weight(FontWeight::MEDIUM).child(if active {
                                        reasons
                                    } else {
                                        "Usage is back to normal".into()
                                    }))
                                    .child(
                                        div()
                                            .mt(px(2.))
                                            .line_height(px(19.5))
                                            .text_color(content(0.55))
                                            .child(if active {
                                                "Things may feel slower. Stop work you no longer need."
                                            } else {
                                                "The slowdown has cleared. You can close this window."
                                            }),
                                    ),
                            ),
                    )
                    .when(self.error.is_some(), |d| {
                        d.child(error_line(
                            "resource-load-error",
                            "Couldn't load running work. Trying again…",
                        ))
                    })
                    .when(self.stop_error.is_some(), |d| {
                        d.child(error_line(
                            "resource-stop-error",
                            "Couldn't stop this work. Please try again.",
                        ))
                    })
                    .when(self.loading && self.processes.is_empty(), |d| {
                        d.child(
                            div()
                                .id("resource-loading-status")
                                .role(Role::Status)
                                .py_8()
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap_2()
                                .text_size(px(12.))
                                .text_color(content(0.5))
                                .child(spinner("resource-loading", "loader", 14.))
                                .child("Checking running work…"),
                        )
                    })
                    .when(
                        !self.loading && self.processes.is_empty() && self.error.is_none(),
                        |d| {
                            d.child(
                                div()
                                    .py_6()
                                    .text_center()
                                    .text_size(px(12.))
                                    .line_height(px(19.5))
                                    .text_color(content(0.5))
                                    .child(if active {
                                        "No running work found in MonoCode. Other apps may be using these resources."
                                    } else {
                                        "No running work found in MonoCode."
                                    }),
                            )
                        },
                    )
                    .when(!groups.is_empty(), |d| {
                        let count = groups.len();
                        let rows: Vec<AnyElement> = groups
                            .iter()
                            .enumerate()
                            .map(|(ix, (id, rows))| {
                                self.session_group(ix, *id, rows, sessions.get(id), busy, cx)
                            })
                            .collect();
                        d.child(
                            div()
                                .mt_4()
                                .mb_1()
                                .text_size(px(11.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(content(0.45))
                                .child(format!(
                                    "{count} {} running",
                                    if count == 1 { "session" } else { "sessions" }
                                )),
                        )
                        .child(
                            div()
                                .id("running-sessions")
                                .role(Role::List)
                                .aria_label("Running sessions")
                                .max_h(px(list_height))
                                .overflow_y_scroll()
                                .track_scroll(&self.scroll)
                                .children(rows),
                        )
                        .child(
                            div()
                                .mt_3()
                                .pt_3()
                                .border_t_1()
                                .border_color(content(0.07))
                                .text_size(px(11.))
                                .text_color(content(0.4))
                                .child("Stopping work keeps your conversations saved."),
                        )
                    }),
            )
            .into_any_element()
    }

    fn session_group(
        &self,
        ix: usize,
        id: i64,
        rows: &[&OwnedProcess],
        session: Option<&SessionInfo>,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title: SharedString = session
            .map(|s| s.title.clone())
            .unwrap_or_else(|| "Background work".into());
        let memory: u64 = rows.iter().map(|r| r.memory_bytes).sum();
        let expanded = self.expanded.contains(&id);
        let stopping_session = self.stopping.as_deref() == Some(&format!("session:{id}"));
        div()
            .id(("resource-session", id as u64))
            .role(Role::ListItem)
            .py_3()
            .when(ix > 0, |d| d.border_t_1().border_color(content(0.07)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .size(px(24.))
                            .flex_shrink_0()
                            .rounded_md()
                            .bg(content(0.05))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(match session {
                                Some(s) => provider_icon(&s.provider, 14.).into_any_element(),
                                None => icon("circle-alert", 14.).into_any_element(),
                            }),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(title.clone()),
                    )
                    .when(session.is_some(), |d| {
                        d.child(
                            div()
                                .id(("stop-session", id as u64))
                                .role(Role::Button)
                                .aria_label(format!("Stop session {title}"))
                                .h(px(28.))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .rounded_md()
                                .bg(content(0.06))
                                .px(px(10.))
                                .text_size(px(12.))
                                .text_color(content(0.7))
                                .map(|d| {
                                    if busy {
                                        d.opacity(0.4)
                                    } else {
                                        d.cursor_pointer()
                                            .hover(|s| s.bg(red(0.1)).text_color(rgb(RED)))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.stop_session(id, cx)
                                            }))
                                    }
                                })
                                .when(stopping_session, |d| {
                                    d.child(spinner(
                                        ("stop-session-spin", id as u64),
                                        "loader",
                                        12.,
                                    ))
                                })
                                .child(if stopping_session {
                                    "Stopping…"
                                } else {
                                    "Stop"
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .id(("resource-processes", id as u64))
                    .role(Role::Button)
                    .aria_expanded(expanded)
                    .ml(px(34.))
                    .mt(px(2.))
                    .flex()
                    .items_center()
                    .gap_1()
                    .cursor_pointer()
                    .text_size(px(11.))
                    .text_color(content(0.45))
                    .hover(|s| s.text_color(content(0.75)))
                    .child(icon(
                        if expanded {
                            "chevron-down"
                        } else {
                            "chevron-right"
                        },
                        12.,
                    ))
                    .child(format!(
                        "{} {} · {} memory",
                        rows.len(),
                        if rows.len() == 1 {
                            "process"
                        } else {
                            "processes"
                        },
                        format_memory(memory)
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.expanded.remove(&id) {
                            this.expanded.insert(id);
                        }
                        cx.notify();
                    })),
            )
            .when(expanded, |d| {
                d.child(
                    div()
                        .ml(px(34.))
                        .mt_2()
                        .pl(px(10.))
                        .border_l_1()
                        .border_color(content(0.07))
                        .flex()
                        .flex_col()
                        .gap_2()
                        .children(rows.iter().map(|process| {
                            self.process_row(process, &title, session.is_some(), busy, cx)
                        })),
                )
            })
            .into_any_element()
    }

    fn process_row(
        &self,
        process: &OwnedProcess,
        title: &SharedString,
        has_session: bool,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (pid, agent, session) = (process.pid, process.agent, process.session);
        let identity = process.identity.clone();
        let disabled = busy || (agent && !has_session);
        let stopping = self.stopping.as_deref() == Some(identity.as_str());
        div()
            .flex()
            .min_w_0()
            .items_center()
            .gap_2()
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .child(
                        div()
                            .truncate()
                            .font_family(crate::fonts::mono())
                            .text_size(px(11.))
                            .text_color(content(0.65))
                            .child(process.command.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(10.))
                            .text_color(content(0.4))
                            .child(format!(
                                "{:.0}% CPU · {} · PID {pid}",
                                process.cpu_percent,
                                format_memory(process.memory_bytes)
                            )),
                    ),
            )
            .child(
                div()
                    .id(("stop-process", pid as u64))
                    .role(Role::Button)
                    .aria_label(format!(
                        "{} for {title} (PID {pid})",
                        if agent {
                            "Stop agent"
                        } else {
                            "Stop build/tool"
                        }
                    ))
                    .flex_shrink_0()
                    .rounded_sm()
                    .px(px(6.))
                    .py_1()
                    .text_size(px(11.))
                    .text_color(content(0.5))
                    .map(|d| {
                        if disabled {
                            d.opacity(0.4)
                        } else {
                            d.cursor_pointer()
                                .hover(|s| s.bg(red(0.1)).text_color(rgb(RED)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if agent {
                                        this.stop_session(session, cx);
                                    } else {
                                        this.stop_tool(pid, identity.clone(), cx);
                                    }
                                }))
                        }
                    })
                    .child(if stopping { "Stopping…" } else { "Stop" }),
            )
            .into_any_element()
    }
}

impl Render for ResourceMonitor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shown = self.pressure.active;
        let classic = self.classic;
        let open = self.open;
        div().when(shown, |d| {
            d.child(
                div()
                    .id("resource-pressure-chip")
                    .debug_selector(|| "resource-pressure-chip".into())
                    .role(Role::Button)
                    .aria_label("High resource usage")
                    .aria_description(format!("{} · Review running work", self.reasons()))
                    .aria_expanded(open)
                    .track_focus(&self.trigger)
                    .tab_index(0)
                    .h(px(20.))
                    .map(|d| {
                        if classic {
                            d.px(px(6.)).rounded_md()
                        } else {
                            d.px_2().rounded_full()
                        }
                    })
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(6.))
                    .whitespace_nowrap()
                    .text_size(px(11.))
                    .cursor_pointer()
                    .map(|d| {
                        if open {
                            d.bg(content(0.08)).text_color(content(1.))
                        } else {
                            d.text_color(content(0.55))
                        }
                    })
                    .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                    .child(icon("circle-alert", 14.).text_color(rgb(AMBER)))
                    .child("High usage")
                    .on_click(cx.listener(|this, _, window, cx| this.open(window, cx)))
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.open(window, cx);
                            cx.stop_propagation();
                        }
                    })),
            )
        })
    }
}

fn red(alpha: f32) -> gpui::Hsla {
    let mut color: gpui::Hsla = rgb(RED).into();
    color.a = alpha;
    color
}

/// Keep controls under the pointer while CPU usage changes between polls:
/// known processes keep their place and sessions keep their order.
fn stable_order(previous: &[OwnedProcess], rows: Vec<OwnedProcess>) -> Vec<OwnedProcess> {
    let mut remaining: Vec<Option<OwnedProcess>> = rows.into_iter().map(Some).collect();
    let mut ordered = Vec::new();
    for old in previous {
        if let Some(row) = remaining
            .iter_mut()
            .find(|row| row.as_ref().is_some_and(|r| r.identity == old.identity))
        {
            ordered.extend(row.take());
        }
    }
    ordered.extend(remaining.into_iter().flatten());
    let mut sessions: Vec<i64> = Vec::new();
    for row in previous {
        if !sessions.contains(&row.session) {
            sessions.push(row.session);
        }
    }
    let rank = |id: i64| {
        sessions
            .iter()
            .position(|s| *s == id)
            .unwrap_or(sessions.len())
    };
    // A stable sort keeps the order above within each session.
    ordered.sort_by_key(|row| rank(row.session));
    ordered
}

fn format_memory(bytes: u64) -> String {
    let mb = bytes as f64 / 1024. / 1024.;
    if mb >= 1024. {
        format!("{:.1} GB", mb / 1024.)
    } else if mb < 1. {
        "<1 MB".into()
    } else {
        format!("{} MB", mb.round())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::resources::Usage;

    fn row(pid: u32, session: i64, cpu: f64) -> OwnedProcess {
        OwnedProcess {
            pid,
            identity: pid.to_string(),
            session,
            command: format!("process {pid}"),
            cpu_percent: cpu,
            memory_bytes: 0,
            agent: false,
        }
    }

    #[test]
    fn known_rows_keep_their_place_as_usage_changes() {
        let first = vec![row(1, 10, 90.), row(2, 20, 50.), row(3, 10, 10.)];
        let first = stable_order(&[], first);
        let next = vec![
            row(3, 10, 99.),
            row(4, 30, 80.),
            row(2, 20, 70.),
            row(1, 10, 1.),
        ];
        let pids: Vec<_> = stable_order(&first, next).iter().map(|r| r.pid).collect();
        assert_eq!(pids, [1, 3, 2, 4]);
    }

    #[test]
    fn memory_reads_like_the_react_dialog() {
        assert_eq!(format_memory(512 * 1024), "<1 MB");
        assert_eq!(format_memory(300 * 1024 * 1024), "300 MB");
        assert_eq!(format_memory(3 * 1024 * 1024 * 1024 / 2), "1.5 GB");
    }

    #[gpui::test]
    fn chip_opens_running_work_and_escape_restores_focus(cx: &mut gpui::TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| cx.new(ResourceMonitor::new))
                .unwrap()
        });
        window
            .update(cx, |monitor, w, cx| {
                let high = Sample {
                    usage: Usage {
                        cpu_percent: Some(97.),
                        memory_pressure: Some(4),
                    },
                    stall_share: 0.,
                };
                let start = Instant::now();
                for s in 0..3 {
                    monitor.observe(high, start + Duration::from_secs(s * 3), cx);
                }
                assert!(monitor.chip_shown());
                assert_eq!(monitor.reasons(), "High CPU usage · Low available memory");
                w.focus(&monitor.trigger, cx);
            })
            .unwrap();
        cx.run_until_parked();
        cx.simulate_keystrokes(window.into(), "enter");
        cx.run_until_parked();
        window
            .update(cx, |monitor, w, cx| {
                assert!(monitor.is_open());
                assert!(monitor.dialog.is_focused(w));
                monitor.close(w, cx);
                assert!(!monitor.is_open());
                assert!(monitor.trigger.is_focused(w));
            })
            .unwrap();
    }
}
