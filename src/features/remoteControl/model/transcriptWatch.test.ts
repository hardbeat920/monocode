import { afterEach, describe, expect, it, vi } from "vitest";
import { createTranscriptReader, watchTranscript } from "./transcriptWatch";
import { emptyCursor } from "./transcript";

const PATH = "/home/.claude/projects/-a/s.jsonl";

function line(text: string): string {
  return `${JSON.stringify({ type: "user", text })}\n`;
}

function bytes(text: string): number {
  return new TextEncoder().encode(text).length;
}

/**
 * Stands in for `read_file_range`: serves a byte range of a growing file and
 * records what was asked for. Mirrors the command's own rules — empty past the
 * end, and never splitting a character.
 */
function fakeFile(initial = "") {
  const state = { text: initial, reads: [] as number[] };
  return {
    state,
    append: (more: string) => {
      state.text += more;
    },
    replaceWith: (next: string) => {
      state.text = next;
    },
    readRange: async (_path: string, offset: number, maxBytes: number) => {
      state.reads.push(offset);
      const all = new TextEncoder().encode(state.text);
      const size = all.length;
      if (offset >= size) return { text: "", size };
      const slice = all.subarray(offset, offset + Math.max(maxBytes, 4));
      const decoded = new TextDecoder("utf-8", { fatal: false }).decode(slice);
      return { text: decoded.replace(/�+$/, ""), size };
    },
  };
}

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("createTranscriptReader", () => {
  it("resumes from the handover offset instead of replaying", async () => {
    const existing = line("old");
    const file = fakeFile(existing + line("new"));
    const seen: Record<string, unknown>[] = [];
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(bytes(existing)),
      onRecords: (records) => seen.push(...records),
      readRange: file.readRange,
    });

    await reader.poll();

    expect(seen).toEqual([{ type: "user", text: "new" }]);
    // It must never have asked for anything before the handover point.
    expect(Math.min(...file.state.reads)).toBe(bytes(existing));
  });

  it("replays from the start only when told to", async () => {
    const file = fakeFile(line("first") + line("second"));
    const seen: Record<string, unknown>[] = [];
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(),
      onRecords: (records) => seen.push(...records),
      readRange: file.readRange,
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
      readRange: file.readRange,
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
      readRange: file.readRange,
    });

    await reader.poll();
    // The half-written line must not be emitted or dropped.
    expect(seen).toEqual([]);

    file.append(whole.slice(cut));
    await reader.poll();

    expect(seen).toEqual([{ type: "user", text: "split-me" }]);
  });

  it("advances the cursor past the partial so it is not asked for again", async () => {
    const whole = line("abc");
    const file = fakeFile(whole.slice(0, whole.length - 2));
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(),
      onRecords: () => {},
      readRange: file.readRange,
    });

    await reader.poll();
    const after = reader.cursor();
    expect(after.offset).toBe(whole.length - 2);
    expect(after.partial).not.toBe("");

    await reader.poll();
    // The second poll resumes at the cursor, not back at the partial's start.
    expect(file.state.reads.at(-1)).toBe(whole.length - 2);
  });

  it("drains a backlog in one poll instead of one chunk per tick", async () => {
    const many = Array.from({ length: 40 }, (_, i) => line(`r${i}`)).join("");
    const file = fakeFile(many);
    const seen: Record<string, unknown>[] = [];
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(),
      onRecords: (records) => seen.push(...records),
      // A tiny window forces many reads to get through the backlog.
      readRange: (path, offset) => file.readRange(path, offset, 64),
    });

    await reader.poll();

    expect(file.state.reads.length).toBeGreaterThan(1);
    expect(seen.length).toBeGreaterThan(1);
  });

  it("stops reading once it has caught up", async () => {
    const file = fakeFile(line("only"));
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(),
      onRecords: () => {},
      readRange: file.readRange,
    });

    await reader.poll();

    // One read to get the content, one to confirm there is no more.
    expect(file.state.reads.length).toBeLessThanOrEqual(2);
  });

  it("does not rewind when the file was replaced by a shorter one", async () => {
    const file = fakeFile(line("a") + line("b"));
    const seen: Record<string, unknown>[] = [];
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(),
      onRecords: (records) => seen.push(...records),
      readRange: file.readRange,
    });

    await reader.poll();
    const before = seen.length;
    file.replaceWith(line("z"));
    await reader.poll();

    // Replaying a replaced transcript would flood the mirror.
    expect(seen).toHaveLength(before);
  });

  it("emits nothing and does not throw when the file is missing", async () => {
    const onRecords = vi.fn();
    const onError = vi.fn();
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(),
      onRecords,
      onError,
      readRange: () => Promise.reject(new Error("ENOENT")),
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
      readRange: () => Promise.reject(new Error("ENOENT")),
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
      readRange: async () => {
        reads += 1;
        await gate;
        return { text, size: bytes(text) };
      },
    });

    const first = reader.poll();
    const second = reader.poll();
    release?.();
    await Promise.all([first, second]);

    // Without the guard both polls would read, from the same offset.
    expect(reads).toBe(1);
    expect(seen).toHaveLength(1);
  });

  it("does not emit when only a partial line is available", async () => {
    const onRecords = vi.fn();
    const partial = '{"type":"user"';
    const reader = createTranscriptReader({
      path: PATH,
      from: emptyCursor(),
      onRecords,
      readRange: async () => ({ text: partial, size: bytes(partial) }),
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
      readRange: file.readRange,
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
      readRange: file.readRange,
      intervalMs: 10,
    });

    expect(() => {
      watcher.stop();
      watcher.stop();
      watcher.stop();
    }).not.toThrow();

    const readsAtStop = file.state.reads.length;
    await vi.advanceTimersByTimeAsync(100);
    expect(file.state.reads.length).toBe(readsAtStop);
  });

  it("tolerates a missing file without throwing", async () => {
    vi.useFakeTimers();
    const onRecords = vi.fn();
    const watcher = watchTranscript({
      path: PATH,
      from: emptyCursor(),
      onRecords,
      readRange: () => Promise.reject(new Error("ENOENT")),
      intervalMs: 10,
    });

    await vi.advanceTimersByTimeAsync(30);
    expect(onRecords).not.toHaveBeenCalled();
    expect(() => watcher.stop()).not.toThrow();
  });
});
