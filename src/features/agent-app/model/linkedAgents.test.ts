// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import { newSession, type Session } from "../../sessions/model/session";
import { handleAgentApp, type AgentAppHost } from "./agentApp";
import { SessionLinks } from "./sessionLinks";
import { linkedAgentListings } from "./linkedAgents";
import { appendUser } from "../../../integrations/harness/core/apply";
import {
  persistAcceptedAssignment,
  type AssignmentReceiptHost,
} from "./assignments";

it("projects only caller-owned tasks and retains removed children until delivery cleanup", async () => {
  const parent = {
    ...newSession("pi", "/tmp/project"),
    id: "parent",
    busy: true,
  };
  const other = {
    ...newSession("pi", "/tmp/project"),
    id: "other",
    busy: true,
  };
  const child = { ...newSession("claude", "/tmp/project"), id: "child" };
  const foreign = { ...newSession("codex", "/tmp/project"), id: "foreign" };
  const open = new Map(
    [parent, other, child, foreign].map((session) => [session.id, session]),
  );
  const links = new SessionLinks({
    load: async () => [],
    save: async () => {},
    remove: async () => {},
  });
  links.bind({
    session: (id) => open.get(id),
    stored: async () => undefined,
    canAutoContinue: () => false,
    submit: () => {},
    notice: () => {},
    schedule: () => () => {},
    now: () => 1,
  });
  await links.link("parent", "child", "app-parent-1", { name: "Reviewer" });
  await links.link("other", "foreign", "app-other-1", { name: "Secret" });
  const receiptHost: AssignmentReceiptHost = {
    session: async (id) => open.get(id),
    replace: (session) => {
      open.set(session.id, session);
      return session;
    },
    persist: async (session) => session,
  };
  open.set(
    "child",
    appendUser(child, "x".repeat(241), [], {
      appRequestId: "app-parent-1",
      acceptedAssignment: links.assignmentFor(
        "parent",
        "child",
        "app-parent-1",
      ),
    }),
  );
  await persistAcceptedAssignment(open.get("child")!, receiptHost);
  const host: AgentAppHost = {
    start: async () => {},
    sessions: async () =>
      [...open.values()].map((session) => ({
        id: session.id,
        title: session.title,
        harness: session.harness,
        model: session.model,
        busy: !!session.busy,
        hasDraft: false,
      })),
    session: async (id) => open.get(id) ?? null,
    openSession: (id) => open.get(id),
    tracked: (id) => links.childrenOf(id),
    send: async () => ({ alreadySubmitted: false }),
    stop: async () => true,
    draft: async () => ({ alreadySaved: false, draft: true }),
    worktrees: async () => {
      throw new Error("not used");
    },
    createWorktree: async () => {
      throw new Error("not used");
    },
    notes: async () => [],
    note: async () => null,
    saveNote: async () => {
      throw new Error("not used");
    },
  };
  const list = (await handleAgentApp(
    parent,
    "list",
    "sessions.list",
    { linkedOnly: true },
    host,
  )) as { sessions: ReturnType<typeof linkedAgentListings> };
  expect(list.sessions.map((row) => row.id)).toEqual(["child"]);
  expect(list.sessions[0]).toMatchObject({
    name: "Reviewer",
    taskPreview: "x".repeat(240),
    taskTruncated: true,
    observed: true,
  });
  expect(JSON.stringify(list)).not.toContain("Secret");
  expect(list.sessions).toEqual(
    linkedAgentListings(
      links.childrenOf("parent"),
      await host.sessions(parent.cwd),
      open.get("parent")!.blocks,
      (id) => open.get(id),
    ),
  );
  await links.removed("child");
  open.delete("child");
  const removed = (await handleAgentApp(
    parent,
    "list-2",
    "sessions.list",
    { linkedOnly: true },
    host,
  )) as typeof list;
  expect(removed.sessions[0]).toMatchObject({
    id: "child",
    removed: true,
    state: "removed",
    name: "Reviewer",
    taskPreview: "x".repeat(240),
    observed: false,
  });
  const normal = (await handleAgentApp(
    parent,
    "list-3",
    "sessions.list",
    {},
    host,
  )) as typeof list;
  expect(normal.sessions.map((row) => row.id)).not.toContain("child");
  await expect(
    handleAgentApp(
      parent,
      "list-4",
      "sessions.list",
      { linkedOnly: "true" },
      host,
    ),
  ).rejects.toThrow("linkedOnly must be a boolean");
});

describe("current-generation activity", () => {
  it("never attributes an old tool to a new user turn or guesses unloaded activity", () => {
    const session: Session = {
      ...newSession("claude", "/tmp/project"),
      id: "child",
      busy: true,
      blocks: [
        { id: "u1", role: "user", text: "old" },
        {
          id: "tool",
          role: "tool",
          text: "OLD TOOL",
          tool: { callId: "tool", kind: "execute" },
        },
        { id: "u2", role: "user", text: "new" },
        { id: "receipt", role: "system", text: "" },
      ],
    };
    const link = {
      childId: "child",
      generation: 2,
      status: "running" as const,
      held: false,
      pending: 0,
    };
    expect(linkedAgentListings([link], [], [], () => session)[0].activity).toBe(
      "Working",
    );
    expect(
      linkedAgentListings([link], [], [], () => undefined)[0],
    ).toMatchObject({
      activity: "Unobserved",
      observed: false,
      harness: "unknown",
    });
  });
});
