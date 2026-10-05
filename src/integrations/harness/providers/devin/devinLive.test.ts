import { beforeEach, describe, expect, it, vi } from "vitest";

const sent: string[] = [];
let onLine: ((line: string) => void) | undefined;
const textFiles = new Map<string, string>();

vi.mock("../../core/child", () => ({
  resolveDevinBinary: async () => ({ path: "/fake/devin" }),
  spawnChild: async () => undefined,
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (value: string) => void) => {
    onLine = line;
  },
  writeChild: async (_id: string, line: string) => {
    sent.push(line);
  },
  execChild: async () =>
    "Logged in.\n  Credentials path: /home/me/.config/devin/credentials.toml\n",
  readHarnessTextFile: async (path: string) => {
    const content = textFiles.get(path);
    if (content == null) throw new Error(`missing ${path}`);
    return content;
  },
}));

vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: async () => "/home/me",
}));

const {
  bindDevinSession,
  forgetDevinSession,
  respondDevinApproval,
  sendDevinTurn,
  waitForDevinSessionTitle,
} = await import("./devin");
const { resetDevinAuthCache } = await import("./devinAuth");
import type { HarnessEvent } from "../../core/types";

const parse = () => sent.map((line) => JSON.parse(line));
const request = (method: string) =>
  parse().find((message) => message.method === method);

function reply(id: number | string, result: unknown) {
  onLine!(JSON.stringify({ jsonrpc: "2.0", id, result }));
}

function notify(update: Record<string, unknown>) {
  onLine!(
    JSON.stringify({
      jsonrpc: "2.0",
      method: "session/update",
      params: { sessionId: "devin-1", update },
    }),
  );
}

async function waitFor(predicate: () => boolean, label: string) {
  for (let index = 0; index < 200; index += 1) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 5));
  }
  throw new Error(
    `timed out waiting for ${label}; sent=${JSON.stringify(parse().map((message) => message.method ?? `reply:${message.id}`))}`,
  );
}

async function answer(method: string, result: unknown) {
  await waitFor(() => !!request(method), method);
  reply(request(method)!.id, result);
}

const CONFIG = [
  { id: "mode", category: "mode", type: "select", currentValue: "accept-edits" },
  {
    id: "model",
    category: "model",
    type: "select",
    currentValue: "glm-5-2",
    options: [
      { value: "glm-5-2", name: "GLM-5.2 High" },
      { value: "swe-2-medium", name: "SWE-2 Medium" },
    ],
  },
];

async function startSession(load = false) {
  await answer("initialize", {
    protocolVersion: 1,
    authMethods: [{ id: "devin-browser" }],
  });
  await answer("authenticate", {});
  if (load) await answer("session/load", { configOptions: CONFIG });
  else await answer("session/new", { sessionId: "devin-1", configOptions: CONFIG });
}

describe("Devin live ACP sequence", () => {
  beforeEach(async () => {
    sent.length = 0;
    onLine = undefined;
    textFiles.clear();
    resetDevinAuthCache();
    await forgetDevinSession("devin-thread");
  });

  it("reuses the CLI login, picks model and mode, and prompts", async () => {
    textFiles.set(
      "/home/me/.config/devin/credentials.toml",
      'windsurf_api_key = "devin-key"\n',
    );
    const events: HarnessEvent[] = [];
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:swe-2-medium",
      runtimeMode: "auto",
      text: "hello",
      onEvent: (event) => events.push(event),
    });

    await startSession();
    expect(request("authenticate")!.params).toEqual({
      methodId: "devin-browser",
      _meta: { api_key: "devin-key" },
    });

    await answer("session/set_config_option", { configOptions: CONFIG });
    expect(request("session/set_config_option")!.params).toEqual({
      sessionId: "devin-1",
      configId: "model",
      value: "swe-2-medium",
    });
    await answer("session/set_mode", {});
    expect(request("session/set_mode")!.params.modeId).toBe("smart");

    await waitFor(() => !!request("session/prompt"), "session/prompt");
    notify({
      sessionUpdate: "agent_message_chunk",
      content: { type: "text", text: "hi there" },
    });
    reply(request("session/prompt")!.id, { stopReason: "end_turn" });
    await turn;

    expect(events).toContainEqual({
      type: "session.providerBound",
      providerSessionId: "devin-1",
    });
    expect(events).toContainEqual({ type: "message.delta", text: "hi there" });
    expect(events.some((event) => event.type === "session.error")).toBe(false);
  });

  it("answers string-id permission requests with the user's decision", async () => {
    const events: HarnessEvent[] = [];
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "supervised",
      text: "list files",
      onEvent: (event) => events.push(event),
    });
    await startSession();
    // No CLI login: Devin's own browser method is requested without a key.
    expect(request("authenticate")!.params).toEqual({ methodId: "devin-browser" });
    await answer("session/set_mode", {});
    expect(request("session/set_mode")!.params.modeId).toBe("accept-edits");
    await waitFor(() => !!request("session/prompt"), "session/prompt");

    notify({
      sessionUpdate: "tool_call",
      toolCallId: "call-1",
      title: "Ran ls",
      kind: "execute",
      rawInput: { command: "ls" },
    });
    onLine!(
      JSON.stringify({
        jsonrpc: "2.0",
        id: "7107bc27-2b44-4a89-a47b-8e4844d43eb3",
        method: "session/request_permission",
        params: {
          sessionId: "devin-1",
          toolCall: { toolCallId: "call-1", _meta: { "cognition.ai/editableCommand": "ls" } },
          options: [
            { optionId: "allow_once", name: "Allow", kind: "allow_once" },
            { optionId: "switch_bypass", name: "Bypass", kind: "allow_always" },
            { optionId: "reject_once", name: "Reject", kind: "reject_once" },
          ],
        },
      }),
    );
    await waitFor(
      () => events.some((event) => event.type === "approval.requested"),
      "approval.requested",
    );
    const asked = events.find((event) => event.type === "approval.requested");
    expect(asked).toMatchObject({ kind: "execute", callId: "call-1" });
    respondDevinApproval(
      "devin-thread",
      (asked as { requestId: number }).requestId,
      "allow",
    );
    await waitFor(
      () => parse().some((message) => message.id === "7107bc27-2b44-4a89-a47b-8e4844d43eb3"),
      "permission reply",
    );
    expect(
      parse().find((message) => message.id === "7107bc27-2b44-4a89-a47b-8e4844d43eb3")!.result,
    ).toEqual({ outcome: { outcome: "selected", optionId: "allow_once" } });

    reply(request("session/prompt")!.id, { stopReason: "end_turn" });
    await turn;
  });

  it("resumes with session/load and reports Devin's own title", async () => {
    bindDevinSession("devin-thread", "devin-1", "/repo");
    const events: HarnessEvent[] = [];
    const title = waitForDevinSessionTitle("devin-thread", 2_000);
    const turn = sendDevinTurn({
      sessionId: "devin-thread",
      cwd: "/repo",
      model: "devin:glm-5-2",
      runtimeMode: "full-access",
      text: "continue",
      onEvent: (event) => events.push(event),
    });
    await startSession(true);
    expect(request("session/load")!.params).toMatchObject({
      sessionId: "devin-1",
      cwd: "/repo",
    });
    expect(request("session/new")).toBeUndefined();
    await answer("session/set_mode", {});
    expect(request("session/set_mode")!.params.modeId).toBe("bypass");
    await waitFor(() => !!request("session/prompt"), "session/prompt");
    notify({ sessionUpdate: "session_info_update", title: "continue the..." });
    notify({ sessionUpdate: "session_info_update", title: "Resume repo work" });
    reply(request("session/prompt")!.id, { stopReason: "end_turn" });
    await turn;
    await expect(title).resolves.toBe("Resume repo work");
  });
});
