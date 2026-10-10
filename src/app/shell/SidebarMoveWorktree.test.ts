// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { formatSessionTitle } from "../../features/sessions/model/session";
import type { Worktree } from "../../features/source-control/model/worktrees";
import { useProjectWorktrees } from "../../features/source-control/hooks/useProjectWorktrees";
import { Sidebar } from "./Sidebar";

vi.mock("../../features/source-control/hooks/useProjectDiffStats", () => ({
  useProjectDiffStats: vi.fn(() => null),
}));
vi.mock("../../features/source-control/hooks/useGitFileStatuses", () => ({
  useGitFileStatuses: () => ({ files: new Map(), dirs: new Map() }),
}));
vi.mock("../../features/source-control/hooks/useProjectWorktrees", () => ({
  useProjectWorktrees: vi.fn(),
}));
vi.mock("./SidebarUpdate", () => ({ SidebarUpdateFooter: () => null }));
vi.mock("../../features/files/ui/FileTree", () => ({ FileTree: () => null }));
vi.mock("./useTrafficLights", () => ({ useTrafficLights: vi.fn(() => true) }));

const tree = (path: string, branch: string, isMain = false): Worktree => ({
  path,
  branch,
  head: "abc",
  isMain,
  locked: false,
  prunable: false,
  missing: false,
  dirty: false,
  unpushed: 0,
  sessionIds: [],
});
const main = tree("/workspace/project", "main", true);
const feature = tree("/workspace/project-worktrees/feature", "feature");

let container: HTMLDivElement;
let root: Root;
let props: ComponentProps<typeof Sidebar>;

function render() {
  root.render(createElement(Sidebar, props));
}

function menuItem(scope: ParentNode, label: string) {
  return Array.from(
    scope.querySelectorAll<HTMLButtonElement>(
      '[role="menuitem"], [role="menuitemcheckbox"]',
    ),
  ).find((item) => item.textContent?.startsWith(label));
}

function openMoveMenu() {
  act(() => {
    container
      .querySelector('[data-session-card="session-1"]')!
      .dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
      );
  });
  const trigger = menuItem(document, "Move to worktree");
  if (!trigger || trigger.disabled) return { trigger, submenu: null };
  act(() => trigger.click());
  return {
    trigger,
    submenu: document.querySelector<HTMLElement>(
      '[role="menu"][aria-label="Move to worktree"]',
    ),
  };
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const stored = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
    removeItem: (key: string) => stored.delete(key),
    clear: () => stored.clear(),
  });
  vi.mocked(useProjectWorktrees).mockReturnValue({
    data: { worktrees: [main, feature], defaultRoot: "/workspace" },
    refresh: vi.fn(),
  });
  props = {
    cwd: "/workspace/project",
    open: true,
    sessions: [
      {
        id: "session-1",
        cwd: "/workspace/project",
        harness: "codex",
        model: "",
        runtimeMode: "supervised",
        title: formatSessionTitle("codex", "Build feature"),
        createdAt: Date.now(),
        updatedAt: Date.now(),
      },
    ],
    busySessionIds: new Set(),
    approvalSessionIds: new Set(),
    activeSessionId: "session-1",
    status: "idle",
    pending: false,
    tab: "sessions",
    filesSearchOpen: false,
    onSelectSession: vi.fn(),
    onMoveSessionToWorktree: vi.fn(),
  };
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => render());
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

describe("sidebar move to worktree", () => {
  it("moves the session to the picked worktree", () => {
    const { submenu } = openMoveMenu();
    const current = menuItem(submenu!, "main")!;
    expect(current.disabled).toBe(true);
    act(() => menuItem(submenu!, "feature")!.click());
    expect(props.onMoveSessionToWorktree).toHaveBeenCalledWith(
      "session-1",
      feature,
    );
  });

  it("marks the session's linked worktree as current", () => {
    props.sessions = [{ ...props.sessions[0], worktreeCwd: feature.path }];
    act(() => render());
    const { submenu } = openMoveMenu();
    expect(menuItem(submenu!, "feature")!.disabled).toBe(true);
    expect(menuItem(submenu!, "main")!.disabled).toBe(false);
  });

  it("is disabled while the session is working", () => {
    props.busySessionIds = new Set(["session-1"]);
    act(() => render());
    expect(openMoveMenu().trigger?.disabled).toBe(true);
  });

  it("is hidden when the project has a single checkout", () => {
    vi.mocked(useProjectWorktrees).mockReturnValue({
      data: { worktrees: [main], defaultRoot: "/workspace" },
      refresh: vi.fn(),
    });
    act(() => render());
    expect(openMoveMenu().trigger).toBeUndefined();
  });
});
