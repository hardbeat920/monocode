import { beforeEach, expect, it, vi } from "vitest";

const { runPrompt, context } = vi.hoisted(() => ({
  runPrompt: vi.fn(async (_input: { cwd: string; prompt: string }) =>
    JSON.stringify({ subject: "Update selected files", body: "" }),
  ),
  context: vi.fn(async (_cwd: string, paths?: readonly string[]) => ({
    branch: "feature",
    summary: paths ? "chosen.txt | 1 +" : "private.txt | 1 +",
    patch: paths ? "SELECTED_CONTENT" : "EXCLUDED_SECRET",
  })),
}));
vi.mock("./availability", () => ({ isHarnessAvailable: () => true }));
vi.mock("../../../platform/tauri/fs", () => ({
  gitStagedContext: context,
  gitRangeContext: vi.fn(),
}));
vi.mock("../providers/claude/claudeText", () => ({
  runClaudeTextPrompt: runPrompt,
}));
vi.mock("../providers/codex/codexText", () => ({
  runCodexTextPrompt: runPrompt,
}));
vi.mock("../providers/cursor/cursorText", () => ({
  runCursorTextPrompt: runPrompt,
  stopCursorTextPrompt: vi.fn(),
}));
vi.mock("../providers/grok/grokText", () => ({ runGrokTextPrompt: runPrompt }));
vi.mock("../providers/opencode/opencodeText", () => ({
  runOpenCodeTextPrompt: runPrompt,
}));

import { generateCommitMessage } from "./textHarness";
import { registerHarness, type HarnessAdapter } from "./registry";
import { generateClaudeCommitMessage } from "../providers/claude/claudeGit";
import { generateCodexCommitMessage } from "../providers/codex/codexGit";
import { generateCursorCommitMessage } from "../providers/cursor/cursorGit";
import { generateGrokCommitMessage } from "../providers/grok/grokGit";
import { generateOpenCodeCommitMessage } from "../providers/opencode/opencodeGit";

const providers = [
  ["claude", generateClaudeCommitMessage],
  ["codex", generateCodexCommitMessage],
  ["cursor", generateCursorCommitMessage],
  ["grok", generateGrokCommitMessage],
  ["opencode", generateOpenCodeCommitMessage],
] as const;

beforeEach(() => {
  vi.clearAllMocks();
  for (const [id, generate] of providers) {
    const adapter: HarnessAdapter = {
      id,
      live: true,
      generateCommitMessage: generate,
      sendTurn: async () => {},
      steerTurn: async () => {},
      cancelTurn: async () => {},
      respondApproval: () => {},
      stopSession: async () => {},
      forgetSession: async () => {},
      bindSession: () => {},
    };
    registerHarness(adapter);
  }
});

it.each(providers)(
  "keeps the chosen files through the public generator and %s adapter",
  async (id) => {
    const signal = new AbortController().signal;
    expect(
      await generateCommitMessage("/repo", id, signal, ["chosen.txt"]),
    ).toBe("Update selected files");
    expect(context).toHaveBeenCalledExactlyOnceWith("/repo", ["chosen.txt"]);
    expect(runPrompt).toHaveBeenCalledOnce();
    const request = runPrompt.mock.calls[0][0];
    expect(request.cwd).toBe("/repo");
    expect(request.prompt).toContain("SELECTED_CONTENT");
    expect(request.prompt).not.toContain("EXCLUDED_SECRET");
  },
);

it("retains whole-index context for callers without a selection", async () => {
  await generateCommitMessage("/repo", "codex");
  expect(context).toHaveBeenCalledExactlyOnceWith("/repo", undefined);
  expect(runPrompt.mock.calls[0][0].prompt).toContain("EXCLUDED_SECRET");
});

it("does not read Git or call a model for an already cancelled generation", async () => {
  const controller = new AbortController();
  controller.abort();
  await expect(
    generateCommitMessage("/repo", "codex", controller.signal, ["chosen.txt"]),
  ).rejects.toThrow();
  expect(context).not.toHaveBeenCalled();
  expect(runPrompt).not.toHaveBeenCalled();
});
