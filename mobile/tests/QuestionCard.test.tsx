import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useConversation, useWorkspace, WorkspaceProvider } from "@/features/workspace/WorkspaceProvider";
import { QuestionCard } from "@/features/sessions/QuestionCard";
import type { QuestionPrompt } from "@/features/workspace/types";

const workspace = vi.hoisted(() => ({
  answerQuestion: vi.fn(),
  keepQuestionOpen: vi.fn(),
  alert: vi.fn(),
  real: false,
  nativeTranscript: false,
  pairing: { desktop: { fingerprint: "desktop" } },
  command: vi.fn(),
  nextUpdates: vi.fn(),
}));
vi.mock("react-native", () => {
  const View = ({ children }: { children?: ReactNode }) => <div>{children}</div>;
  return {
    AppState: { currentState: "active", addEventListener: () => ({ remove() {} }) },
    View,
    ScrollView: View,
    Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
    TextInput: ({
      value,
      onChangeText,
      placeholder,
    }: {
      value: string;
      onChangeText: (value: string) => void;
      placeholder: string;
    }) => (
      <input
        value={value}
        placeholder={placeholder}
        onChange={(event) => onChangeText(event.target.value)}
      />
    ),
    Pressable: ({
      children,
      onPress,
      disabled,
    }: {
      children?: ReactNode;
      onPress?: () => void;
      disabled?: boolean;
    }) => (
      <button disabled={disabled} onClick={onPress}>
        {children}
      </button>
    ),
    Alert: { alert: workspace.alert },
    StyleSheet: { absoluteFill: {}, hairlineWidth: 1 },
    useWindowDimensions: () => ({ width: 390, height: 844 }),
  };
});
vi.mock("react-native-reanimated", () => ({
  default: {
    View: ({ children, onTouchStart }: { children?: ReactNode; onTouchStart?: () => void }) => (
      <section onPointerDown={onTouchStart}>{children}</section>
    ),
  },
  FadeIn: { duration: () => ({}) },
  FadeOut: { duration: () => ({}) },
}));
vi.mock("@/ui/LiquidView", () => ({ LiquidView: () => null }));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/theme", () => ({
  radii: { card: 22, row: 12, pill: 999 },
  themedStyles: () => () => new Proxy({}, { get: () => ({}) }),
  useColors: () => new Proxy({}, { get: () => "#000" }),
  useScheme: () => "dark",
}));
vi.mock("@/features/workspace/WorkspaceProvider", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/features/workspace/WorkspaceProvider")>();
  return { ...actual, useWorkspace: () => workspace.real ? actual.useWorkspace() : workspace };
});
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({
    pairing: workspace.pairing,
    catalog: { projects: [], monos: [], sessions: [] },
    selection: { projectIds: [], monoIds: [] },
    command: workspace.command,
    refresh: async () => {},
  }),
}));
vi.mock("@/features/connection/nativeLink", () => ({
  get hasNativeTranscript() { return workspace.nativeTranscript; },
  makeCommandId: () => "command",
  openWatch: async () => 1,
  nextUpdates: workspace.nextUpdates,
  closeWatch: async () => {},
}));
vi.mock("expo-sqlite/kv-store", () => ({
  default: { getItem: async () => null, setItem: async () => {}, removeItem: async () => {} },
}));
vi.mock("@/features/sessions/attachments", () => ({ toWire: () => [] }));

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean })
  .IS_REACT_ACT_ENVIRONMENT = true;

const colour = {
  id: "colour",
  prompt: "Which colour?",
  multiSelect: false,
  allowCustom: true,
  options: [
    { id: "red", label: "Red", description: "Warm" },
    { id: "blue", label: "Blue" },
  ],
};
const size = {
  id: "size",
  prompt: "Which sizes?",
  multiSelect: true,
  allowCustom: false,
  options: [
    { id: "s", label: "Small" },
    { id: "l", label: "Large" },
  ],
};

describe("question card", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const render = (prompt: QuestionPrompt) =>
    act(() => root.render(<QuestionCard conversationId="session" prompt={{ ...prompt, displayId: 1 }} />));
  const button = (label: string) =>
    [...host.querySelectorAll("button")].find((element) => element.textContent === label)!;
  const press = (label: string) => act(async () => { button(label).click(); });

  beforeEach(() => {
    vi.clearAllMocks();
    workspace.real = false;
    workspace.nativeTranscript = false;
    vi.stubGlobal("__DEV__", false);
    workspace.answerQuestion.mockResolvedValue(undefined);
    workspace.keepQuestionOpen.mockResolvedValue(undefined);
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(() => {
    act(() => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  });

  it.each([false, true])("resets the whole card for a reused ID (native: %s)", async (native) => {
    workspace.nativeTranscript = native;
    workspace.real = true;
    workspace.command.mockResolvedValue(undefined);
    let deliver!: (frames: object[]) => void;
    workspace.nextUpdates.mockImplementation(() => new Promise<object[]>((resolve) => { deliver = resolve; }));
    let provider!: ReturnType<typeof useWorkspace>;
    function ConversationCard() {
      provider = useWorkspace();
      const { question } = useConversation("session");
      return question ? <QuestionCard conversationId="session" prompt={question} /> : null;
    }
    await act(async () => root.render(<WorkspaceProvider><ConversationCard /></WorkspaceProvider>));
    const prompt = { requestId: 4, questions: [colour, size], autoResolveAt: Date.now() + 30_000 };
    const status = { busy: true, error: null, hasTurns: false, userCount: 0, pendingId: null };
    await act(async () => {
      if (native) provider.nativeTranscriptStatus("session", { ...status, question: prompt, questionGeneration: 1 });
      else deliver([{ question: prompt }]);
    });
    const touch = () => host.querySelector("section")!.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    await act(async () => { touch(); });
    await press("Other");
    await act(async () => {
      const input = host.querySelector("input")!;
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "Teal");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await press("Continue");
    await press("Small");
    expect(host.textContent).toContain("2 of 2");

    // Refreshing the same display must preserve the user's progress.
    await act(async () => {
      const refreshed = { ...prompt, autoResolveAt: undefined };
      if (native) provider.nativeTranscriptStatus("session", { ...status, question: refreshed, questionGeneration: 1 });
      else deliver([{ question: refreshed }]);
    });
    expect(host.textContent).toContain("2 of 2");
    expect(button("Send").disabled).toBe(false);

    await act(async () => {
      if (native) provider.nativeTranscriptStatus("session", { ...status, question: prompt, questionGeneration: 3 });
      else deliver([{ question: null }, { question: prompt }]);
    });
    expect(host.textContent).toContain("1 of 2");
    expect(button("Continue").disabled).toBe(true);
    expect(host.querySelector("input")).toBeNull();
    await press("Other");
    expect(host.querySelector("input")!.value).toBe("");
    await press("Skip");
    expect(button("Send").disabled).toBe(true);
    await act(async () => { touch(); });
    expect(workspace.command.mock.calls.filter(([method]) => method === "session.keepQuestionOpen")).toHaveLength(2);
  });

  it("sends the chosen option once one is picked", async () => {
    render({ requestId: 1, questions: [colour] });
    expect(button("Send").disabled).toBe(true);
    await press("RedWarm");
    await press("Send");
    expect(workspace.answerQuestion).toHaveBeenCalledWith("session", 1, {
      kind: "answered",
      answers: { colour: ["red"] },
    });
  });

  it("steps through questions, sending only the ones answered", async () => {
    render({ requestId: 2, questions: [colour, size] });
    await press("Skip");
    await press("Small");
    await press("Large");
    await press("Small");
    await press("Send");
    expect(workspace.answerQuestion).toHaveBeenCalledWith("session", 2, {
      kind: "answered",
      answers: { size: ["l"] },
    });
  });

  it("needs words for Other before it can send them", async () => {
    render({ requestId: 3, questions: [colour] });
    await press("Other");
    expect(button("Send").disabled).toBe(true);
    const input = host.querySelector("input")!;
    await act(async () => {
      const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
      setValue.call(input, " Teal ");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await press("Send");
    expect(workspace.answerQuestion).toHaveBeenCalledWith("session", 3, {
      kind: "answered",
      answers: { colour: ["__custom__"] },
      custom: { colour: "Teal" },
    });
  });

  it("keeps a timed question open on the first touch, and says when a send fails", async () => {
    render({ requestId: 4, questions: [colour], autoResolveAt: Date.now() + 30_000 });
    const card = host.querySelector("section")!;
    act(() => card.dispatchEvent(new Event("pointerdown", { bubbles: true })));
    act(() => card.dispatchEvent(new Event("pointerdown", { bubbles: true })));
    expect(workspace.keepQuestionOpen).toHaveBeenCalledTimes(1);
    expect(workspace.keepQuestionOpen).toHaveBeenCalledWith("session", 4);

    workspace.answerQuestion.mockRejectedValue(new Error("This question was already answered"));
    await press("Skip");
    expect(workspace.answerQuestion).toHaveBeenCalledWith("session", 4, { kind: "skipped" });
    expect(workspace.alert).toHaveBeenCalledWith("Answer not sent", "This question was already answered");
  });

  it("retries keeping a question open after failure, suppressing duplicates during and after success", async () => {
    let reject!: (reason: Error) => void;
    workspace.keepQuestionOpen.mockReturnValueOnce(new Promise<void>((_, fail) => { reject = fail; }));
    render({ requestId: 4, questions: [colour], autoResolveAt: Date.now() + 30_000 });
    const touch = () => host.querySelector("section")!.dispatchEvent(new Event("pointerdown", { bubbles: true }));
    act(() => { touch(); touch(); });
    expect(workspace.keepQuestionOpen).toHaveBeenCalledTimes(1);

    await act(async () => { reject(new Error("Connection lost")); });
    let resolve!: () => void;
    workspace.keepQuestionOpen.mockReturnValueOnce(new Promise<void>((done) => { resolve = done; }));
    act(() => { touch(); touch(); });
    expect(workspace.keepQuestionOpen).toHaveBeenCalledTimes(2);
    await act(async () => { resolve(); });
    act(() => touch());
    expect(workspace.keepQuestionOpen).toHaveBeenCalledTimes(2);
    expect(workspace.keepQuestionOpen).toHaveBeenLastCalledWith("session", 4);
  });
});
