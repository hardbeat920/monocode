use crate::corpus::{self, Case};
use crate::{
    editor_search::{self, SearchOptions, SearchResult},
    old,
};
use serde::Serialize;
use std::{fs, hint::black_box, time::Instant};

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    result: SearchResult,
    output: Option<String>,
}

#[derive(Default, Serialize)]
struct Timing {
    samples_us: Vec<f64>,
    median_us: f64,
    // Fewer than 20 samples cannot support even a basic empirical p95.
    p95_us: Option<f64>,
}

impl Timing {
    fn finish(&mut self) {
        let mut sorted = self.samples_us.clone();
        sorted.sort_by(f64::total_cmp);
        if sorted.is_empty() {
            return;
        }
        self.median_us = (sorted[(sorted.len() - 1) / 2] + sorted[sorted.len() / 2]) / 2.0;
        if sorted.len() >= 20 {
            self.p95_us = Some(sorted[(sorted.len() * 95).div_ceil(100) - 1]);
        }
    }
}

#[derive(Serialize)]
struct Row {
    id: String,
    bytes: usize,
    input_hash: u64,
    query: String,
    replacement: String,
    case_sensitive: bool,
    whole_word: bool,
    regex: bool,
    replace_all: bool,
    old: Timing,
    new: Timing,
    matches: Option<usize>,
    parity: bool,
    error: Option<String>,
}

#[derive(Serialize)]
struct Report {
    profile: &'static str,
    platform: String,
    iterations: usize,
    warmups: usize,
    baseline: String,
    candidate: String,
    engine_hash: u64,
    bundle_hash: u64,
    cases: Vec<Row>,
}

struct Options {
    json: Option<String>,
    bundle: String,
    iterations: usize,
    large: bool,
    filter: Option<String>,
}

impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Self {
            json: None,
            bundle: String::new(),
            iterations: 20,
            large: false,
            filter: None,
        };
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--json" => options.json = Some(args.next().ok_or("--json needs a path")?),
                "--bundle" => options.bundle = args.next().ok_or("--bundle needs a path")?,
                "--iterations" => {
                    options.iterations = args
                        .next()
                        .ok_or("--iterations needs a number")?
                        .parse()
                        .map_err(|_| "--iterations must be a positive integer")?;
                    if options.iterations == 0 {
                        return Err("--iterations must be positive".into());
                    }
                }
                "--large" => options.large = true,
                "--filter" => options.filter = Some(args.next().ok_or("--filter needs text")?),
                _ => return Err(format!("unknown option: {arg}")),
            }
        }
        if options.bundle.is_empty() {
            return Err("--bundle PATH is required".into());
        }
        Ok(options)
    }
}

pub fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "help".into());
    if command == "help" || command == "--help" {
        println!(
            "search-bench <run|verify|once-old|once-new> --bundle PATH [--json PATH] [--large] [--filter TEXT] [--iterations N]\n\
                  run: alternating paired release measurements (default 20 samples, two warmups).\n\
                  verify: exact semantic comparison against the old bundle.\n\
                  once-old/new: one case, for external process-memory measurement."
        );
        return Ok(());
    }
    let options = Options::parse(args)?;
    let bundle = fs::read_to_string(&options.bundle).map_err(|e| e.to_string())?;
    if command == "verify" {
        return crate::parity::verify(&bundle);
    }
    if cfg!(debug_assertions) {
        return Err("timing requires cargo run --release".into());
    }
    if !matches!(command.as_str(), "run" | "once-old" | "once-new") {
        return Err(format!("unknown command: {command}"));
    }
    let cases = corpus::cases(options.large, options.filter.as_deref());
    if cases.is_empty() {
        return Err("no matching cases".into());
    }
    if command.starts_with("once-") {
        if cases.len() != 1 {
            return Err("once requires --filter matching exactly one case".into());
        }
        let outcome = run_once(
            &cases[0],
            (command == "once-old").then_some(bundle.as_str()),
        )?;
        println!(
            "{} matches={} timeout={}",
            cases[0].id,
            outcome.result.matches.len(),
            outcome.result.timed_out
        );
        black_box(outcome);
        return Ok(());
    }
    let mut rows = Vec::new();
    for case in &cases {
        eprintln!(
            "[paired] {}: {} bytes, {} samples/implementation",
            case.id,
            case.source.len(),
            options.iterations
        );
        let mut row = Row {
            id: case.id.clone(),
            bytes: case.source.len(),
            input_hash: hash(&case.source),
            query: case.query.clone(),
            replacement: case.replacement.clone(),
            case_sensitive: case.case_sensitive,
            whole_word: case.whole_word,
            regex: case.regex,
            replace_all: case.replace_all,
            old: Timing::default(),
            new: Timing::default(),
            matches: None,
            parity: true,
            error: None,
        };
        for iteration in 0..options.iterations + 2 {
            // Alternate order so thermal/scheduler drift does not consistently
            // favor one implementation. Corpus creation and parity are untimed.
            let order = if iteration % 2 == 0 {
                [true, false]
            } else {
                [false, true]
            };
            let mut outcomes = Vec::with_capacity(2);
            for is_old in order {
                let started = Instant::now();
                let outcome = run_once(black_box(case), is_old.then_some(bundle.as_str()));
                let us = started.elapsed().as_secs_f64() * 1_000_000.0;
                if iteration >= 2 {
                    (if is_old { &mut row.old } else { &mut row.new })
                        .samples_us
                        .push(us);
                }
                outcomes.push(
                    outcome.map_err(|e| format!("{}: {e}", if is_old { "old" } else { "new" })),
                );
            }
            match (&outcomes[0], &outcomes[1]) {
                (Ok(a), Ok(b)) if !a.result.timed_out && !b.result.timed_out => {
                    row.matches = Some(a.result.matches.len());
                    if a != b {
                        row.parity = false;
                        row.error =
                            Some("exact ranges, replacement text, status or output differ".into());
                    }
                }
                (Err(e), _) | (_, Err(e)) => {
                    row.error = Some(e.clone());
                    row.parity = false;
                }
                _ => {
                    row.error = Some("incomplete search (timeout); no comparable speedup".into());
                    row.parity = false;
                }
            }
            if row.error.is_some() {
                break;
            }
        }
        row.old.finish();
        row.new.finish();
        rows.push(row);
    }
    let report = Report {
        profile: "release",
        platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        iterations: options.iterations,
        warmups: 2,
        baseline: std::env::var("SEARCH_BASELINE").unwrap_or_default(),
        candidate: std::env::var("SEARCH_CANDIDATE").unwrap_or_default(),
        engine_hash: hash(include_str!(
            "../../../../crates/monocode-gpui/src/editor_search.rs"
        )),
        bundle_hash: hash(&bundle),
        cases: rows,
    };
    if let Some(path) = options.json {
        fs::write(
            path,
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    println!(
        "{:<30} {:>10} {:>10} {:>10} {:>10} {:>9} parity",
        "case", "old p50 ms", "new p50 ms", "old p95 ms", "new p95 ms", "speedup"
    );
    for row in &report.cases {
        if let Some(error) = &row.error {
            println!("{:<30} INCOMPARABLE: {error}", row.id);
            continue;
        }
        println!(
            "{:<30} {:>10.3} {:>10.3} {:>10} {:>10} {:>8.1}x exact",
            row.id,
            row.old.median_us / 1000.0,
            row.new.median_us / 1000.0,
            p95(&row.old),
            p95(&row.new),
            row.old.median_us / row.new.median_us
        );
    }
    // There is no defensible single "overall speedup" without a workload mix.
    if report.cases.iter().any(|r| !r.parity) {
        Err("one or more cases are incomplete or differ; see report".into())
    } else {
        Ok(())
    }
}

fn run_once(case: &Case, bundle: Option<&str>) -> Result<Outcome, String> {
    let result = if let Some(bundle) = bundle {
        old::search(
            bundle,
            &case.source,
            &case.query,
            &case.replacement,
            case.case_sensitive,
            case.whole_word,
            case.regex,
        )?
    } else {
        editor_search::search(
            &case.source,
            &case.query,
            &case.replacement,
            SearchOptions {
                case_sensitive: case.case_sensitive,
                whole_word: case.whole_word,
                regex: case.regex,
            },
        )
    };
    let output = if case.replace_all && !result.timed_out && !result.invalid {
        Some(if bundle.is_some() {
            let mut output = case.source.clone();
            for m in result.matches.iter().rev() {
                output.replace_range(m.from..m.to, &m.insert);
            }
            output
        } else {
            editor_search::replace_all(&case.source, &result.matches)?
        })
    } else {
        None
    };
    Ok(Outcome { result, output })
}

fn p95(timing: &Timing) -> String {
    timing
        .p95_us
        .map_or_else(|| "—".into(), |us| format!("{:.3}", us / 1000.0))
}

fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}
