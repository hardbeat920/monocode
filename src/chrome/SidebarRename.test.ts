// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { formatSessionTitle } from "../lib/session";
import { formatReminderTime } from "../lib/sessionReminders";
import { Sidebar } from "./Sidebar";
import { loadSessionFolders } from "../lib/sessionFolders";

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

function projectSearchInput(): HTMLInputElement | null {
  return document.querySelector('input[placeholder="Search projects..."]');
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
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("sidebar session multiselection", () => {
  it.each(["ctrlKey", "metaKey"] as const)(
    "starts the next range at the active session after %s clears the last selection",
    (modifier) => {
      props.sessions = [1, 2, 3, 4].map((n) => ({
        ...props.sessions[0],
        id: `session-${n}`,
        updatedAt: 100 - n,
      }));
      act(() => render());
      const thirdCard = container.querySelector<HTMLElement>(
        '[data-session-card="session-3"]',
      )!;
      for (let click = 0; click < 2; click++) {
        act(() => thirdCard.dispatchEvent(new MouseEvent("click", {
          bubbles: true,
          [modifier]: true,
        })));
        expect(container.querySelectorAll('[data-session-selected="true"]'))
          .toHaveLength(click === 0 ? 1 : 0);
      }
      act(() => container.querySelector('[data-session-card="session-4"]')!
        .dispatchEvent(new MouseEvent("click", {
          bubbles: true,
          shiftKey: true,
        })));
      expect(Array.from(
        container.querySelectorAll('[data-session-selected="true"]'),
        (el) => el.getAttribute("data-session-card"),
      )).toEqual(["session-1", "session-2", "session-3", "session-4"]);
      expect(props.onSelectSession).not.toHaveBeenCalled();
    },
  );

  it("clears the previous session focus when Ctrl-clicking another session", () => {
    props.sessions = [1, 2].map((n) => ({
      ...props.sessions[0],
      id: `session-${n}`,
      updatedAt: 100 - n,
    }));
    props.onDeleteSession = vi.fn();
    act(() => render());
    const firstTitle = card().querySelector<HTMLElement>("[data-session-select]")!;
    const secondTitle = container.querySelector<HTMLElement>(
      '[data-session-select="session-2"]',
    )!;
    act(() => firstTitle.focus());
    act(() => {
      secondTitle.dispatchEvent(new MouseEvent("mousedown", {
        bubbles: true,
        cancelable: true,
        button: 0,
        ctrlKey: true,
      }));
      secondTitle.dispatchEvent(new MouseEvent("click", {
        bubbles: true,
        ctrlKey: true,
      }));
    });
    expect(card().getAttribute("data-session-selected")).toBeNull();
    expect(secondTitle.closest("[data-session-card]")!.getAttribute(
      "data-session-selected",
    )).toBe("true");
    expect(props.onSelectSession).not.toHaveBeenCalled();
    pressKey(document.activeElement as HTMLElement, "Delete");
    expect(props.onDeleteSession).not.toHaveBeenCalled();
    pressKey(document.activeElement as HTMLElement, "F2");
    expect(renameInput()).toBeNull();
    expect(document.activeElement).not.toBe(firstTitle);
    expect(document.activeElement).not.toBe(secondTitle);
  });

  it("keeps Shift-click selection free of title focus while preserving keyboard focus", () => {
    act(() => render());
    const title = card().querySelector<HTMLElement>("[data-session-select]")!;
    act(() => title.focus());
    expect(document.activeElement).toBe(title);
    const mouseDown = new MouseEvent("mousedown", {
      bubbles: true,
      cancelable: true,
      shiftKey: true,
      button: 0,
    });
    act(() => title.dispatchEvent(mouseDown));
    expect(mouseDown.defaultPrevented).toBe(true);
    expect(document.activeElement).not.toBe(title);
    act(() => title.dispatchEvent(new MouseEvent("click", {
      bubbles: true,
      shiftKey: true,
    })));
    expect(card().getAttribute("data-session-selected")).toBe("true");
    expect(props.onSelectSession).not.toHaveBeenCalled();
    act(() => title.focus());
    expect(document.activeElement).toBe(title);
    pressKey(title, "Enter");
    expect(props.onSelectSession).toHaveBeenCalledWith("session-1");
  });

  it("selects the visible range from a plain click with Shift-click", () => {
    props.sessions = [1, 2, 3, 4].map((n) => ({
      ...props.sessions[0],
      id: `session-${n}`,
      updatedAt: 100 - n,
    }));
    act(() => render());
    act(() => card().click());
    act(() =>
      container
        .querySelector('[data-session-card="session-3"]')!
        .dispatchEvent(
          new MouseEvent("click", { bubbles: true, shiftKey: true }),
        ),
    );
    expect(
      Array.from(
        container.querySelectorAll('[data-session-selected="true"]'),
        (el) => el.getAttribute("data-session-card"),
      ),
    ).toEqual(["session-1", "session-2", "session-3"]);
    expect(props.onSelectSession).toHaveBeenCalledTimes(1);
  });

  it("creates a folder containing the Ctrl-clicked sessions without opening them", () => {
    props.sessions = [1, 2, 3].map((n) => ({
      ...props.sessions[0],
      id: `session-${n}`,
      updatedAt: 100 - n,
    }));
    act(() => render());
    for (const id of ["session-1", "session-3"]) {
      act(() =>
        container
          .querySelector(`[data-session-card="${id}"]`)!
          .dispatchEvent(
            new MouseEvent("click", { bubbles: true, ctrlKey: true }),
          ),
      );
    }
    expect(props.onSelectSession).not.toHaveBeenCalled();
    expect(
      container.querySelectorAll('[data-session-selected="true"]'),
    ).toHaveLength(2);
    act(() =>
      card().dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
      ),
    );
    const newFolder = Array.from(
      document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
    ).find((item) => item.textContent === "New folder")!;
    act(() => newFolder.click());
    expect(loadSessionFolders(props.cwd)[0].sessionIds).toEqual([
      "session-1",
      "session-3",
    ]);
  });
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
    pressKey(card().querySelector("[data-session-select]")!, "F2");
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

  it("prefetches after a deliberate hover and immediately on press", () => {
    vi.useFakeTimers();
    props.onPrefetchSession = vi.fn();
    act(() => render());

    act(() => {
      card().dispatchEvent(new MouseEvent("pointerover", { bubbles: true }));
      vi.advanceTimersByTime(119);
    });
    expect(props.onPrefetchSession).not.toHaveBeenCalled();

    act(() => vi.advanceTimersByTime(1));
    expect(props.onPrefetchSession).toHaveBeenCalledExactlyOnceWith(
      "session-1",
    );

    act(() => {
      card().dispatchEvent(new MouseEvent("pointerover", { bubbles: true }));
      card().dispatchEvent(new MouseEvent("pointerout", { bubbles: true }));
      vi.advanceTimersByTime(120);
    });
    expect(props.onPrefetchSession).toHaveBeenCalledTimes(1);

    act(() => {
      card().dispatchEvent(
        new MouseEvent("pointerdown", { bubbles: true, button: 0 }),
      );
    });
    expect(props.onPrefetchSession).toHaveBeenCalledTimes(2);
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

describe("sidebar project picker", () => {
  it("focuses the project search input when opened", () => {
    // Hold animation frames so the deferred focus retry runs on demand.
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    });
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    props.onSelectProject = vi.fn();
    act(() => render());

    const trigger = container.querySelector<HTMLButtonElement>(
      '[aria-label^="Switch project"]',
    )!;
    act(() => trigger.click());

    const input = projectSearchInput();
    expect(input).not.toBeNull();
    expect(document.activeElement).toBe(input);

    // Focus lost before the next frame is restored by the retry.
    act(() => input!.blur());
    expect(document.activeElement).not.toBe(input);
    expect(frames).toHaveLength(1);
    act(() => frames.forEach((frame) => frame(0)));
    expect(document.activeElement).toBe(input);
  });
});

describe("sidebar reorder affordances", () => {
  it("keeps the default cursor on reorderable tabs and folders", () => {
    props.sessions = [
      props.sessions[0],
      {
        ...props.sessions[0],
        id: "session-2",
        title: formatSessionTitle("codex", "Second conversation"),
      },
    ];
    localStorage.setItem(
      "monocode.sessionFolders",
      JSON.stringify({
        "/workspace/project": [
          {
            id: "folder-1",
            name: "Folder one",
            sessionIds: ["session-1"],
            collapsed: false,
          },
          {
            id: "folder-2",
            name: "Folder two",
            sessionIds: ["session-2"],
            collapsed: false,
          },
        ],
      }),
    );

    act(() => render());

    const tabs = container.querySelectorAll<HTMLElement>('[role="tab"]');
    expect(tabs).toHaveLength(3);
    for (const tab of tabs) {
      expect(tab.className).not.toContain("cursor-grab");
      expect(tab.parentElement?.className).not.toContain("cursor-grab");
    }
    for (const name of ["Folder one", "Folder two"]) {
      expect(
        container.querySelector<HTMLButtonElement>(`button[title="${name}"]`)!
          .className,
      ).not.toContain("cursor-grab");
    }
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

describe("sidebar orchestration card", () => {
  it.each([false, true])(
    "keeps agents inside a distinct lead card (pinned=%s)",
    (pinned) => {
      props.onArchiveSession = vi.fn();
      props.linkedSessionUpdateIds = new Set(["session-1"]);
      const lead = {
        ...props.sessions[0],
        pinned,
        linkedWorkItem: {
          kind: "pr" as const,
          repo: "acme/app",
          number: 42,
          url: "https://github.com/acme/app/pull/42",
        },
        orchestration: {
          status: "active" as const,
          live: true,
          tasks: [
            {
              sessionId: "worker-a",
              title: "Build settings",
              harness: "codex" as const,
              model: "codex:worker-a",
              status: "running" as const,
            },
            {
              sessionId: "worker-b",
              title: "Review changes",
              harness: "claude" as const,
              model: "claude:worker-b",
              status: "running" as const,
              needsInput: true,
            },
            {
              sessionId: "worker-c",
              title: "Check types",
              harness: "codex" as const,
              model: "codex:worker-c",
              status: "completed" as const,
            },
          ],
        },
      };
      props.sessions = [
        lead,
        { ...props.sessions[0], id: "unrelated" },
        {
          ...props.sessions[0],
          id: "worker-a",
          orchestrationLeadId: lead.id,
        },
      ];
      act(() => render());
      expect(container.querySelectorAll("[data-session-card]")).toHaveLength(2);
      expect(card().dataset.orchestrationCard).toBe("true");
      // The lead card carries the sidebar's ordinary active treatment.
      expect(card().className).toContain("bg-selection");
      // The lead names its own model, like every agent row beneath it.
      expect(card().textContent).toContain("Claude Sonnet 5");
      expect(card().textContent).not.toContain("Orchestrator");
      const orchestrationIcon = card().querySelector<HTMLButtonElement>(
        "[data-orchestration-icon]",
      );
      expect(orchestrationIcon).not.toBeNull();
      expect(orchestrationIcon?.tagName).toBe("BUTTON");
      expect(orchestrationIcon?.classList.contains("opacity-0")).toBe(false);
      expect(
        orchestrationIcon?.querySelector("svg")?.classList.contains("size-3"),
      ).toBe(true);
      expect(orchestrationIcon?.hasAttribute("title")).toBe(false);
      expect(
        card().querySelector("[data-session-select] [data-orchestration-icon]"),
      ).toBeNull();
      expect(card().textContent).toContain("3 agents");
      expect(card().textContent).toContain("1/3 done");
      expect(card().textContent).toContain("Build settings");
      expect(card().textContent).toContain("Needs input");
      expect(
        card().querySelector('[aria-label="Linked work item updated"]'),
      ).not.toBeNull();
      const archive = card().querySelector<HTMLButtonElement>(
        '[aria-label^="Archive "]',
      )!;
      const pullRequest = card().querySelector('[aria-label="Open PR #42"]');
      expect(card().querySelectorAll('[aria-label^="Archive "]')).toHaveLength(
        1,
      );
      expect(card().lastElementChild?.contains(orchestrationIcon)).toBe(true);
      expect(card().lastElementChild?.contains(pullRequest)).toBe(true);
      expect(archive.nextElementSibling).toBe(pullRequest);
      expect(orchestrationIcon?.parentElement?.lastElementChild).toBe(
        orchestrationIcon,
      );
      act(() => archive.click());
      expect(props.onArchiveSession).toHaveBeenCalledExactlyOnceWith(
        lead.id,
        true,
      );
      expect(props.onSelectSession).not.toHaveBeenCalled();
      // A collapsed row stays one line; the model rides along in the tooltip.
      expect(
        card()
          .querySelector('[data-orchestration-agent="worker-a"] button')
          ?.getAttribute("title"),
      ).toContain("codex:worker-a");
      expect(
        card().querySelectorAll("[data-orchestration-agent]"),
      ).toHaveLength(3);
      const normal = container.querySelector<HTMLElement>(
        '[data-session-card="unrelated"]',
      )!;
      expect(normal.hasAttribute("data-orchestration-card")).toBe(false);
      expect(normal.querySelector("[data-orchestration-agent]")).toBeNull();
      // Working the agents list is not a request to open the lead's tab: the
      // row expands in place and the card stays where it is.
      const agentRow = card().querySelector<HTMLButtonElement>(
        '[data-orchestration-agent="worker-a"] button',
      )!;
      expect(card().hasAttribute("role")).toBe(false);
      for (const action of card().querySelectorAll('button, [role="button"]')) {
        expect(
          action.parentElement?.closest('button, [role="button"]'),
        ).toBeNull();
      }
      const selection = card().querySelector<HTMLElement>(
        "[data-session-select]",
      )!;
      act(() => selection.focus());
      expect(document.activeElement).toBe(selection);
      expect(pressKey(selection, "Enter").defaultPrevented).toBe(true);
      expect(props.onSelectSession).toHaveBeenCalledWith(lead.id);
      vi.mocked(props.onSelectSession).mockClear();
      expect(pressKey(agentRow, "Enter").defaultPrevented).toBe(false);
      expect(props.onSelectSession).not.toHaveBeenCalled();
      const wasOpen = agentRow.getAttribute("aria-expanded");
      act(() => agentRow.click());
      expect(props.onSelectSession).not.toHaveBeenCalled();
      expect(
        card()
          .querySelector('[data-orchestration-agent="worker-a"] button')
          ?.getAttribute("aria-expanded"),
      ).not.toBe(wasOpen);
      // A blocked agent stays expanded while it needs the lead's attention.
      expect(
        card()
          .querySelector('[data-orchestration-agent="worker-b"] button')
          ?.getAttribute("aria-expanded"),
      ).toBe("true");
      props.approvalSessionIds = new Set([lead.id]);
      act(() => render());
      expect(card().className).toContain("border-dashed");
      expect(card().textContent).toContain("Needs input");
    },
  );

  it("matches an ordinary card at rest and expands when opened or working", () => {
    props.sessions = [
      {
        ...props.sessions[0],
        orchestration: {
          status: "active",
          tasks: [
            {
              sessionId: "worker",
              title: "Review changes",
              harness: "codex",
              model: "codex:test",
              status: "completed",
            },
          ],
        },
      },
    ];
    props.activeSessionId = "another-session";
    props.busySessionIds = new Set();
    act(() => render());

    expect(card().classList.contains("py-2")).toBe(true);
    expect(card().classList.contains("py-2.5")).toBe(false);
    expect(card().classList.contains("bg-content/5")).toBe(false);
    expect(card().querySelector("[data-orchestration-icon]")).not.toBeNull();
    expect(card().querySelector("[data-orchestration-agent]")).toBeNull();

    props.activeSessionId = "session-1";
    act(() => render());
    expect(card().classList.contains("pt-2")).toBe(true);
    expect(card().classList.contains("pb-2.5")).toBe(true);
    expect(card().classList.contains("py-2.5")).toBe(false);
    expect(
      card().querySelector('[data-orchestration-agent="worker"]'),
    ).not.toBeNull();

    props.activeSessionId = "another-session";
    props.busySessionIds = new Set(["session-1"]);
    act(() => render());
    expect(card().classList.contains("pt-2")).toBe(true);
    expect(card().classList.contains("pb-2.5")).toBe(true);
    expect(
      card().querySelector('[data-orchestration-agent="worker"]'),
    ).not.toBeNull();
  });

  it("opens the custom subagent tooltip immediately on hover", () => {
    props.busySessionIds = new Set();
    props.sessions[0].orchestration = {
      status: "active",
      live: true,
      tasks: [
        {
          sessionId: "worker-a",
          title: "Review changes",
          harness: "codex",
          model: "codex:test",
          status: "running",
        },
        {
          sessionId: "worker-b",
          title: "Check types",
          harness: "claude",
          model: "claude:test",
          status: "completed",
        },
      ],
    };
    act(() => render());
    const trigger = card().querySelector<HTMLButtonElement>(
      "[data-orchestration-icon]",
    )!;

    act(() =>
      trigger.dispatchEvent(new MouseEvent("mouseover", { bubbles: true })),
    );

    const tooltip = document.querySelector<HTMLElement>('[role="tooltip"]')!;
    expect(tooltip).not.toBeNull();
    expect(tooltip.classList.contains("popover-open")).toBe(true);
    expect(tooltip.textContent).toContain("1/2 done");
    expect(tooltip.textContent).toContain("Review changes");
    expect(tooltip.textContent).toContain("Working");
    expect(tooltip.textContent).toContain("Check types");
    expect(tooltip.textContent).toContain("Done");

    act(() =>
      trigger.dispatchEvent(new MouseEvent("mouseout", { bubbles: true })),
    );
    expect(document.querySelector('[role="tooltip"]')).toBeNull();
  });

  it("renders saved worker details without claiming the workers are running", () => {
    props.busySessionIds = new Set();
    props.sessions[0].orchestration = {
      status: "active",
      tasks: [
        {
          sessionId: "worker",
          title: "Saved task",
          harness: "codex",
          model: "codex:test",
          status: "running",
        },
      ],
    };
    act(() => render());
    expect(card().textContent).toContain("Saved task");
    expect(card().textContent).toContain("Saved");
    expect(card().textContent).not.toContain("Working");
    expect(card().querySelector(".motion-safe\\:animate-pulse")).toBeNull();
  });
});

describe("sidebar linked work item updates", () => {
  it("opens a linked item beside the session that owns it", () => {
    props.busySessionIds = new Set();
    props.onOpenInboxItem = vi.fn();
    const linkedWorkItem = {
      kind: "pr" as const,
      repo: "acme/app",
      number: 42,
      url: "https://github.com/acme/app/pull/42",
    };
    props.sessions = [{ ...props.sessions[0], linkedWorkItem }];
    act(() => render());

    const pullRequest = card().querySelector<HTMLButtonElement>(
      '[aria-label="Open PR #42"]',
    )!;
    expect(pullRequest.title).toContain("beside this session");
    act(() => pullRequest.click());
    expect(props.onOpenInboxItem).toHaveBeenCalledExactlyOnceWith(
      linkedWorkItem,
      "session-1",
    );
    expect(props.onSelectSession).not.toHaveBeenCalled();
  });

  it("uses the footer for the linked issue or PR instead of a second harness icon", () => {
    props.busySessionIds = new Set();
    props.onArchiveSession = vi.fn();
    props.sessions = [
      {
        ...props.sessions[0],
        branch: "feature/session-card",
        repo: "acme/app",
        linkedWorkItem: {
          kind: "pr",
          repo: "acme/app",
          number: 42,
          url: "https://github.com/acme/app/pull/42",
        },
      },
    ];
    act(() => render());

    const rows = card().children;
    const archive = card().querySelector('[aria-label^="Archive "]');
    const pullRequest = card().querySelector('[aria-label="Open PR #42"]');
    expect(card().querySelectorAll('img[alt=""]')).toHaveLength(1);
    expect(rows.item(rows.length - 1)?.contains(pullRequest)).toBe(true);
    expect(archive?.parentElement).toBe(pullRequest?.parentElement);
    expect(archive?.nextElementSibling).toBe(pullRequest);
  });

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

describe("sidebar working agents", () => {
  it("shows the original working widget when the project rail is collapsed", () => {
    props.projectRailOpen = false;
    props.onSelectAgent = vi.fn();
    props.liveAgents = [
      {
        id: "agent-a",
        cwd: "/workspace/alpha",
        title: "Build settings",
        harness: "codex",
        activity: "Editing Settings.tsx",
        startedAt: Date.now() - 10_000,
        needsApproval: false,
        done: false,
      },
      {
        id: "agent-b",
        cwd: "/workspace/beta",
        title: "Review tests",
        harness: "claude",
        activity: "Running tests",
        startedAt: Date.now() - 5_000,
        needsApproval: false,
        done: false,
      },
    ];
    act(() => render());

    const preview = container.querySelector<HTMLElement>(
      '[data-live-agents-preview="full"]',
    )!;
    expect(preview.querySelectorAll("[data-live-agent-card]")).toHaveLength(2);
    expect(
      preview.querySelector('[data-live-agent-card="agent-a"]')!.textContent,
    ).toContain("alpha");
    expect(
      preview.querySelector('[data-live-agent-card="agent-b"]')!.textContent,
    ).toContain("beta");
    expect(preview.querySelector(".mascot-active")).not.toBeNull();

    act(() =>
      preview
        .querySelector<HTMLButtonElement>('[data-live-agent-card="agent-b"]')!
        .click(),
    );
    expect(props.onSelectAgent).toHaveBeenCalledExactlyOnceWith("agent-b");

    props.projectRailOpen = true;
    act(() => render());
    expect(container.querySelector("[data-live-agents-preview]")).toBeNull();
  });
});
