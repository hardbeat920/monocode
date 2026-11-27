#!/usr/bin/env python3
"""A stand-in `claude` that streams a long scripted reply, for profiling.

Point the app at it with MONOCODE_CLAUDE_BINARY (see scripts/perf/README.md).
It speaks just enough of Claude Code's stream-json protocol for a turn: it
answers the initialize request, and every user message gets one long Markdown
reply streamed as text deltas at a steady, realistic pace, then a result. No
network, no tokens, the same text every run, so CPU profiles compare.

Environment:
  REPLAY_SECONDS      how long each reply streams (default 60)
  REPLAY_INTERVAL_MS  time between deltas (default 40, like Claude's ~25 Hz)
  REPLAY_CHARS        characters per delta (default 16, about 4 tokens)
  REPLAY_MARK         a file to create when a reply starts streaming
"""
import json
import os
import sys
import time
import uuid

SECONDS = float(os.environ.get("REPLAY_SECONDS", "60"))
INTERVAL = float(os.environ.get("REPLAY_INTERVAL_MS", "40")) / 1000
CHARS = max(1, int(os.environ.get("REPLAY_CHARS", "16")))
MODEL = "claude-replay"
SESSION = str(uuid.uuid4())


def emit(frame):
    sys.stdout.write(json.dumps(frame, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def section(n):
    """One deterministic chunk of mixed Markdown: prose, a list, code, a table."""
    return (
        f"## Step {n}: tighten the frame loop\n\n"
        f"Frame {n} keeps the transcript retained while only the live row grows. "
        "Every settled row keeps its layout, its measured height and its painted "
        "glyphs, so a streamed token costs what the new words cost and nothing more. "
        "The window still presents at the display's rate, which keeps the reveal smooth.\n\n"
        f"- Measure row {n} once per content version, not once per frame.\n"
        "- Move rows that only glide by translating their paint, not by laying them out again.\n"
        "- Keep `Shell` and `Chrome` out of the rebuild when the transcript notifies.\n\n"
        "```rust\n"
        f"fn step_{n}(rows: &mut [Row], width: Pixels) -> usize {{\n"
        "    let mut changed = 0;\n"
        "    for row in rows.iter_mut().filter(|r| r.version != r.measured_version) {\n"
        "        row.height = row.measure(width);\n"
        "        row.measured_version = row.version;\n"
        "        changed += 1;\n"
        "    }\n"
        "    changed\n"
        "}\n"
        "```\n\n"
        "| Phase | Before | After |\n"
        "|---|---:|---:|\n"
        f"| layout | {20 + n % 7}% | {3 + n % 3}% |\n"
        f"| paint | {11 + n % 5}% | {4 + n % 2}% |\n\n"
    )


def reply_text():
    total = int(SECONDS / INTERVAL) * CHARS
    parts, size, n = [], 0, 1
    while size < total:
        chunk = section(n)
        parts.append(chunk)
        size += len(chunk)
        n += 1
    return "".join(parts)[:total]


def stream_reply():
    text = reply_text()
    mark = os.environ.get("REPLAY_MARK")
    if mark:
        open(mark, "a").close()
    emit({"type": "stream_event", "session_id": SESSION, "parent_tool_use_id": None,
          "event": {"type": "message_start", "message": {"model": MODEL, "role": "assistant"}}})
    emit({"type": "stream_event", "session_id": SESSION, "parent_tool_use_id": None,
          "event": {"type": "content_block_start", "index": 0,
                    "content_block": {"type": "text", "text": ""}}})
    started = time.monotonic()
    for i in range(0, len(text), CHARS):
        # Pace against the start, so a slow write does not stretch the stream.
        delay = started + (i // CHARS) * INTERVAL - time.monotonic()
        if delay > 0:
            time.sleep(delay)
        emit({"type": "stream_event", "session_id": SESSION, "parent_tool_use_id": None,
              "event": {"type": "content_block_delta", "index": 0,
                        "delta": {"type": "text_delta", "text": text[i:i + CHARS]}}})
    emit({"type": "stream_event", "session_id": SESSION, "parent_tool_use_id": None,
          "event": {"type": "content_block_stop", "index": 0}})
    emit({"type": "assistant", "session_id": SESSION, "parent_tool_use_id": None,
          "message": {"model": MODEL, "role": "assistant",
                      "content": [{"type": "text", "text": text}],
                      "usage": {"input_tokens": 1000, "output_tokens": len(text) // 4}}})
    emit({"type": "stream_event", "session_id": SESSION, "parent_tool_use_id": None,
          "event": {"type": "message_stop"}})
    emit({"type": "result", "subtype": "success", "is_error": False, "session_id": SESSION,
          "result": text, "duration_ms": int((time.monotonic() - started) * 1000),
          "usage": {"input_tokens": 1000, "output_tokens": len(text) // 4},
          "modelUsage": {MODEL: {"contextWindow": 200000}}})


def main():
    if "--version" in sys.argv[1:] or "-v" in sys.argv[1:]:
        print("2.1.999 (Claude Code replay)")
        return
    for line in sys.stdin:
        try:
            frame = json.loads(line)
        except ValueError:
            continue
        kind = frame.get("type")
        if kind == "control_request":
            # Model discovery asks for list_models; answer with the replay model.
            answer = {}
            if frame.get("request", {}).get("subtype") == "list_models":
                answer = {"models": [{"value": MODEL, "displayName": "Replay",
                                      "description": "Scripted stream for profiling"}]}
            emit({"type": "control_response",
                  "response": {"subtype": "success", "request_id": frame.get("request_id"),
                               "response": answer}})
            if frame.get("request", {}).get("subtype") == "initialize":
                emit({"type": "system", "subtype": "init", "session_id": SESSION,
                      "model": MODEL, "tools": [], "mcp_servers": []})
        elif kind == "user":
            stream_reply()


if __name__ == "__main__":
    main()
