import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionRecord } from "./sessionStore";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

const { getSession, readSession, repairSession, applySessionRepair } =
  await import("./sessionStore");

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

  it("restores sidebar visibility when a hidden session is opened by id", async () => {
    const record = codexRecord();
    record.sidebarHidden = true;
    invoke.mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "session_get" ? record : null),
    );
    const session = await getSession(record.id);
    expect(session?.sidebarHidden).toBe(true);
    expect(session?.blocks[0].text).toBe("list the agents");
    const repaired = invoke.mock.calls.find(
      ([cmd]) => cmd === "session_upsert",
    );
    expect(repaired?.[1].session.sidebarHidden).toBe(true);
  });

  it("restores the turn's launched session links", async () => {
    const record = codexRecord();
    const launch = {
      sessionId: "app-review",
      cwd: "/repo",
      title: "Review",
      harness: "codex" as const,
      model: "gpt-6",
    };
    record.blocks[0].monoSpawnedSessions = [launch];
    invoke.mockImplementation((cmd: string) =>
      Promise.resolve(cmd === "session_get" ? record : null),
    );
    expect((await getSession("s1"))?.blocks[0].monoSpawnedSessions).toEqual([
      launch,
    ]);
  });

  it("still returns the repaired session when the write fails", async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === "session_get") return Promise.resolve(codexRecord());
      if (cmd === "session_upsert")
        return Promise.reject(new Error("db locked"));
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
    expect(invoke.mock.calls.some(([cmd]) => cmd === "session_upsert")).toBe(
      true,
    );
  });
});

describe("progressive session restoration", () => {
  beforeEach(() => invoke.mockReset());

  it("returns saved text without reading provider logs or writing repairs", async () => {
    const record = { ...codexRecord(), harness: "claude" } as SessionRecord;
    invoke.mockResolvedValue(record);
    const session = await readSession("s1");
    expect(session?.blocks[1].text).toBe("Shell");
    expect(invoke.mock.calls.map(([cmd]) => cmd)).toEqual(["session_get"]);
  });

  it("does not mutate or persist a conversation while repairing it for a visible caller", async () => {
    invoke.mockResolvedValueOnce(codexRecord());
    const source = (await readSession("s1"))!;
    const repaired = await repairSession(source, { persist: false });
    expect(source.blocks[1].text).toBe("Shell");
    expect(repaired.blocks[1].text).toBe("Find files");
    expect(invoke.mock.calls.map(([cmd]) => cmd)).toEqual(["session_get"]);
    const renamed = { ...source, title: "Renamed while loading" };
    expect(applySessionRepair(renamed, source, repaired)).toMatchObject({
      title: "Renamed while loading",
      blocks: repaired.blocks,
    });
  });

  it("discards late repairs after streaming, editing, or a provider change", async () => {
    invoke.mockResolvedValueOnce(codexRecord());
    const source = (await readSession("s1"))!;
    const repaired = await repairSession(source, { persist: false });
    for (const current of [
      {
        ...source,
        blocks: [
          ...source.blocks,
          { id: "new", role: "assistant" as const, text: "Live output" },
        ],
      },
      { ...source, blocks: source.blocks.slice(0, 1) },
      { ...source, providerSessionId: "new-provider-session" },
      { ...source, harness: "claude" as const },
    ])
      expect(applySessionRepair(current, source, repaired)).toBe(current);
  });
});
