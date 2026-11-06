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
