//! Ignored perf test: key dispatch to test-executor idle in the real FileEditor.
//!
//! Run with:
//!   cargo test --locked --profile native-dev search_keystroke_latency \
//!     -- --ignored --nocapture
//!
//! Results are checked after EVERY sample. This uses GPUI's mock platform and
//! deterministic executor, not OS input, real worker contention or presentation.
//! Coalesced typing dispatches six keys before background work starts; it does
//! not measure cancelling an already-running search. See scripts/perf/README.md.

use super::*;
use gpui::{
    Bounds, Keystroke, TestAppContext, VisualTestContext, WindowBounds, WindowOptions, point, size,
};

#[gpui::test]
#[ignore = "perf measurement; run with --ignored --nocapture"]
fn search_keystroke_latency(cx: &mut TestAppContext) {
    assert!(
        std::env::var_os("MONOCODE_SEARCH_LATENCY").is_none(),
        "disable probe logging when benchmarking"
    );
    let rounds: usize = std::env::var("SEARCH_LATENCY_ROUNDS").map_or(20, |s| s.parse().unwrap());
    assert!(rounds >= 20, "p95 needs at least 20 samples");
    cx.update(|cx| {
        cx.set_global(crate::settings::Preferences::default());
        input::bind(cx);
        bind(cx);
    });
    for (label, bytes) in [
        ("10KB", 10 * 1024),
        ("100KB", 100 * 1024),
        ("1MB", 1024 * 1024),
    ] {
        let path = std::env::temp_dir().join(format!(
            "monocode-search-latency-{}-{bytes}.txt",
            std::process::id()
        ));
        let source = latency_corpus(bytes);
        std::fs::write(&path, &source).unwrap();
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(900.), px(600.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| {
                    let git = cx.new(|cx| {
                        ProjectGit::new(std::env::temp_dir().to_string_lossy().into_owned(), cx)
                    });
                    cx.new(|cx| FileEditor::new(path.to_string_lossy().into_owned(), git, cx))
                },
            )
            .unwrap()
        });
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        window
            .update(visual, |editor, window, cx| editor.open_search(window, cx))
            .unwrap();
        visual.run_until_parked();

        let mut isolated = vec![Vec::new(); 6];
        let mut bursts = Vec::new();
        for round in 0..rounds + 2 {
            clear_search(&window, visual);
            for (index, ch) in "needle".chars().enumerate() {
                let started = std::time::Instant::now();
                visual.simulate_keystrokes(&ch.to_string());
                let elapsed = started.elapsed();
                assert_results(&window, visual, &source, &"needle"[..index + 1]);
                if round >= 2 {
                    isolated[index].push(elapsed);
                }
            }

            clear_search(&window, visual);
            let started = std::time::Instant::now();
            for ch in "needle".chars() {
                visual
                    .dispatch_keystroke(window.into(), Keystroke::parse(&ch.to_string()).unwrap());
            }
            visual.run_until_parked();
            let elapsed = started.elapsed();
            assert_results(&window, visual, &source, "needle");
            if round >= 2 {
                bursts.push(elapsed);
            }
        }
        for (index, times) in isolated.iter().enumerate() {
            report_latency(label, &format!("key:{}", &"needle"[..index + 1]), times);
        }
        report_latency(label, "coalesced:needle", &bursts);
        window.update(visual, |_, w, _| w.remove_window()).unwrap();
        std::fs::remove_file(path).unwrap();
    }
}

fn assert_results(
    window: &gpui::WindowHandle<FileEditor>,
    cx: &mut VisualTestContext,
    source: &str,
    query: &str,
) {
    window
        .update(cx, |e, _, cx| {
            assert_eq!(e.search.read(cx).content.as_ref(), query);
            assert!(!e.search_invalid);
            let actual: Vec<_> = e
                .matches
                .iter()
                .map(|m| (m.from, m.to, m.insert.as_str()))
                .collect();
            let expected: Vec<_> = source
                .match_indices(query)
                .map(|(from, s)| (from, from + s.len(), ""))
                .collect();
            assert_eq!(actual, expected, "query={query}");
        })
        .unwrap();
}

fn clear_search(window: &gpui::WindowHandle<FileEditor>, cx: &mut VisualTestContext) {
    window
        .update(cx, |editor, _, cx| {
            editor
                .search
                .update(cx, |input, cx| input.set_text(String::new(), cx))
        })
        .unwrap();
    cx.run_until_parked();
}

fn latency_corpus(target: usize) -> String {
    let mut out = String::with_capacity(target + 64);
    let mut i = 0;
    while out.len() < target {
        if i % 10 == 0 {
            out.push_str("needle alpha beta gamma\n");
        } else {
            out.push_str("lorem ipsum dolor sit amet consectetur adipiscing elit\n");
        }
        i += 1;
    }
    out
}

fn report_latency(label: &str, scenario: &str, times: &[std::time::Duration]) {
    fn stats(times: &[std::time::Duration]) -> (f64, f64) {
        let mut ms: Vec<f64> = times
            .iter()
            .map(|duration| duration.as_secs_f64() * 1000.0)
            .collect();
        ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = (ms[(ms.len() - 1) / 2] + ms[ms.len() / 2]) / 2.0;
        let p95 = ms[((ms.len() as f64 * 0.95).ceil() as usize)
            .saturating_sub(1)
            .min(ms.len() - 1)];
        (median, p95)
    }
    let (median, p95) = stats(times);
    eprintln!(
        "search latency {label} {scenario}: n={} p50={median:.3}ms p95={p95:.3}ms",
        times.len()
    );
}
