// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { formatSessionTitle } from "../../features/sessions/model/session";
import { Sidebar } from "./Sidebar";

// Keep native services out of these layout tests.
vi.mock("../../features/source-control/hooks/useProjectDiffStats", () => ({
  useProjectDiffStats: vi.fn(() => null),
}));
vi.mock("../../features/source-control/hooks/useGitFileStatuses", () => ({
  useGitFileStatuses: () => ({ files: new Map(), dirs: new Map() }),
}));
vi.mock("./SidebarUpdate", () => ({ SidebarUpdateFooter: () => null }));
vi.mock("../../features/files/ui/FileTree", () => ({ FileTree: () => null }));

let container: HTMLDivElement;
let root: Root;
let props: ComponentProps<typeof Sidebar>;

function render(patch: Partial<ComponentProps<typeof Sidebar>> = {}) {
  props = { ...props, ...patch };
  act(() => root.render(createElement(Sidebar, props)));
}

function button(label: string): HTMLButtonElement | undefined {
  return Array.from(container.querySelectorAll("button")).find(
    (item) =>
      item.getAttribute("aria-label") === label ||
      item.textContent?.trim() === label,
  );
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
  props = {
    cwd: "~",
    open: true,
    sessions: [
      {
        id: "chat-1",
        cwd: "~",
        harness: "codex",
        model: "",
        runtimeMode: "supervised",
        title: formatSessionTitle("codex", "Quick question"),
        createdAt: Date.now(),
        updatedAt: Date.now(),
      },
    ],
    busySessionIds: new Set(),
    approvalSessionIds: new Set(),
    activeSessionId: "chat-1",
    status: "idle",
    pending: false,
    // The stored tab belongs to a project; chats only ever list sessions.
    tab: "files",
    filesSearchOpen: false,
    recents: [],
    onSelectSession: vi.fn(),
    onSelectProject: vi.fn(),
    onOpenProject: vi.fn(),
    onOpenChats: vi.fn(),
    onOpenFile: vi.fn(),
    onTabChange: vi.fn(),
    onFilesSearchOpenChange: vi.fn(),
    projectRailOpen: true,
  };
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

describe("Sidebar chats", () => {
  it("lists projectless chats without the project tabs", () => {
    render();

    expect(container.querySelector('[data-session-card="chat-1"]')).not.toBeNull();
    expect(container.textContent).toContain("Chats");
    expect(container.textContent).not.toContain("No project folder");
    expect(container.querySelector('[role="tablist"]')).toBeNull();
  });

  it("marks Chats active in the rail and opens it on click", () => {
    render();

    const chats = button("Chats")!;
    expect(chats).toBeDefined();
    act(() => chats.click());
    expect(props.onOpenChats).toHaveBeenCalledTimes(1);
  });

  it("keeps the collapsed rail whole while in chats", () => {
    render({ projectRailOpen: false, compactProjectRail: true });

    const rail = container.querySelector('[aria-label="Project shortcuts"]')!;
    expect(rail).not.toBeNull();
    // No empty workspace tab list, and the project picker still shows an icon.
    expect(rail.querySelector('[role="tablist"]')).toBeNull();
    const picker = rail.querySelector('button[aria-haspopup="dialog"]')!;
    expect(picker.querySelector("svg")).not.toBeNull();
    expect(button("Chats")).toBeDefined();
  });

  it("says where new chats will appear when there are none", () => {
    render({ sessions: [], activeSessionId: undefined });

    expect(container.textContent).toContain(
      "Chats you start will show up here",
    );
  });

  it("keeps the project tabs for a project", () => {
    render({
      cwd: "/workspace/project",
      sessions: [],
      activeSessionId: undefined,
      tab: "sessions",
    });

    expect(container.querySelector('[role="tablist"]')).not.toBeNull();
    expect(container.textContent).toContain(
      "Sessions you start will show up here",
    );
  });
});
