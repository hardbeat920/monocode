import { beforeEach, describe, expect, it, vi } from "vitest";

const spawned: Array<{ command: string; args: string[]; cwd: string }> = [];
let onLine: ((line: string) => void) | undefined;
let onClose: (() => void) | undefined;
let killCalls = 0;

vi.mock("../../core/child", () => ({
  resolveOpenCrabsBinary: async () => ({ path: "/fake/opencrabs" }),
  spawnChild: async (_id: string, command: string, args: string[], cwd: string) => {
    spawned.push({ command, args, cwd });
  },
  killChild: async () => {
    killCalls += 1;
  },
  unwatchChild: () => undefined,
  watchChild: (
    _id: string,
    line: (l: string) => void,
    close: () => void,
  ) => {
    onLine = line;
    onClose = close;
  },
}));

import { parseRunSummary, runOpenCrabsTextPrompt } from "./opencrabsText";

describe("parseRunSummary", () => {
  it("reads the content field of the final JSON object", () => {
    const stdout = [
      "🤔 Processing...",
      "TITLE-OK",
      "{",
      '  "content": "TITLE-OK",',
      '  "cost": 0.01,',
      '  "session_id": "abc"',
      "}",
    ].join("\n");
    expect(parseRunSummary(stdout)).toBe("TITLE-OK");
  });

  it("skips brace lines that are not the summary object", () => {
    const stdout = '{"noise": true}\nplain answer\n{"content": "real"}';
    expect(parseRunSummary(stdout)).toBe("real");
  });

  it("returns null when no summary object is present", () => {
    expect(parseRunSummary("just plain text\nmore text")).toBeNull();
  });
});

describe("runOpenCrabsTextPrompt", () => {
  beforeEach(() => {
    spawned.length = 0;
    onLine = undefined;
    onClose = undefined;
    killCalls = 0;
  });

  it("spawns opencrabs run with json format and returns the content", async () => {
    const pending = runOpenCrabsTextPrompt({
      cwd: "/repo",
      prompt: "title this",
    });
    await vi.waitFor(() => {
      expect(spawned).toHaveLength(1);
    });
    expect(spawned[0]).toEqual({
      command: "/fake/opencrabs",
      args: ["run", "--quiet", "--format", "json", "title this"],
      cwd: "/repo",
    });
    onLine?.("🤔 Processing...");
    onLine?.('{"content": "My Title"}');
    onClose?.();
    await expect(pending).resolves.toBe("My Title");
  });

  it("rejects when the run produces no summary object", async () => {
    const pending = runOpenCrabsTextPrompt({ cwd: "/repo", prompt: "hi" });
    await vi.waitFor(() => {
      expect(spawned).toHaveLength(1);
    });
    onLine?.("everything broke");
    onClose?.();
    await expect(pending).rejects.toThrow(/everything broke/);
  });

  it("rejects with a timeout error, kills once, and keeps the queue usable", async () => {
    vi.useFakeTimers();
    try {
      const timed = runOpenCrabsTextPrompt({
        cwd: "/repo",
        prompt: "slow",
        timeoutMs: 1000,
      });
      // Let the spawn settle; the child never emits an exit event.
      await vi.advanceTimersByTimeAsync(0);
      onLine?.("partial output");

      // Attach the rejection handler before the timer fires, so the rejection
      // never has an unhandled window (Node PromiseRejectionHandledWarning).
      const rejection = expect(timed).rejects.toThrow(/timed out after 1000ms/);
      await vi.advanceTimersByTimeAsync(1000);
      await rejection;
      // Timeout kill only — the finally must not fire a second kill.
      expect(killCalls).toBe(1);

      // The serialized queue must not be wedged by the timed-out turn.
      const next = runOpenCrabsTextPrompt({
        cwd: "/repo",
        prompt: "recover",
        timeoutMs: 1000,
      });
      await vi.advanceTimersByTimeAsync(0);
      expect(spawned).toHaveLength(2);
      onLine?.('{"content": "recovered"}');
      onClose?.();
      await expect(next).resolves.toBe("recovered");
    } finally {
      vi.useRealTimers();
    }
  });
});
