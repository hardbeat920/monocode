import { describe, expect, it } from "vitest";
import {
  historyWithLiveSessions,
  filterSessionsByArchive,
  filterSessionsByQuery,
  mergeHistorySummary,
  mergeProjectHistorySummary,
  replaceProjectHistory,
} from "./sessionHistory";
import { newSession } from "./session";
import type { SessionSummary } from "./sessionStore";
import type { OrchestrationRun } from "./orchestration";

function summary(id: string, cwd: string, updatedAt = 1): SessionSummary {
  return {
    id,
    cwd,
    harness: "cursor",
    model: "gpt-5",
    runtimeMode: "supervised",
    title: `cursor · ${id}`,
    createdAt: updatedAt,
    updatedAt,
    additions: 0,
    deletions: 0,
  };
}

describe("historyWithLiveSessions", () => {
  const run: OrchestrationRun = {
    version: 1,
    leadId: "lead",
    cwd: "/tmp/project-a",
    status: "active",
    allowedHarnesses: ["codex"],
    maxWorkers: 2,
    cli: "monocode",
    continuations: 0,
    requests: {},
    tasks: ["worker-a", "worker-b"].map((id) => ({
      id,
      sessionId: id,
      title: id,
      harness: "codex",
      model: "codex:test",
      prompt: "Task",
      files: [id],
      scopes: [id],
      dependsOn: [],
      status: "running",
      accepted: false,
      result: "",
      delivered: false,
    })),
  };

  it("groups live and already-saved workers under their lead before adoption effects run", () => {
    const sessions = ["lead", "worker-a", "worker-b"].map((id) => ({
      ...newSession("codex", run.cwd),
      id,
      blocks: [{ id: "u", role: "user" as const, text: "Build" }],
      busy: id !== "lead",
    }));
    const rows = historyWithLiveSessions(
      [
        summary("lead", run.cwd),
        summary("worker-a", run.cwd),
        summary("unrelated", run.cwd),
      ],
      sessions,
      run.cwd,
      undefined,
      [run],
    );
    expect(rows.map((row) => row.id).sort()).toEqual(["lead", "unrelated"]);
    expect(rows.find((row) => row.id === "lead")?.orchestration).toMatchObject({
      live: true,
      status: "active",
      tasks: [{ sessionId: "worker-a" }, { sessionId: "worker-b" }],
    });
  });

  it("keeps ownership after restart, cache merges, and replacement runs", () => {
    const history: SessionSummary[] = [
      {
        ...summary("lead", run.cwd),
        orchestration: { status: "paused", tasks: run.tasks },
      },
      summary("worker-a", run.cwd),
      { ...summary("older-worker", run.cwd), orchestrationLeadId: "lead" },
    ];
    const merged = mergeHistorySummary(history, summary("lead", run.cwd, 2));
    const rows = historyWithLiveSessions(merged, [], run.cwd);
    expect(rows.map((row) => row.id)).toEqual(["lead"]);
    expect(rows[0].orchestration?.tasks).toHaveLength(2);
    const replacement = historyWithLiveSessions(
      merged,
      [],
      run.cwd,
      undefined,
      [{ ...run, tasks: [] }],
    );
    expect(replacement.map((row) => row.id)).toEqual(["lead"]);
    expect(replacement[0].orchestration?.tasks).toEqual([]);
  });

  it("does not inject an internal worker without a loaded run", () => {
    const worker = {
      ...newSession("codex", run.cwd),
      id: "worker",
      orchestrationLeadId: "lead",
      busy: true,
    };
    expect(historyWithLiveSessions([], [worker], run.cwd)).toEqual([]);
  });

  it("drops persisted sessions from other projects", () => {
    const history = [
      summary("a1", "/tmp/project-a"),
      summary("b1", "/tmp/project-b"),
    ];
    const rows = historyWithLiveSessions(history, [], "/tmp/project-a");
    expect(rows.map((row) => row.id)).toEqual(["a1"]);
  });

  it("does not inject live sessions from other projects", () => {
    const session = newSession("cursor", "/tmp/project-b");
    session.blocks = [{ id: "u1", role: "user", text: "hello" }];
    session.busy = true;

    const rows = historyWithLiveSessions([], [session], "/tmp/project-a");
    expect(rows).toEqual([]);
  });

  it("includes live sessions for the active project", () => {
    const session = newSession("cursor", "/tmp/project-a");
    session.blocks = [{ id: "u1", role: "user", text: "hello" }];
    session.busy = true;

    const rows = historyWithLiveSessions([], [session], "/tmp/project-a");
    expect(rows.map((row) => row.id)).toEqual([session.id]);
    expect(rows[0]?.repo).toBe("project-a");
  });

  it("marks saved drafts and clears stale draft status when they are sent", () => {
    const session = newSession("cursor", "/tmp/project-a");
    session.id = "draft-session";
    session.blocks = [
      { id: "draft", role: "user", text: "Explore this", draft: true },
    ];

    const draftRows = historyWithLiveSessions([], [session], "/tmp/project-a");
    expect(draftRows[0]?.draft).toBe(true);

    session.blocks = [{ id: "sent", role: "user", text: "Explore this" }];
    session.busy = true;
    const sentRows = historyWithLiveSessions(
      [{ ...draftRows[0], draft: true }],
      [session],
      "/tmp/project-a",
    );
    expect(sentRows[0]?.draft).toBeUndefined();
  });

  it("stamps composer git onto a live session that is not persisted yet", () => {
    const session = newSession("cursor", "/tmp/monocode");
    session.blocks = [{ id: "u1", role: "user", text: "hello" }];
    session.busy = true;

    const rows = historyWithLiveSessions([], [session], "/tmp/monocode", {
      repo: "monocode",
      branch: "main",
    });
    expect(rows[0]).toMatchObject({
      id: session.id,
      repo: "monocode",
      branch: "main",
    });
  });

  it("copies origin repo from sibling history and prefers the live branch", () => {
    const history = [
      {
        ...summary("a1", "/tmp/agent-terminal"),
        repo: "monocode",
        branch: "main",
      },
    ];
    const session = newSession("cursor", "/tmp/agent-terminal");
    session.blocks = [{ id: "u1", role: "user", text: "hello" }];
    session.busy = true;

    const rows = historyWithLiveSessions(
      history,
      [session],
      "/tmp/agent-terminal",
      { repo: "agent-terminal", branch: "fix-gutter" },
    );
    const live = rows.find((row) => row.id === session.id);
    expect(live).toMatchObject({
      repo: "monocode",
      branch: "fix-gutter",
    });
  });

  it("keeps a session's own branch instead of the project overlay", () => {
    const session = newSession("cursor", "/tmp/agent-terminal");
    session.blocks = [{ id: "u1", role: "user", text: "hello" }];
    session.busy = true;
    session.branch = "feat/picker";

    const rows = historyWithLiveSessions([], [session], "/tmp/agent-terminal", {
      repo: "monocode",
      branch: "main",
    });
    expect(rows[0]).toMatchObject({
      id: session.id,
      repo: "monocode",
      branch: "feat/picker",
    });
  });

  it("matches project paths with trailing slashes", () => {
    const history = [summary("a1", "/tmp/project-a/")];

    const rows = historyWithLiveSessions(history, [], "/tmp/project-a");
    expect(rows.map((row) => row.id)).toEqual(["a1"]);
  });
});

describe("filterSessionsByArchive", () => {
  it("hides archived sessions by default", () => {
    const rows = [
      summary("a1", "/tmp/project-a"),
      { ...summary("a2", "/tmp/project-a"), archived: true },
    ];
    expect(filterSessionsByArchive(rows, false).map((row) => row.id)).toEqual([
      "a1",
    ]);
  });

  it("shows only archived sessions when filtered", () => {
    const rows = [
      summary("a1", "/tmp/project-a"),
      { ...summary("a2", "/tmp/project-a"), archived: true },
    ];
    expect(filterSessionsByArchive(rows, true).map((row) => row.id)).toEqual([
      "a2",
    ]);
  });
});

describe("filterSessionsByQuery", () => {
  it("returns all rows when the query is empty", () => {
    const rows = [
      summary("a1", "/tmp/project-a"),
      summary("a2", "/tmp/project-a"),
    ];
    expect(filterSessionsByQuery(rows, "  ").map((row) => row.id)).toEqual([
      "a1",
      "a2",
    ]);
  });

  it("matches conversation titles", () => {
    const rows = [
      {
        ...summary("a1", "/tmp/project-a"),
        title: "cursor · Fix sidebar search",
      },
      {
        ...summary("a2", "/tmp/project-a"),
        title: "cursor · Archive sessions",
      },
    ];
    expect(filterSessionsByQuery(rows, "sidebar").map((row) => row.id)).toEqual(
      ["a1"],
    );
  });

  it("matches model and branch labels", () => {
    const rows = [
      { ...summary("a1", "/tmp/project-a"), model: "gpt-5", branch: "main" },
      {
        ...summary("a2", "/tmp/project-a"),
        model: "opus",
        branch: "fix-gutter",
      },
    ];
    expect(filterSessionsByQuery(rows, "opus").map((row) => row.id)).toEqual([
      "a2",
    ]);
    expect(filterSessionsByQuery(rows, "gutter").map((row) => row.id)).toEqual([
      "a2",
    ]);
  });
});

describe("replaceProjectHistory", () => {
  it("swaps one project's rows and keeps the others cached", () => {
    const current = [
      summary("a1", "/tmp/project-a", 3),
      summary("b1", "/tmp/project-b", 2),
    ];
    const next = replaceProjectHistory(current, "/tmp/project-a", [
      summary("a2", "/tmp/project-a", 5),
    ]);
    expect(next.map((row) => row.id).sort()).toEqual(["a2", "b1"]);
  });

  it("clears a project that came back empty without touching the rest", () => {
    const current = [
      summary("a1", "/tmp/project-a", 3),
      summary("b1", "/tmp/project-b", 2),
    ];
    const next = replaceProjectHistory(current, "/tmp/project-a", []);
    expect(next.map((row) => row.id)).toEqual(["b1"]);
  });
});

describe("mergeProjectHistorySummary", () => {
  it("merges into its own project and leaves other projects cached", () => {
    const current = [
      summary("a1", "/tmp/project-a", 1),
      summary("b1", "/tmp/project-b", 2),
    ];
    const next = mergeProjectHistorySummary(
      current,
      summary("a1", "/tmp/project-a", 9),
    );
    expect(next.map((row) => row.id).sort()).toEqual(["a1", "b1"]);
    expect(next.find((row) => row.id === "a1")?.updatedAt).toBe(9);
  });

  it("does not leave a duplicate behind when a session changes project", () => {
    const current = [
      summary("a1", "/tmp/project-a", 1),
      summary("b1", "/tmp/project-b", 2),
    ];
    const next = mergeProjectHistorySummary(
      current,
      summary("a1", "/tmp/project-b", 9),
    );
    expect(next.map((row) => row.id).sort()).toEqual(["a1", "b1"]);
    expect(next.find((row) => row.id === "a1")?.cwd).toBe("/tmp/project-b");
  });
});

describe("pinned sessions", () => {
  it("keeps pinned sessions above newer unpinned ones", () => {
    const current = [
      summary("new", "/tmp/project-a", 20),
      { ...summary("pin", "/tmp/project-a", 5), pinned: true },
    ];
    const next = mergeHistorySummary(
      current,
      summary("new", "/tmp/project-a", 30),
    );
    expect(next.map((row) => row.id)).toEqual(["pin", "new"]);
  });

  it("preserves pin when an incoming summary omits it", () => {
    const current = [{ ...summary("pin", "/tmp/project-a", 5), pinned: true }];
    const next = mergeHistorySummary(
      current,
      summary("pin", "/tmp/project-a", 9),
    );
    expect(next[0]).toMatchObject({ id: "pin", pinned: true, updatedAt: 9 });
  });

  it("returns an unpinned session to recency order", () => {
    const current = [
      { ...summary("pin", "/tmp/project-a", 5), pinned: true },
      summary("new", "/tmp/project-a", 20),
    ];
    const next = mergeHistorySummary(current, {
      ...summary("pin", "/tmp/project-a", 5),
      pinned: false,
    });
    expect(next.map((row) => row.id)).toEqual(["new", "pin"]);
    expect(next.find((row) => row.id === "pin")?.pinned).toBe(false);
  });

  it("sorts pinned history to the top even without a live inject", () => {
    const history = [
      summary("new", "/tmp/project-a", 20),
      { ...summary("pin", "/tmp/project-a", 5), pinned: true },
    ];
    const rows = historyWithLiveSessions(history, [], "/tmp/project-a");
    expect(rows.map((row) => row.id)).toEqual(["pin", "new"]);
  });
});
