// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { formatSessionTitle } from "../lib/session";
import { formatReminderTime } from "../lib/sessionReminders";
import { Sidebar } from "./Sidebar";

// Keep native services out of these menu/input interaction tests.
vi.mock("../hooks/useProjectDiffStats", () => ({
  useProjectDiffStats: () => null,
}));
vi.mock("../hooks/useGitFileStatuses", () => ({
  useGitFileStatuses: () => ({ files: new Map(), dirs: new Map() }),
}));
vi.mock("./SidebarUpdate", () => ({ SidebarUpdateFooter: () => null }));
vi.mock("./FileTree", () => ({ FileTree: () => null }));

let container: HTMLDivElement;
let root: Root;
let props: ComponentProps<typeof Sidebar>;

function render() {
  root.render(createElement(Sidebar, props));
}

function card(): HTMLElement {
  return container.querySelector('[data-session-card="session-1"]')!;
}

function renameInput(): HTMLInputElement {
  return container.querySelector("input:not([placeholder])")!;
}

function pressKey(target: HTMLElement, key: string) {
  const event = new KeyboardEvent("keydown", {
    key,
    bubbles: true,
    cancelable: true,
  });
  act(() => target.dispatchEvent(event));
  return event;
}

function typeTitle(input: HTMLInputElement, title: string) {
  // Use the native setter so React sees a user change, not its own value write.
  const setter = Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    "value",
  )!.set!;
  act(() => {
    setter.call(input, title);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

function startRename() {
  act(() => {
    card().dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
    );
  });
  const rename = Array.from(
    document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
  ).find((item) => item.textContent?.startsWith("Rename"))!;
  expect(rename.disabled).toBe(false);
  act(() => rename.click());
  return renameInput();
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
    cwd: "/workspace/project",
    open: true,
    sessions: [
      {
        id: "session-1",
        cwd: "/workspace/project",
        harness: "codex",
        model: "",
        runtimeMode: "supervised",
        title: formatSessionTitle("codex", "Original conversation"),
        createdAt: Date.now(),
        updatedAt: Date.now(),
      },
    ],
    busySessionIds: new Set(["session-1"]),
    approvalSessionIds: new Set(),
    activeSessionId: "session-1",
    status: "idle",
    pending: false,
    tab: "sessions",
    filesSearchOpen: false,
    onSelectSession: vi.fn(),
    onRenameSession: vi.fn((id: string, title: string) => {
      props = {
        ...props,
        sessions: props.sessions.map((session) =>
          session.id === id
            ? { ...session, title: formatSessionTitle(session.harness, title) }
            : session,
        ),
      };
      render();
    }),
    onOpenFile: vi.fn(),
    onTabChange: vi.fn(),
    onFilesSearchOpenChange: vi.fn(),
  };
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  localStorage.clear();
  vi.unstubAllGlobals();
});

describe("sidebar session rename", () => {
  it.each(["idle", "working", "needs approval"])(
    "renames from the menu and restores navigation (status=%s)",
    (status) => {
      if (status === "idle") props.busySessionIds = new Set();
      if (status === "needs approval") {
        props.approvalSessionIds = new Set(["session-1"]);
      }
      act(() => render());
      const input = startRename();

      expect(input.disabled).toBe(false);
      expect(document.activeElement === input).toBe(true);
      expect(input.selectionStart).toBe(0);
      expect(input.selectionEnd).toBe(input.value.length);
      typeTitle(input, "  Renamed conversation  ");
      expect(pressKey(input, "Enter").defaultPrevented).toBe(true);

      expect(props.onRenameSession).toHaveBeenCalledExactlyOnceWith(
        "session-1",
        "Renamed conversation",
      );
      expect(renameInput()).toBeNull();
      expect(card().textContent).toContain("Renamed conversation");
      act(() => card().click());
      expect(props.onSelectSession).toHaveBeenCalledWith("session-1");
    },
  );

  it("allows F2 to rename a working conversation", () => {
    act(() => render());
    pressKey(card(), "F2");
    const input = renameInput();
    expect(input.disabled).toBe(false);
    expect(document.activeElement === input).toBe(true);
    typeTitle(input, "Keyboard rename");
    pressKey(input, "Enter");
    expect(props.onRenameSession).toHaveBeenCalledExactlyOnceWith(
      "session-1",
      "Keyboard rename",
    );
  });

  it("cancels with Escape while the agent is working", () => {
    act(() => render());
    const input = startRename();
    expect(document.activeElement === input).toBe(true);
    typeTitle(input, "Discard this name");
    expect(pressKey(input, "Escape").defaultPrevented).toBe(true);
    expect(props.onRenameSession).not.toHaveBeenCalled();
    expect(renameInput()).toBeNull();
    expect(card().textContent).toContain("Original conversation");
  });

  it("commits once on blur while the agent is working", () => {
    act(() => render());
    const input = startRename();
    expect(document.activeElement === input).toBe(true);
    typeTitle(input, "  Saved on blur  ");
    act(() => input.blur());
    expect(props.onRenameSession).toHaveBeenCalledExactlyOnceWith(
      "session-1",
      "Saved on blur",
    );
    expect(renameInput()).toBeNull();
  });

  it("cancels an empty name without trapping the conversation in the editor", () => {
    act(() => render());
    const input = startRename();
    expect(document.activeElement === input).toBe(true);
    typeTitle(input, "   ");
    pressKey(input, "Enter");
    expect(props.onRenameSession).not.toHaveBeenCalled();
    expect(renameInput()).toBeNull();
    expect(card().textContent).toContain("Original conversation");
  });

  it("keeps an in-progress rename editable when a turn starts", () => {
    props.busySessionIds = new Set();
    act(() => render());
    const input = startRename();
    typeTitle(input, "My draft title");

    props = { ...props, busySessionIds: new Set(["session-1"]) };
    act(() => render());
    expect(renameInput()).toBe(input);
    expect(input.disabled).toBe(false);
    expect(document.activeElement === input).toBe(true);
    expect(input.value).toBe("My draft title");
    pressKey(input, "Enter");
    expect(props.onRenameSession).toHaveBeenCalledExactlyOnceWith(
      "session-1",
      "My draft title",
    );
  });
});

describe("sidebar pinned sessions", () => {
  it("renders them as a collapsible folder-style group without a divider", () => {
    props.sessions = [
      { ...props.sessions[0], pinned: true },
      {
        ...props.sessions[0],
        id: "session-2",
        title: formatSessionTitle("codex", "Unpinned conversation"),
        updatedAt: props.sessions[0].updatedAt - 1,
      },
    ];
    act(() => render());

    const group = container.querySelector<HTMLElement>(
      "[data-pinned-sessions]",
    )!;
    const toggle = group.querySelector<HTMLButtonElement>(
      'button[title="Pinned"]',
    )!;
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(
      group.querySelector('[data-session-card="session-1"]'),
    ).not.toBeNull();
    expect(container.querySelector("li[aria-hidden]")).toBeNull();

    act(() => toggle.click());
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(group.querySelector('[data-session-card="session-1"]')).toBeNull();
    expect(
      JSON.parse(
        localStorage.getItem("monocode.pinnedSessionsCollapsed") ?? "{}",
      ),
    ).toEqual({ "/workspace/project": true });
  });
});

describe("sidebar linked work item updates", () => {
  it("renders an unread dot without changing session order", () => {
    props.busySessionIds = new Set();
    props.activeSessionId = undefined;
    props.sessions = [
      {
        ...props.sessions[0],
        id: "session-2",
        title: formatSessionTitle("codex", "Newer conversation"),
        updatedAt: 200,
      },
      {
        ...props.sessions[0],
        id: "session-1",
        title: formatSessionTitle("codex", "Updated PR conversation"),
        updatedAt: 100,
        linkedWorkItem: {
          kind: "pr",
          repo: "acme/app",
          number: 42,
          url: "https://github.com/acme/app/pull/42",
        },
      },
    ];
    props.linkedSessionUpdateIds = new Set(["session-1"]);
    act(() => render());

    expect(
      Array.from(
        container.querySelectorAll<HTMLElement>("[data-session-card]"),
      ).map((row) => row.dataset.sessionCard),
    ).toEqual(["session-2", "session-1"]);
    expect(
      card().querySelector('[aria-label="Linked work item updated"]'),
    ).not.toBeNull();
  });
});

describe("sidebar session reminders", () => {
  function openReminderMenu() {
    act(() =>
      card().dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
      ),
    );
    const trigger = Array.from(
      document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
    ).find((item) => item.textContent === "Remind me")!;
    act(() =>
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true })),
    );
    return document.querySelector<HTMLElement>(
      '[role="menu"][aria-label="Remind me"]',
    )!;
  }

  it("schedules the selected session from the hover submenu", () => {
    props.onSetReminders = vi.fn();
    act(() => render());
    const submenu = openReminderMenu();
    const preset = Array.from(
      submenu.querySelectorAll<HTMLButtonElement>("button"),
    ).find((item) => item.textContent?.startsWith("In 3 hours"))!;
    const before = Date.now();
    act(() => preset.click());
    const [ids, dueAt] = vi.mocked(props.onSetReminders).mock.calls[0];
    expect(ids).toEqual(["session-1"]);
    expect(dueAt).toBeGreaterThanOrEqual(before + 3 * 60 * 60 * 1000);
    expect(dueAt).toBeLessThanOrEqual(Date.now() + 3 * 60 * 60 * 1000);
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  it("groups reminders first without a card clock and offers cancellation first", () => {
    props.onSetReminders = vi.fn();
    props.onCancelReminders = vi.fn();
    props.reminders = [
      {
        sessionId: "session-1",
        dueAt: Date.now() + 3600_000,
        firedAt: null,
        title: "Original conversation",
        harness: "codex",
        cwd: props.cwd,
      },
    ];
    props.sessions = [
      { ...props.sessions[0], pinned: true },
      { ...props.sessions[0], id: "session-2", pinned: true },
    ];
    act(() => render());
    const group = container.querySelector<HTMLElement>(
      "[data-reminder-sessions]",
    )!;
    expect(group.parentElement!.firstElementChild).toBe(group);
    expect(group.querySelector('[data-session-card="session-1"]')).toBe(card());
    expect(
      container.querySelectorAll('[data-session-card="session-1"]'),
    ).toHaveLength(1);
    expect(group.hasAttribute("data-session-folder")).toBe(false);
    expect(
      group.querySelector('button[title="Reminders"]')!.className,
    ).not.toContain("cursor-grab");
    expect(card().querySelector('[aria-label^="Reminder:"]')).toBeNull();
    act(() =>
      card().dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
      ),
    );
    const cancel = document.querySelector<HTMLButtonElement>(
      '[aria-label="Session actions"] [role="menuitem"]',
    )!;
    expect(cancel.textContent).toBe(
      `Cancel reminder${formatReminderTime(props.reminders[0].dueAt)}`,
    );
    act(() => cancel.click());
    expect(props.onCancelReminders).toHaveBeenCalledExactlyOnceWith([
      "session-1",
    ]);
    expect(props.onSetReminders).not.toHaveBeenCalled();
    props.reminders = [];
    act(() => render());
    expect(container.querySelector("[data-reminder-sessions]")).toBeNull();
    expect(
      container.querySelector(
        '[data-pinned-sessions] [data-session-card="session-1"]',
      ),
    ).toBe(card());
  });
});
