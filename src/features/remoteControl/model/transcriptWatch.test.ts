import { afterEach, describe, expect, it, vi } from "vitest";
import {
  chunkAfter,
  createTranscriptReader,
  watchTranscript,
} from "./transcriptWatch";
import { emptyCursor } from "./transcript";

const PATH = "/home/.claude/projects/-a/s.jsonl";

function line(text: string): string {
  return `${JSON.stringify({ type: "user", text })}\n`;
}

/** A file whose contents the test can grow between reads. */
function fakeFile(initial = "") {
  const state = { text: initial, reads: 0 };
  return {
    state,
    append: (more: string) => {
      state.text += more;
    },
    read: async () => {
      state.reads += 1;
      return state.text;
    },
  };
}

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("chunkAfter", () => {
  it("returns everything from a zero offset", () => {
    expect(chunkAfter("abc", 0)).toBe("abc");
  });

  it("returns only what was appended", () => {
    expect(chunkAfter("abcdef", 3)).toBe("def");
  });

  it("returns nothing when the file has not grown", () => {
    expect(chunkAfter("abc", 3)).toBe("");
  });

  it("slices by byte, not by UTF-16 unit", () => {
    // "é" is two bytes, so a code-unit slice would cut the wrong place and a
    // byte offset of 2 must land exactly after it.
    const text = "é" + "xy";
    expect(chunkAfter(text, 2)).toBe("xy");
  });

  it("refuses to rewind when the file shrank", () => {
    // A replaced transcript must not be replayed from the start.
    expect(chunkAfter("ab", 10)).toBe("");
  });
});

describe("createTranscriptReader", () => {
  it("resumes from the handover offset instead of replaying", async () => {
    const existing = line("old");
    const file = fakeFile(existing + line("new"));
    const seen: Record<string, unknown>[] = [];
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(new TextEncoder().encode(existing).length),
      onRecords: (records) => seen.push(...records),
      readFile: file.read,
    });

    await reader.poll();

    expect(seen).toEqual([{ type: "user", text: "new" }]);
  });

  it("replays from the start only when told to", async () => {
    const file = fakeFile(line("first") + line("second"));
    const seen: Record<string, unknown>[] = [];
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(),
      onRecords: (records) => seen.push(...records),
      readFile: file.read,
    });

    await reader.poll();

    expect(seen).toHaveLength(2);
  });

  it("delivers records appended between polls", async () => {
    const file = fakeFile(line("one"));
    const seen: Record<string, unknown>[] = [];
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(),
      onRecords: (records) => seen.push(...records),
      readFile: file.read,
    });

    await reader.poll();
    file.append(line("two"));
    await reader.poll();

    expect(seen.map((rec) => rec.text)).toEqual(["one", "two"]);
  });

  it("survives a line split across two reads", async () => {
    const whole = line("split-me");
    const cut = whole.length - 5;
    const file = fakeFile(whole.slice(0, cut));
    const seen: Record<string, unknown>[] = [];
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(),
      onRecords: (records) => seen.push(...records),
      readFile: file.read,
    });

    await reader.poll();
    // The half-written line must not be emitted or dropped.
    expect(seen).toEqual([]);

    file.append(whole.slice(cut));
    await reader.poll();

    expect(seen).toEqual([{ type: "user", text: "split-me" }]);
  });

  it("advances the cursor past the partial so it is not re-read", async () => {
    const whole = line("abc");
    const file = fakeFile(whole.slice(0, whole.length - 2));
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(),
      onRecords: () => {},
      readFile: file.read,
    });

    await reader.poll();
    const after = reader.cursor();
    expect(after.offset).toBe(whole.length - 2);
    expect(after.partial).not.toBe("");
  });

  it("emits nothing and does not throw when the file is missing", async () => {
    const onRecords = vi.fn();
    const onError = vi.fn();
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(),
      onRecords,
      onError,
      readFile: () => Promise.reject(new Error("ENOENT")),
    });

    await expect(reader.poll()).resolves.toBeUndefined();
    expect(onRecords).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledTimes(1);
  });

  it("keeps its cursor when a read fails", async () => {
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(7),
      onRecords: () => {},
      readFile: () => Promise.reject(new Error("ENOENT")),
    });

    await reader.poll();
    expect(reader.cursor().offset).toBe(7);
  });

  it("skips a poll that starts while another read is in flight", async () => {
    let release: (() => void) | undefined;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const text = line("once");
    let reads = 0;
    const seen: Record<string, unknown>[] = [];
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(),
      onRecords: (records) => seen.push(...records),
      readFile: async () => {
        reads += 1;
        await gate;
        return text;
      },
    });

    const first = reader.poll();
    const second = reader.poll();
    release?.();
    await Promise.all([first, second]);

    // The second poll must not have issued a read of its own; without the
    // guard both would read, and both would be re-reading the same offset.
    expect(reads).toBe(1);
    expect(seen).toHaveLength(1);
  });

  it("does not emit when only a partial line is available", async () => {
    const onRecords = vi.fn();
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(),
      onRecords,
      readFile: async () => '{"type":"user"',
    });

    await reader.poll();
    expect(onRecords).not.toHaveBeenCalled();
  });
});

describe("watchTranscript", () => {
  it("polls on an interval until stopped", async () => {
    vi.useFakeTimers();
    const file = fakeFile(line("a"));
    const seen: Record<string, unknown>[] = [];
    const watcher = watchTranscript({
      path: PATH,
      from: emptyCursor(),
      onRecords: (records) => seen.push(...records),
      readFile: file.read,
      intervalMs: 10,
    });

    await vi.advanceTimersByTimeAsync(0);
    expect(seen).toHaveLength(1);

    file.append(line("b"));
    await vi.advanceTimersByTimeAsync(10);
    expect(seen).toHaveLength(2);

    watcher.stop();
    file.append(line("c"));
    await vi.advanceTimersByTimeAsync(50);
    expect(seen).toHaveLength(2);
  });

  it("stops idempotently", async () => {
    vi.useFakeTimers();
    const file = fakeFile("");
    const watcher = watchTranscript({
      path: PATH,
      from: emptyCursor(),
      onRecords: () => {},
      readFile: file.read,
      intervalMs: 10,
    });

    expect(() => {
      watcher.stop();
      watcher.stop();
      watcher.stop();
    }).not.toThrow();

    const readsAtStop = file.state.reads;
    await vi.advanceTimersByTimeAsync(100);
    expect(file.state.reads).toBe(readsAtStop);
  });

  it("tolerates a missing file without throwing", async () => {
    vi.useFakeTimers();
    const onRecords = vi.fn();
    const watcher = watchTranscript({
      path: PATH,
      from: emptyCursor(),
      onRecords,
      readFile: () => Promise.reject(new Error("ENOENT")),
      intervalMs: 10,
    });

    await vi.advanceTimersByTimeAsync(30);
    expect(onRecords).not.toHaveBeenCalled();
    expect(() => watcher.stop()).not.toThrow();
  });
});
