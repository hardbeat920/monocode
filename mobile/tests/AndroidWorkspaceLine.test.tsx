import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import {
  WorkspaceLine,
  WorkspaceMenu,
} from "@/features/workspace/WorkspaceMenu.android";
import type { ChangeSummary } from "@/features/workspace/types";

vi.mock("react-native", () => ({
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
      style={style}
      aria-label={accessibilityLabel}
      onClick={onPress}
    >
      {children}
    </button>
  ),
  View: ({ children, style }: { children?: ReactNode; style?: object }) => (
    <div style={style}>{children}</div>
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
      style={style}
      data-ellipsis={ellipsizeMode}
      data-lines={numberOfLines}
    >
      {children}
    </span>
  ),
}));
vi.mock("react-native-safe-area-context", () => ({
  useSafeAreaInsets: () => ({ bottom: 0 }),
}));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/theme", () => ({
  useColors: () => ({ content: "white", muted: "gray" }),
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
  const summary: ChangeSummary = { files: 1, staged: 0, additions: 12, deletions: 3 };
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
              summary={summary}
              loading={false}
              onOpenChanges={() => {}}
            />
          ) : (
            <WorkspaceLine current branch={branch} summary={summary} />
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
          summary={{ files: 0, staged: 0, additions: 0, deletions: 0 }}
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
      (item) => item.textContent === "New worktree",
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
