//! Times parse and layout of a 2,000-line reply with 30 code blocks, cold and
//! while it streams. Prints the numbers and fails if a streaming frame is far
//! over budget.
//!
//! The crate's own code builds without optimization in `cargo test`, so the
//! limits here are loose; run with `--release` for production numbers:
//! `cargo test -p monocode-markdown --release --test perf -- --nocapture`.

#![cfg(target_os = "macos")]

mod common;

use std::time::{Duration, Instant};

use gpui::AppContext as _;
use monocode_markdown::{IncrementalParser, MarkdownView, parse};

use common::{app, draw, open};

/// A long agent reply: prose, lists, tables, and 30 code blocks in four
/// languages, about 2,000 lines.
fn big_reply() -> String {
    let languages = ["rust", "ts", "python", "bash"];
    let mut out = String::new();
    for section in 0..30 {
        out.push_str(&format!("## Section {section}: what changed\n\n"));
        for p in 0..8 {
            out.push_str(&format!(
                "Paragraph {p} explains **why** the parser keeps `blocks[{section}]` shared, \
                 with a [link](https://example.com/{section}/{p}) and *emphasis* on the parts \
                 that matter while streaming.\n\n"
            ));
        }
        out.push_str("- First point with `inline code`\n- Second point\n  - Nested detail\n- [x] Done\n- [ ] Open\n\n");
        out.push_str("1. Ordered step\n2. Another step with **bold**\n3. Last step\n\n");
        if section % 3 == 0 {
            out.push_str("| Step | Cost | Notes |\n|:--|--:|:--|\n");
            for row in 0..6 {
                out.push_str(&format!("| step {row} | {row}ms | note {row} |\n"));
            }
            out.push('\n');
        }
        let language = languages[section % languages.len()];
        out.push_str(&format!("```{language}\n"));
        for line in 0..36 {
            out.push_str(&match language {
                "rust" => {
                    format!("    let value_{line} = compute({line}, \"label\"); // step {line}\n")
                }
                "ts" => {
                    format!("  const value{line}: number = compute({line}, 'label'); // step\n")
                }
                "python" => {
                    format!("    value_{line} = compute({line}, \"label\")  # step {line}\n")
                }
                _ => format!("echo \"step {line}\" && run --flag {line} | tee log.txt\n"),
            });
        }
        out.push_str(
            "```\n\n> A quoted note that wraps across a line or two in a narrow column.\n\n",
        );
    }
    out
}

fn percentile(samples: &mut [Duration], p: f64) -> Duration {
    samples.sort();
    let ix = ((samples.len() as f64 - 1.) * p).round() as usize;
    samples[ix]
}

#[test]
fn parse_and_layout_of_a_long_reply() {
    let _serial = common::serial();
    let doc = big_reply();
    let lines = doc.lines().count();
    let fences = doc.matches("```").count() / 2;
    assert!(lines >= 2000, "{lines} lines");
    assert_eq!(fences, 30);

    // Full parse.
    let start = Instant::now();
    let parsed = parse(&doc);
    let full_parse = start.elapsed();

    // Streamed parse in token-sized chunks: total and worst append.
    let mut parser = IncrementalParser::new();
    let mut worst = Duration::ZERO;
    let mut worst_bytes = 0;
    let start = Instant::now();
    let mut at = 0;
    while at < doc.len() {
        let mut end = (at + 7).min(doc.len());
        while !doc.is_char_boundary(end) {
            end += 1;
        }
        let t = Instant::now();
        parser.append(&doc[at..end]);
        let _ = parser.display_tree();
        worst = worst.max(t.elapsed());
        worst_bytes = worst_bytes.max(parser.last_parse_bytes());
        at = end;
    }
    let streamed_parse = start.elapsed();
    assert_eq!(parser.tree(), &parsed);

    // Layout: a finished reply, cold, then steady frames.
    let mut cx = app();
    monocode_markdown::highlight::syntaxes_blocking();
    // Warm the font caches with a short message, so the cold number below is
    // about the reply and not about loading fonts.
    let warm_start = Instant::now();
    let (_warm, _) = open(&mut cx, 760., 900., |cx| {
        MarkdownView::with_text(
            "Warm **up** *fonts* `mono` and\n\n```rust\nfn a() {}\n```\n\n| a |\n|---|\n| b |",
            cx,
        )
    });
    let warm = warm_start.elapsed();
    let text = doc.clone();
    // Opening the window draws the first frame, which lays out every block.
    let start = Instant::now();
    let (window, markdown) = open(&mut cx, 760., 900., move |cx| {
        MarkdownView::with_text(text, cx)
    });
    let cold = start.elapsed();
    // The highlight budget spreads the code over a few frames.
    for _ in 0..4 {
        draw(&mut cx, window);
    }
    let mut steady = Vec::new();
    for _ in 0..20 {
        cx.update(|cx| markdown.update(cx, |_, cx| cx.notify()));
        let t = Instant::now();
        draw(&mut cx, window);
        steady.push(t.elapsed());
    }

    // Streaming: the last 3,000 bytes arrive a few bytes per frame while the
    // reveal paces and words fade.
    let cut = doc.len() - 3000;
    let mut cut = cut;
    while !doc.is_char_boundary(cut) {
        cut += 1;
    }
    let mut cx2 = app();
    let head = doc[..cut].to_string();
    let (window2, markdown2) = open(&mut cx2, 760., 900., move |cx| {
        MarkdownView::with_text(head, cx)
    });
    for _ in 0..4 {
        draw(&mut cx2, window2);
    }
    cx2.update(|cx| markdown2.update(cx, |view, cx| view.set_streaming(true, cx)));
    let mut frames = Vec::new();
    let mut at = cut;
    while at < doc.len() {
        let mut end = (at + 12).min(doc.len());
        while !doc.is_char_boundary(end) {
            end += 1;
        }
        let delta = doc[at..end].to_string();
        cx2.update(|cx| markdown2.update(cx, |view, cx| view.push_str(&delta, cx)));
        at = end;
        let t = Instant::now();
        draw(&mut cx2, window2);
        frames.push(t.elapsed());
        std::thread::sleep(Duration::from_millis(2));
    }
    let blocks = cx2.read_entity(&markdown2, |view, _| view.document().len());

    let steady_p50 = percentile(&mut steady, 0.5);
    let frame_p50 = percentile(&mut frames, 0.5);
    let frame_p95 = percentile(&mut frames, 0.95);
    let frame_max = *frames.iter().max().unwrap();
    eprintln!(
        "reply: {lines} lines, {} bytes, {fences} code blocks, {} top-level blocks",
        doc.len(),
        parsed.len()
    );
    eprintln!("full parse: {full_parse:?}");
    eprintln!("first frame of a short message (font loading): {warm:?}");
    eprintln!(
        "streamed parse ({} appends): total {streamed_parse:?}, worst append {worst:?}, \
         most bytes reparsed by one append {worst_bytes}",
        doc.len() / 7
    );
    eprintln!("first frame (parse, prepare, highlight, layout of every block, paint): {cold:?}");
    eprintln!("steady redraw p50 (blocks a viewport away skip layout): {steady_p50:?}");
    eprintln!(
        "streaming frames ({}): p50 {frame_p50:?}, p95 {frame_p95:?}, max {frame_max:?}; {blocks} blocks shown",
        frames.len()
    );

    // Appends reparse only the tail, never the whole reply.
    assert!(
        worst_bytes < 4_000,
        "an append reparsed {worst_bytes} bytes"
    );
    // A 60Hz frame is 16.7ms. The crate's code is unoptimized in a debug
    // test build, so the debug limit is looser.
    let budget = if cfg!(debug_assertions) {
        Duration::from_millis(40)
    } else {
        Duration::from_millis(16)
    };
    assert!(
        frame_p50 < budget,
        "streaming frame p50 {frame_p50:?} over {budget:?}"
    );
    assert!(
        steady_p50 < budget,
        "steady frame p50 {steady_p50:?} over {budget:?}"
    );
}
