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

Compare a change with the report from before it (CPU, main thread, and the
view and layout rows). Profile a release build; a debug build's numbers say
little about release.
