import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import {
  WorkspaceLine,
  WorkspaceMenu,
} from "@/features/workspace/WorkspaceMenu.android";
import type { ChangedFile } from "@/features/workspace/types";

vi.mock("react-native-svg", () => ({
  default: () => <svg />,
  Path: () => null,
}));
vi.mock("react-native", () => ({
  useWindowDimensions: () => ({ height: 800 }),
  ActivityIndicator: () => <i data-spinner />,
  Modal: ({ visible, children }: { visible: boolean; children?: ReactNode }) =>
    visible ? <section>{children}</section> : null,
  ScrollView: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Pressable: ({
    children,
    onPress,
    disabled,
    style,
    accessibilityLabel,
  }: {
    children?: ReactNode;
    onPress?: () => void;
    disabled?: boolean;
    style?: object;
    accessibilityLabel?: string;
  }) => (
    <button
      disabled={disabled}
      style={
        Array.isArray(style)
          ? Object.assign({}, ...style.filter(Boolean))
          : style
      }
      aria-label={accessibilityLabel}
      onClick={onPress}
    >
      {children}
    </button>
  ),
  View: ({ children, style }: { children?: ReactNode; style?: object }) => (
    <div
      style={
        Array.isArray(style)
          ? Object.assign({}, ...style.filter(Boolean))
          : style
      }
    >
      {children}
    </div>
  ),
  Text: ({
    children,
    style,
    ellipsizeMode,
    numberOfLines,
    accessibilityLabel,
  }: {
    children?: ReactNode;
    style?: object;
    ellipsizeMode?: string;
    numberOfLines?: number;
    accessibilityLabel?: string;
  }) => (
    <span
      aria-label={accessibilityLabel}
      style={
        Array.isArray(style)
          ? Object.assign({}, ...style.filter(Boolean))
          : style
      }
      data-ellipsis={ellipsizeMode}
      data-lines={numberOfLines}
    >
      {children}
    </span>
  ),
}));
vi.mock("react-native-safe-area-context", () => ({
  useSafeAreaInsets: () => ({ top: 24, bottom: 16 }),
}));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/theme", () => ({
  useColors: () => ({ content: "white", muted: "gray" }),
  themedStyles: (create: (colors: object) => object) => () =>
    create({ content: "white", muted: "gray" }),
}));
vi.mock("@/features/changes/diffTheme", () => ({
  formatCount: String,
  useDiffTheme: () => ({ add: { fg: "green" }, del: { fg: "red" } }),
}));

it("bounds long branch labels in empty-session and chat workspace lines while preserving counts", () => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  const container = document.createElement("div");
  const root = createRoot(container);
  const branch = "feat/dispatch_and_benchmarks_and_a_very_long_branch_suffix";
  const files: ChangedFile[] = [
    {
      relative: "app.tsx",
      status: "modified",
      additions: 12,
      deletions: 3,
      lines: [],
    },
  ];
  try {
    for (const empty of [true, false]) {
      act(() =>
        root.render(
          empty ? (
            <WorkspaceMenu
              value={{ mode: "current" }}
              worktrees={[]}
              onChange={() => {}}
              branch={branch}
              files={files}
              loading={false}
              onOpenChanges={() => {}}
            />
          ) : (
            <WorkspaceLine current branch={branch} files={files} />
          ),
        ),
      );
      const label = container.querySelector<HTMLElement>(
        `[aria-label="${branch}"]`,
      )!;
      expect(label.textContent).toBe(branch);
      expect(label.dataset.ellipsis).toBe("middle");
      expect(label.dataset.lines).toBe("1");
      expect(label.style.maxWidth).toBe("160px");
      expect(label.style.flexShrink).toBe("1");
      expect(label.parentElement!.style.maxWidth).toBe("100%");
      expect(container.textContent).toContain("1 files+12-3");
      if (empty)
        expect(container.querySelector("button")!.style.maxWidth).toBe("100%");
    }
  } finally {
    act(() => root.unmount());
  }
});

it("keeps worktree selection available while changes load and replaces the pending summary when ready", () => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  const container = document.createElement("div");
  const root = createRoot(container);
  const onChange = vi.fn();
  const render = (loading: boolean) =>
    act(() =>
      root.render(
        <WorkspaceMenu
          value={{ mode: "current" }}
          worktrees={[]}
          onChange={onChange}
          branch="main"
          files={[]}
          loading={loading}
          onOpenChanges={() => {}}
        />,
      ),
    );
  try {
    render(true);
    expect(container.textContent).toContain("Loading changes…");
    expect(container.textContent).not.toContain("0 files");
    const trigger = container.querySelector<HTMLButtonElement>(
      '[aria-label="Workspace: main"]',
    )!;
    expect(trigger.disabled).toBe(false);
    act(() => trigger.click());
    const worktree = [...container.querySelectorAll("button")].find(
      (item) => item.getAttribute("aria-label") === "New worktree",
    )!;
    act(() => worktree.click());
    expect(onChange).toHaveBeenCalledWith({ mode: "worktree" });
    render(false);
    expect(container.textContent).not.toContain("Loading changes…");
    expect(container.textContent).toContain("0 files+0-0");
  } finally {
    act(() => root.unmount());
  }
});

it("groups Workspace options and opens existing worktrees as a separate page", () => {
  const container = document.createElement("div");
  const root = createRoot(container);
  const onChange = vi.fn();
  const trees = [
    { name: "mc/orch-first", path: "/repo/.worktrees/first" },
    { name: "mc/orch-second", path: "/repo/.worktrees/second" },
  ];
  const button = (label: string) =>
    container.querySelector<HTMLButtonElement>(`[aria-label="${label}"]`)!;
  const click = (label: string) => act(() => button(label).click());
  try {
    act(() =>
      root.render(
        <WorkspaceMenu
          value={{ mode: "current" }}
          worktrees={trees}
          onChange={onChange}
          branch="main"
          files={[]}
          loading={false}
          onOpenChanges={() => {}}
        />,
      ),
    );
    click("Workspace: main");
    expect(container.textContent).toContain("Start the session in");
    expect(container.textContent).toContain("The project's own folder");
    expect(container.textContent).toContain(
      "A fresh branch, in a folder of its own",
    );
    expect(container.textContent).toContain("2 worktrees");
    expect(button("mc/orch-first")).toBeNull();
    click("Existing worktree");
    expect(container.textContent).toContain("Worktrees");
    expect(container.textContent).toContain(trees[1].path);
    expect(button("Current checkout")).toBeNull();
    click("Back to Workspace");
    expect(button("Current checkout")).not.toBeNull();
    click("Existing worktree");
    click("mc/orch-second");
    expect(onChange).toHaveBeenCalledWith({
      mode: "existing",
      worktree: trees[1],
    });
    expect(container.querySelector("section")).toBeNull();
    click("Workspace: main");
    expect(button("Current checkout")).not.toBeNull();
    click("Close workspace");
    expect(container.querySelector("section")).toBeNull();
  } finally {
    act(() => root.unmount());
  }
});

it("disables changes for an uncreated worktree and dismisses before opening changes", () => {
  const container = document.createElement("div");
  const root = createRoot(container);
  const onOpenChanges = vi.fn();
  const frame = vi.fn();
  vi.stubGlobal("requestAnimationFrame", frame);
  const render = (mode: "current" | "worktree") =>
    act(() =>
      root.render(
        <WorkspaceMenu
          value={{ mode }}
          worktrees={[]}
          onChange={() => {}}
          branch="main"
          files={[]}
          loading={false}
          onOpenChanges={onOpenChanges}
        />,
      ),
    );
  const click = (label: string) =>
    act(() =>
      container
        .querySelector<HTMLButtonElement>(`[aria-label="${label}"]`)!
        .click(),
    );
  try {
    render("worktree");
    click("Workspace: main");
    expect(
      container.querySelector<HTMLButtonElement>('[aria-label="View changes"]')!
        .disabled,
    ).toBe(true);
    expect(container.textContent).toContain(
      "Changes are available after the worktree is created.",
    );
    render("current");
    click("View changes");
    expect(container.querySelector("section")).toBeNull();
    expect(onOpenChanges).not.toHaveBeenCalled();
    act(() => frame.mock.calls[0][0]());
    expect(onOpenChanges).toHaveBeenCalledOnce();
  } finally {
    act(() => root.unmount());
    vi.unstubAllGlobals();
  }
});
