import { beforeEach, describe, expect, it, vi } from "vitest";
import { newSession } from "../session";
import { applyHarnessEvent } from "./apply";
import type { StoredCursorSubagentRun } from "./cursorStore";
import { cursorAgentLabel, recoverCursorSubagents } from "./cursorSubagents";

const read = vi.hoisted(() => vi.fn());
vi.mock("./cursorStore", () => ({ readStoredCursorSubagentRuns: read }));

const run: StoredCursorSubagentRun = {
  agentId: "child",
  toolCallId: "spawn",
  revision: "7",
  agentType: "generalPurpose",
  prompt:
    "Perform a read-only, defect-first code review of OpenCode subagent work in /repo.\nInspect the diff.",
  steps: [
    { id: "child:blob:0", kind: "message", text: "Inspecting the diff." },
    {
      id: "child:tool:shell",
      kind: "tool",
      text: "",
      toolName: "Shell",
      args: { command: "git diff" },
      status: "completed",
      output: "diff --git a/a.ts b/a.ts",
    },
    { id: "child:last:0", kind: "message", text: "One finding." },
  ],
};

beforeEach(() => {
  read.mockReset().mockResolvedValue([run]);
});

function savedSession() {
  let session = newSession("cursor", "/repo");
  session.providerSessionId = "parent";
  session = applyHarnessEvent(session, {
    type: "tool.updated",
    callId: "spawn",
    kind: "agent",
    title: "Task: Subagent task",
    status: "completed",
  });
  return session;
}

describe("Cursor stored subagents", () => {
  it.each([
    "Task: Subagent task",
    ": Subagent task",
    "Subagent",
    "Task",
    "Agent",
  ])("rejects the placeholder %s as a name", (title) => {
    expect(cursorAgentLabel(title)).toBeUndefined();
  });

  it("restores a saved row's name, steps, previews and completed status", async () => {
    const recovered = await recoverCursorSubagents(savedSession());
    const row = recovered.blocks[0];
    expect(row.text).toBe("Review OpenCode subagent work");
    expect(row.tool?.status).toBe("completed");
    expect(row.agentRun?.steps).toHaveLength(3);
    expect(row.agentRun?.steps[1]).toMatchObject({
      text: "git diff",
      toolKind: "execute",
      status: "completed",
    });
    expect(recovered.busy).toBeFalsy();
    expect((await recoverCursorSubagents(recovered)).blocks).toEqual(
      recovered.blocks,
    );
  });

  it("preserves a useful task description and failed parent status", async () => {
    const session = applyHarnessEvent(savedSession(), {
      type: "tool.updated",
      callId: "spawn",
      title: "Task: Review OpenCode",
      status: "failed",
    });
    const recovered = await recoverCursorSubagents(session);
    expect(recovered.blocks[0].agentRun?.name).toBe("Review OpenCode");
    expect(recovered.blocks[0].tool?.status).toBe("failed");
  });

  it("ignores unrelated calls and tolerates an unavailable store", async () => {
    const session = savedSession();
    read.mockResolvedValue([{ ...run, toolCallId: "unrelated" }]);
    expect(await recoverCursorSubagents(session)).toBe(session);
    read.mockRejectedValue(new Error("store unavailable"));
    expect(await recoverCursorSubagents(session)).toBe(session);
    session.harness = "codex";
    read.mockClear();
    expect(await recoverCursorSubagents(session)).toBe(session);
    expect(read).not.toHaveBeenCalled();
  });
});
