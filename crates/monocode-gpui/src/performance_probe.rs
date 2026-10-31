//! Frame and foreground-task traces for builds with `perf-trace` and `MONOCODE_PERF_LOG`.
use gpui::{
    App,
    profiler::{
        self, FrameEvent, FrameTimingCollector,
        journal::{ForegroundEvent, ForegroundJournalEntry},
    },
};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{self, BufWriter, Write},
    sync::mpsc,
    time::{Duration, Instant},
};

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.
}
fn write(file: &mut BufWriter<File>, value: Value) -> io::Result<()> {
    serde_json::to_writer(&mut *file, &value).map_err(io::Error::other)?;
    file.write_all(b"\n")
}
#[cfg(unix)]
fn cpu_seconds() -> Option<f64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage initializes this buffer on success, for this process only.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: the successful call initialized the rusage value.
    let usage = unsafe { usage.assume_init() };
    Some(
        (usage.ru_utime.tv_sec + usage.ru_stime.tv_sec) as f64
            + (usage.ru_utime.tv_usec + usage.ru_stime.tv_usec) as f64 / 1_000_000.,
    )
}
/// libc has no getrusage on Windows, so traces there carry no CPU samples.
#[cfg(not(unix))]
fn cpu_seconds() -> Option<f64> {
    None
}

pub(crate) fn start(launched: Instant, cx: &mut App) {
    let Some(path) = std::env::var_os("MONOCODE_PERF_LOG") else {
        return;
    };
    profiler::set_trace_enabled(true);
    let mut frames = FrameTimingCollector::new();
    let mut journal = cx.foreground_journal().collector();
    let (stop, stopped) = mpsc::channel();
    cx.on_app_quit(move |_| {
        let _ = stop.send(());
        async {}
    })
    .detach();
    let work = move || -> io::Result<()> {
        let mut file = BufWriter::new(File::create(path)?);
        let mut cpu = cpu_seconds().map(|value| (Instant::now(), value));
        loop {
            let done = !matches!(
                stopped.recv_timeout(Duration::from_millis(100)),
                Err(mpsc::RecvTimeoutError::Timeout)
            );
            for frame in frames.collect_unseen() {
                match frame {
                    FrameEvent::Draw(frame) => write(
                        &mut file,
                        json!({
                            "kind": "draw", "at_ms": ms(launched.elapsed().saturating_sub(frame.draw_start.elapsed())),
                            "window": format!("{:?}", frame.window_id), "duration_ms": ms(frame.draw_duration()),
                            "dirty_ms": frame.dirty_to_draw_duration().map(ms), "layout_ms": ms(frame.phases.request_layout),
                            "prepaint_ms": ms(frame.phases.prepaint), "paint_ms": ms(frame.phases.paint),
                        }),
                    )?,
                    FrameEvent::Present(frame) => write(
                        &mut file,
                        json!({
                            "kind": "present", "at_ms": ms(launched.elapsed().saturating_sub(frame.present_end.elapsed())),
                            "duration_ms": ms(frame.present_end.duration_since(frame.present_start)),
                            "animation_interval_ms": frame.animation_interval.map(ms),
                        }),
                    )?,
                }
            }
            let drained = journal.collect_unseen();
            if drained.lost > 0 {
                write(&mut file, json!({"kind": "lost", "count": drained.lost}))?;
            }
            for entry in drained.entries {
                let ForegroundJournalEntry::Event(event) = entry else {
                    continue;
                };
                let self_ms = match event {
                    ForegroundEvent::View(view) => Some(ms(view.self_duration)),
                    _ => None,
                };
                let (kind, source) = match event {
                    ForegroundEvent::TaskPoll(task) => (
                        "task",
                        format!("{}:{}", task.location.file(), task.location.line()),
                    ),
                    ForegroundEvent::Input(input) => ("input", input.kind.to_owned()),
                    ForegroundEvent::View(view) => {
                        ("view", format!("{}:{}", view.name, view.phase.as_str()))
                    }
                    _ => continue,
                };
                write(
                    &mut file,
                    json!({"kind": kind, "source": source,
                        "at_ms": ms(launched.elapsed().saturating_sub(event.end_time().elapsed())),
                        "duration_ms": ms(event.duration()), "self_ms": self_ms,
                    }),
                )?;
            }
            if let Some((at, before)) = cpu
                && at.elapsed() >= Duration::from_secs(1)
            {
                if let Some(value) = cpu_seconds() {
                    write(
                        &mut file,
                        json!({"kind": "cpu", "at_ms": ms(launched.elapsed()),
                        "percent": (value - before) / at.elapsed().as_secs_f64() * 100.}),
                    )?;
                    cpu = Some((Instant::now(), value));
                } else {
                    cpu = None;
                }
            }
            file.flush()?;
            if done {
                return Ok(());
            }
        }
    };
    if let Err(error) = std::thread::Builder::new()
        .name("performance-trace".into())
        .spawn(move || {
            if let Err(error) = work() {
                eprintln!("Performance trace failed: {error}");
            }
        })
    {
        eprintln!("Cannot start performance trace: {error}");
    }
}
