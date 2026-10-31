# Native GPUI performance audit and plan

Date: 2026-10-26. Branch: `feat/gpui` at `a6797621`.

Scope: the native app only (`crates/monocode-gpui`, `crates/monocode-core`).
The React/Tauri app is out of scope. The shared 30 Hz clock in `pulse.rs`
(spinners, composer runner, stream fade) is deliberate and stays.

Method: read-only passes over the transcript, lists, animation, and
main-thread paths, then a focused pass on large diffs. Items marked
**verified** were re-read in the code. Everything else is a code-reading
finding with no profile behind it yet, so phase 0 measures before anything
changes.

## Already in good shape (don't rework)

- Transcript is virtualized (`ListState`), reuses rows by `Arc::ptr_eq`, and
  stops scheduling frames when hidden or under reduced motion.
- Sidebar, explorer, project rail, changes list, history, diff view and inbox
  use `list` / `uniform_list`.
- Decorative loops share one clock (`pulse.rs`) that parks when idle and
  slows 8x when no window is visible. `reflow_motion`, `composer_motion`,
  `sidebar_peek`, `popover`, `particle_text`, `review` stop requesting frames
  once settled and honor reduced motion.
- Git reads, image decode, editor analysis and resource sampling run off the
  UI thread. `fs_watch` coalesces events through a `bounded(1)` channel.
  The review loader already skips unchanged files using
  `scm::file_diff_versions`.
- The diff overview rail is cheap (a few quads from precomputed offsets).

## Phase 0: measure first (half a day)

No fix lands without a before/after number.

| Scenario | What to do |
| --- | --- |
| Large diff | A diff of about 2,000 files and 500k lines: open it, scroll it, watch the Changes sidebar while it loads |
| Long transcript | Open a 2,000-row session, scroll top to bottom |
| Streaming | 5 min streaming answer with tool calls, 1 agent and 4 agents in parallel |
| Big lists | 3,000 sessions in the sidebar, 20k-file repo in search / quick open |
| Idle | App visible, nothing running, 2 min |
| Cold start | Launch to first paint |

Tools: Instruments (Time Profiler + Animation Hitches), the existing
`hang_watchdog.rs` logs for UI stalls over five seconds. It cannot verify a
16 ms budget. Use the opt-in `perf-trace` build below for shorter events. Record per scenario:
hitch time, p95 frame time, longest UI-thread stall, idle CPU %, cold-start
time to first paint.

## Phase A: large diffs and the changes sidebar (top priority)

Symptom: with a large diff, the Changes sidebar and the diff view lag.
Everything below runs on the UI thread, so it stalls the whole window,
including the sidebar.

1. **Quadratic rebuild as files stream in.** **Verified.** Each finished file
   calls `review.rs` `publish` (`:~1218`), which rebuilds the `DiffFile` model
   for every file: a `format!` and several string clones per file, plus
   `entries` (`:307`) cloning every `ChangedFile`. `diff_view.rs:370`
   `set_files` then compares all files, and `rebuild` (`:417-515`) re-clones
   every row of every expanded file, recomputes all offsets and calls
   `list.reset`, which drops every measured row height. Cost is
   O(files + rows) per file load, so loading N files costs O(N × rows).
   *Fix:*
   - Coalesce `publish` to once per frame (or at most every 50 ms) while
     loads are in flight.
   - Update only the changed file: splice its rows and shift later offsets
     instead of rebuilding everything.
   - Use `list.splice` for the changed range instead of `list.reset`, so
     measured heights survive.
   - Make `entries` borrow instead of clone.
   *Expected:* per-file-load cost on the UI thread drops from O(all rows) to
   O(that file's rows), with no stall over 16 ms while a large diff loads.
2. **Changes panel rebuild.** **Verified.** `changes.rs:372` deep-clones the
   whole `DiffIndex` on every `rebuild`, `staged_unstaged` makes two passes,
   and `build_tree` (`:108`) does a linear `position` scan plus a `format!`
   for every path segment of every file. `rebuild` runs on every git change.
   *Fix:* borrow the index instead of cloning it, keep a `HashMap` of child
   dirs per node while building, and build the line list in the background
   task that already produces the index.
   *Expected:* sidebar refresh cost for 20k changed files falls from tens of
   milliseconds on the UI thread to a swap of a prebuilt list.
3. **git_status reload on the UI thread.** `git_status.rs:250-276` compares
   the full index, builds `status_maps`, clones the index, and sums file
   counts across up to 24 remembered checkouts, all in the foreground
   update. Build `statuses` in the background task and keep a running count.
   Also `resume()` (`:194-202`) forces a reload on every focus while
   watching; skip it when the last reload is recent and no fs event arrived.
4. **`git_cache::get`** (`git_cache.rs:19-23`) deep-clones a snapshot under a
   global mutex. Store `Arc<Snapshot>`.
5. **Small items in the diff view:**
   - `current_hunk` (`diff_view.rs:~636`) scans all hunks linearly; binary
     search the sorted offsets.
   - `rebuild`'s scroll-anchor lookup (`:~495`) walks all rows building a
     `RowKey` each time; look the key up through `file_rows` first.
   - `request_highlights` highlights one file at a time and runs every
     frame it is idle; confirm it dedupes (unverified) and consider
     prioritising the file under the viewport.

*Phase A expected result:* with the large-diff scenario, loading the diff
causes no UI-thread stall over 16 ms, the Changes sidebar stays responsive
throughout, and scrolling the diff holds the display rate.

## Phase 1: transcript and streaming

1. **Coalesce stream updates to one patch per frame.** `conversation.rs:1154`
   handles each `Update` as it arrives (`rx.recv().await`), and each calls
   `transcript.patch` → `rebuild` (`:1193`). The producer already blocks on a
   full channel (`runtime.rs:56-72`). Drain everything queued with
   `try_recv`, merge rows by start index, and patch once.
   *Expected:* rebuilds during a fast stream are capped at the display rate
   instead of the token rate.
2. **Hoist the `last_prompt` scan.** **Verified.** `transcript.rs:3476`
   runs `self.source.iter().rposition(..)` once per rebuilt message and isn't
   gated by `self.mono`. Compute once per rebuild.
3. **Stop parsing tool-call JSON on every rebuild.** **Verified.**
   `transcript.rs:2931-2935` runs `serde_json::from_str::<ToolCall>` on every
   tool row of the current turn per rebuild (non-mono). Cache an
   `is_agent_panel` flag when the row is ingested.
4. **Avoid cloning the row suffix per rebuild.** **Verified.**
   `transcript.rs:2987` `self.rows[display_start..].to_vec()` plus a per-row
   clone at `:2992`. Move the old rows out (`split_off`) instead.
5. **Confirm markdown reparse is incremental.** `transcript.rs:3330` hands
   the whole message to `parser.set_text`. Check in the profile that
   `markdown_model.rs` only parses new bytes; fix only if it doesn't.

*Expected result:* in a 2,000-row session, per-token cost tracks the size of
the live message, not the whole conversation.

## Phase 2: lists and pickers

1. **Search ranking off the UI thread.** **Verified.** `search.rs:233` `rank`
   runs `search::filenames` over every project file synchronously on each
   keystroke, and `monocode-core/src/search.rs:142` `fuzzy` allocates a
   lowercased char `Vec` per candidate. Precompute lowercased haystacks when
   files load, then rank on `background_executor` using the revision pattern
   `refresh_live` already uses.
2. **Virtualize search results.** `search.rs:692` and `project_search.rs:~608`
   render every hit inside `overflow_y_scroll`. Rows are fixed height; use
   `uniform_list`.
3. **Git picker.** `git_picker.rs:173-210` rebuilds and lowercases every
   choice on each render. Cache choices and use `uniform_list`.
4. **Quick open.** `quick_open.rs:278` renders all rows. Cap or virtualize.
5. **Not yet audited:** `git_graph.rs` (rows built per render), and the
   `pickers.rs`, `notes.rs`, `automations.rs`, `settings` scroll lists. Check
   their row counts in phase 0; convert any list that can exceed ~100 rows.

*Expected result:* typing in search, quick open or the branch picker causes
no stall over 16 ms, and render cost scales with the viewport.

## Phase 3: animation

Scope is limited to timing and wasted work; the 30 Hz clock stays.
Archive animation is outside this performance PR's scope.

1. **Pulse notifies whole views.** `pulse.rs:169` `cx.notify(view)` re-renders
   the entire leased view each tick. Make sure each looping element
   (spinner, mascot) is its own small entity, so a tick doesn't re-render a
   transcript or sidebar.
2. **Transcript burst** (`transcript.rs:3963-3970`) runs on uncapped
   `on_next_frame`. Confirm that `animate_frame` repaints only the burst
   layer and not the whole transcript (unverified).
3. **Arcade.** `arcade.rs:118` parses JSON per QuickJS step and `:190-213`
   paints a full grid per frame. Low priority.

## Phase 4: main-thread stalls and startup

1. **Defer the thread-dir sweep.** **Verified.** `main.rs:320`
   `db.sweep_thread_dirs()` runs before `Application::run`, doing one SQLite
   query per directory and a `remove_dir_all` per deleted thread. Run it on a
   background task after the window opens.
2. **Store lock on the UI thread.** `shell/app_views.rs:319` locks the shared
   store in an event handler while a turn can be writing large row batches.
   `runtime/persistence.rs:65-71` scans all sessions under that lock on each
   flush. Replace the scan with a lookup by id.
3. **Editor keystrokes.** `file_editor.rs:609-616` clones the whole buffer and
   reruns native analysis on each edit. Use `Arc<str>` snapshots and a
   30-50 ms debounce.
4. **Polling.** `usage_footer.rs:90-99` refreshes every 30 s regardless of
   visibility; gate it on window visibility.

*Expected result:* no `hang_watchdog` entries over 50 ms in the phase 0
scenarios, and cold start no longer waits on disk cleanup.

## Suggested order

| # | Work | Effort | Impact |
| --- | --- | --- | --- |
| 0 | Baselines | S | needed for everything |
| 1 | Large diffs + changes sidebar (Phase A 1-3) | M-L | the reported lag |
| 2 | Stream coalescing + rebuild fixes (Phase 1) | M | streaming in long sessions |
| 3 | Search off-thread + virtualize results (Phase 2.1-2.2) | M | typing hitches |
| 4 | Startup sweep, store lock, git_cache (Phase 4, A4) | S-M | cold start, stalls |
| 5 | Pulse scoping (Phase 3) | S | animation smoothness |
| 6 | Remaining lists, diff-view small items | M | long tail |

Re-run the phase 0 scenarios after each step and record the numbers here.


## Implementation status, 2026-10-26

This report records the investigation in the shared `feat/gpui` checkout. The
performance PR targets `mc/migrate-monocode-to-gpui` directly and carries only
its performance changes. Archive animation is outside this PR's scope and is
not a prerequisite for merging it. Deferred thread-directory cleanup is also
excluded because thread-file support is absent from this upstream base.
The implementation table below describes that shared checkout. The live
capture and benchmark numbers also came from that checkout, not the isolated
PR branch. Validation of the isolated PR branch is recorded separately below.

The native changes below are implemented. The first isolated live capture is
recorded below. The in-memory benchmarks do not measure frame time, Git I/O,
painting, startup, or idle CPU. The user approved a new temporary synthetic
database for the live capture. Existing application databases stayed unopened.

| Phase | Changes | Remaining work |
| --- | --- | --- |
| A | Review publications batch for 50 ms, with an immediate final flush. They cache the manifest and update only completed file models and their totals. Diff updates replace changed files through `ListState::splice`. Entry metadata indexes the original files. The Git worker prepares status maps and both sidebar layouts. Indices and cached checkout snapshots use `Arc`. Focus reloads respect the existing TTL. Hunk lookup uses binary search, and row anchors search within their file. | Flat row storage still moves later rows and shifts their offsets. Verify the complete window under Instruments before claiming the 16 ms target. |
| 1 | Incoming updates batch for 16 ms. Source patches retain their order and rebuild once from the earliest changed row. Agent-panel detection is cached at ingestion and follows history offsets. Rebuilds move the old row suffix and compute the last prompt once, only for Mono. | Measure a real stream with one agent and four agents. |
| 2 | Filename ranking runs in the background with a revision check. `FilenameIndex` prepares lowercase characters when files load. Project search virtualizes headers and matches. The branch picker caches its filtered choices and virtualizes variable-height rows. | Profile notes, automations, and model lists with realistic counts before changing their rendering. |
| 3 | The shared 30 Hz pulse cadence stays unchanged. Archive animation is outside this PR's scope. | A child-entity prototype did not isolate pulse redraws because GPUI dirties ancestor views. The prototype was removed. Transcript animation still traverses rows. Arcade remains low priority. |
| 4 | Native editor analysis waits for a 40 ms typing pause before taking shared text snapshots. The QuickJS fallback retains its 150 ms delay. Usage polling pauses when no window is visible. Thread-directory cleanup starts in the background after the first frame. Context persistence queries the session's provider directly instead of listing all sessions. | Cleanup still holds the shared store lock while it runs. Measure contention with many orphaned thread directories before changing its locking. The automation handler's store lock remains. |

The source checks corrected several assumptions in the original plan:

- Global search already caps results at 88 rows, and Quick Open caps files at
  80. Their rendering stays unchanged.
- History computes graph rows when commits change and renders them through
  `uniform_list`. It does not rebuild the graph on every render.
- Diff highlighting already deduplicates by block `Arc`, skips empty files,
  and selects from visible files. No scheduler change was needed.
- `MarkdownParser::set_text` reparses from a block boundary near the changed
  suffix, with full parsing for reference definitions and other global syntax.
  The existing incremental-parser tests cover this behavior.

### Measured code paths

Each cell gives the median followed by the minimum and maximum, in milliseconds.
The first three rows have 25 trials per side, across five alternating process
pairs. The tree builder has five alternating runs per side.

Speedup is the before median divided by the after median, rounded to one decimal.

| Timed work | Before median and range, ms | After median and range, ms | Speedup |
| --- | --- | --- | --- |
| 2,000 files, 500 diff updates | 3313.0, 3161.2 to 3534.9 | 249.2, 238.4 to 262.9 | 13.3× |
| 2,000 source rows, 20 rebuilds | 701.2, 686.1 to 744.6 | 112.6, 107.9 to 120.8 | 6.2× |
| 20,000 files, 20 filename queries | 2953.3, 2901.7 to 3136.9 | 542.5, 529.8 to 594.3 | 5.4× |
| 20,000-file tree builder, optimized standalone build | 29.9, 27.1 to 63.3 | 7.1, 6.8 to 14.7 | 4.2× |

The largest measured diff update across the alternating runs was 14.892 ms
before and 1.764 ms after, an 8.4× speedup comparing those maxima.
These durations exclude painting and do not prove
that the complete window meets its frame budget.

The source changes remove repeated row copies and JSON parsing. The remaining
diff work includes model-vector copies, row moves, and offset shifts. A separate
`sample` attempt could not inspect the benchmark process under the OS sandbox,
so profiler confirmation remains pending.

The [loading measurements](performance-runs.tsv) and
[tree measurements](performance-tree-runs.txt) contain the raw runs.

The follow-up manifest cache compares against the first implementation, which
already splices diff rows. Across 25 trials per side, 500 review publications
fell from a median 311.629 ms, range 302.416 to 348.529, to 236.308 ms, range
221.578 to 282.688, a 1.3× median speedup. See [publication runs](performance-review-runs.tsv).
Separate process counters were about one occupied CPU core on both sides.
See [resource counters](performance-review-resources.txt).

An isolated provider-lookup check created 3,000 sessions in an in-memory store,
then alternated five pairs of 100 lookups. Listing summaries and finding the
session took a median 876.254 ms per 100 calls, range 865.226 to 933.880.
The indexed provider query took 0.536 ms, range 0.529 to 0.627, a 1,634.8×
median speedup for this isolated check. Each call checked
the returned provider. Setup was outside the timer. This measures only the
provider check, not persistence or lock wait. The core used `native-dev`.
See [provider lookup runs](performance-provider-runs.tsv).

### Isolated live capture

`scripts/prepare-native-performance.py` creates a fresh directory under
`/private/tmp`, refuses an existing fixture database, seeds through the core
store API, and copies the native binary into a uniquely identified app bundle.
The bundle and launch environment set `MONOCODE_NATIVE_DATA_DIR` explicitly.
The fixture has 3,000 sessions, a 2,000-row transcript, 20,000 files, and 2,000
modified files with 500,000 added lines. No provider prompt was submitted.

One `native-dev` build with `--features perf-trace` captured GPUI draw timings,
foreground task polls, input dispatch, and process CPU. Set `MONOCODE_PERF_LOG`
to an output JSONL path to enable collection. Normal builds omit the module.
The collector writes on a background thread and records no prompt text.

| Capture interval | Draws | p95 draw, ms | Maximum draw, ms | Maximum retained task poll, ms |
| --- | --- | --- | --- | --- |
| First five seconds | 13 | 64.140 | 64.140 | 6.669 |
| Open all 2,000 changed files | 29 | 32.630 | 45.927 | 14.059 |
| Representative diff/transcript scroll | 5 | 22.400 | 22.400 | No retained polls |
| Quick Open query `module-19999` | 12 | 10.545 | 10.545 | No retained polls |
| Two minutes without input | 20 | 32.085 | 33.250 | 9.740 |

First frame submission finished 951.994 ms after entering `main`. The two-minute
idle interval averaged 2.096% process CPU, median 1.718%, across 116 samples.
One fully occupied core is 100%. The trace lost no foreground-journal entries.
The first draw spent 38.335 ms in prepaint, 18.237 ms in layout, and 7.452 ms
in paint. Startup and later draw work still exceed the 16 ms target.

These are after-only observations from one instrumented development build,
under concurrent machine load. Draw duration excludes frame submission and
does not measure display hitch duration. Task polls below GPUI's retention
threshold are absent. Recorded view durations include nested views, so they
cannot assign the parent's inclusive cost to that parent alone. The collector
now also emits `self_ms` for future captures.

The capture verified the loaded diff totals and Quick Open result. Scrolling
was sampled; it did not traverse the entire transcript or prove session-list
navigation. Five-minute streams with one and four agents remain unmeasured.
The capture predates the final deferred-cleanup and provider-lookup changes.
No orphaned-directory startup comparison was made.

See [exact intervals and results](performance-live-results.json) and the
[compressed complete trace](performance-live-trace.jsonl.gz). The temporary
app was closed after capture. Its synthetic database remains under the
temporary fixture directory for inspection; no reset or database cleanup ran.

### Verification

- 167 focused GPUI tests passed, including batched transcript updates, fold
  state, scroll anchors, branch navigation, project-search scrolling, editor
  analysis, review reuse, pulse visibility, and persistence. A new regression
  check confirms late context flushes preserve a newly selected provider or account.
- Both core filename checks passed, including recency, basename priority,
  Unicode case expansion, and UTF-8 highlight ranges.
- The provider lookup follows picker changes in both native and shared schemas.
- Strict Clippy checks pass for GPUI with all targets and for the core library.
  The combined all-targets check is blocked by unrelated permission-test module
  placement in `monocode-core/src/provider/lifecycle.rs`.
- The edited files pass Rustfmt with their crate editions. The repository-wide
  formatting check still flags unrelated working-tree edits in thread files,
  images, visual publication, follow-ups, the shell, and mobile rendering.
- Persistence tests use synthetic in-memory stores. The live app used only the
  approved temporary fixture. No existing application database was opened.

### Isolated PR validation

The PR branch starts at upstream migration commit `e1051f66` and excludes the
shared checkout's unrelated changes. On that checkout, 164 focused GPUI tests
and three core checks passed. Both crates passed strict Clippy with all targets
and `monocode-gpui/perf-trace` enabled. Workspace Rustfmt and `git diff --check`
also passed. The native tests compile with the default feature set.

The checks cover transcript batching and history, incremental diff publication,
scroll anchors, Changes layouts, branch and project-search virtualization,
editor analysis, cache reuse, visibility, and persistence ownership. This is
not a complete test-suite run. These checks used synthetic stores and fixtures.

### Pulse isolation experiment

A prototype moved each `pulse::looping` element into a persistent child entity.
The clock's existing eight-tick test still produced eight renders of the
containing view, in addition to eight spinner frames. Two isolation assertions
failed, while reduced-motion and stream-clock checks passed.

`Window::mark_view_dirty` walks `view_path_reversed` and dirties ancestor views.
Moving the loop alone therefore does not isolate its owner under this renderer.
The prototype and its caller changes were removed. The original pulse tests
pass, and the 30 Hz cadence is unchanged. A renderer-level change needs separate
profiling and cache-invalidation tests before implementation.

### Repeatable measurements

`scripts/bench-native-loading.py` alternates saved before and after test
binaries. Each test process runs five checked trials. The default five process
pairs produce 25 trials per side. Select datasets with the corresponding
`--*-before` flags. Use `--output` to keep separate comparison records.
The loading, transcript, and filename tests use the repository's `native-dev` profile. They do not
establish release-build or end-to-end UI performance.

The benchmark datasets and timed regions are:

- `large_diff_loading_timing`: 2,000 files, 500,000 added lines, four files per
  update. It times 500 calls to `DiffView::set_files` and verifies 504,000
  final rows. Diff-block generation and initial placeholder installation are
  outside the timer. The per-update model-vector clone is inside.
- `long_tool_turn_rebuild_timing`: 2,000 source rows, including 1,999 tool
  calls. It times 20 suffix rebuilds after initial ingestion.
- `filename_ranking_timing`: 20,000 project files. It times 20 queries for
  `src components`. Index preparation is outside the timer.

`scripts/bench-native-changes.py` compiles the production tree builder in
isolation with `rustc -O`. With `--baseline PATH`, it alternates five runs
against the earlier `changes.rs`. Each run verifies 20,000 files in 2,000
sibling directories. It excludes Git, GPUI, and sidebar painting.

The machine was busy during the runs. Treat these comparisons as evidence
about the named code paths. The full Phase 0 scenarios and profiler evidence
are still required before making an app-wide performance claim.
