import { afterEach, expect, it, vi } from "vitest";
import { runInNewContext } from "node:vm";
import { PI_BACKGROUND_BRIDGE, PI_BACKGROUND_STATUS_KEY } from "./piBackgroundBridge";

const registryKey = Symbol.for("@agegr/pi-web/session-liveness/v1");

afterEach(() => vi.useRealTimers());

it("retains running work and pending delivery across a 30 minute wait, then releases", () => {
  vi.useFakeTimers();
  const sandbox: Record<PropertyKey, unknown> = { Symbol, setInterval, clearInterval, [registryKey]: undefined };
  const handlers = new Map<string, (...args: unknown[]) => void>();
  const statuses: string[] = [];
  const factory = runInNewContext("(" + PI_BACKGROUND_BRIDGE.replace("export default", "") + ")", sandbox);
  factory({ on: (name: string, handler: (...args: unknown[]) => void) => handlers.set(name, handler) });
  expect(vi.getTimerCount()).toBe(0);
  const registry = sandbox[registryKey] as { register(entry: unknown): () => void };
  let working = true;
  let pendingDelivery = false;
  const release = registry.register({ name: "pi-subagents", sessionId: "parent", isActive: () => working || pendingDelivery });
  registry.register({ name: "other session", sessionId: "other", isActive: () => true });
  handlers.get("session_start")!({}, {
    sessionManager: { getSessionId: () => "parent" },
    ui: { setStatus: (key: string, text: string) => { expect(key).toBe(PI_BACKGROUND_STATUS_KEY); statuses.push(text); } },
  });
  vi.advanceTimersByTime(30 * 60_000);
  expect(statuses.map(text => JSON.parse(text).tasks)).toEqual([["pi-subagents"]]);
  working = false;
  pendingDelivery = true;
  handlers.get("agent_end")!();
  expect(statuses).toHaveLength(1);
  pendingDelivery = false;
  handlers.get("agent_settled")!();
  expect(JSON.parse(statuses.at(-1)!).tasks).toEqual([]);
  release();
  handlers.get("session_shutdown")!();
  expect(vi.getTimerCount()).toBe(0);
  expect(sandbox[registryKey]).toBeUndefined();
});

it("retains work on observer errors and preserves an existing host registry", () => {
  vi.useFakeTimers();
  const previousRelease = vi.fn();
  const previous = { version: 1, register: vi.fn(() => previousRelease) };
  const sandbox: Record<PropertyKey, unknown> = { Symbol, setInterval, clearInterval, [registryKey]: previous };
  const handlers = new Map<string, (...args: unknown[]) => void>();
  const setStatus = vi.fn();
  runInNewContext("(" + PI_BACKGROUND_BRIDGE.replace("export default", "") + ")", sandbox)({
    on: (name: string, handler: (...args: unknown[]) => void) => handlers.set(name, handler),
  });
  const release = (sandbox[registryKey] as { register(entry: unknown): () => void }).register({
    name: "pi-subagents", sessionId: "parent", isActive: () => { throw new Error("observer failed"); },
  });
  handlers.get("session_start")!({}, { sessionManager: { getSessionId: () => "parent" }, ui: { setStatus } });
  expect(JSON.parse(setStatus.mock.calls[0][1]).tasks).toEqual(["pi-subagents"]);
  release();
  expect(previousRelease).toHaveBeenCalledOnce();
  handlers.get("session_shutdown")!();
  expect(sandbox[registryKey]).toBe(previous);
});
