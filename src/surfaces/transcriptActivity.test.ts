import { describe, expect, it } from "vitest";
import type { Block } from "../lib/session";
import {
  activityPhaseTitle,
  activityStillRunning,
  buildActivityPhases,
  editVerb,
  groupTurnItems,
  groupTurns,
  hasRunningSubagent,
  initialThinkingIndex,
  lastActivityIndex,
  nestedScrollAbsorbsWheel,
  proseSummary,
  toolCallLabel,
  turnCopyText,
} from "./transcriptActivity";

function shell(
  id: string,
  status = "completed",
  approval?: Block["approval"],
): Block {
  return {
    id,
    role: "tool",
    text: "bash ls",
    tool: { kind: "shell", title: "bash ls", status },
    ...(approval ? { approval } : {}),
  };
}

function edit(id: string, path = "src/App.tsx"): Block {
  const fileName = path.split("/").pop() ?? path;
  return {
    id,
    role: "tool",
    text: `Edited ${path}`,
    tool: {
      kind: "edit",
      title: `Edited ${path}`,
      status: "completed",
      preview: { kind: "write", path, fileName },
    },
  };
}

function read(id: string, path = "src/App.tsx"): Block {
  const fileName = path.split("/").pop() ?? path;
  return {
    id,
    role: "tool",
    text: `Read ${path}`,
    tool: {
      kind: "read",
      title: `Read ${path}`,
      status: "completed",
      preview: { kind: "read", path, fileName },
    },
  };
}

function search(id: string, query = "color tokens"): Block {
  return {
    id,
    role: "tool",
    text: `Find ${query}`,
    tool: {
      kind: "search",
      title: `Find ${query}`,
      status: "completed",
      preview: { kind: "search", query },
    },
  };
}

function note(id: string, text: string): Block {
  return { id, role: "assistant", text };
}

function thought(id: string, text = "Weighing the options."): Block {
  return { id, role: "reasoning", text };
}

describe("groupTurnItems", () => {
  it("keeps consecutive shell calls in one activity stack", () => {
    const items = groupTurnItems([
      shell("a"),
      shell("b"),
      shell("c", "pending"),
    ]);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({
      type: "activity",
      blocks: [{ id: "a" }, { id: "b" }, { id: "c" }],
    });
  });

  it("does not split a stack when tools are waiting for approval", () => {
    const items = groupTurnItems([
      shell("a"),
      shell("b", "pending", { requestId: 1 }),
      shell("c", "pending", { requestId: 2 }),
    ]);
    expect(items).toHaveLength(1);
    expect(items[0]?.type).toBe("activity");
    if (items[0]?.type !== "activity") return;
    expect(items[0].blocks.map((block) => block.id)).toEqual(["a", "b", "c"]);
  });

  it("does not split a stack across empty assistant placeholders", () => {
    const items = groupTurnItems([
      shell("a"),
      { id: "ghost", role: "assistant", text: "", streaming: true },
      shell("b", "pending", { requestId: 1 }),
      shell("c", "pending", { requestId: 2 }),
    ]);
    expect(items).toHaveLength(1);
    expect(items[0]?.type).toBe("activity");
    if (items[0]?.type !== "activity") return;
    expect(items[0].blocks.map((block) => block.id)).toEqual(["a", "b", "c"]);
  });

  it("hides provider todo calls in favor of the shared tasks block", () => {
    const tasks: Block = {
      id: "tasks",
      role: "tasks",
      text: "[~] Implement",
      taskList: {
        items: [{ text: "Implement", status: "in_progress" }],
      },
    };
    expect(
      groupTurnItems([
        {
          id: "todo-tool",
          role: "tool",
          text: "Update TODOs",
          tool: {
            kind: "tasks",
            title: "Update TODOs",
            status: "completed",
          },
        },
        tasks,
      ]),
    ).toEqual([{ type: "block", block: tasks }]);
    expect(
      activityStillRunning([
        {
          id: "todo-tool",
          role: "tool",
          text: "Update TODOs",
          streaming: true,
          tool: { kind: "tasks", status: "pending" },
        },
      ]),
    ).toBe(false);
  });

  it("folds edits into the activity stack", () => {
    const items = groupTurnItems([shell("a"), edit("b"), shell("c")]);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({
      type: "activity",
      blocks: [{ id: "a" }, { id: "b" }, { id: "c" }],
    });
  });

  it("keeps an edit awaiting approval out of the stack", () => {
    const pending = edit("b");
    pending.approval = { requestId: 1 };
    const items = groupTurnItems([shell("a"), pending]);
    expect(items.map((item) => item.type)).toEqual(["activity", "block"]);
  });

  it("keeps all assistant prose outside reasoning and tool activity", () => {
    const items = groupTurnItems([
      { id: "u", role: "user", text: "cut the release" },
      { id: "a1", role: "assistant", text: "Running the checks first." },
      shell("a"),
      { id: "a2", role: "assistant", text: "Checks pass. Bumping:" },
      edit("b"),
      { id: "a3", role: "assistant", text: "Released." },
    ]);
    expect(items.map((item) => item.type)).toEqual([
      "block",
      "block",
      "activity",
      "block",
      "activity",
      "block",
    ]);
    expect(items[1]).toMatchObject({ type: "block", block: { id: "a1" } });
    expect(items[2]).toMatchObject({ type: "activity", blocks: [{ id: "a" }] });
    expect(items[3]).toMatchObject({ type: "block", block: { id: "a2" } });
    expect(items[4]).toMatchObject({ type: "activity", blocks: [{ id: "b" }] });
    expect(items[5]).toMatchObject({ type: "block", block: { id: "a3" } });
  });

  it("keeps the trailing run of prose blocks out of the stack", () => {
    const items = groupTurnItems([
      shell("a"),
      { id: "a1", role: "assistant", text: "Half" },
      { id: "a2", role: "assistant", text: "Done", streaming: true },
    ]);
    expect(items.map((item) => item.type)).toEqual([
      "activity",
      "block",
      "block",
    ]);
  });

  it("keeps prose standalone when the turn ends on a tool call", () => {
    const items = groupTurnItems([
      { id: "a1", role: "assistant", text: "Looking now." },
      shell("a"),
    ]);
    expect(items).toMatchObject([
      { type: "block", block: { id: "a1" } },
      { type: "activity", blocks: [{ id: "a" }] },
    ]);
  });

  it("keeps thinking in the stack so a long think is visible", () => {
    const items = groupTurnItems([
      { id: "r", role: "reasoning", text: "**Checking the config**" },
      shell("a"),
      { id: "done", role: "assistant", text: "Done." },
    ]);
    expect(items.map((item) => item.type)).toEqual(["activity", "block"]);
    if (items[0]?.type !== "activity") return;
    expect(items[0].blocks.map((block) => block.id)).toEqual(["r", "a"]);
  });

  it("uses assistant prose as boundaries between reasoning and tool groups", () => {
    const items = groupTurnItems([
      note("a1", "I’ll inspect the config first."),
      thought("r1"),
      read("t1"),
      note("a2", "The config is healthy. I’m checking the build next."),
      thought("r2"),
      shell("t2"),
    ]);

    expect(items).toMatchObject([
      { type: "block", block: { id: "a1" } },
      { type: "activity", blocks: [{ id: "r1" }, { id: "t1" }] },
      { type: "block", block: { id: "a2" } },
      { type: "activity", blocks: [{ id: "r2" }, { id: "t2" }] },
    ]);
  });

  it("replaces leading reasoning with the first assistant prose", () => {
    const items = groupTurnItems([
      { id: "u", role: "user", text: "Investigate it" },
      thought("r1", "I should inspect the current changes."),
      thought("r2", "I need a structured checklist."),
      note("a1", "I’ll investigate the current changes."),
      read("t1"),
    ]);

    expect(items).toMatchObject([
      { type: "block", block: { id: "u" } },
      { type: "block", block: { id: "a1" } },
      { type: "activity", blocks: [{ id: "t1" }] },
    ]);
  });

  it("identifies leading reasoning while the first prose is pending", () => {
    const thinking = groupTurnItems([
      { id: "u", role: "user", text: "Investigate it" },
      thought("r1"),
      thought("r2"),
    ]);
    expect(initialThinkingIndex(thinking)).toBe(1);

    const toolActivity = groupTurnItems([
      { id: "u", role: "user", text: "Investigate it" },
      thought("r1"),
      read("t1"),
    ]);
    expect(initialThinkingIndex(toolActivity)).toBe(-1);
  });
});

describe("turnCopyText", () => {
  it("joins assistant and plan markdown from the turn", () => {
    expect(
      turnCopyText([
        { id: "u", role: "user", text: "fix it" },
        { id: "a1", role: "assistant", text: "I'll inspect the file." },
        shell("t"),
        { id: "r", role: "reasoning", text: "thinking" },
        {
          id: "tasks",
          role: "tasks",
          text: "[x] inspect\n[~] implement",
          taskList: {
            items: [
              { text: "inspect", status: "completed" },
              { text: "implement", status: "in_progress" },
            ],
          },
        },
        { id: "p", role: "plan", text: "## Plan\n\n- edit App.tsx" },
        { id: "a2", role: "assistant", text: "Done.\n\n```ts\nfixed\n```" },
        { id: "s", role: "system", text: "session error" },
      ]),
    ).toBe(
      "I'll inspect the file.\n\n[x] inspect\n[~] implement\n\n## Plan\n\n- edit App.tsx\n\nDone.\n\n```ts\nfixed\n```",
    );
  });

  it("returns empty when the turn has no readable output", () => {
    expect(
      turnCopyText([
        { id: "u", role: "user", text: "go" },
        shell("t"),
        { id: "a", role: "assistant", text: "  " },
      ]),
    ).toBe("");
  });
});

describe("groupTurns", () => {
  it("keeps a handoff divider on its own row between providers", () => {
    const turns = groupTurns([
      { id: "u1", role: "user", text: "go" },
      { id: "a1", role: "assistant", text: "working" },
      {
        id: "h1",
        role: "handoff",
        text: "Goal: go",
        handoff: { from: "cursor", to: "claude", status: "ready" },
      },
      { id: "u2", role: "user", text: "continue" },
    ]);
    expect(turns.map((turn) => turn.map((block) => block.id))).toEqual([
      ["u1", "a1"],
      ["h1"],
      ["u2"],
    ]);
  });
});

describe("buildActivityPhases", () => {
  it("groups a run of calls under the line that introduced it", () => {
    const phases = buildActivityPhases([
      note("n1", "Now I need to find the theme provider."),
      search("s1"),
      read("r1", "src/globals.css"),
      read("r2", "src/layout.tsx"),
      note("n2", "Updating the dark mode tokens."),
      edit("e1", "src/globals.css"),
      edit("e2", "src/theme.ts"),
    ]);
    expect(phases).toHaveLength(2);
    expect(phases[0]).toMatchObject({
      kind: "research",
      headline: { id: "n1" },
    });
    expect(phases[0].steps.map((block) => block.id)).toEqual([
      "s1",
      "r1",
      "r2",
    ]);
    expect(phases[1]).toMatchObject({ kind: "edit", headline: { id: "n2" } });
    expect(phases[1].steps.map((block) => block.id)).toEqual(["e1", "e2"]);
  });

  it("starts a group when the work changes shape, narrated or not", () => {
    const phases = buildActivityPhases([
      read("r1", "a.ts"),
      read("r2", "b.ts"),
      edit("e1", "a.ts"),
      edit("e2", "b.ts"),
    ]);
    expect(phases.map((phase) => phase.kind)).toEqual(["research", "edit"]);
  });

  it("folds a lone uninvited call into the group before it", () => {
    const phases = buildActivityPhases([
      edit("e1", "a.ts"),
      read("r1", "a.ts"),
      edit("e2", "b.ts"),
    ]);
    expect(phases).toHaveLength(1);
    expect(phases[0].kind).toBe("edit");
    expect(phases[0].steps.map((block) => block.id)).toEqual([
      "e1",
      "r1",
      "e2",
    ]);
  });

  it("keeps a group the agent announced out of that fold", () => {
    const phases = buildActivityPhases([
      read("r1"),
      note("n1", "Now the edit."),
      edit("e1"),
    ]);
    expect(phases).toHaveLength(2);
    expect(phases[1]).toMatchObject({ kind: "edit", headline: { id: "n1" } });
  });

  it("keeps a second paragraph as a step rather than a group of its own", () => {
    const phases = buildActivityPhases([
      note("n1", "First."),
      note("n2", "Second."),
      read("r1"),
    ]);
    expect(phases).toHaveLength(1);
    expect(phases[0]).toMatchObject({
      kind: "research",
      headline: { id: "n1" },
    });
    expect(phases[0].steps.map((block) => block.id)).toEqual(["n2", "r1"]);
  });

  it("gives a turn that only thought a group to sit in", () => {
    const phases = buildActivityPhases([thought("r")]);
    expect(phases).toHaveLength(1);
    expect(phases[0]).toMatchObject({ kind: "think", headline: undefined });
    expect(phases[0].steps.map((block) => block.id)).toEqual(["r"]);
  });

  it("keeps reasoning inside the group instead of titling it", () => {
    const phases = buildActivityPhases([
      thought("t1"),
      search("s1"),
      thought("t2"),
      search("s2"),
    ]);
    expect(phases).toHaveLength(1);
    expect(phases[0]).toMatchObject({ kind: "research", headline: undefined });
    expect(phases[0].steps.map((block) => block.id)).toEqual([
      "t1",
      "s1",
      "t2",
      "s2",
    ]);
  });

  it("moves a thought at the end of a group into the group it introduced", () => {
    const phases = buildActivityPhases([
      read("r1", "a.ts"),
      read("r2", "b.ts"),
      thought("t1", "Now to apply the change."),
      edit("e1", "a.ts"),
      edit("e2", "b.ts"),
    ]);
    expect(phases.map((phase) => phase.kind)).toEqual(["research", "edit"]);
    expect(phases[0].steps.map((block) => block.id)).toEqual(["r1", "r2"]);
    expect(phases[1].steps.map((block) => block.id)).toEqual([
      "t1",
      "e1",
      "e2",
    ]);
  });

  it("lets the agent's own words title a group that opened on a thought", () => {
    const phases = buildActivityPhases([
      thought("t1"),
      note("n1", "Looking for the theme provider."),
      search("s1"),
    ]);
    expect(phases).toHaveLength(1);
    expect(phases[0]).toMatchObject({
      kind: "research",
      headline: { id: "n1" },
      id: "t1",
    });
    expect(phases[0].steps.map((block) => block.id)).toEqual(["t1", "s1"]);
  });
});

describe("activityPhaseTitle", () => {
  const title = (blocks: Block[], live = false) =>
    activityPhaseTitle(buildActivityPhases(blocks)[0], live);

  it("uses the agent's own line when it wrote one", () => {
    expect(
      title([
        note("n1", "**Found it** — the tokens live in `globals.css`."),
        read("r1"),
      ]),
    ).toBe("Found it — the tokens live in globals.css.");
  });

  it("says what the calls add up to, in the tense of the moment", () => {
    expect(title([read("r1", "a.ts"), read("r2", "b.ts")], true)).toBe(
      "Reading 2 files",
    );
    expect(title([read("r1", "a.ts"), read("r2", "b.ts")])).toBe(
      "Read 2 files",
    );
    expect(title([read("r1", "src/index.css")])).toBe("Read index.css");
    expect(title([search("s1"), search("s2")])).toBe("Searched the project");
    expect(title([search("s1"), read("r1")])).toBe("Explored the project");
    expect(title([edit("e1", "a.ts"), edit("e2", "b.ts")])).toBe(
      "Edited 2 files",
    );
    expect(title([shell("a"), shell("b")])).toBe("Ran 2 commands");
    expect(title([shell("a")], true)).toBe("Running a command");
    expect(
      title(
        [
          {
            id: "ag",
            role: "tool",
            text: "Explore the auth module",
            tool: {
              kind: "agent",
              title: "Explore the auth module",
              status: "in_progress",
            },
          },
        ],
        true,
      ),
    ).toBe("Running a subagent");
  });
});

describe("running subagents", () => {
  const agent = (
    id: string,
    status = "in_progress",
  ): Block => ({
    id,
    role: "tool",
    text: "Explore the auth module",
    tool: { kind: "agent", title: "Explore the auth module", status },
  });

  it("keeps a pending Agent tool visible as its own phase", () => {
    const phases = buildActivityPhases([read("r1"), agent("ag")]);
    expect(phases.map((phase) => phase.kind)).toEqual(["research", "agent"]);
  });

  it("flags a live subagent until the tool completes", () => {
    expect(hasRunningSubagent([agent("ag")])).toBe(true);
    expect(activityStillRunning([agent("ag")])).toBe(true);
    expect(hasRunningSubagent([agent("ag", "completed")])).toBe(false);
    expect(activityStillRunning([agent("ag", "completed")])).toBe(false);
  });
});

describe("lastActivityIndex", () => {
  it("points at the fold that sits under the final answer", () => {
    const items = groupTurnItems([
      { id: "u", role: "user", text: "go" },
      shell("a"),
      { id: "p", role: "plan", text: "## Plan" },
      shell("b"),
      { id: "done", role: "assistant", text: "Done." },
    ]);
    expect(lastActivityIndex(items)).toBe(3);
  });

  it("returns -1 for a turn that ran no tools", () => {
    expect(
      lastActivityIndex(
        groupTurnItems([{ id: "a", role: "assistant", text: "Hi." }]),
      ),
    ).toBe(-1);
  });
});

describe("toolCallLabel", () => {
  it("shows the shell command, not the tool name", () => {
    expect(
      toolCallLabel({
        id: "a",
        role: "tool",
        text: "git status -s",
        tool: { kind: "execute", title: "git status -s" },
      }),
    ).toBe("git status -s");
    expect(
      toolCallLabel({
        id: "b",
        role: "tool",
        text: "Skill /code-review",
        tool: { kind: "skill", title: "Skill /code-review" },
      }),
    ).toBe("Skill /code-review");
  });

  it("renders file-reading bash as a Read/Find label", () => {
    expect(
      toolCallLabel({
        id: "c",
        role: "tool",
        text: "cat src/lib/appearance.ts",
        tool: { kind: "execute", title: "cat src/lib/appearance.ts" },
      }),
    ).toBe("Read src/lib/appearance.ts");
    expect(
      toolCallLabel(
        {
          id: "d",
          role: "tool",
          text: "cat /Users/me/proj/src/lib/appearance.ts",
          tool: {
            kind: "execute",
            title: "cat /Users/me/proj/src/lib/appearance.ts",
          },
        },
        "/Users/me/proj",
      ),
    ).toBe("Read src/lib/appearance.ts");
  });
});

describe("editVerb", () => {
  it("canonicalises past-tense harness phrasing", () => {
    expect(editVerb("Edited src/App.tsx")).toBe("Edit");
    expect(editVerb("Deleted src/old.ts")).toBe("Delete");
    expect(editVerb("Renamed src/a.ts")).toBe("Move");
    expect(editVerb("Created src/new.ts")).toBe("Create");
    expect(editVerb("Wrote src/new.ts")).toBe("Write");
  });

  it("falls back to Edit for unknown phrasing", () => {
    expect(editVerb("Patching src/App.tsx")).toBe("Edit");
    expect(editVerb("")).toBe("Edit");
  });
});

describe("nestedScrollAbsorbsWheel", () => {
  const overflowing = {
    scrollTop: 40,
    scrollHeight: 200,
    clientHeight: 80,
  };

  it("lets the parent handle the wheel when the list does not overflow", () => {
    expect(
      nestedScrollAbsorbsWheel(
        { scrollTop: 0, scrollHeight: 80, clientHeight: 80 },
        -20,
      ),
    ).toBe(false);
  });

  it("consumes scrolling that still has room inside the list", () => {
    expect(nestedScrollAbsorbsWheel(overflowing, -20)).toBe(true);
    expect(nestedScrollAbsorbsWheel(overflowing, 20)).toBe(true);
  });

  it("releases the wheel at the edges so the transcript can take over", () => {
    expect(
      nestedScrollAbsorbsWheel({ ...overflowing, scrollTop: 0 }, -20),
    ).toBe(false);
    expect(
      nestedScrollAbsorbsWheel({ ...overflowing, scrollTop: 120 }, 20),
    ).toBe(false);
  });
});

describe("proseSummary", () => {
  it("reduces a paragraph to one plain line", () => {
    expect(
      proseSummary(
        "**Full checks pass** — `cargo fmt` and 134 tests.\n\nBumping:",
      ),
    ).toBe("Full checks pass — cargo fmt and 134 tests.");
  });

  it("skips fenced code and list markers", () => {
    expect(
      proseSummary("```ts\nconst a = 1;\n```\n\n- Ran [checks](x.md)"),
    ).toBe("Ran checks");
  });
});
