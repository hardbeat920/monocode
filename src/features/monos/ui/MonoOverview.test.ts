// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { MonoOverview, type MonoOverviewTab } from "./MonoOverview";

vi.mock("../model/monoHabits", async (original) => ({
  ...(await original<object>()),
  loadHabits: async () => [],
}));

vi.mock("./MonoChangesView", () => ({
  MonoChangesView: () => createElement("p", { "data-changes-view": "" }),
}));

vi.mock("../../sessions/data/sessionStore", async (original) => ({
  ...(await original<object>()),
  // Both launches are still saved; deleted ones are hidden by default.
  listSessionsByProject: async (cwd: string) =>
    ["a", "b"].map((id) => ({
      id,
      cwd,
      harness: "codex",
      model: "codex:gpt-5.4",
      runtimeMode: "auto",
      title: id,
      createdAt: 1,
      updatedAt: 1,
    })),
  getSession: async () => null,
}));

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

async function render(
  tab: MonoOverviewTab,
  onTabChange = vi.fn(),
  extra: Record<string, unknown> = {},
) {
  await act(async () =>
    root.render(
      createElement(MonoOverview, {
        open: true,
        monoId: "mono-1",
        cwd: "/home",
        agent: { name: "Broski", mascot: "cat", color: "#9c9", projects: [] },
        state: { status: "idle" },
        launches: [
          {
            sessionId: "a",
            cwd: "/p",
            title: "First",
            harness: "codex",
            model: "codex:gpt-5.4",
          },
          {
            sessionId: "b",
            cwd: "/p",
            title: "Second",
            harness: "codex",
            model: "codex:gpt-5.4",
          },
        ],
        sessions: [],
        documents: [{ id: "d1", kind: "document", title: "Plan" }],
        tab,
        onTabChange,
        onOpenSession: vi.fn(),
        onOpenDocument: vi.fn(),
        onClose: vi.fn(),
        ...extra,
      }),
    ),
  );
  return onTabChange;
}

function tabs() {
  return [...container.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
}

it("names only the open tab, with its count, and switches tabs", async () => {
  const onTabChange = await render("sessions");
  const labels = tabs().map((tab) =>
    tab.querySelector("[data-tab-label]")!.getAttribute("data-tab-label"),
  );
  expect(labels).toEqual(["open", "closed", "closed", "closed"]);
  expect(tabs().map((tab) => tab.getAttribute("aria-label"))).toEqual([
    "Sessions",
    "Documents",
    "Habits",
    "Changes",
  ]);
  expect(tabs()[0].textContent).toBe("Sessions2");
  expect(tabs()[0].getAttribute("aria-selected")).toBe("true");
  act(() => tabs()[1].click());
  expect(onTabChange).toHaveBeenCalledWith("documents");
});

it("toggles activity from beside the panel toggle", async () => {
  const onTabChange = await render("documents");
  const button = () =>
    container.querySelector<HTMLButtonElement>(
      '[aria-label="Show activity"], [aria-label="Hide activity"]',
    )!;
  expect(button().getAttribute("aria-pressed")).toBe("false");
  expect(
    button().querySelector("[data-tab-label]")!.getAttribute("data-tab-label"),
  ).toBe("closed");
  act(() => button().click());
  expect(onTabChange).toHaveBeenLastCalledWith("activity");
  await render("activity", onTabChange);
  expect(button().getAttribute("aria-label")).toBe("Hide activity");
  expect(button().getAttribute("aria-pressed")).toBe("true");
  expect(
    button().querySelector("[data-tab-label]")!.getAttribute("data-tab-label"),
  ).toBe("open");
  expect(button().textContent).toBe("Activity");
  act(() => button().click());
  expect(onTabChange).toHaveBeenLastCalledWith("documents");
});

it("shows the picked turn's activity, or how to pick one", async () => {
  await render("activity");
  const panel = container.querySelector('[data-mono-overview="activity"]')!;
  expect(panel.textContent).toContain(
    "Select the activity on one of Broski's messages",
  );
  await render("activity", vi.fn(), {
    activity: createElement("p", { "data-trail": "" }, "Steps"),
  });
  expect(panel.querySelector("[data-trail]")?.textContent).toBe("Steps");
  expect(panel.textContent).not.toContain("Select the activity");
});

const changes = (totals: unknown) => ({
  changes: {
    sessionId: "mono",
    cwd: "/home",
    totals,
    request: {},
    projects: [],
  },
});

it("always offers a Changes tab and keeps its view mounted once opened", async () => {
  await render("sessions", vi.fn(), changes(null));
  expect(tabs()[3].getAttribute("aria-label")).toBe("Changes");
  expect(container.querySelector("[data-changes-view]")).toBeNull();
  const onTabChange = vi.fn();
  await render(
    "changes",
    onTabChange,
    changes({ files: 3, additions: 12, deletions: 4 }),
  );
  expect(tabs()[3].textContent).toBe("Changes3");
  expect(container.querySelector("[data-changes-view]")).not.toBeNull();
  // Another tab keeps the view mounted, hidden, so a commit draft survives.
  await render("sessions", onTabChange, changes(null));
  expect(tabs()[3].getAttribute("aria-label")).toBe("Changes");
  expect(
    container.querySelector("[data-changes-view]")!.closest("[hidden]"),
  ).not.toBeNull();
});

it("dots the folded Changes icon while there are changes", async () => {
  await render("sessions", vi.fn(), changes(null));
  expect(tabs()[3].querySelector("[data-changes-dot]")).toBeNull();
  await render(
    "sessions",
    vi.fn(),
    changes({ files: 2, additions: 3, deletions: 1 }),
  );
  expect(tabs()[3].querySelector("[data-changes-dot]")).not.toBeNull();
  // Open, the tab names its count instead.
  await render(
    "changes",
    vi.fn(),
    changes({ files: 2, additions: 3, deletions: 1 }),
  );
  expect(tabs()[3].querySelector("[data-changes-dot]")).toBeNull();
});

it("lists every session it started, newest first", async () => {
  await render("sessions");
  const rows = [...container.querySelectorAll("[data-mono-session]")];
  expect(rows.map((row) => row.getAttribute("data-mono-session"))).toEqual([
    "b",
    "a",
  ]);
});

it("shows documents in their own tab, and a new-habit button only on Habits", async () => {
  await render("documents");
  expect(container.querySelector('[data-mono-document="d1"]')).not.toBeNull();
  expect(container.querySelector('[aria-label="New habit"]')).toBeNull();
  await render("habits");
  expect(container.querySelector('[aria-label="New habit"]')).not.toBeNull();
});

it("opens a session over the list, at the same width, with back, workspace and hide", async () => {
  const onBackFromSession = vi.fn();
  const onOpenSessionInWorkspace = vi.fn();
  const onClose = vi.fn();
  await render("sessions", vi.fn(), {
    shownSession: {
      sessionId: "a",
      title: "First",
      pane: createElement("p", { "data-pane": "" }, "Session pane"),
    },
    onBackFromSession,
    onOpenSessionInWorkspace,
    onClose,
  });
  // The same panel, not another sidebar beside or instead of it.
  expect(container.querySelectorAll("aside")).toHaveLength(1);
  const page = container.querySelector("[data-mono-launched-session]")!;
  expect(page.querySelector("[data-pane]")).not.toBeNull();
  // The list stays beneath, out of reach while the session covers it.
  expect(
    container.querySelector("[data-mono-session]")!.closest("[inert]"),
  ).not.toBeNull();
  const button = (label: string) =>
    page.parentElement!.querySelector<HTMLButtonElement>(
      `button[aria-label="${label}"]`,
    )!;
  act(() => button("Back").click());
  expect(onBackFromSession).toHaveBeenCalledOnce();
  await act(async () => button("Open in workspace").click());
  expect(onOpenSessionInWorkspace).toHaveBeenCalledWith("a");
  act(() => button("Hide overview").click());
  expect(onClose).toHaveBeenCalledOnce();
});

it("opens a document over its tab with a way back", async () => {
  const onBackFromDocument = vi.fn();
  await render("documents", vi.fn(), {
    shownDocumentId: "d1",
    onBackFromDocument,
  });
  const page = container.querySelector("[data-mono-document-page]")!;
  expect(page).not.toBeNull();
  act(() =>
    page.querySelector<HTMLButtonElement>('button[aria-label="Back"]')!.click(),
  );
  expect(onBackFromDocument).toHaveBeenCalledOnce();
});
