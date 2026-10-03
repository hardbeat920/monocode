import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessEvent, SendTurnInput } from "../../core/types";

const spawned: { args: string[]; account?: unknown }[] = [];
const sent: Record<string, any>[] = [];
let onLine: ((line: string) => void) | undefined;
vi.mock("../../core/child", () => ({
  resolveClaudeBinary: async () => ({ path: "/fake/claude" }),
  spawnChild: async (
    _id: string,
    _path: string,
    args: string[],
    _cwd: string,
    account?: unknown,
  ) => {
    spawned.push({ args, account });
    return 123;
  },
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (line: string) => void) => {
    onLine = line;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(JSON.parse(line));
  },
}));
const { sendClaudeTurn, respondClaudeApproval, __claudeTestReset } =
  await import("./claude");
const { buildClaudeSpawnArgs } = await import("./claudeProtocol");
function emit(value: unknown) {
  onLine!(JSON.stringify(value));
}
async function waitFor(predicate: () => boolean) {
  for (let n = 0; n < 100; n++) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 2));
  }
  throw new Error("Probe timed out");
}
function input(
  events: HarnessEvent[],
  patch: Partial<SendTurnInput> = {},
): SendTurnInput {
  return {
    sessionId: "probe",
    cwd: "/repo",
    model: "claude:claude-opus-5-5",
    modelSettings: {},
    runtimeMode: "supervised",
    text: "Implement this",
    attachments: [],
    onEvent: (event) => events.push(event),
    ...patch,
  };
}
async function start(
  events: HarnessEvent[],
  patch: Partial<SendTurnInput> = {},
) {
  const turn = sendClaudeTurn(input(events, patch));
  await waitFor(() => sent.some((m) => m.request?.subtype === "initialize"));
  emit({
    type: "control_response",
    response: { subtype: "success", request_id: "monocode_2", response: {} },
  });
  emit({
    type: "system",
    subtype: "init",
    session_id: "00000000-0000-4000-8000-000000000001",
  });
  await waitFor(() => sent.some((m) => m.type === "user"));
  return { turn };
}
function result() {
  emit({
    type: "result",
    subtype: "success",
    session_id: "00000000-0000-4000-8000-000000000001",
  });
}
beforeEach(() => {
  spawned.length = 0;
  sent.length = 0;
  __claudeTestReset();
});
afterEach(() => {
  __claudeTestReset();
});
describe("official v0.7.0 backend integration defect probes", () => {
  it("overrides inherited Thinking On with explicit Thinking Off", async () => {
    const { turn } = await start([], {
      modelSettings: { thinking: "false", fast: "false" },
    });
    const argv = spawned[0].args;
    const settings = JSON.parse(argv[argv.indexOf("--settings") + 1]);
    expect(settings).toEqual({ alwaysThinkingEnabled: false });
    expect(argv).toContain("--setting-sources=user,project,local");
    result();
    await turn;
  });
  it("requests foreground subagent text forwarding", () => {
    expect(buildClaudeSpawnArgs({})).toContain("--forward-subagent-text");
  });
  it("allows ExitPlanMode during a full-access implementation turn", async () => {
    const events: HarnessEvent[] = [];
    const { turn } = await start(events, { runtimeMode: "full-access" });
    emit({
      type: "control_request",
      request_id: "exit-plan",
      request: {
        subtype: "can_use_tool",
        tool_name: "ExitPlanMode",
        tool_use_id: "exit-tool",
        input: { plan: "# Plan\n\n1. Modify the file" },
      },
    });
    await waitFor(() =>
      sent.some((m) => m.response?.request_id === "exit-plan"),
    );
    expect(
      sent.find((m) => m.response?.request_id === "exit-plan")?.response
        .response.behavior,
    ).toBe("allow");
    expect(events.some((event) => event.type === "approval.requested")).toBe(
      false,
    );
    result();
    await turn;
  });
  it("restarts in Build mode after an autonomous plan", async () => {
    const events: HarnessEvent[] = [];
    const { turn } = await start(events);
    emit({
      type: "control_request",
      request_id: "enter-plan",
      request: {
        subtype: "can_use_tool",
        tool_name: "EnterPlanMode",
        tool_use_id: "enter-tool",
        input: {},
      },
    });
    const approval = events.find(
      (event) => event.type === "approval.requested",
    ) as Extract<HarnessEvent, { type: "approval.requested" }>;
    respondClaudeApproval("probe", approval.requestId, "allow");
    await waitFor(() =>
      sent.some((m) => m.response?.request_id === "enter-plan"),
    );
    emit({
      type: "control_request",
      request_id: "exit-plan",
      request: {
        subtype: "can_use_tool",
        tool_name: "ExitPlanMode",
        tool_use_id: "exit-tool",
        input: { plan: "# Plan\n\n1. Modify the file" },
      },
    });
    await waitFor(
      () =>
        events.filter((event) => event.type === "approval.requested").length ===
        2,
    );
    const exitApproval = events.filter(
      (event) => event.type === "approval.requested",
    )[1] as Extract<HarnessEvent, { type: "approval.requested" }>;
    respondClaudeApproval("probe", exitApproval.requestId, "deny");
    await waitFor(() =>
      sent.some((m) => m.response?.request_id === "exit-plan"),
    );
    result();
    await turn;
    sent.length = 0;
    const build = sendClaudeTurn(
      input(events, { intent: "build", text: "Build approved plan" }),
    );
    await waitFor(() => spawned.length === 2);
    emit({
      type: "control_response",
      response: { subtype: "success", request_id: "monocode_2", response: {} },
    });
    await waitFor(() => sent.some((m) => m.type === "user"));
    expect(spawned.length).toBe(2);
    expect(sent.some((m) => m.request?.subtype === "set_permission_mode")).toBe(
      false,
    );
    result();
    await build;
  });
});
