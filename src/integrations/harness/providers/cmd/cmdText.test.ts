import { beforeEach, describe, expect, it, vi } from "vitest";

type Watch = {
  id: string;
  onLine: (line: string) => void;
  onExit: (code: number | null) => void;
};

const state = vi.hoisted(() => ({
  spawned: [] as string[],
  watches: [] as Watch[],
}));

vi.mock("../../core/child", () => ({
  resolveCmdBinary: async () => ({ path: "/fake/cmd" }),
  spawnChild: async (id: string) => {
    state.spawned.push(id);
  },
  watchChild: (
    id: string,
    onLine: (line: string) => void,
    onExit: (code: number | null) => void,
  ) => {
    state.watches.push({ id, onLine, onExit });
  },
  unwatchChild: (id: string) => {
    state.watches = state.watches.filter((watch) => watch.id !== id);
  },
  killChild: async () => undefined,
}));

const { runCmdTextPrompt } = await import("./cmdText");

const waitFor = async (pred: () => boolean, label: string) => {
  for (let i = 0; i < 400; i += 1) {
    if (pred()) return;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  throw new Error(`timed out waiting for ${label}`);
};

describe("cmd text generation", () => {
  beforeEach(() => {
    state.spawned.length = 0;
    state.watches.length = 0;
  });

  it("gives overlapping helpers separate child ids", async () => {
    const first = runCmdTextPrompt({ cwd: "/repo", prompt: "first" });
    const second = runCmdTextPrompt({ cwd: "/repo", prompt: "second" });
    await waitFor(
      () => state.spawned.length === 2 && state.watches.length === 2,
      "two children",
    );

    expect(new Set(state.spawned).size).toBe(2);
    for (const watch of state.watches) {
      watch.onLine(
        JSON.stringify({
          type: "result",
          subtype: "success",
          finalText: watch.id,
        }),
      );
    }
    await expect(first).resolves.toContain("monocode-cmd-text-");
    await expect(second).resolves.toContain("monocode-cmd-text-");
  });
});
