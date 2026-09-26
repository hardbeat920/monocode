import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessEvent, SendTurnInput } from "../../core/types";
import { resetHarnessModelOverlays } from "../../../../features/sessions/model/models";

const mock = vi.hoisted(() => {
  const listeners = new Map<string, (line: string) => void>();
  const exits = new Map<string, (code: number | null) => void>();
  return {
    listeners,
    exits,
    sent: [] as {
      thread: string;
      id?: number;
      method?: string;
      params?: Record<string, unknown>;
      result?: unknown;
    }[],
    spawn: vi.fn(async () => undefined),
    kill: vi.fn(async (id: string) => {
      listeners.delete(id);
      exits.delete(id);
    }),
    silent: new Set<string>(),
    autoPrompt: false,
    resolveGates: null as Array<() => void> | null,
  };
});

vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: async () => "/home/test",
}));

vi.mock("../../core/child", () => ({
  resolveDevinBinary: async () => {
    if (mock.resolveGates) {
      await new Promise<void>((resolve) => mock.resolveGates!.push(resolve));
    }
    return { path: "/fake/devin" };
  },
  spawnChild: mock.spawn,
  killChild: mock.kill,
  unwatchChild: (id: string) => {
    mock.listeners.delete(id);
    mock.exits.delete(id);
  },
  watchChild: (
    id: string,
    line: (line: string) => void,
    exit?: (code: number | null) => void,
  ) => {
    mock.listeners.set(id, line);
    if (exit) mock.exits.set(id, exit);
  },
  writeChild: async (thread: string, line: string) => {
    const message = JSON.parse(line);
    mock.sent.push({ thread, ...message });
    if (!message.method || message.id == null) return;
    if (mock.silent.has(message.method)) return;
    if (message.method === "session/prompt" && !mock.autoPrompt) return;

    queueMicrotask(() => {
      const configOptions = [
        {
          id: "model",
          category: "model",
          currentValue: "adaptive",
          options: [
            { value: "adaptive", name: "Adaptive" },
            { value: "sonnet", name: "Sonnet" },
          ],
        },
        {
          id: "mode",
          category: "mode",
          currentValue: "ask",
          options: [
            { value: "ask", name: "Ask" },
            { value: "smart", name: "Smart" },
            { value: "bypass", name: "Bypass" },
          ],
        },
      ];
      const result = ["session/new", "session/load"].includes(message.method)
        ? { sessionId: "devin-session", configOptions }
        : message.method === "session/prompt"
          ? { stopReason: "end_turn" }
          : {};
      mock.listeners.get(thread)?.(
        JSON.stringify({ jsonrpc: "2.0", id: message.id, result }),
      );
    });
  },
}));

const devin = await import("./devin");
const { refreshDevinCatalog } = await import("./devinCatalog");

function liveKey(): string {
  return [...mock.listeners.keys()].find((key) => key.startsWith("thread#devin-"))!;
}

function permission(id = 91): void {
  mock.listeners.get(liveKey())!(
    JSON.stringify({
      jsonrpc: "2.0",
      id,
      method: "session/request_permission",
      params: {
        toolCall: {
          toolCallId: "tool-1",
          title: "Run command",
          kind: "execute",
        },
        options: [
          { optionId: "yes", kind: "allow_once" },
          { optionId: "no", kind: "reject_once" },
        ],
      },
    }),
  );
}

function response(id: number): unknown {
  return mock.sent.findLast(
    (message: (typeof mock.sent)[number]) => message.id === id && message.result != null,
  )?.result;
}

const input = (events: HarnessEvent[] = []): SendTurnInput => ({
  sessionId: "thread",
  cwd: "/repo",
  model: "devin:adaptive",
  runtimeMode: "supervised",
  text: "hello",
  onEvent: (event) => events.push(event),
});

describe("Devin session lifecycle", () => {
  beforeEach(() => {
    mock.sent.length = 0;
    mock.listeners.clear();
    mock.exits.clear();
    mock.spawn.mockClear();
    mock.kill.mockClear();
    mock.silent.clear();
    mock.autoPrompt = false;
    mock.resolveGates = null;
  });

  afterEach(async () => {
    mock.resolveGates = null;
    await devin.forgetDevinSession("thread");
    resetHarnessModelOverlays();
    vi.useRealTimers();
  });

  it("never spawns when forget lands while binary resolution is pending", async () => {
    mock.resolveGates = [];
    const send = devin.sendDevinTurn(input());
    await vi.waitFor(() => expect(mock.resolveGates?.length).toBe(1));

    await devin.forgetDevinSession("thread");
    const gates = mock.resolveGates;
    mock.resolveGates = null;
    gates!.forEach((release: () => void) => release());

    await send;
    expect(mock.spawn).not.toHaveBeenCalled();
  });

  it("aborts setup immediately when the session is stopped", async () => {
    mock.silent.add("session/new");
    const send = devin.sendDevinTurn(input());
    await vi.waitFor(() =>
      expect(mock.sent.some((message: (typeof mock.sent)[number]) => message.method === "session/new")).toBe(true),
    );
    const child = liveKey();

    await devin.stopDevinSession("thread");
    await send;

    expect(mock.kill).toHaveBeenCalledWith(child);
    expect(mock.sent.some((message: (typeof mock.sent)[number]) => message.method === "session/prompt")).toBe(false);
  });

  it("fails a restore timeout instead of creating a replacement session", async () => {
    devin.bindDevinSession("thread", "saved-session", "/repo");
    mock.silent.add("session/load");
    vi.useFakeTimers();

    const send = devin.sendDevinTurn(input());
    const outcome = send.then(() => "resolved", (error: Error) => error.message);
    await vi.advanceTimersByTimeAsync(46_000);

    await expect(outcome).resolves.toMatch(/timed out/i);
    expect(mock.sent.some((message: (typeof mock.sent)[number]) => message.method === "session/new")).toBe(false);
  });

  it("cancels a permission request that arrives after the turn finished", async () => {
    const events: HarnessEvent[] = [];
    mock.autoPrompt = true;
    await devin.sendDevinTurn({
      ...input(events),
      runtimeMode: "full-access",
    });

    permission(91);
    await vi.waitFor(() =>
      expect(response(91)).toEqual({ outcome: { outcome: "cancelled" } }),
    );
    expect(events.some((event) => event.type === "approval.requested")).toBe(false);
    expect(events.some((event) => event.type === "tool.updated")).toBe(false);
  });

  it("uses a distinct child id for each catalog probe", async () => {
    await refreshDevinCatalog();
    await refreshDevinCatalog();

    const probeIds = mock.spawn.mock.calls
      .map((call: unknown[]) => call[0] as string)
      .filter((id: string) => id.startsWith("monocode-devin-probe-"));
    expect(probeIds).toHaveLength(2);
    expect(probeIds[0]).not.toBe(probeIds[1]);
  });
});