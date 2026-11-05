# Editor search benchmark

Compares the actual Rust engine with the old QuickJS/CodeMirror bridge, using
the same inputs and a shared release profile. The default baseline is
`c8ec3de2`, the upstream parent of the rebased search refactor.

The latest measured review is in [RESULTS.md](RESULTS.md).

```sh
KEEP=1 scripts/perf/search-bench.sh --iterations 30
KEEP=1 scripts/perf/search-bench.sh --filter 1MB
OLD_REF=<commit> scripts/perf/search-bench.sh
```

`KEEP=1` retains the raw samples, input hashes, revision/compiler/platform
metadata, baseline bundle and comparison table in the printed directory.
Run on an otherwise idle machine. Avoid concurrent builds or other benchmarks.

## What is measured

- Two warmups, then 20 samples per implementation by default. Old/new execution
  order alternates on each sample. JSON keeps every measurement.
- Median and empirical p95 wall time; p95 is omitted with fewer than 20 samples.
  These are sample statistics, not confidence intervals or production tail SLOs.
- Old includes fresh QuickJS initialization, bundle evaluation, JSON decoding
  and UTF-16 → UTF-8 mapping, exactly as the app did on every query.
- New includes normalization/regex compilation, scanning and replacement-string
  expansion. Neither implementation caches compiled queries across requests.
- Replace All cases include search **and** rewriting the document. Old uses its
  reverse replacement loop; new uses its single-pass rewrite.
- Input generation, exact parity comparisons, reporting and output hashing are
  outside the timed interval. Returned results are retained until comparison;
  their eventual destruction is outside both timings.

Every sample checks **every byte range, replacement string, status flag and
complete rewritten output**, not just the match count. A difference, timeout
or engine error makes the run fail and is excluded from speedup claims. A
separate `verify` pass also checks the frozen semantic fixtures and a compact
matrix of Unicode, whole-word, capture, zero-width and multiline cases against
the actual old bundle. It is opt-in rather than extra application CI tests.

There is deliberately no aggregate speedup: the answer depends on the workload
mix. Empty-query and invalid-regex rows measure overhead, not scanning speed.

## Workloads and scope

10 KiB, 100 KiB and 1 MiB inputs cover ASCII literal (both case settings),
Unicode normalization, whole words, captured regexes, backreferences and
Replace All. Additional cases cover dense replacement, a high-hit one-character
query, no results, and the repository's real `src/app/App.tsx`. Input hashes
make changes to that real-code corpus visible in the JSON.

The editor accepts at most **2 MiB and 16 KiB per line**. Default corpora respect
those limits. `--large` adds **engine-only stress cases**: 5 MiB inputs and a
1 MiB single line. They are not currently editable files and are not evidence
of app performance at those sizes.

These are engine timings with the standalone harness's system allocator, not
GPUI frame timings or OS keystroke latency. The macOS app uses mimalloc. For the
editor integration path use `../search-ui-latency.sh`; its `native-dev` numbers
must not be mixed with these release-engine numbers.

## Process memory

For an indicative process high-water mark on macOS, run each side in a fresh
process (after building), selecting exactly one case:

```sh
/usr/bin/time -l scripts/perf/search-bench/target/release/search-bench \
  once-old --bundle /path/to/native-editor-old.js --filter unicode-literal-1MB
/usr/bin/time -l scripts/perf/search-bench/target/release/search-bench \
  once-new --bundle /path/to/native-editor-old.js --filter unicode-literal-1MB
```

Repeat each side and compare `maximum resident set size` (bytes on macOS).
This includes process startup, the selected corpus, bundle text and results;
it is **not** an allocation count or the running application's memory usage.
The Unicode path builds a normalized buffer and compact ASCII-run/Unicode-
expansion offset mapping. Dense Unicode still has materially higher temporary
memory and latency than ASCII; see the measured memory table in RESULTS.md.

## Integration and regression coverage

`crates/monocode-gpui/src/editor_search.rs` is a standalone synchronous module:
`search(buffer, query, replacement, SearchOptions) -> SearchResult` and
`replace_all(buffer, matches) -> Result<String, String>`. The harness imports
that exact production module, so it cannot benchmark a divergent copy.

Inputs are UTF-8, LF-normalized editor text; results are ordered, non-overlapping
byte ranges. Regex searches have a 50 ms per-match limit and a 3 s scan deadline
(cooperative checks, not a hard wall-clock guarantee on compilation). Timeout
results contain no partial matches. CodeMirror's pinned semantics—including
its multiline whole-word rejection quirk—are kept for replacement parity.

`file_editor/search.rs` owns the GPUI adapter and panel: immutable snapshots,
background work, validation before applying results or edits, and optional
latency logging. It rejects changed buffers, queries, replacements, options,
generations and closed panels. Obsolete task handles are dropped; the mock
typing benchmark does not establish cancellation latency for running workers.

The normal suite keeps three engine tests: a frozen semantic table (also checks
replacement output), invalid-range/UTF-8 replacement safety, and timeout bounds.
One adapter test exercises deterministic late-result races. Existing editor
tests cover navigation, Replace All undo, autosave and original file encoding.
