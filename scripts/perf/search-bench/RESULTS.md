# Search refactor review — 2026-11-05

Baseline: `c8ec3de2` (upstream immediately before the rebased refactor).
Candidate: `aa9eceba` plus this review's working-tree fixes.
Machine: Apple M4 Max, macOS arm64, Rust 1.99.0.

## Correctness and integration

Reviewing beyond the original nine passing tests found and fixed:

- Whole-word literal searches skipping valid overlapping candidates after a
  rejected match, in both ASCII and normalized Unicode paths.
- Regex multiline selection differing from the pinned CodeMirror behavior
  (`\S` classification and rejected whole-word candidates).
- Regex timeouts continuing through later lines and exposing incomplete
  matches to replacement. A timeout now aborts the scan and discards partials.
- Late/queued searches surviving panel dismissal, stale highlights while a
  new query runs, and navigation/replacement using obsolete input snapshots.
- Excess temporary memory in Unicode search: the initial offset map peaked at
  46.6 MiB versus 30.2 MiB for the old bridge on the mixed-Unicode 1 MiB corpus.
  Compact ASCII-run/Unicode-expansion spans fix that regression (see below).

The engine is independent of GPUI, file I/O and task scheduling, with a named
`SearchOptions` API. `file_editor/search.rs` contains the adapter and panel.
The benchmark compiles the production engine directly without linking GPUI.

The default search-specific suite is **three engine tests plus one adapter
lifecycle test**. A frozen table replaces overlapping small unit tests; the
remaining tests cover UTF-8/range validation, bounded regex failure and
deterministically ordered late-result races. Existing editor tests retain
navigation, undo, autosave and file-encoding coverage. Timing tests stay ignored.

Checks completed:

- Core: **671 passed**, 2 ignored.
- GPUI, serial: **845 passed**, 12 ignored; **3 integration tests passed**.
- After compacting the Unicode map: all **3 engine tests**, **12 editor tests**
  and **203 exact differential compatibility cases** passed again.
- A default parallel GPUI run failed seven tests involving QuickJS deadlines,
  FSEvents scheduling and visual layout. All passed in the full serial run.
- Clippy passes with the existing `items_after_test_module` lint allowed;
  unqualified `-D warnings` remains blocked by that unrelated existing lint.

## Release engine timings

Thirty measured old/new pairs per workload, after two warmups, alternating
execution order. Complete ranges, insertion strings and outputs were compared
outside the timed region on every sample.

| Workload | Old p50 | New p50 | New p95 | Speedup |
|---|---:|---:|---:|---:|
| ASCII literal, 10 KiB | 38.88 ms | 0.0025 ms | 0.003 ms | ~15,300× |
| ASCII literal, 100 KiB | 82.24 ms | 0.0095 ms | 0.012 ms | ~8,700× |
| ASCII literal, 1 MiB | 532.80 ms | 0.074 ms | 0.087 ms | ~7,200× |
| Unicode literal, 1 MiB | 572.13 ms | 1.85 ms | 1.94 ms | 309.5× |
| Unicode whole-word, 1 MiB | 632.41 ms | 14.97 ms | 15.90 ms | 42.3× |
| Captured regex, 1 MiB | 123.98 ms | 1.08 ms | 1.14 ms | 114.6× |
| Backreference regex, 1 MiB | 174.90 ms | 23.28 ms | 24.27 ms | 7.5× |
| Search + Replace All, 1 MiB | 550.13 ms | 0.113 ms | 0.135 ms | ~4,900× |
| Dense search + Replace All, 1 MiB | 2,762.43 ms | 3.737 ms | 3.902 ms | 739.3× |
| High-hit one-character query, 1 MiB | 949.10 ms | 3.337 ms | 3.462 ms | 284.4× |
| No matches, 1 MiB | 498.24 ms | 0.048 ms | 0.058 ms | ~10,500× |
| Real App.tsx regex, 519 KiB | 73.98 ms | 0.833 ms | 0.938 ms | 88.9× |

These ratios describe the old bridge versus the native engine, including the
old per-query JavaScript startup cost. They do not describe UI/frame speedups.
Unicode normalization and backreferences remain substantially more expensive
than ASCII search; there is no meaningful unweighted "overall speedup."

The Unicode rows were remeasured after compacting the offset map; the other
rows exercise unchanged paths. On the same mixed-Unicode corpus the initial
Rust implementation took 40.43 ms, versus 1.85 ms after that change. Dense
Unicode whole-word scanning improved from 44.86 ms to 14.97 ms.

The initial benchmark also exercised a 1 MiB single line. It is now explicitly
an optional stress case, since the editor's supported limits are 2 MiB per file
and 16 KiB per line. Representative dense corpora use bounded lines.

## Editor keystroke measurements

Both snapshots used `native-dev` (opt-level 1). Two warmups and 20 samples per
prefix per pass, in new/old then old/new order. Each sample validated the exact
result ranges and replacement strings. The ranges below span the two passes;
they are not pooled medians or confidence intervals.

| Scenario | Old p50 range | New p50 range | Highest new p95 |
|---|---:|---:|---:|
| 10 KiB, final key of `needle` | 44.76–45.56 ms | 0.299–0.302 ms | 0.319 ms |
| 100 KiB, final key of `needle` | 101.08–104.73 ms | 0.369–0.387 ms | 0.472 ms |
| 1 MiB, final key of `needle` | 683.25–698.35 ms | 1.022–1.053 ms | 1.128 ms |
| 1 MiB, first key `n` (many matches) | 712.16–730.46 ms | 8.935–8.995 ms | 10.130 ms |
| 1 MiB, six coalesced keys | 682.47–698.01 ms | 1.773–1.798 ms | 1.925 ms |

The earlier pooled keystroke median hid the cost of the first, high-match key.
The Rust path is still much faster there, but **~9 ms**, not ~1 ms. This is why
the revised test reports individual prefixes.

These are dispatch-to-idle timings on GPUI's mock platform, including result
application and any other runnable work. They do not establish physical input
latency, frame rate or real-worker cancellation behavior. The coalesced case
queues six keys before background work runs.

## Process memory

Median of three fresh processes per implementation/workload, measured with
macOS `/usr/bin/time -l`. These are **total process peak RSS**, including input,
bundle text and results, not per-query allocations or GPUI app memory.

| 1 MiB workload | Old peak RSS | New peak RSS |
|---|---:|---:|
| ASCII, case-sensitive | 29.4 MiB | 4.7 MiB |
| Mixed-Unicode literal | 29.9 MiB | 8.2 MiB |
| Dense Unicode whole-word | 30.8 MiB | 21.0 MiB |
| Backreference regex | 32.4 MiB | 6.2 MiB |
| Dense search + Replace All | 71.1 MiB | 16.0 MiB |

The Unicode map now stores one entry per ASCII run or Unicode expansion,
rather than one per normalized character. Its UTF-8 mapping and partial-
expansion rules are covered by the same frozen fixtures and differential audit.

See [README.md](README.md) for methodology, limitations and reproduction.
