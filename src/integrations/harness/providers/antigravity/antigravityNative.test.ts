import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessEvent, SendTurnInput } from "../../core/types";

const mock = vi.hoisted(() => ({
  windows: true, tauri: true, headless: false,
  invoke: vi.fn(), spawn: vi.fn(), resolve: vi.fn(), unlisten: vi.fn(),
  listen: vi.fn(), listener: undefined as undefined | ((event: { payload: { sessionId: string; turnId: string; event: unknown } }) => void),
  models: vi.fn(), saveImage: vi.fn(), deleteImages: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => mock.tauri, invoke: mock.invoke }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ listen: mock.listen }) }));
vi.mock("../../../../platform/tauri/platform", () => ({ get IS_WIN() { return mock.windows; } }));
vi.mock("../../core/child", () => ({ hasHeadlessChildBackend: () => mock.headless, spawnChild: mock.spawn, resolveAntigravityBinary: mock.resolve }));
vi.mock("../../core/registry", () => ({ isLiveHarness: (id: string) => id === "antigravity" }));
vi.mock("../../../../features/sessions/model/models", () => ({ setHarnessModels: mock.models }));
vi.mock("../../../../platform/tauri/fs", () => ({ homeDir: async () => "C:/test", saveGeneratedImage: mock.saveImage, deleteGeneratedImages: mock.deleteImages }));

import * as native from "./antigravityNative";
import * as agy from "./antigravity";
import { refreshAntigravityCatalog } from "./antigravityCatalog";
import { loginHarness } from "../../core/auth";
import { supportsHarnessLogin } from "../../core/authSupport";
import { harnessUnavailableHint, isHarnessAvailable, probeHarnessAvailability } from "../../core/availability";

const deferred = () => {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => { resolve = done; });
  return { promise, resolve };
};
function input(id: string): SendTurnInput {
  return { sessionId: id, cwd: "C:/test", model: "antigravity:gemini-3.8-flash-high", modelSettings: { thinking: "high" }, runtimeMode: "supervised", text: "Change file", onEvent: vi.fn(), onAccepted: vi.fn() };
}
function payload(command: string) {
  return mock.invoke.mock.calls.find(([name]) => name === command)?.[1];
}
function emit(turn: { sessionId: string; turnId: string }, event: unknown) {
  mock.listener?.({ payload: { ...turn, event } });
}

beforeEach(() => {
  vi.clearAllMocks(); mock.windows = true; mock.tauri = true; mock.headless = false;
  mock.invoke.mockResolvedValue(undefined);
  mock.resolve.mockResolvedValue({ path: "C:/agy_acp_server.par", args: [] });
  mock.saveImage.mockResolvedValue({ path: "C:/test/image.png", mimeType: "image/png", size: 4 });
  mock.listen.mockImplementation(async (_name, listener) => { mock.listener = listener; return mock.unlisten; });
});

describe("native Windows Antigravity", () => {
  it("routes only local desktop Windows and never starts a CLI", async () => {
    expect(native.usesNativeAntigravity("C:/test")).toBe(true);
    expect(native.usesNativeAntigravity("remote://host/project")).toBe(false);
    mock.headless = true; expect(native.usesNativeAntigravity("C:/test")).toBe(false);
    mock.headless = false; mock.windows = false; expect(native.usesNativeAntigravity("C:/test")).toBe(false);
    mock.windows = true; mock.tauri = false; expect(native.usesNativeAntigravity("C:/test")).toBe(false);
    mock.tauri = true;
    await agy.sendAntigravityTurn(input("native-route"));
    expect(mock.invoke).toHaveBeenCalledWith("antigravity_native_send", expect.any(Object));
    expect(mock.spawn).not.toHaveBeenCalled(); expect(mock.resolve).not.toHaveBeenCalled();
  });

  it("separates signed-out backend status from explicit login and catalog refresh", async () => {
    mock.invoke.mockImplementation(async (command) => command === "antigravity_native_status"
      ? { backendAvailable: true, authenticated: false, loginPending: false, authError: "Unreadable account" }
      : command === "antigravity_native_catalog" ? [{ id: "antigravity:gemini-3.8-flash-high", harness: "antigravity" }] : undefined);
    expect(await native.nativeAntigravityAccount()).toMatchObject({ backendAvailable: true, authenticated: false });
    expect(mock.invoke).toHaveBeenCalledTimes(1);
    await loginHarness("antigravity", undefined, "C:/test"); await refreshAntigravityCatalog("C:/test");
    expect(mock.models).toHaveBeenCalledWith("antigravity", expect.arrayContaining([expect.objectContaining({ harness: "antigravity" })]));
    await native.logoutNativeAntigravity();
    expect(mock.invoke).toHaveBeenCalledWith("antigravity_native_logout", undefined);
    expect(mock.spawn).not.toHaveBeenCalled(); expect(mock.resolve).not.toHaveBeenCalled();
  });

  it("does not offer or join local native sign-in from a remote Windows workspace", async () => {
    const cwd = "remote://host/project";
    expect(supportsHarnessLogin("antigravity", "C:/test")).toBe(true);
    expect(supportsHarnessLogin("antigravity", cwd)).toBe(false);
    const gate = deferred();
    mock.invoke.mockImplementation(async (command) => command === "antigravity_native_login"
      ? gate.promise : [{ id: "antigravity:gemini-3.8-flash-high", harness: "antigravity" }]);
    const localLogin = loginHarness("antigravity", undefined, "C:/test");
    await expect(loginHarness("antigravity", undefined, cwd)).rejects.toThrow("does not offer a single browser sign-in flow");
    gate.resolve();
    await localLogin;
    expect(mock.invoke.mock.calls.map(([command]) => command)).toEqual([
      "antigravity_native_login", "antigravity_native_catalog",
    ]);
  });

  it("reprobes availability when switching between local native and remote CLI workspaces", async () => {
    mock.invoke.mockResolvedValue({ backendAvailable: true });
    mock.resolve.mockRejectedValue(new Error("Remote CLI not installed"));
    await probeHarnessAvailability({ cwd: "C:/test", force: true });
    expect(isHarnessAvailable("antigravity")).toBe(true);
    await probeHarnessAvailability({ cwd: "remote://host/project" });
    expect(isHarnessAvailable("antigravity")).toBe(false);
    expect(mock.resolve).toHaveBeenCalledOnce();
    expect(mock.invoke).toHaveBeenCalledTimes(1);
    expect(harnessUnavailableHint("antigravity", "remote://host/project")).toContain("agy_acp_server.par");
    expect(harnessUnavailableHint("antigravity", "C:/test")).toContain("native backend");
    await probeHarnessAvailability({ cwd: "C:/test" });
    expect(isHarnessAvailable("antigravity")).toBe(true);
    expect(mock.invoke).toHaveBeenCalledTimes(2);
  });

  it("does not reuse an in-flight local availability probe for a remote workspace", async () => {
    const gate = deferred();
    mock.invoke.mockImplementation(async () => { await gate.promise; return { backendAvailable: true }; });
    mock.resolve.mockRejectedValue(new Error("Remote CLI not installed"));
    const local = probeHarnessAvailability({ cwd: "C:/test", force: true });
    const remote = probeHarnessAvailability({ cwd: "remote://host/project" });
    gate.resolve();
    await Promise.all([local, remote]);
    expect(mock.resolve).toHaveBeenCalledOnce();
    expect(mock.invoke).toHaveBeenCalledOnce();
    expect(isHarnessAvailable("antigravity")).toBe(false);
  });

  it("routes a remote Windows catalog refresh to the CLI without querying the native account", async () => {
    const debug = vi.spyOn(console, "debug").mockImplementation(() => undefined);
    try {
      mock.resolve.mockRejectedValue(new Error("Remote CLI not installed"));
      await refreshAntigravityCatalog("remote://host/project");
      expect(mock.resolve).toHaveBeenCalledOnce();
      expect(mock.invoke).not.toHaveBeenCalled();
    } finally { debug.mockRestore(); }
  });

  it("maps native events and concrete approval previews with session and turn filtering", async () => {
    const gate = deferred(); mock.invoke.mockImplementation(async (command) => command === "antigravity_native_send" ? gate.promise : undefined);
    const request = input("native-preview"); const events: HarnessEvent[] = []; request.onEvent = (event) => events.push(event);
    const turn = agy.sendAntigravityTurn(request);
    await vi.waitFor(() => expect(payload("antigravity_native_send")).toBeDefined());
    const ids = payload("antigravity_native_send").input;
    expect(ids.modelSettings).toEqual({ thinking: "high" });
    emit({ ...ids, turnId: "retired" }, { type: "message.delta", text: "wrong" });
    emit({ ...ids, sessionId: "other-window-session" }, { type: "message.delta", text: "wrong" });
    emit(ids, { type: "turn.accepted" }); emit(ids, { type: "turn.accepted" });
    const preview = { kind: "write", title: "Edit file.txt", path: "file.txt", contentOnly: false, additions: 1, deletions: 1, lines: [{ kind: "del", text: "old" }, { kind: "add", text: "new" }] };
    emit(ids, { type: "tool.started", callId: "tool", title: "Edit file.txt", kind: "edit", status: "running", preview });
    emit(ids, { type: "approval.requested", requestId: 4, title: "Edit file.txt", kind: "edit", callId: "tool", preview });
    agy.respondAntigravityApproval(request.sessionId, 4, "allow");
    await vi.waitFor(() => expect(mock.invoke).toHaveBeenCalledWith("antigravity_native_approve", { sessionId: request.sessionId, turnId: ids.turnId, requestId: 4, decision: "allow" }));
    emit(ids, { type: "approval.resolved", requestId: 4, decision: "allow" });
    expect(events).toHaveLength(3); expect(events[1]).toMatchObject({ preview });
    expect(request.onAccepted).toHaveBeenCalledTimes(1);
    gate.resolve(); await turn; expect(mock.unlisten).toHaveBeenCalledTimes(1);
    expect(mock.spawn).not.toHaveBeenCalled(); expect(mock.resolve).not.toHaveBeenCalled();
  });

  it("maps plan deltas, suppresses cancelled events and waits before forget", async () => {
    const gate = deferred(); mock.invoke.mockImplementation(async (command) => command === "antigravity_native_send" ? gate.promise : undefined);
    const request = { ...input("native-cancel"), intent: "plan" as const };
    const turn = agy.sendAntigravityTurn(request);
    await vi.waitFor(() => expect(payload("antigravity_native_send")).toBeDefined());
    const ids = payload("antigravity_native_send").input;
    emit(ids, { type: "message.delta", text: "Plan" }); emit(ids, { type: "message.completed" });
    expect(request.onEvent).toHaveBeenLastCalledWith({ type: "plan", text: "Plan", key: ids.turnId, streaming: false });
    const forget = agy.forgetAntigravitySession(request.sessionId);
    await vi.waitFor(() => expect(mock.invoke).toHaveBeenCalledWith("antigravity_native_cancel", expect.any(Object)));
    emit(ids, { type: "message.delta", text: "late" });
    expect(request.onEvent).toHaveBeenCalledTimes(2);
    expect(payload("antigravity_native_stop")).toBeUndefined();
    gate.resolve(); await turn; await forget;
    expect(mock.invoke).toHaveBeenCalledWith("antigravity_native_stop", { sessionId: request.sessionId, forget: true });
    expect(native.hasNativeAntigravitySession(request.sessionId)).toBe(false);
  });

  it("binds before sending and surfaces native resume errors without CLI fallback", async () => {
    const gate = deferred(); mock.invoke.mockImplementation(async (command) => command === "antigravity_native_bind" ? gate.promise : undefined);
    agy.bindAntigravitySession("native-bind", "provider-uuid", "C:/test");
    const turn = agy.sendAntigravityTurn(input("native-bind"));
    expect(payload("antigravity_native_send")).toBeUndefined(); gate.resolve(); await turn;
    expect(mock.invoke.mock.calls.map(([command]) => command)).toEqual(["antigravity_native_bind", "antigravity_native_send"]);
    mock.invoke.mockRejectedValueOnce({ code: "session", message: "ACP history cannot resume natively" });
    agy.bindAntigravitySession("native-bind-error", "acp-id", "C:/test");
    await expect(agy.sendAntigravityTurn(input("native-bind-error"))).rejects.toThrow("ACP history cannot resume natively");
    expect(mock.spawn).not.toHaveBeenCalled(); expect(mock.resolve).not.toHaveBeenCalled();
  });

  it("materializes generated images before marking the turn ready", async () => {
    const gate = deferred(); const image = deferred();
    mock.invoke.mockImplementation(async (command) => command === "antigravity_native_send" ? gate.promise : undefined);
    mock.saveImage.mockImplementation(async () => { await image.promise; return { path: "C:/test/image.png", mimeType: "image/png", size: 4 }; });
    const request = input("native-image"); const turn = agy.sendAntigravityTurn(request);
    await vi.waitFor(() => expect(payload("antigravity_native_send")).toBeDefined());
    const ids = payload("antigravity_native_send").input;
    emit(ids, { type: "image.generated", itemId: "image", data: "aW1hZ2U=", name: "Antigravity image" });
    emit(ids, { type: "turn.ready" }); gate.resolve();
    expect(request.onEvent).not.toHaveBeenCalled(); image.resolve(); await turn;
    expect(request.onEvent).toHaveBeenNthCalledWith(1, expect.objectContaining({ type: "image.generated", path: "C:/test/image.png" }));
    expect(request.onEvent).toHaveBeenLastCalledWith({ type: "turn.ready" });
    expect(mock.saveImage).toHaveBeenCalledWith({ data: "aW1hZ2U=", name: "Antigravity image" });
  });
});
