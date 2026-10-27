import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  useWorkspace,
  WorkspaceProvider,
} from "@/features/workspace/WorkspaceProvider";
import MonoScreen from "@/app/mono/[monoId]";
import type { ApprovalPrompt } from "@/features/workspace/types";

const link = vi.hoisted(() => ({
  pairing: { desktop: { fingerprint: "desktop" } },
  command: vi.fn(),
  refresh: vi.fn(),
  makeCommandId: vi.fn(() => "request-id"),
  openWatch: vi.fn(),
  nextUpdates: vi.fn(),
  closeWatch: vi.fn(),
  storage: new Map<string, string>(),
  nativeTranscript: false,
}));
vi.mock("react-native", () => ({
  AppState: { currentState: "active", addEventListener: () => ({ remove() {} }) },
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  ScrollView: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Pressable: ({ children, onPress, disabled }: { children?: ReactNode; onPress: () => void; disabled?: boolean }) =>
    <button onClick={onPress} disabled={disabled}>{children}</button>,
  StyleSheet: { absoluteFill: {} },
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
    catalog: { projects: [{ id: "project" }], monos: [{ id: "one", name: "Mono", mascot: "cat", color: "blue" }], sessions: [] },
    selection: { projectIds: ["project"], monoIds: [] },
    command: link.command,
    refresh: link.refresh,
  }),
}));
vi.mock("@/features/connection/nativeLink", () => ({
  get hasNativeTranscript() { return link.nativeTranscript; },
  makeCommandId: link.makeCommandId,
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

// The Mono route is currently hidden by a rollout flag. Enable its catalog
// selection here without replacing the provider, approval card, or actions.
vi.mock("@/features/connection/selection", () => ({ selectedWorkspace: (catalog: unknown) => catalog }));
vi.mock("expo-router", () => ({ useLocalSearchParams: () => ({ monoId: "one" }), Redirect: () => null }));
vi.mock("@/features/connection/ConnectionNotice", () => ({ ConnectionNotice: () => null }));
vi.mock("@/features/mascots/PixelMascot", () => ({ PixelMascot: () => null }));
vi.mock("@/features/sessions/Composer", () => ({ Composer: () => null }));
vi.mock("@/features/sessions/EmptySession", () => ({ EmptySession: () => null }));
vi.mock("@/features/sessions/QuestionCard", () => ({ QuestionCard: () => null }));
vi.mock("@/features/sessions/SessionScreen", () => ({ SessionScreen: ({ children, composer }: { children: ReactNode; composer: ReactNode }) => <>{children}{composer}</> }));
vi.mock("@/features/sessions/Transcript", () => ({ Transcript: () => null }));
vi.mock("@/features/sessions/NativeTranscript", () => ({ NativeTranscript: () => null }));
vi.mock("@/ui/LiquidView", () => ({ LiquidView: () => null }));
vi.mock("@/ui/theme", () => ({ radii: { card: 22 }, themedStyles: () => () => new Proxy({}, { get: () => ({}) }) }));

describe("Mono approvals through the real workspace provider and card", () => {
  it.each([false, true])("sends the routed command and keeps a replacement after a delayed reply (native: %s)", async (native) => {
    vi.clearAllMocks();
    link.nativeTranscript = native;
    vi.stubGlobal("__DEV__", false);
    link.openWatch.mockResolvedValue(1);
    link.command.mockResolvedValue({});
    let update = deferred<object[]>();
    link.nextUpdates.mockImplementation(() => { update = deferred<object[]>(); return update.promise; });
    let workspace!: ReturnType<typeof useWorkspace>;
    function Probe() { workspace = useWorkspace(); return null; }
    const host = document.createElement("div");
    const root = createRoot(host);
    const prompt: ApprovalPrompt = { token: "first", requestId: 7, title: "Run?", detail: "git diff",
      decisions: [{ id: "allow", label: "Allow once" }] };
    const show = async (prompts: ApprovalPrompt[]) => act(async () => {
      if (native) workspace.nativeTranscriptStatus("mono:one", {
        busy: true, hasTurns: true, userCount: 1, pendingId: null, error: null, approvals: prompts,
      });
      else { update.resolve([{ approvals: prompts }]); await Promise.resolve(); }
    });
    try {
      await act(async () => root.render(<WorkspaceProvider><Probe /><MonoScreen /></WorkspaceProvider>));
      await show([prompt]);
      expect(host.textContent).toContain("git diff");
      const response = deferred<object>();
      link.command.mockReturnValueOnce(response.promise);
      act(() => host.querySelector("button")!.click());
      expect(link.command).toHaveBeenCalledWith("session.respondApproval", {
        conversationId: "mono:one", requestId: 7, token: "first", decision: "allow",
      }, "request-id");
      await show([{ ...prompt, token: "replacement", detail: "new command" }]);
      await act(async () => { response.resolve({}); });
      expect(host.textContent).toContain("new command");
      expect(host.querySelector("button")!.disabled).toBe(false);
    } finally {
      act(() => root.unmount());
      vi.unstubAllGlobals();
    }
  });
});
