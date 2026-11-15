//! `MONOCODE_DRAFT_SELFTEST=page.html` opens one window that hosts only the
//! Draft view, runs the page through the same request path the agent's tools
//! use, and writes `page.html.N.png` and `page.html.N.json` for every step.
//!
//! `MONOCODE_DRAFT_ACTIONS` is a JSON array of further `draft_preview` inputs;
//! `"$draft"` stands for the draft's id. `{"debug":"takeover"}` plays the part
//! of the person clicking in the page; `{"debug":"note"}` pins a note as they
//! would, `{"debug":"message"}` queues a line as a page's mc.send would, `"expand"` and
//! `"collapse"` open and close the full view, `"send"` sends it and `"send_later"` sends it a second from now. `{"debug":"away"}` draws an empty
//! surface in the thread's place, as Settings does, `{"debug":"cover"}` opens a
//! modal's scrim over the thread, `{"debug":"float"}` floats two overlapping
//! controls over the page as the thread's pills do, and `{"debug":"back"}`
//! undoes any of them. A click's `reference` written as
//! `@Name` means the control with that name in the latest snapshot. `_WIDTH`, `_HEIGHT` and `_APPEARANCE`
//! shape the first step, `_URL` opens a dev server instead of the file,
//! `_START=away` starts with the card out of sight, as when the agent opens a
//! draft in a conversation the person is not looking at, and `_HOLD` keeps
//! the window open that many seconds for a screen capture.
use super::Draft;
use gpui::{
    App, Context, Entity, Window, WindowBackgroundAppearance, WindowOptions, div, prelude::*, px,
    size,
};
use monocode_core::visual_bridge::{DraftReply, DraftRequest};
use serde_json::{Value, json};
use std::{ffi::OsString, time::Duration};

/// The draft on its own, or inside a thread when `_THREAD` is set, with
/// whatever the last `away`, `cover` or `back` step left in its place.
struct Root(gpui::AnyView, Entity<Draft>, &'static str);
impl Render for Root {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            .when(self.2 != "away", |root| root.child(self.0.clone()))
            .when(self.2 == "cover", |root| {
                root.child(
                    div()
                        .absolute()
                        .inset_0()
                        .child(crate::modal::scrim("selftest-scrim")),
                )
            })
            .when(self.2 == "float", |root| {
                let control = |left: f32, color: u32| {
                    div()
                        .absolute()
                        .top(px(300.))
                        .left(px(left))
                        .w(px(120.))
                        .h(px(30.))
                        .rounded_full()
                        .bg(gpui::rgb(color))
                        .child(crate::draft::float(px(15.)))
                };
                root.child(control(260., 0xff3b30))
                    .child(control(320., 0x34c759))
            })
            .child(crate::draft::sentinel())
    }
}

fn env_number(name: &str) -> Option<u32> {
    std::env::var(name).ok()?.parse().ok()
}

pub(crate) fn run(path: OsString) {
    if let Some(out) = std::env::var_os("MONOCODE_DRAFT_SELFTEST_PERF") {
        perf::run(path, out);
        return;
    }
    let platform = gpui_platform::current_platform(false);
    gpui::Application::with_platform(platform)
        .with_assets(crate::assets::Assets)
        .run(move |cx: &mut App| {
            crate::popover::bind(cx);
            let mut first = json!({});
            if let Ok(url) = std::env::var("MONOCODE_DRAFT_SELFTEST_URL") {
                first["url"] = url.into();
            } else {
                let html = std::fs::read_to_string(&path).unwrap_or_default();
                first["html"] = monocode_core::visual_bridge::embed_draft_images(&html)
                    .unwrap_or_else(|error| {
                        eprintln!("{error}");
                        html
                    })
                    .into();
            }
            for (key, name) in [
                ("width", "MONOCODE_DRAFT_SELFTEST_WIDTH"),
                ("height", "MONOCODE_DRAFT_SELFTEST_HEIGHT"),
            ] {
                if let Some(value) = env_number(name) {
                    first[key] = value.into();
                }
            }
            if let Ok(appearance) = std::env::var("MONOCODE_DRAFT_SELFTEST_APPEARANCE") {
                first["appearance"] = appearance.into();
            }
            let steps: Vec<Value> = std::env::var("MONOCODE_DRAFT_ACTIONS")
                .ok()
                .and_then(|actions| serde_json::from_str(&actions).ok())
                .unwrap_or_default();
            let (reply, _) = std::sync::mpsc::channel();
            let Ok(opening) = DraftRequest::parse(1, &first, reply) else {
                eprintln!("The first request is invalid");
                cx.quit();
                return;
            };
            let content = opening.content.clone().expect("the first step has content");
            let draft = cx.new(|cx| {
                Draft::new(
                    None,
                    1,
                    content,
                    (opening.width, opening.height),
                    opening.light,
                    None,
                    cx,
                )
            });
            let start = match std::env::var("MONOCODE_DRAFT_SELFTEST_START").as_deref() {
                Ok("away") => "away",
                _ => "back",
            };
            let window_width = env_number("MONOCODE_DRAFT_SELFTEST_PANEL").unwrap_or(460) as f32;
            let root: gpui::AnyView = if std::env::var("MONOCODE_DRAFT_SELFTEST_THREAD").is_ok() {
                let id = draft.read(cx).id().to_owned();
                crate::draft::register(&id, &draft, cx);
                let card = monocode_core::visual_bridge::DraftCard {
                    draft: id,
                    title: "Pricing page".into(),
                };
                crate::transcript::pages::selftest_thread(&card, cx).into()
            } else {
                draft.clone().into()
            };
            let handle = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds::centered(
                            None,
                            size(px(window_width), px(780.)),
                            cx,
                        ))),
                        window_background: WindowBackgroundAppearance::Blurred,
                        ..Default::default()
                    },
                    {
                        let keep = draft.clone();
                        move |_, cx| cx.new(|_| Root(root, keep, start))
                    },
                )
                .expect("open the self-test window");
            let weak = draft.downgrade();
            // A window that nobody is looking at stops drawing; keep frames
            // coming so the page is placed the way a visible panel would be.
            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(120))
                        .await;
                    if handle.update(cx, |_, window, _| window.refresh()).is_err() {
                        return;
                    }
                }
            })
            .detach();
            cx.spawn(async move |cx| {
                // The window has to paint once before the page has a view.
                cx.background_executor().timer(Duration::from_secs(1)).await;
                // The draft already holds the first page; the first step looks
                // at it rather than loading it again.
                let mut id = weak
                    .read_with(cx, |draft, _| draft.id().to_owned())
                    .unwrap_or_default();
                let mut opening = json!({"draft": id});
                for key in ["width", "height", "appearance"] {
                    if let Some(value) = first.get(key) {
                        opening[key] = value.clone();
                    }
                }
                let mut named: Vec<(String, String)> = Vec::new();
                let mut all = vec![opening];
                all.extend(steps);
                // A new page follows its content's height, as it does when the
                // agent opens one, and runs for the agent as the shell has it.
                let lease = handle
                    .update(cx, |root, window, cx| {
                        root.1.update(cx, |draft, cx| draft.lease(window, cx))
                    })
                    .ok();
                if let Ok(web) = Draft::ready(&weak, cx).await {
                    Draft::fit_height(&weak, &web, cx).await;
                }
                drop(lease);
                for (n, mut input) in all.into_iter().enumerate() {
                    let debug = input.get("debug").and_then(Value::as_str);
                    let surface = ["away", "cover", "float", "back"]
                        .into_iter()
                        .find(|surface| debug == Some(surface));
                    let outcome = if let Some(what) = surface {
                        let _ = handle.update(cx, |root, _, cx| {
                            root.2 = what;
                            cx.notify();
                        });
                        cx.background_executor()
                            .timer(Duration::from_millis(400))
                            .await;
                        Ok(json!({"surface": what}))
                    } else if let Some(what) = debug {
                        Draft::debug(&weak, what, cx).await
                    } else {
                        if let Some(draft) = input.get_mut("draft")
                            && draft == "$draft"
                        {
                            *draft = id.clone().into();
                        }
                        if let Some(reference) = input.pointer_mut("/action/reference")
                            && let Some(name) = reference.as_str().and_then(|r| r.strip_prefix('@'))
                            && let Some((_, found)) = named.iter().find(|(n, _)| n == name)
                        {
                            *reference = found.clone().into();
                        }
                        let (reply, _) = std::sync::mpsc::channel();
                        // As the shell does: the page runs for the request
                        // even with its card away.
                        let lease = handle
                            .update(cx, |root, window, cx| {
                                root.1.update(cx, |draft, cx| draft.lease(window, cx))
                            })
                            .ok();
                        let outcome = match DraftRequest::parse(1, &input, reply) {
                            Ok(request) => Draft::run(&weak, &request, cx).await.map(|reply| {
                                if let DraftReply::Snapshot(snapshot) = &reply {
                                    id = snapshot.draft.clone();
                                    named = snapshot
                                        .elements
                                        .iter()
                                        .map(|e| (e.name.clone(), e.reference.clone()))
                                        .collect();
                                    let _ = std::fs::write(
                                        format!("{}.{n}.png", path.to_string_lossy()),
                                        &snapshot.png,
                                    );
                                }
                                match reply {
                                    DraftReply::Snapshot(snapshot) => {
                                        serde_json::to_value(&*snapshot).unwrap_or_default()
                                    }
                                    DraftReply::Closed { draft } => json!({"closed": draft}),
                                }
                            }),
                            Err(error) => Err(error),
                        };
                        drop(lease);
                        outcome
                    };
                    let mut json = match outcome {
                        Ok(value) => value,
                        Err(error) => json!({"error": error}),
                    };
                    json["placed"] = weak
                        .read_with(cx, |draft, _| draft.placement())
                        .ok()
                        .flatten()
                        .map_or(Value::Null, |frame| json!(frame));
                    json["holes"] = weak
                        .read_with(cx, |draft, _| draft.holes())
                        .map_or(Value::Null, |holes| json!(holes));
                    let _ = std::fs::write(
                        format!("{}.{n}.json", path.to_string_lossy()),
                        json.to_string(),
                    );
                }
                if let Some(hold) = env_number("MONOCODE_DRAFT_SELFTEST_HOLD") {
                    cx.background_executor()
                        .timer(Duration::from_secs(hold.into()))
                        .await;
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        });
}

/// `MONOCODE_DRAFT_SELFTEST_PERF=out.json` measures instead: a thread of
/// filler messages ending in `_DRAFTS` live copies of the page (0 to 3, 1 by
/// default), drawn the way the conversation draws its thread. After the pages
/// show, it records for `_HOLD` seconds (8 by default) how long each frame
/// took from the root's render to the end of its paint, how many frames were
/// drawn and the app's CPU, then how many pixels each kind of picture has for
/// the points it is shown at. `_TICK` adds a
/// view beside the thread that redraws every frame, as a caret or a working
/// spinner does, to show what the thread costs when it is not what changed.
/// `_WIDTH` gives the pages a width of their own and `_ZOOM` zooms the window.
mod perf {
    use super::{Draft, env_number};
    use gpui::{
        App, AppContext, Entity, Window, WindowBackgroundAppearance, WindowOptions, canvas, div,
        prelude::*, px, size,
    };
    use monocode_core::{
        transcript::Row,
        visual_bridge::{DRAFT_ROLE, DraftCard, DraftContent},
    };
    use serde_json::{Value, json};
    use std::{
        cell::{Cell, RefCell},
        ffi::OsString,
        rc::Rc,
        time::{Duration, Instant},
    };

    #[derive(Default)]
    struct Frames {
        started: Cell<Option<Instant>>,
        recording: Cell<bool>,
        durations: RefCell<Vec<f64>>,
    }

    struct Ticker;
    impl Render for Ticker {
        fn render(&mut self, window: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            window.request_animation_frame();
            div().size(px(1.))
        }
    }

    struct Root {
        thread: Entity<crate::transcript::TranscriptView>,
        ticker: Option<Entity<Ticker>>,
        frames: Rc<Frames>,
    }
    impl Render for Root {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            self.frames.started.set(Some(Instant::now()));
            let frames = self.frames.clone();
            div()
                .relative()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    div().flex_1().min_h_0().child(
                        self.thread
                            .clone()
                            .cached(gpui::StyleRefinement::default().size_full()),
                    ),
                )
                .children(self.ticker.clone())
                .child(crate::draft::sentinel())
                .child(
                    canvas(
                        |_, _, _| {},
                        move |_, _, _, _| {
                            if let Some(started) = frames.started.take()
                                && frames.recording.get()
                            {
                                frames
                                    .durations
                                    .borrow_mut()
                                    .push(started.elapsed().as_secs_f64() * 1000.);
                            }
                        },
                    )
                    .absolute()
                    .size_0(),
                )
        }
    }

    fn row(role: &str, text: String) -> Row {
        Row {
            role: role.into(),
            text,
            first: true,
            sent_at: None,
        }
    }

    /// Messages of the length and shape a working thread has.
    fn filler(n: usize) -> Vec<Row> {
        let mut rows = Vec::new();
        for i in 0..n {
            rows.push(row("user", format!("Step {i}: can you tighten the layout of the settings page and check the spacing on small screens?")));
            rows.push(row(
                "assistant",
                format!(
                    "Here is what I changed in step {i}.\n\n\
                     - The header now **wraps** below 480 px instead of overflowing.\n\
                     - Cards use a 12 px gap and the grid drops to one column on phones.\n\
                     - The save button stays pinned at the bottom.\n\n\
                     ```rust\nfn layout(width: f32) -> Columns {{\n    if width < 480. {{ Columns::One }} else {{ Columns::Two }}\n}}\n```\n\n\
                     The spacing scale is unchanged, so the rest of the app lines up with it. \
                     I also checked the dark theme and the contrast of the muted labels, which pass at 4.6:1."
                ),
            ));
        }
        rows
    }

    fn cpu_seconds() -> f64 {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
        // SAFETY: getrusage fills the buffer for this process.
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
            return 0.;
        }
        // SAFETY: the call succeeded.
        let usage = unsafe { usage.assume_init() };
        (usage.ru_utime.tv_sec + usage.ru_stime.tv_sec) as f64
            + (usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) as f64 / 1e6
    }

    /// CPU seconds so far of every process by pid, with its parent and name.
    fn processes() -> std::collections::HashMap<u32, (u32, f64, String)> {
        let out = std::process::Command::new("ps")
            .args(["-A", "-o", "pid=,ppid=,time=,comm="])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
            .unwrap_or_default();
        out.lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let pid = parts.next()?.parse().ok()?;
                let ppid = parts.next()?.parse().ok()?;
                // [[H:]MM:]SS.ss
                let time = parts.next()?.split(':').fold(0., |total, part| {
                    total * 60. + part.parse::<f64>().unwrap_or(0.)
                });
                Some((pid, (ppid, time, parts.collect::<Vec<_>>().join(" "))))
            })
            .collect()
    }

    /// The CPU, in percent of one core, of WebKit's processes that started
    /// after `existing` was taken.
    fn engine_cpu(
        before: &std::collections::HashMap<u32, (u32, f64, String)>,
        after: &std::collections::HashMap<u32, (u32, f64, String)>,
        existing: &std::collections::HashSet<u32>,
        seconds: f64,
    ) -> Value {
        let mut webkit = 0.;
        for (pid, (_, time, name)) in after {
            if name.contains("com.apple.WebKit") && !existing.contains(pid) {
                webkit += time - before.get(pid).map_or(0., |(_, time, _)| *time);
            }
        }
        json!({ "webkitPercent": webkit / seconds * 100. })
    }

    fn summary(mut durations: Vec<f64>, seconds: f64) -> Value {
        durations.sort_by(f64::total_cmp);
        let at = |p: f64| {
            durations
                .get(((durations.len() as f64 - 1.) * p).round() as usize)
                .copied()
                .unwrap_or(0.)
        };
        let mean = durations.iter().sum::<f64>() / durations.len().max(1) as f64;
        json!({
            "frames": durations.len(),
            "fps": durations.len() as f64 / seconds,
            "mean_ms": mean, "p50_ms": at(0.5), "p95_ms": at(0.95), "max_ms": at(1.),
            "busy_ms_per_s": durations.iter().sum::<f64>() / seconds,
        })
    }

    async fn revise(draft: &gpui::WeakEntity<Draft>, html: &str, cx: &mut gpui::AsyncApp) -> Value {
        let Ok(id) = draft.read_with(cx, |draft, _| draft.id().to_owned()) else {
            return Value::Null;
        };
        let (reply, _) = std::sync::mpsc::channel();
        let input = json!({"draft": id, "html": html});
        let Ok(request) = monocode_core::visual_bridge::DraftRequest::parse(1, &input, reply)
        else {
            return Value::Null;
        };
        let started = Instant::now();
        let _ = draft.update(cx, |draft, cx| draft.apply(&request, cx));
        loop {
            let shown = draft
                .read_with(cx, |draft, _| {
                    (
                        draft.shown == draft.shared.generation(),
                        draft.layouts.len(),
                    )
                })
                .unwrap_or((true, 0));
            if shown.0 {
                return json!({"shownAfterMs": started.elapsed().as_secs_f64() * 1000., "layouts": shown.1});
            }
            if started.elapsed() > Duration::from_secs(15) {
                return json!({"timedOut": true});
            }
            cx.background_executor()
                .timer(Duration::from_millis(5))
                .await;
        }
    }

    pub(super) fn run(path: OsString, out: OsString) {
        let platform = gpui_platform::current_platform(false);
        gpui::Application::with_platform(platform)
            .with_assets(crate::assets::Assets)
            .run(move |cx: &mut App| {
                crate::popover::bind(cx);
                let existing: std::collections::HashSet<u32> = processes().into_keys().collect();
                let html = std::fs::read_to_string(&path).unwrap_or_default();
                let count = env_number("MONOCODE_DRAFT_SELFTEST_DRAFTS").unwrap_or(1) as usize;
                let hold = env_number("MONOCODE_DRAFT_SELFTEST_HOLD").unwrap_or(8);
                let width = env_number("MONOCODE_DRAFT_SELFTEST_WIDTH");
                let mut rows = filler(16);
                let mut drafts = Vec::new();
                for n in 0..count {
                    let draft = cx.new(|cx| {
                        Draft::new(
                            None,
                            1,
                            DraftContent::Html(html.clone()),
                            (width, None),
                            None,
                            None,
                            cx,
                        )
                    });
                    let id = draft.read(cx).id().to_owned();
                    crate::draft::register(&id, &draft, cx);
                    rows.push(row("assistant", format!("Variant {n}:")));
                    let card = DraftCard {
                        draft: id,
                        title: format!("Variant {n}"),
                    };
                    rows.push(row(
                        DRAFT_ROLE,
                        serde_json::to_string(&card).unwrap_or_default(),
                    ));
                    drafts.push(draft);
                }
                rows.push(row("assistant", "Pick the one you like.".into()));
                let thread = cx.new(|cx| {
                    let mut view = crate::transcript::TranscriptView::new();
                    view.patch(0, rows, cx);
                    view
                });
                let scrolled = thread.clone();
                let scroll = std::env::var("MONOCODE_DRAFT_SELFTEST_SCROLL").ok();
                let ticker =
                    std::env::var_os("MONOCODE_DRAFT_SELFTEST_TICK").map(|_| cx.new(|_| Ticker));
                let frames = Rc::new(Frames::default());
                let handle = cx
                    .open_window(
                        WindowOptions {
                            window_bounds: Some(gpui::WindowBounds::Windowed(
                                gpui::Bounds::centered(None, size(px(820.), px(1000.)), cx),
                            )),
                            window_background: WindowBackgroundAppearance::Blurred,
                            ..Default::default()
                        },
                        {
                            let frames = frames.clone();
                            move |_, cx| {
                                cx.new(|_| Root {
                                    thread,
                                    ticker,
                                    frames,
                                })
                            }
                        },
                    )
                    .expect("open the measuring window");
                if let Some(zoom) = std::env::var("MONOCODE_DRAFT_SELFTEST_ZOOM")
                    .ok()
                    .and_then(|zoom| zoom.parse().ok())
                {
                    let _ = handle.update(cx, |_, window, cx| window.set_content_zoom(zoom, cx));
                }
                cx.activate(true);
                let weak: Vec<_> = drafts.iter().map(Entity::downgrade).collect();
                cx.spawn(async move |cx| {
                    for draft in &weak {
                        let _ = Draft::ready(draft, cx).await;
                    }
                    // The shell holds a conversation's drafts, and runs only the
                    // newest; here the task does.
                    for draft in drafts.iter().rev().skip(1) {
                        draft.update(cx, |draft, cx| draft.sleep(cx));
                    }
                    let _drafts = drafts;
                    // Let the fades, fits and first frames settle, and the thread
                    // come to rest with every card on screen.
                    // `_SCROLL=early` scrolls as soon as the page shows.
                    let early = scroll.as_deref() == Some("early");
                    cx.background_executor()
                        .timer(Duration::from_millis(if early { 250 } else { 2500 }))
                        .await;
                    let settled = Instant::now();
                    while settled.elapsed() < Duration::from_secs(5) {
                        let on_screen = weak.iter().rev().take(1).all(|draft| {
                            draft
                                .read_with(cx, |draft, _| draft.placement())
                                .ok()
                                .flatten()
                                .is_some_and(|[_, y, _, height, hidden]| {
                                    hidden == 0. && y >= 0. && y + height <= 1000.
                                })
                        });
                        if on_screen {
                            break;
                        }
                        cx.background_executor()
                            .timer(Duration::from_millis(100))
                            .await;
                    }
                    let cpu_before = cpu_seconds();
                    let engines_before = processes();
                    let started = Instant::now();
                    frames.recording.set(true);
                    let before = super::super::web::stats();
                    // How late each scroll step ran: the main thread was busy.
                    let mut late = Vec::new();
                    if scroll.is_some() {
                        let step = Duration::from_millis(8);
                        let mut direction = -1.;
                        while started.elapsed() < Duration::from_secs(hold.into()) {
                            for _ in 0..100 {
                                let at = Instant::now();
                                cx.background_executor().timer(step).await;
                                late.push(at.elapsed().saturating_sub(step).as_secs_f64() * 1000.);
                                scrolled.update(cx, |view, cx| {
                                    view.list.scroll_by(px(direction * 6.));
                                    cx.notify();
                                });
                            }
                            direction = -direction;
                            cx.background_executor()
                                .timer(Duration::from_millis(500))
                                .await;
                        }
                    } else {
                        cx.background_executor()
                            .timer(Duration::from_secs(hold.into()))
                            .await;
                    }
                    frames.recording.set(false);
                    let after = super::super::web::stats();
                    let moved = json!({
                        "stillFrames": after.still - before.still,
                        "draggedFrames": after.dragged - before.dragged,
                        "stillsTaken": after.taken - before.taken,
                        "convertMs": after.convert_ms - before.convert_ms,
                        "stepLate": summary(late, 1.),
                    });
                    let seconds = started.elapsed().as_secs_f64();
                    let cpu = (cpu_seconds() - cpu_before) / seconds * 100.;
                    let engines = engine_cpu(&engines_before, &processes(), &existing, seconds);
                    let (scale, zoom) = handle
                        .update(cx, |_, window, _| {
                            (window.scale_factor(), window.content_zoom())
                        })
                        .unwrap_or((0., 0.));
                    let mut pages = Vec::new();
                    for draft in &weak {
                        let placed = draft
                            .read_with(cx, |draft, _| draft.placement())
                            .ok()
                            .flatten();
                        let density = Draft::debug(draft, "density", cx)
                            .await
                            .unwrap_or_else(|e| json!({"error": e}));
                        pages.push(json!({"placed": placed, "density": density}));
                    }
                    // A new version from the agent, as `draft_preview` sends one:
                    // how long the card waits before it shows it.
                    // The running draft, as the shell wakes the one it works on.
                    let revise = match weak.last() {
                        Some(draft) => json!({
                            "first": revise(draft, &format!("{html}<!-- v1 -->"), cx).await,
                            "next": revise(draft, &format!("{html}<!-- v2 -->"), cx).await,
                        }),
                        None => Value::Null,
                    };
                    let report = json!({
                        "drafts": weak.len(),
                        "seconds": seconds,
                        "scaleFactor": scale,
                        "contentZoom": zoom,
                        "appCpuPercent": cpu,
                        "engineCpu": engines,
                        "revise": revise,
                        "frameTime": summary(frames.durations.take(), seconds),
                        "scroll": moved,
                        "pages": pages,
                    });
                    let _ = std::fs::write(
                        &out,
                        serde_json::to_string_pretty(&report).unwrap_or_default(),
                    );
                    cx.update(|cx| cx.quit());
                })
                .detach();
            });
    }
}
