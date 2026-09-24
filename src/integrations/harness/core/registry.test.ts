import { afterEach, describe, expect, it, vi } from "vitest";
import { resetHarnessModelOverlays, setHarnessModels } from "../../../features/sessions/model/models";
import type { HarnessId } from "../../../features/sessions/model/session";
import {
  HARNESS_IDLE_PARK_MS,
  canCompactHarnessContext,
  canRewindHarnessLastTurn,
  compactHarnessContext,
  isLiveHarness,
  listHarnesses,
  refreshHarnessCatalogs,
  registerHarness,
  resetHarnessIdlePark,
  sendHarnessTurn,
  type HarnessAdapter,
} from "./registry";
import type { SendTurnInput, SteerTurnInput } from "./types";
import { registerBuiltinHarnesses } from "./register";

function stub(
  id: "cursor" | "codex" | "claude" | "pi",
  extra: Partial<HarnessAdapter> = {},
): HarnessAdapter {
  return {
    id,
    live: true,
    async sendTurn(_input: SendTurnInput) {},
    async steerTurn(_input: SteerTurnInput) {},
    async cancelTurn() {},
    respondApproval() {},
    async stopSession() {},
    async forgetSession() {},
    bindSession() {},
    ...extra,
  };
}

describe("harness registry", () => {
  afterEach(() => {
    resetHarnessModelOverlays();
    resetHarnessIdlePark();
    vi.useRealTimers();
  });

  it("tracks live adapters", () => {
    registerHarness(stub("cursor"));
    registerHarness(stub("codex"));
    registerHarness(stub("claude"));
    expect(isLiveHarness("cursor")).toBe(true);
    expect(isLiveHarness("codex")).toBe(true);
    expect(isLiveHarness("claude")).toBe(true);
    expect(
      listHarnesses()
        .map((a) => a.id)
        .filter((id) => id === "claude" || id === "codex" || id === "cursor")
        .sort(),
    ).toEqual(["claude", "codex", "cursor"]);
  });

  it("advertises and dispatches compaction only when an adapter supports it", async () => {
    const compactContext = vi.fn(async () => undefined);
    registerHarness(stub("codex", { compactContext }));
    registerHarness(stub("claude"));

    expect(canCompactHarnessContext("codex")).toBe(true);
    expect(canCompactHarnessContext("claude")).toBe(false);

    await compactHarnessContext({
      harness: "codex",
      sessionId: "compact-1",
      cwd: "/tmp",
      model: "codex:gpt-5.4",
      runtimeMode: "supervised",
      onEvent: () => undefined,
    });

    expect(compactContext).toHaveBeenCalledOnce();
    await expect(
      compactHarnessContext({
        harness: "claude",
        sessionId: "compact-2",
        cwd: "/tmp",
        model: "claude:sonnet",
        runtimeMode: "supervised",
        onEvent: () => undefined,
      }),
    ).rejects.toThrow("does not support manual compaction");
  });

  it("exposes the native compaction support matrix", () => {
    registerBuiltinHarnesses();
    const ids: HarnessId[] = [
      "claude",
      "codex",
      "cursor",
      "grok",
      "opencode",
      "pi",
      "omp",
      "fx",
      "antigravity",
      "muse",
    ];

    expect(
      Object.fromEntries(ids.map((id) => [id, canCompactHarnessContext(id)])),
    ).toEqual({
      claude: true,
      codex: true,
      cursor: false,
      grok: true,
      opencode: true,
      pi: true,
      omp: true,
      fx: false,
      antigravity: false,
      muse: false,
    });
  });

  it("exposes the edit-last-turn support matrix", () => {
    registerBuiltinHarnesses();
    const ids: HarnessId[] = [
      "claude",
      "codex",
      "cursor",
      "grok",
      "opencode",
      "pi",
      "omp",
      "fx",
      "muse",
    ];

    expect(
      Object.fromEntries(ids.map((id) => [id, canRewindHarnessLastTurn(id)])),
    ).toEqual({
      claude: false,
      codex: true,
      cursor: false,
      grok: false,
      opencode: true,
      pi: true,
      omp: true,
      fx: false,
      muse: false,
    });
  });

  it("registers Muse as a live exec harness", () => {
    registerBuiltinHarnesses();
    expect(isLiveHarness("muse")).toBe(true);
    const adapter = listHarnesses().find((adapter) => adapter.id === "muse")!;
    expect(adapter.canSteer).toBe(false);
    expect(adapter.bindSession).toBeTypeOf("function");
    expect(adapter.refreshCatalog).toBeTypeOf("function");
    expect(adapter.generateTitle).toBeUndefined();
    expect(adapter.generateCommitMessage).toBeUndefined();
  });

  it("registers Antigravity as a live fx-tier harness", () => {
    registerBuiltinHarnesses();
    expect(isLiveHarness("antigravity")).toBe(true);
    const adapter = listHarnesses().find((adapter) => adapter.id === "antigravity")!;
    expect(adapter.canSteer).toBe(false);
    expect(adapter.bindSession).toBeTypeOf("function");
    expect(adapter.refreshCatalog).toBeTypeOf("function");
    expect(adapter.generateTitle).toBeUndefined();
    expect(adapter.generateCommitMessage).toBeUndefined();
  });

  it("refreshes only the requested catalogs", async () => {
    const pi = vi.fn(async () => undefined);
    const claude = vi.fn(async () => undefined);
    registerHarness(stub("pi", { refreshCatalog: pi }));
    registerHarness(stub("claude", { refreshCatalog: claude }));

    await refreshHarnessCatalogs(["claude"]);

    expect(claude).toHaveBeenCalledOnce();
    expect(pi).not.toHaveBeenCalled();
  });

  it("does not spawn a catalog probe twice after a live list lands", async () => {
    const pi = vi.fn(async () => {
      setHarnessModels("pi", [
        {
          id: "pi:opus",
          harness: "pi",
          name: "Opus",
          nativeId: "anthropic/opus",
        },
      ]);
    });
    registerHarness(stub("pi", { refreshCatalog: pi }));

    await refreshHarnessCatalogs(["pi"]);
    await refreshHarnessCatalogs(["pi"]);

    expect(pi).toHaveBeenCalledOnce();
  });

  it("skips catalog refresh when no harness is in use", async () => {
    const pi = vi.fn(async () => undefined);
    registerHarness(stub("pi", { refreshCatalog: pi }));
    await refreshHarnessCatalogs([]);
    expect(pi).not.toHaveBeenCalled();
  });

  it("parks a live child a few minutes after the turn settles", async () => {
    vi.useFakeTimers();
    const stopSession = vi.fn(async () => undefined);
    registerHarness(stub("cursor", { stopSession }));

    await sendHarnessTurn({
      harness: "cursor",
      sessionId: "s1",
      cwd: "/tmp",
      model: "cursor:composer-2.5",
      text: "hi",
      runtimeMode: "supervised",
      onEvent: () => undefined,
    });

    expect(stopSession).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(HARNESS_IDLE_PARK_MS - 1);
    expect(stopSession).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(stopSession).toHaveBeenCalledWith("s1");
  });
  it("serializes provider-state operations per session", async () => {
    const order: string[] = [];
    let releaseFirst!: () => void;
    let markFirstStarted!: () => void;
    const firstStarted = new Promise<void>((resolve) => {
      markFirstStarted = resolve;
    });
    const firstGate = new Promise<void>((resolve) => {
      releaseFirst = resolve;
    });
    const compactContext = vi.fn(async (input: { sessionId: string }) => {
      order.push(`start:${input.sessionId}`);
      if (input.sessionId === "s1") {
        markFirstStarted();
        await firstGate;
      }
      order.push(`end:${input.sessionId}`);
    });
    registerHarness(stub("codex", { compactContext }));

    const compact1 = compactHarnessContext({
      harness: "codex",
      sessionId: "s1",
      cwd: "/tmp",
      model: "codex:gpt-5.4",
      runtimeMode: "supervised",
      onEvent: () => undefined,
    });
    await firstStarted;
    const compact2 = compactHarnessContext({
      harness: "codex",
      sessionId: "s1",
      cwd: "/tmp",
      model: "codex:gpt-5.4",
      runtimeMode: "supervised",
      onEvent: () => undefined,
    });
    const independent = compactHarnessContext({
      harness: "codex",
      sessionId: "s2",
      cwd: "/tmp",
      model: "codex:gpt-5.4",
      runtimeMode: "supervised",
      onEvent: () => undefined,
    });

    await independent;
    expect(order).toEqual(["start:s1", "start:s2", "end:s2"]);
    releaseFirst();
    await Promise.all([compact1, compact2]);
    expect(order).toEqual([
      "start:s1",
      "start:s2",
      "end:s2",
      "end:s1",
      "start:s1",
      "end:s1",
    ]);
  });
});
