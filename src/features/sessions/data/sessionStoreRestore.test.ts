import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionRecord } from "./sessionStore";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

const { getSession } = await import("./sessionStore");

/** A saved Codex session holding the one row this PR repairs. */
function codexRecord(): SessionRecord {
  return {
    id: "s1",
    harness: "codex",
    model: "gpt-5",
    cwd: "/repo",
    providerSessionId: "01a0e6f4-13e3-7692-9250-4befceed807b",
    blocks: [
      { id: "b0", role: "user", text: "list the agents" },
      {
        id: "b1",
        role: "tool",
        text: "Shell",
        tool: {
          callId: "exec-1",
          title: "Shell",
          kind: "execute",
          status: "completed",
          preview: {
            kind: "shell",
            title: "rg --files -g AGENTS.md -g '!node_modules'",
          },
        },
      },
    ],
  } as unknown as SessionRecord;
}

describe("restoring a session whose repair cannot be persisted", () => {
  beforeEach(() => invoke.mockReset());

  it("still returns the repaired session when the write fails", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "session_get") return Promise.resolve(codexRecord());
      if (cmd === "session_upsert") return Promise.reject(new Error("db locked"));
      return Promise.resolve(null);
    });

    // A failed persistence must not cost the reader the session: the repair
    // stays in memory and the next load retries the write.
    const session = await getSession("s1");
    expect(session).not.toBeNull();
    expect(session?.blocks[1].text).toBe("Find files");
  });

  it("persists the repair when the write succeeds", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "session_get") return Promise.resolve(codexRecord());
      return Promise.resolve(null);
    });

    const session = await getSession("s1");
    expect(session?.blocks[1].text).toBe("Find files");
    expect(
      invoke.mock.calls.some(([cmd]) => cmd === "session_upsert"),
    ).toBe(true);
  });
});
