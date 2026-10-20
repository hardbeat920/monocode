import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  SessionTitle,
  SessionToolbar,
} from "@/features/sessions/SessionToolbar";
import type { SessionSummary } from "@/features/workspace/types";

const app = vi.hoisted(() => ({
  os: "android",
  push: vi.fn(),
  back: vi.fn(),
  copy: vi.fn(),
  alert: vi.fn(),
  archive: vi.fn(async () => {}),
  remove: vi.fn(async () => {}),
  rename: vi.fn(async () => {}),
}));
vi.mock("@/ui/LiquidView", () => ({
  LiquidView: ({ children, innerStyle }: { children: ReactNode; innerStyle: object }) => <div data-glass style={innerStyle}>{children}</div>,
}));
vi.mock("expo-router", () => ({
  router: { push: app.push, back: app.back },
  Stack: {
    Toolbar: () => {
      throw new Error("Android must not render native iOS toolbar items");
    },
  },
}));
vi.mock("expo-clipboard", () => ({ setStringAsync: app.copy }));
vi.mock("expo-haptics", () => ({
  notificationAsync: vi.fn(),
  NotificationFeedbackType: { Success: "success" },
}));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({
    archiveSession: app.archive,
    deleteSession: app.remove,
    renameSession: app.rename,
  }),
}));
vi.mock("@/features/sessions/useSessionActions", () => ({
  useSessionActions: () => ({ rename: vi.fn() }),
}));
vi.mock("react-native-safe-area-context", () => ({
  useSafeAreaInsets: () => ({ bottom: 0 }),
}));
vi.mock("react-native-svg", () => ({
  default: () => <svg />,
  Circle: () => null,
  Path: () => null,
}));
vi.mock("react-native", () => ({
  Platform: {
    get OS() {
      return app.os;
    },
  },
  useWindowDimensions: () => ({ width: 320 }),
  Alert: { alert: app.alert },
  View: ({ children, style }: { children?: ReactNode; style?: object }) => (
    <div style={style}>{children}</div>
  ),
  Text: ({
    children,
    style,
    numberOfLines,
    ellipsizeMode,
  }: {
    children?: ReactNode;
    style?: object;
    numberOfLines?: number;
    ellipsizeMode?: string;
  }) => (
    <span
      style={style}
      data-lines={numberOfLines}
      data-ellipsis={ellipsizeMode}
    >
      {children}
    </span>
  ),
  Pressable: ({
    children,
    onPress,
    disabled,
    accessibilityLabel,
  }: {
    children?: ReactNode;
    onPress?: () => void;
    disabled?: boolean;
    accessibilityLabel?: string;
  }) => (
    <button
      onClick={onPress}
      disabled={disabled}
      aria-label={accessibilityLabel}
    >
      {children}
    </button>
  ),
  Modal: ({ visible, children }: { visible: boolean; children?: ReactNode }) =>
    visible ? <section role="dialog">{children}</section> : null,
  ScrollView: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  TextInput: ({
    value,
    onChangeText,
    accessibilityLabel,
  }: {
    value: string;
    onChangeText: (value: string) => void;
    accessibilityLabel: string;
  }) => (
    <input
      aria-label={accessibilityLabel}
      value={value}
      onInput={(e) => onChangeText(e.currentTarget.value)}
    />
  ),
}));
vi.mock("@/ui/theme", () => {
  const colors = {
    content: "white",
    muted: "gray",
    fill: "gray",
    sheet: "black",
  };
  return {
    useColors: () => colors,
    themedStyles: (fn: (colors: object) => object) => () => fn(colors),
  };
});
let container: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const settings = vi.fn();
const session = {
  id: "session",
  title: "Prepare Mobile Upstream",
  branch: "android-fixes",
} as SessionSummary;
const render = (changed = 14, target: SessionSummary | undefined = session) =>
  act(() =>
    root.render(
      <>
        <SessionTitle
          title={session.title}
          subtitle="monocode · android-fixes"
        />
        <SessionToolbar
          projectId="project"
          sessionId="session"
          session={target}
          changed={changed}
          worktreePath="/repo/worktree"
          harness="codex"
          onSettings={settings}
        />
      </>,
    ),
  );
const button = (label: string) =>
  [...container.querySelectorAll("button")].find(
    (item) =>
      item.getAttribute("aria-label") === label || item.textContent === label,
  )!;
const click = async (label: string) => act(async () => button(label).click());
beforeEach(() => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  vi.clearAllMocks();
  container = document.createElement("div");
  root = createRoot(container);
});
afterEach(() => act(() => root.unmount()));

it("keeps the title start visible with end truncation and no iOS negative margin/fixed width", () => {
  render();
  const title = [...container.querySelectorAll("span")].find(
    (item) => item.textContent === session.title,
  )!;
  expect(title.dataset.lines).toBe("1");
  expect(title.dataset.ellipsis).toBe("tail");
  expect(title.parentElement!.style.marginLeft).toBe("");
  expect(title.parentElement!.style.width).toBe("");
  expect(title.parentElement!.style.minWidth).toBe("0");
  const subtitle = [...container.querySelectorAll("span")].find(
    (item) => item.textContent === "monocode · android-fixes",
  )!;
  expect(subtitle.dataset.ellipsis).toBe("middle");
});
it("shows changes/count and opens the correct working copy", async () => {
  render();
  const badgeText = [...container.querySelectorAll("span")].find(
    (item) => item.textContent === "14",
  )!;
  const badge = badgeText.parentElement!;
  expect(badge.style.top).toBe("0px");
  // The badge sits above the pill but stays within the measured header wrapper.
  const toolbar = button("Changes, 14 files").parentElement!.parentElement!;
  expect(badge.parentElement).toBe(toolbar);
  expect(button("Changes, 14 files").parentElement!.contains(badge)).toBe(
    false,
  );
  expect(toolbar.style.paddingTop).toBe("6px");
  expect(button("Changes, 14 files").parentElement!.hasAttribute("data-glass")).toBe(true);
  await click("Changes, 14 files");
  expect(app.push).toHaveBeenCalledWith({
    pathname: "/changes",
    params: {
      projectId: "project",
      worktreePath: "/repo/worktree",
      harness: "codex",
    },
  });
  render(0);
  expect(button("Changes, 0 files").textContent).toBe("");
});
it("opens the ellipsis menu and retains copy and settings", async () => {
  render();
  await click("Session actions");
  for (const label of [
    "Rename",
    "Copy Session ID",
    "Settings",
    "Archive",
    "Delete",
  ])
    expect(button(label)).toBeDefined();
  await click("Copy Session ID");
  expect(app.copy).toHaveBeenCalledWith("session");
  await click("Session actions");
  await click("Settings");
  expect(settings).toHaveBeenCalledOnce();
});
it("renames on Android without the iOS-only Alert.prompt", async () => {
  render();
  await click("Session actions");
  await click("Rename");
  const input = container.querySelector("input")!;
  act(() => {
    input.value = "New name";
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await click("Rename");
  expect(app.rename).toHaveBeenCalledWith(session, "New name");
});
it("keeps archive/delete confirmations and returns before removing the session", async () => {
  render();
  for (const action of ["Archive", "Delete"]) {
    await click("Session actions");
    await click(action);
    const buttons = app.alert.mock.lastCall![2] as {
      text: string;
      onPress?: () => void;
    }[];
    expect(app.archive).not.toHaveBeenCalled();
    expect(app.remove).not.toHaveBeenCalled();
    await act(async () =>
      buttons.find((item) => item.text === action)!.onPress!(),
    );
    expect(app.back).toHaveBeenCalled();
    expect(
      action === "Archive" ? app.archive : app.remove,
    ).toHaveBeenCalledWith("session");
    vi.clearAllMocks();
  }
});
