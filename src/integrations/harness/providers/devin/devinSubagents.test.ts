import { describe, expect, it } from "vitest";
import { AcpSubagents } from "../../core/acpSubagents";
import { applyHarnessEvent } from "../../core/apply";
import { newSession, type Session } from "../../../../features/sessions/model/session";
import { devinAgentMessageText, devinEventsFromUpdate } from "./devinProtocol";
import { DevinSubagents } from "./devinSubagents";

function harness() {
  const devin = new DevinSubagents();
  const router = new AcpSubagents();
  let session: Session = newSession("devin", "/repo");
  return {
    push(update: Record<string, unknown>) {
      const params = { sessionId: "s", update };
      const routed = devin.translate(params, devinEventsFromUpdate(params));
      for (const event of router.route(routed.params, routed.events))
        session = applyHarnessEvent(session, event);
    },
    get tools() {
      return session.blocks.filter((block) => block.role === "tool");
    },
    get prose() {
      return session.blocks
        .filter((block) => block.role === "assistant")
        .map((block) => block.text)
        .join("");
    },
  };
}

const run = (callId: string, title: string, task: string, meta = {}) => ({
  sessionUpdate: "tool_call",
  toolCallId: callId,
  title: `Ran explore subagent ${title}`,
  rawInput: { title, task, profile: "subagent_explore", is_background: true },
  _meta: { "cognition.ai/inferenceToolName": "run_subagent", ...meta },
});

const runStatus = (callId: string, status: string, meta = {}) => ({
  sessionUpdate: "tool_call_update",
  toolCallId: callId,
  status,
  ...(status === "completed"
    ? {
        content: [
          {
            type: "content",
            content: { type: "text", text: "Background subagent started." },
          },
        ],
      }
    : {}),
  _meta: { "cognition.ai/inferenceToolName": "run_subagent", ...meta },
});

const started = (agentId: string, title: string, task: string, depth = 1) => ({
  sessionUpdate: "tool_call_update",
  toolCallId: agentId,
  status: "in_progress",
  _meta: {
    "cognition.ai/subagent_started": {
      agentId,
      title,
      task,
      profile: "Explore",
      depth,
      isBackground: true,
      model: "Subagent Default",
    },
  },
});

const completed = (agentId: string, success: boolean, summary: string) => ({
  sessionUpdate: "tool_call_update",
  toolCallId: agentId,
  status: success ? "completed" : "failed",
  _meta: {
    "cognition.ai/subagent_completed": { agentId, success, summary, depth: 1 },
  },
});

const child = (agentId: string, update: Record<string, unknown>) => ({
  ...update,
  _meta: {
    ...(update._meta as Record<string, unknown> | undefined),
    "cognition.ai/subagent_context": { parentAgentId: agentId },
  },
});

describe("Devin subagents", () => {
  // Condensed from a real `devin acp` 3000.11.3 stream with subagentSupport.
  it("turns run_subagent into agent rows that carry each child's own work", () => {
    const h = harness();
    h.push(run("call_list", "List top-level files", "List files."));
    h.push(run("call_pkg", "Read package name", "Read package.json."));
    h.push(runStatus("call_list", "in_progress"));
    h.push(started("00efcd6b", "List top-level files", "List files."));
    h.push(runStatus("call_list", "completed"));
    h.push(runStatus("call_pkg", "in_progress"));
    h.push(started("8ac5cd37", "Read package name", "Read package.json."));
    h.push(runStatus("call_pkg", "completed"));
    h.push({
      sessionUpdate: "tool_call",
      toolCallId: "call_wait",
      title: "Checked on subagent Read package name",
      rawInput: { agent_id: "8ac5cd37", block: true, timeout: 600 },
      _meta: { "cognition.ai/inferenceToolName": "read_subagent" },
    });
    h.push(
      child("8ac5cd37", {
        sessionUpdate: "agent_thought_chunk",
        content: { type: "text", text: "Looking for package.json" },
      }),
    );
    h.push(
      child("8ac5cd37", {
        sessionUpdate: "tool_call",
        toolCallId: "read:0#83d6",
        title: "Read file",
        kind: "read",
        locations: [{ path: "/repo/package.json" }],
        rawInput: { file_path: "/repo/package.json" },
        _meta: { "cognition.ai/inferenceToolName": "read" },
      }),
    );
    h.push(
      child("8ac5cd37", {
        sessionUpdate: "tool_call_update",
        toolCallId: "read:0#83d6",
        status: "completed",
      }),
    );
    h.push(
      child("8ac5cd37", {
        sessionUpdate: "agent_message_chunk",
        content: { type: "text", text: "The name is probe-pkg." },
      }),
    );
    h.push(completed("8ac5cd37", true, "The name is probe-pkg."));
    h.push(
      child("00efcd6b", {
        sessionUpdate: "tool_call",
        toolCallId: "find_file_by_name:0#c5b8",
        title: "Find files matching `*`",
        kind: "search",
        rawInput: { query: "*" },
      }),
    );
    h.push({
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: "Top-level: README.md, package.json." },
    });

    // No row of its own for an agent id, and no child call leaks to the top.
    expect(h.tools.map((block) => block.tool?.callId)).toEqual([
      "call_list",
      "call_pkg",
      "call_wait",
    ]);
    const [list, pkg] = h.tools;
    expect(list.tool).toMatchObject({
      kind: "agent",
      title: "List top-level files",
      // The spawn returned, but the agent itself has not finished.
      status: "in_progress",
    });
    expect(list.tool?.preview).toBeUndefined();
    expect(list.agentRun?.steps.map((step) => step.text)).toEqual([
      "Find *",
    ]);
    expect(pkg.tool).toMatchObject({
      kind: "agent",
      title: "Read package name",
      status: "completed",
    });
    expect(
      pkg.agentRun?.steps.map((step) => [step.kind, step.text, step.status]),
    ).toEqual([
      ["reasoning", "Looking for package.json", undefined],
      ["tool", expect.stringContaining("package.json"), "completed"],
      ["message", "The name is probe-pkg.", undefined],
    ]);
    expect(h.prose).toBe("Top-level: README.md, package.json.");
  });

  it("keeps a failed agent's summary and still settles a spawn with no agent", () => {
    const h = harness();
    h.push(run("call_a", "Audit", "Audit auth."));
    h.push(started("a1", "Audit", "Audit auth."));
    h.push(runStatus("call_a", "completed"));
    h.push(completed("a1", false, "Read was denied."));
    // An older CLI that sends no subagent metadata.
    h.push(run("call_b", "Legacy", "Old CLI."));
    h.push(runStatus("call_b", "completed"));

    expect(h.tools[0].tool).toMatchObject({
      kind: "agent",
      status: "failed",
      detail: "Read was denied.",
    });
    expect(h.tools[1].tool).toMatchObject({
      kind: "agent",
      title: "Legacy",
      status: "completed",
    });
  });

  it("names the row by agent id when the spawn call was never seen", () => {
    const h = harness();
    h.push(started("a9", "Resumed audit", "Continue."));
    h.push(
      child("a9", {
        sessionUpdate: "tool_call",
        toolCallId: "grep:0",
        title: "Search `token`",
        kind: "search",
      }),
    );
    h.push(completed("a9", true, "Done."));

    expect(h.tools).toHaveLength(1);
    expect(h.tools[0].tool).toMatchObject({
      callId: "a9",
      kind: "agent",
      title: "Resumed audit",
      status: "completed",
    });
    expect(h.tools[0].agentRun?.steps).toHaveLength(1);
  });

  it("folds a nested subagent's work onto the top-level agent row", () => {
    const h = harness();
    h.push(run("call_top", "Plan", "Plan it."));
    h.push(started("top", "Plan", "Plan it."));
    h.push(run("call_inner", "Dig", "Dig in.", { "cognition.ai/subagent_context": { parentAgentId: "top" } }));
    h.push(started("inner", "Dig", "Dig in.", 2));
    h.push(
      child("inner", {
        sessionUpdate: "tool_call",
        toolCallId: "read:9",
        title: "Read file",
        kind: "read",
      }),
    );
    h.push(completed("inner", true, "Dug."));

    expect(h.tools.map((block) => block.tool?.callId)).toEqual(["call_top"]);
    const steps = h.tools[0].agentRun?.steps ?? [];
    expect(steps.map((step) => [step.id, step.status])).toEqual([
      ["tool:call_inner", "completed"],
      ["tool:read:9", undefined],
    ]);
  });

  it("leaves a subagent's reply out of collected answer text", () => {
    const reply = (meta?: Record<string, unknown>) => ({
      sessionId: "s",
      update: {
        sessionUpdate: "agent_message_chunk",
        content: { type: "text", text: "hi" },
        ...(meta ? { _meta: meta } : {}),
      },
    });
    expect(devinAgentMessageText(reply())).toBe("hi");
    expect(
      devinAgentMessageText(
        reply({ "cognition.ai/subagent_context": { parentAgentId: "a" } }),
      ),
    ).toBe("");
  });
});
