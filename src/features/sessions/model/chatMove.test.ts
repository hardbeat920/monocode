import { describe, expect, it } from "vitest";
import { moveChatToProject } from "./chatMove";
import { pendingHandoff } from "./handoff";
import { newSession, type Session } from "./session";

function chat(): Session {
  return {
    ...newSession("claude", "~"),
    providerSessionId: "thread-1",
    providerAccountId: "work",
    context: { used: 10, window: 100 },
    blocks: [
      { id: "u1", role: "user", text: "How do I parse a CSV in Rust?" },
      { id: "a1", role: "assistant", text: "Use the csv crate." },
    ],
  };
}

describe("moveChatToProject", () => {
  it("files the chat under the project and starts a fresh provider thread", () => {
    const source = chat();
    const moved = moveChatToProject(source, "/Users/me/app");

    expect(moved.id).toBe(source.id);
    expect(moved.cwd).toBe("/Users/me/app");
    expect(moved.providerSessionId).toBeUndefined();
    expect(moved.context).toBeUndefined();
    expect(moved.providerAccountId).toBe("work");
    expect(moved.harness).toBe("claude");
    expect(moved.blocks.slice(0, 2)).toEqual(source.blocks);
  });

  it("carries the conversation to the next turn as a brief", () => {
    const moved = moveChatToProject(chat(), "/Users/me/app");
    const handoff = pendingHandoff(moved);

    expect(handoff?.from).toBe("claude");
    expect(handoff?.to).toBe("claude");
    expect(handoff?.text).toContain("How do I parse a CSV in Rust?");
  });

  it("leaves the source session untouched", () => {
    const source = chat();
    moveChatToProject(source, "/Users/me/app");
    expect(source.cwd).toBe("~");
    expect(source.providerSessionId).toBe("thread-1");
    expect(source.blocks).toHaveLength(2);
  });
});
