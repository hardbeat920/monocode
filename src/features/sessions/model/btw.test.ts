import { describe, expect, it } from "vitest";
import {
  BTW_COMMAND,
  BTW_MAX_BLOCK_CHARS,
  BTW_MAX_SNAPSHOT_CHARS,
  buildBtwPrompt,
  btwVisibleBlocks,
  consumeBtwCommand,
  replaceBtwThread,
  serializeBtwBlock,
  serializeBtwSnapshot,
  supportsBtwHarness,
} from "./btw";
import type { Block, BtwThread } from "./session";

function block(id: string, role: Block["role"], text = id): Block {
  return { id, role, text };
}

function thread(overrides: Partial<BtwThread> = {}): BtwThread {
  return {
    id: "btw-1",
    sourceEndBlockId: "user-1",
    createdAt: 1,
    updatedAt: 2,
    status: "ready",
    messages: [
      { id: "question", role: "user", text: "Why?", createdAt: 1 },
      { id: "answer", role: "assistant", text: "Because.", createdAt: 2 },
    ],
    ...overrides,
  };
}

describe("supportsBtwHarness", () => {
  it("allows harnesses with an isolated text runner", () => {
    expect(supportsBtwHarness("claude")).toBe(true);
    expect(supportsBtwHarness("codex")).toBe(true);
    expect(supportsBtwHarness("opencode")).toBe(true);
    expect(supportsBtwHarness("pi")).toBe(true);
  });

  it("rejects unsupported and missing harnesses", () => {
    expect(supportsBtwHarness("fx")).toBe(false);
    expect(supportsBtwHarness("hermes")).toBe(false);
    expect(supportsBtwHarness("antigravity")).toBe(false);
    expect(supportsBtwHarness(undefined)).toBe(false);
  });
});

describe("consumeBtwCommand", () => {
  it("opens an empty BTW thread for the bare command", () => {
    expect(BTW_COMMAND).toMatchObject({
      kind: "builtin",
      invocation: "btw",
    });
    expect(consumeBtwCommand("  /BTW  ")).toEqual({
      text: "",
      matched: true,
    });
  });

  it("keeps the text after the command as the side question", () => {
    expect(consumeBtwCommand("/btw  what does this do?  ")).toEqual({
      text: "what does this do?",
      matched: true,
    });
  });

  it("only consumes a leading BTW command", () => {
    expect(consumeBtwCommand("ask /btw about this")).toEqual({
      text: "ask /btw about this",
      matched: false,
    });
  });
});

describe("btwVisibleBlocks", () => {
  it("returns only visible blocks through the anchored turn", () => {
    const blocks: Block[] = [
      block("before", "user"),
      { ...block("hidden-user", "user"), internal: true },
      block("assistant", "assistant"),
      block("reasoning", "reasoning"),
      { ...block("hidden-tool", "tool"), orchestration: {} as never },
      block("tool", "tool"),
      block("tasks", "tasks"),
      block("plan", "plan"),
      block("user-1", "user"),
      block("after", "assistant"),
    ];

    expect(btwVisibleBlocks(blocks, "user-1").map(({ id }) => id)).toEqual([
      "before",
      "assistant",
      "tool",
      "tasks",
      "plan",
      "user-1",
    ]);
  });

  it("returns no blocks when the anchor is no longer present", () => {
    expect(btwVisibleBlocks([block("user-1", "user")], "missing")).toEqual([]);
  });
});

describe("serializeBtwBlock", () => {
  it("serializes regular blocks and attachments with normalized text", () => {
    expect(
      serializeBtwBlock({
        ...block("u", "user", "  first line\r\nsecond line  "),
        attachments: [
          {
            id: "file-1",
            name: " screenshot.png ",
            mimeType: " image/png ",
            kind: "image",
            size: 10,
          },
          {
            id: "file-2",
            name: "",
            mimeType: "text/plain",
            kind: "file",
            size: 20,
          },
        ],
      }),
    ).toBe(
      "User: first line\nsecond line\nAttachment: screenshot.png (image/png)\nAttachment: unnamed file (text/plain)",
    );
  });

  it("includes tool metadata and displays preview paths relative to cwd", () => {
    expect(
      serializeBtwBlock(
        {
          ...block("tool", "tool", "ignored tool text"),
          tool: {
            title: "Read file",
            kind: "read",
            status: "completed",
            detail: "  loaded\r\n  successfully ",
            preview: {
              kind: "read",
              path: "/repo/src/App.tsx",
              query: "  TODO  ",
              startLine: 12,
              additions: 2,
              deletions: 1,
              output: "  result\r\n  ",
              lines: [
                { number: 12, kind: "context", text: "const app = true;" },
                { kind: "add", text: "new line" },
              ],
            },
          },
        },
        "/repo",
      ),
    ).toBe(
      "Tool: Read file\nKind: read\nStatus: completed\nDetail: loaded\n  successfully\nFile: src/App.tsx\nQuery: TODO\nStart line: 12\nAdditions: 2\nDeletions: 1\nOutput:\nresult\nLines:\n12: context: const app = true;\nadd: new line",
    );
  });
});

describe("serializeBtwSnapshot", () => {
  it("throws when the completed turn is unavailable", () => {
    expect(() => serializeBtwSnapshot([], "missing")).toThrow(
      "The completed turn is no longer available.",
    );
  });

  it("keeps the newest context and bounds both block and snapshot size", () => {
    const blocks: Block[] = [
      block("old", "user", "old context"),
      ...Array.from({ length: 9 }, (_, index) =>
        block(
          `assistant-${index}`,
          "assistant",
          "m".repeat(BTW_MAX_BLOCK_CHARS + 100),
        ),
      ),
      block("user-1", "user", "latest context"),
    ];

    const snapshot = serializeBtwSnapshot(blocks, "user-1");

    expect(snapshot.length).toBeLessThanOrEqual(BTW_MAX_SNAPSHOT_CHARS);
    expect(snapshot).toContain("User: latest context");
    expect(snapshot).not.toContain("User: old context");
    expect(snapshot).toContain("…");
    expect(snapshot).not.toContain("m".repeat(BTW_MAX_BLOCK_CHARS));
  });
});

describe("buildBtwPrompt", () => {
  it("combines safety instructions, transcript context, and side messages", () => {
    const prompt = buildBtwPrompt({
      blocks: [
        block("user-1", "user", "  Explain this\r\n  change. "),
        block("assistant-1", "assistant", "It changes the parser."),
      ],
      thread: thread({
        sourceEndBlockId: "assistant-1",
        messages: [
          {
            id: "q",
            role: "user",
            text: "  What does it affect? ",
            createdAt: 1,
          },
          { id: "a", role: "assistant", text: " The parser. ", createdAt: 2 },
        ],
      }),
    });

    expect(prompt).toContain("You are answering an isolated, read-only");
    expect(prompt).toContain(
      "The main conversation snapshot below is reference context only",
    );
    expect(prompt).toContain("User: Explain this\n  change.");
    expect(prompt).toContain("Assistant: It changes the parser.");
    expect(prompt).toContain(
      "User: What does it affect?\n\nAssistant: The parser.",
    );
  });

  it("uses a placeholder when a thread has no non-empty messages", () => {
    const prompt = buildBtwPrompt({
      blocks: [block("user-1", "user", "Context")],
      thread: thread({
        messages: [{ id: "empty", role: "user", text: " \r\n ", createdAt: 1 }],
      }),
    });

    expect(prompt).toContain(
      "## By-the-way conversation\n(no side question yet)",
    );
  });
});

describe("replaceBtwThread", () => {
  it("replaces an existing thread without mutating the block", () => {
    const original = thread();
    const source = block("user-1", "user", "Question");
    const withThread = { ...source, btwThreads: [original] };
    const updated = { ...original, status: "error" as const, error: "Failed" };

    const result = replaceBtwThread(withThread, updated);

    expect(result).not.toBe(withThread);
    expect(result.btwThreads).toEqual([updated]);
    expect(withThread.btwThreads).toEqual([original]);
  });

  it("returns the same block when the thread is not present", () => {
    const source = { ...block("user-1", "user"), btwThreads: [thread()] };

    expect(replaceBtwThread(source, thread({ id: "missing" }))).toBe(source);
  });
});
