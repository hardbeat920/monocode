import { beforeEach, expect, it, vi } from "vitest";

const spawned: string[][] = [];
let onLine: ((line: string) => void) | null = null;

vi.mock("../../core/child", () => ({
  resolveClaudeBinary: async () => ({ path: "/fake/claude" }),
  spawnChild: async (_id: string, _path: string, args: string[]) => {
    spawned.push(args);
    onLine?.(JSON.stringify({ type: "system", subtype: "init" }));
  },
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (l: string) => void) => {
    onLine = line;
  },
  writeChild: async () => {
    onLine?.(
      JSON.stringify({
        type: "assistant",
        message: { content: [{ type: "text", text: '{"title":"Review PR"}' }] },
      }),
    );
    onLine?.(JSON.stringify({ type: "result", subtype: "success" }));
  },
}));

const { generateClaudeSessionTitle } = await import("./claudeTitle");
const { runClaudeTextPrompt, warmupClaudeText } = await import("./claudeText");

function toolsArg(args: string[] | undefined): string | undefined {
  if (!args) return undefined;
  const at = args.indexOf("--tools");
  return at === -1 ? undefined : args[at + 1];
}

beforeEach(() => {
  spawned.length = 0;
});

it("generates titles with every tool disabled", async () => {
  await generateClaudeSessionTitle({
    sessionId: "s1",
    cwd: "/repo",
    message: "Review the newly opened pull request and leave a comment.",
  });
  expect(spawned).toHaveLength(1);
  expect(toolsArg(spawned[0])).toBe("");
});

it("keeps the CLI default tools for prompts that do not restrict them", async () => {
  await runClaudeTextPrompt({ cwd: "/repo", prompt: "Explain this file" });
  expect(spawned).toHaveLength(1);
  expect(spawned[0]).not.toContain("--tools");
});

it("warms up the tool-less helper", async () => {
  await warmupClaudeText("/repo");
  expect(toolsArg(spawned.at(-1))).toBe("");
});
