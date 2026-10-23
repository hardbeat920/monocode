import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  useConversation,
  useConversationTurns,
  useWorkspace,
  WorkspaceProvider,
} from "@/features/workspace/WorkspaceProvider";
import type { Turn } from "@/features/workspace/types";

const link = vi.hoisted(() => ({
  pairing: { desktop: { fingerprint: "desktop" } },
  command: vi.fn(),
  refresh: vi.fn(),
  openWatch: vi.fn(),
  nextUpdates: vi.fn(),
  closeWatch: vi.fn(),
  storage: new Map<string, string>(),
  nativeTranscript: false,
}));
vi.mock("react-native", () => ({
  AppState: { currentState: "active", addEventListener: () => ({ remove() {} }) },
}));
vi.mock("expo-sqlite/kv-store", () => ({
  default: {
    getItem: async (key: string) => link.storage.get(key) ?? null,
    setItem: async (key: string, value: string) => void link.storage.set(key, value),
    removeItem: async (key: string) => void link.storage.delete(key),
  },
}));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({
    pairing: link.pairing,
    catalog: { projects: [{ id: "project" }], monos: [], sessions: [] },
    selection: { projectIds: ["project"], monoIds: [] },
    command: link.command,
    refresh: link.refresh,
  }),
}));
vi.mock("@/features/connection/nativeLink", () => ({
  get hasNativeTranscript() { return link.nativeTranscript; },
  makeCommandId: () => "request-id",
  openWatch: link.openWatch,
  nextUpdates: link.nextUpdates,
  closeWatch: link.closeWatch,
}));
vi.mock("@/features/sessions/attachments", () => ({
  toWire: (attachments: { name: string; mimeType: string; data: string }[]) =>
    attachments.map(({ name, mimeType, data }) => ({ name, mimeType, data })),
}));

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean })
  .IS_REACT_ACT_ENVIRONMENT = true;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

describe("starting mobile sessions", () => {
  let root: ReturnType<typeof createRoot>;
  let workspace: ReturnType<typeof useWorkspace>;
  let conversation: ReturnType<typeof useConversation> &
    ReturnType<typeof useConversationTurns>;
  let update: ReturnType<typeof deferred<object[]>>;
  function Probe({ id }: { id: string | null }) {
    workspace = useWorkspace();
    conversation = { ...useConversation(id), ...useConversationTurns(id) };
    return <output>{conversation.turns.flatMap((turn) => turn.items).map((item) =>
      item.type === "user" ? item.text : "",
    )}</output>;
  }
  const render = (id: string | null = null) => act(() => root.render(
    <WorkspaceProvider><Probe id={id} /></WorkspaceProvider>,
  ));
  const frame = (value: object[]) => act(async () => {
    update.resolve(value);
    await Promise.resolve();
  });

  beforeEach(() => {
    vi.clearAllMocks();
    link.nativeTranscript = false;
    link.storage.clear();
    vi.stubGlobal("__DEV__", false);
    link.openWatch.mockResolvedValue(1);
    link.nextUpdates.mockImplementation(() => {
      update = deferred<object[]>();
      return update.promise;
    });
    link.closeWatch.mockResolvedValue(undefined);
    root = createRoot(document.createElement("div"));
  });
  afterEach(() => {
    act(() => root.unmount());
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("passes the chosen account and model to the desktop when starting a session", async () => {
    link.command.mockResolvedValue({ sessionId: "session" });
    render();
    await act(async () => {
      await workspace.startSession("project", "Hello", {
        harness: "codex", model: "codex:gpt", providerAccountId: "account-work",
      });
    });
    expect(link.command).toHaveBeenCalledWith("session.start", {
      projectId: "project", text: "Hello", harness: "codex", model: "codex:gpt",
      providerAccountId: "account-work",
    }, "request-id");
  });

  it("shows the first prompt without waiting for the catalog or first watch frame", async () => {
    const catalog = deferred<void>();
    link.refresh.mockReturnValue(catalog.promise);
    link.command.mockResolvedValue({ sessionId: "session" });
    render();
    let started: string | undefined;
    await act(async () => {
      void workspace.startSession("project", "Hello").then((id) => { started = id; });
      await Promise.resolve();
    });
    expect(started).toBe("session");

    await act(async () => { render("session"); });
    expect(conversation.turns).toHaveLength(1);
    expect(conversation.turns[0].items[0]).toMatchObject({ type: "user", text: "Hello" });
    expect(conversation.busy).toBe(true);
    // A local pending ID must never be sent as the watch's desktop history cursor.
    expect(link.openWatch).toHaveBeenCalledWith(expect.anything(), "conversation.watch", {
      conversationId: "session", tail: 3, incremental: true,
    });

    await frame([{ order: [], turns: [], busy: true }]);
    expect(conversation.turns[0].items[0]).toMatchObject({ type: "user", text: "Hello" });

    const confirmed: Turn = {
      id: "confirmed",
      items: [{ type: "user", id: "confirmed", text: "Hello" }],
    };
    await frame([{ order: [confirmed.id], turns: [confirmed], busy: false }]);
    expect(conversation.turns).toEqual([confirmed]);
    expect(conversation.busy).toBe(false);
  });

  it("leaves streaming to native and retires a send only after its native echo", async () => {
    link.nativeTranscript = true;
    link.command.mockResolvedValue(undefined);
    render("session");
    expect(link.openWatch).not.toHaveBeenCalled();
    const status = { busy: false, error: null, hasTurns: true, userCount: 3, pendingId: null };
    act(() => workspace.nativeTranscriptStatus("session", status));
    await act(async () => { await workspace.sendMessage("session", "Next prompt"); });
    expect(link.command).toHaveBeenCalledTimes(1);
    expect(link.command).toHaveBeenCalledWith("conversation.send", expect.objectContaining({ text: "Next prompt" }), "request-id");
    expect(conversation.turns[0].items[0]).toMatchObject({ type: "user", text: "Next prompt" });
    act(() => workspace.nativeTranscriptStatus("session", { ...status, busy: true }));
    expect(conversation.turns).toHaveLength(1);
    expect(conversation.busy).toBe(true);
    act(() => workspace.nativeTranscriptStatus("session", { ...status, userCount: 4 }));
    expect(conversation.turns).toEqual([]); // Confirmed turns remain native.
  });

  it("keeps a new native session busy during its temporary routing error", async () => {
    link.nativeTranscript = true;
    link.command.mockResolvedValue({ sessionId: "session" });
    render();
    await act(async () => { await workspace.startSession("project", "Hello"); });
    render("session");
    act(() => workspace.nativeTranscriptStatus("session", {
      ready: false, busy: false, error: "This workspace item is no longer available",
      hasTurns: true, userCount: 0, pendingId: conversation.turns[0].id,
    }));
    expect(conversation.busy).toBe(true);
    expect(conversation.error).toBeNull();
    expect(conversation.turns[0].items[0]).toMatchObject({ text: "Hello" });
  });

  it("does not create a pending conversation when starting fails", async () => {
    link.command.mockRejectedValue(new Error("Desktop could not start this session"));
    link.refresh.mockResolvedValue(undefined);
    render();
    await act(async () => {
      await expect(workspace.startSession("project", "Hello")).rejects.toThrow("could not start");
    });
    expect(link.refresh).not.toHaveBeenCalled();
    render("session");
    expect(conversation.turns).toEqual([]);
  });

  it("retries a new session's temporary routing error without showing the label", async () => {
    vi.useFakeTimers();
    const unavailable = "This workspace item is no longer available";
    link.refresh.mockResolvedValue(undefined);
    link.command.mockImplementation(async (method: string) => {
      if (method === "session.start") return { sessionId: "session" };
      throw new Error(unavailable);
    });
    link.openWatch.mockRejectedValueOnce(new Error(unavailable));
    render();
    await act(async () => { await workspace.startSession("project", "Hello"); });
    await act(async () => { render("session"); });
    expect(link.command).toHaveBeenCalledWith("conversation.read", { conversationId: "session" });
    expect(conversation.error).toBeNull();
    expect(conversation.turns[0].items[0]).toMatchObject({ type: "user", text: "Hello" });

    await act(async () => { await vi.advanceTimersByTimeAsync(200); });
    expect(link.openWatch).toHaveBeenCalledTimes(2);
    await frame([{ error: unavailable }]);
    expect(conversation.error).toBeNull();

    // A real failure is still reported if the desktop never makes the session available.
    vi.setSystemTime(Date.now() + 5_001);
    await frame([{ error: unavailable }]);
    expect(conversation.error).toBe(unavailable);
  });

  it("reports unavailability immediately for an existing conversation", async () => {
    await act(async () => { render("existing"); });
    await frame([{ error: "This workspace item is no longer available" }]);
    expect(conversation.error).toBe("This workspace item is no longer available");
  });

  it("reports other startup errors immediately", async () => {
    link.command.mockResolvedValue({ sessionId: "session" });
    link.refresh.mockResolvedValue(undefined);
    render();
    await act(async () => { await workspace.startSession("project", "Hello"); });
    await act(async () => { render("session"); });
    await frame([{ error: "Update Monocode on both devices" }]);
    expect(conversation.error).toBe("Update Monocode on both devices");
  });

  it("shows a conversation's cached turns at once and keeps the latest for next time", async () => {
    const cached: Turn = { id: "old", items: [{ type: "user", id: "old", text: "Before" }] };
    link.storage.set("monocode.mobile.desktop.turns.existing", JSON.stringify([cached]));
    await act(async () => { render("existing"); });
    expect(conversation.turns).toEqual([cached]);
    // Possibly long out of date, so the watch opens on the latest turns.
    expect(link.openWatch).toHaveBeenCalledWith(expect.anything(), "conversation.watch", {
      conversationId: "existing", tail: 3, incremental: true,
    });

    const latest: Turn = { id: "new", items: [{ type: "user", id: "new", text: "After" }] };
    await frame([{ order: [latest.id], turns: [latest], busy: false }]);
    expect(conversation.turns).toEqual([latest]);
    await act(async () => { render(null); });
    expect(JSON.parse(link.storage.get("monocode.mobile.desktop.turns.existing")!)).toEqual([latest]);
    expect(JSON.parse(link.storage.get("monocode.mobile.desktop.turns")!)).toEqual(["existing"]);
  });
});
