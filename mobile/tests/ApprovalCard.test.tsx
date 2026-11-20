import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ApprovalCard } from "@/features/sessions/ApprovalCard";
import type { ApprovalPrompt } from "@/features/workspace/types";

const workspace = vi.hoisted(() => ({
  respondApproval: vi.fn(),
  keepQuestionOpen: vi.fn(),
  alert: vi.fn(),
}));
vi.mock("react-native", () => {
  const View = ({ children }: { children?: ReactNode }) => (
    <div>{children}</div>
  );
  return {
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
    View: ({
      children,
      onTouchStart,
    }: {
      children?: ReactNode;
      onTouchStart?: () => void;
    }) => <section onPointerDown={onTouchStart}>{children}</section>,
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
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => workspace,
}));

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

const prompt: ApprovalPrompt = {
  token: "first",
  requestId: 7,
  title: "Run a command?",
  detail: "rm -rf build",
  decisions: [
    { id: "deny", label: "Deny" },
    { id: "session", label: "Allow for this session" },
    { id: "allow", label: "Allow once" },
  ],
};
describe("mobile approval card", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const render = (prompts: ApprovalPrompt[] = [prompt]) =>
    act(() =>
      root.render(<ApprovalCard conversationId="mono:one" prompts={prompts} />),
    );
  const button = (label: string) =>
    Array.from(host.querySelectorAll("button")).find(
      (b) => b.textContent === label,
    )!;
  beforeEach(() => {
    vi.clearAllMocks();
    workspace.respondApproval.mockResolvedValue(undefined);
    host = document.createElement("div");
    root = createRoot(host);
  });
  afterEach(() => act(() => root.unmount()));
  it("shows context and desktop-supported decisions, submits once, and allows retry after failure", async () => {
    let reject!: (reason: Error) => void;
    workspace.respondApproval.mockImplementationOnce(
      () =>
        new Promise((_, fail) => {
          reject = fail;
        }),
    );
    render();
    expect(host.textContent).toContain("rm -rf build");
    act(() => {
      button("Allow for this session").click();
      button("Allow once").click();
    });
    expect(workspace.respondApproval).toHaveBeenCalledExactlyOnceWith(
      "mono:one",
      prompt,
      "session",
    );
    expect(button("Deny").disabled).toBe(true);
    await act(async () => reject(new Error("Connection lost")));
    expect(host.textContent).toContain("Decision not sent: Connection lost");
    expect(host.textContent).toContain("rm -rf build");
    expect(button("Allow once").disabled).toBe(false);
    await act(async () => button("Deny").click());
    expect(workspace.respondApproval).toHaveBeenLastCalledWith(
      "mono:one",
      prompt,
      "deny",
    );
  });
  it("starts a replacement card independently while an earlier response is pending", async () => {
    let resolve!: () => void;
    workspace.respondApproval.mockImplementationOnce(
      () =>
        new Promise<void>((done) => {
          resolve = done;
        }),
    );
    render();
    act(() => button("Allow once").click());
    render([{ ...prompt, token: "replacement", detail: "git diff" }]);
    expect(button("Allow once").disabled).toBe(false);
    await act(async () => resolve());
    expect(host.textContent).toContain("git diff");
    await act(async () => button("Deny").click());
    expect(workspace.respondApproval).toHaveBeenLastCalledWith(
      "mono:one", expect.objectContaining({ token: "replacement" }), "deny",
    );
  });
  it("lets the reader inspect other pending approvals even when the first is unsupported", async () => {
    render([
      { ...prompt, decisions: [], unsupported: "Use desktop" },
      { ...prompt, token: "second", detail: "git status" },
    ]);
    act(() => button("Next").click());
    expect(host.textContent).toContain("git status");
    await act(async () => button("Allow once").click());
    expect(workspace.respondApproval.mock.calls[0][1].token).toBe("second");
  });
  it("explains unsupported requests without exposing decisions", () => {
    render([
      { ...prompt, decisions: [], unsupported: "Respond on the desktop" },
    ]);
    expect(host.textContent).toContain("Respond on the desktop");
    expect(host.querySelectorAll("button")).toHaveLength(0);
  });
});
