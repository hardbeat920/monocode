import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { TerminalActionsSheet } from "@/features/terminal/TerminalActionsSheet";

const desktop = vi.hoisted(() => ({
  request: vi.fn(),
  refresh: vi.fn(),
  actions: [] as {
    id: string;
    name: string;
    command: string;
    icon: string;
  }[],
}));
vi.mock("@/features/connection/nativeLink", () => ({
  request: desktop.request,
}));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({ pairing: {}, refresh: desktop.refresh }),
}));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({ projects: [{ id: "p", actions: desktop.actions }] }),
}));
vi.mock("@expo/ui", () => ({
  BottomSheet: ({ children }: { children?: ReactNode }) => (
    <div>{children}</div>
  ),
  RNHostView: ({ children }: { children?: ReactNode }) => <>{children}</>,
}));
vi.mock("react-native", async () => {
  const { forwardRef } = await import("react");
  type Props = {
    children?: ReactNode;
    accessibilityLabel?: string;
    onPress?: () => void;
    disabled?: boolean;
  };
  return {
    View: ({ children }: Props) => <div>{children}</div>,
    Text: ({ children }: Props) => <span>{children}</span>,
    Pressable: ({ children, accessibilityLabel, onPress, disabled }: Props) => (
      <button
        aria-label={accessibilityLabel}
        disabled={disabled}
        onClick={onPress}
      >
        {children}
      </button>
    ),
    TextInput: forwardRef<
      HTMLInputElement,
      {
        value: string;
        onChangeText: (text: string) => void;
        onSubmitEditing?: () => void;
        accessibilityLabel?: string;
      }
    >(function TextInput(
      { value, onChangeText, onSubmitEditing, accessibilityLabel },
      ref,
    ) {
      return (
        <input
          ref={ref}
          aria-label={accessibilityLabel}
          value={value}
          onChange={(event) => onChangeText(event.target.value)}
          onKeyDown={(event) => event.key === "Enter" && onSubmitEditing?.()}
        />
      );
    }),
    useWindowDimensions: () => ({ width: 390, height: 844 }),
  };
});
vi.mock("react-native-safe-area-context", () => ({
  useSafeAreaInsets: () => ({ bottom: 34 }),
}));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/theme", () => {
  const colors = {
    content: "white",
    muted: "gray",
    fill: "gray",
    sheet: "black",
    accent: "blue",
  };
  return {
    useColors: () => colors,
    themedStyles: (create: (value: typeof colors) => object) => () =>
      create(colors),
  };
});

let container: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const render = (isPresented = true) =>
  act(() =>
    root.render(
      <TerminalActionsSheet
        projectId="p"
        projectName="monocode"
        isPresented={isPresented}
        onDismiss={() => {}}
      />,
    ),
  );
const field = (label: string) =>
  container.querySelector<HTMLInputElement>(`input[aria-label="${label}"]`)!;
const button = (label: string) =>
  container.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!;
// Typed as React sees it: through the input's value setter, then an input event.
const type = (label: string, text: string) =>
  act(() => {
    const input = field(label);
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(input, text);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
const sent = () =>
  desktop.request.mock.calls.map(([, method, params]) => ({
    method,
    ...params,
  }));

beforeEach(() => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  desktop.request.mockReset().mockResolvedValue({});
  desktop.refresh.mockReset().mockResolvedValue(undefined);
  desktop.actions = [
    { id: "dev", name: "Run Dev", command: "npm run dev", icon: "rocket" },
    { id: "test", name: "Test", command: "cargo test", icon: "test-tube" },
  ];
  container = document.createElement("div");
  root = createRoot(container);
});
afterEach(() => act(() => root.unmount()));

it("lists the desktop's actions, and reads them only while it is open", () => {
  render(false);
  expect(container.textContent).not.toContain("Run Dev");
  render();
  expect(container.textContent).toContain("Run Dev");
  expect(container.textContent).toContain("cargo test");
});

it("adds an action, trimmed, after the desktop's, and sends the whole list", async () => {
  render();
  expect(button("Add action").disabled).toBe(true);
  type("New action's name", "  Lint ");
  expect(button("Add action").disabled).toBe(true);
  type("New action's command", " cargo clippy ");
  await act(async () => button("Add action").click());
  expect(sent()).toEqual([
    {
      method: "terminal.actions.save",
      projectId: "p",
      actions: [
        ...desktop.actions,
        { name: "Lint", command: "cargo clippy", icon: "play" },
      ],
    },
  ]);
  expect(desktop.refresh).toHaveBeenCalled();
  expect(field("New action's name").value).toBe("");
  expect(field("New action's command").value).toBe("");
});

it("adds from the keyboard's return key on the command", async () => {
  render();
  type("New action's name", "Build");
  type("New action's command", "cargo build");
  await act(async () =>
    field("New action's command").dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    ),
  );
  expect(sent()[0].actions.at(-1)).toEqual({
    name: "Build",
    command: "cargo build",
    icon: "play",
  });
});

it("removes an action, keeping the others' IDs so the desktop keeps them", async () => {
  render();
  await act(async () => button("Remove Run Dev").click());
  expect(sent()).toEqual([
    {
      method: "terminal.actions.save",
      projectId: "p",
      actions: [desktop.actions[1]],
    },
  ]);
});

it("says why the desktop refused a change, and shows its list again", async () => {
  desktop.request.mockRejectedValue(
    new Error("Give each action a name and a command"),
  );
  render();
  await act(async () => button("Remove Test").click());
  expect(container.textContent).toContain(
    "Give each action a name and a command",
  );
  expect(container.textContent).toContain("Test");
});

it("says there are none yet", () => {
  desktop.actions = [];
  render();
  expect(container.textContent).toContain("No actions yet");
});
