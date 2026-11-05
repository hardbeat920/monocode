# Streaming profiles

A repeatable stream for measuring the UI's CPU while a reply arrives: no
agent, no network, the same text at the same pace every run.

- `fake-claude-stream.py` stands in for `claude`. Each message gets one long
  Markdown reply (prose, lists, code fences, tables) streamed as text deltas,
  every 40 ms by default. `REPLAY_SECONDS`, `REPLAY_INTERVAL_MS` and
  `REPLAY_CHARS` change its length and pace. The test
  `provider::process_tests::perf_replay_streams_a_whole_claude_turn` keeps it in
  step with the parser.
- `stream-profile.sh [binary] [seconds]` opens a separate Monocode window on
  throwaway data with the replay as Claude, waits for you to send a message,
  then profiles that window with Monocode Performance's `--profile`.

```sh
scripts/perf/stream-profile.sh                                   # the installed app
scripts/perf/stream-profile.sh target/release/monocode-gpui 30   # a local build
```

- `frame-run.sh [binary] [seconds]` does the same hands-off:
  `MONOCODE_PERF_PROMPT` makes the first chat switch to the replayed Claude
  and send a message by itself, and `MONOCODE_FRAME_LOG` writes one line per
  second. It prints the log and its averages. `PROFILE=1` also samples stacks
  over the same window.

```sh
scripts/perf/frame-run.sh crates/monocode-gpui/target/release/monocode-gpui 20
PROFILE=1 scripts/perf/frame-run.sh crates/monocode-gpui/target/release/monocode-gpui 15
```

A frame log line reads:

```
fps=39 layout=34 paint=5 refresh=0 draw=3.35/5.10ms busy=12.7% main=14.8% views=330/30
  by: session_mark::SessionMark=60 transcript::TranscriptView=39 ... | measure:markdown=7417 shape:lines=91
```

`fps` splits into frames where content changed (`layout`), animation-only
frames (`paint`) and whole-window redraws (`refresh`). `draw` is the average
and slowest `Window::draw`, `busy` the share of the second spent drawing,
`main` the main thread's CPU. `views` is entity views rendered vs. cached ones
reused. `by` names the views whose own `notify` asked for those frames. After
`|` come counts the app adds with `gpui::frame_stats::count` / `add`: fresh
row measurements in microseconds by row kind, and lines and characters GPUI
had to shape because its line cache missed. Any run of the app takes
`MONOCODE_FRAME_LOG=<file>`, not just the replay.

Compare a change with the report from before it (CPU, main thread, and the
view and layout rows). Profile a release build; a debug build's numbers say
little about release.

# Editor search

- `KEEP=1 scripts/perf/search-bench.sh --iterations 30` performs paired release
  measurements and exact result/output parity checks. See
  [methodology and integration contract](search-bench/README.md).
- `KEEP=1 scripts/perf/search-ui-latency.sh` builds the current tree and the
  upstream baseline (`c8ec3de2`) before timing. It runs the same ignored GPUI
  test in new/old, then old/new order and retains each log. `PROFILE` defaults
  to `native-dev` (the crate and dependencies use opt-level 1); set
  `PROFILE=release` for a separately built release comparison.

The UI test measures **key dispatch → test executor idle**, validating the
complete results after every sample. It reports each prefix of `needle`
separately, with two warmups and 20 measured rounds per pass (override with
`SEARCH_LATENCY_ROUNDS`, minimum 20). `coalesced:needle` dispatches all six keys
before background work runs, so only the final query needs to complete. It
does not simulate real typing intervals or cancelling in-flight work.

GPUI's mock platform and deterministic scheduler exercise the real editor
integration but exclude OS dispatch, real worker contention and physical
presentation. The time-to-idle boundary can also include other runnable work;
these are not hardware-to-photon measurements. Prefixes with more matches can
be much slower than the completed query, which a pooled median conceals.

For a live application probe, `MONOCODE_SEARCH_LATENCY=1 npm run native:dev`
logs search scheduling → results applied with `applied=true/false`. Cancelled
tasks that never deliver a result produce no completion line. The probe takes
timestamps only when enabled; leave it unset during benchmarks to avoid
timing log I/O. It does not measure paint or physical input latency.
