// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  getSession,
  listSessionsByProject,
  type SessionSummary,
} from "../../sessions/data/sessionStore";
import {
  newSession,
  type MonoSpawnedSession,
  type Session,
} from "../../sessions/model/session";
import { MonoSessionList } from "./MonoSessionList";

vi.mock("../../sessions/data/sessionStore", () => ({
  getSession: vi.fn(),
  listSessionsByProject: vi.fn(),
}));

let container: HTMLDivElement;
let root: Root;
const onOpenSession = vi.fn();
const agent = {
  name: "MonoInvader",
  mascot: "invader",
  color: "#6ba",
  projects: [],
};
const launch: MonoSpawnedSession = {
  sessionId: "app-review",
  cwd: "/repo",
  title: "Review latest PR",
  harness: "codex",
  model: "gpt-6",
};

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(listSessionsByProject).mockResolvedValue([]);
  vi.mocked(getSession).mockResolvedValue(null);
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
  sessions: Session[] = [],
  launches = [launch],
  props: Record<string, unknown> = {},
) {
  await act(async () =>
    root.render(
      createElement(MonoSessionList, {
        agent,
        launches,
        sessions,
        onOpenSession,
        ...props,
      }),
    ),
  );
}

it("shows live cards, updates their status and opens the selected session", async () => {
  const child = {
    ...newSession("codex", "/repo"),
    id: launch.sessionId,
    title: "PR #799 review",
    busy: true,
  };
  const unrelated = { ...child, id: "other", title: "Unrelated" };
  await render([child, unrelated]);
  expect(container.querySelectorAll("[data-mono-session]")).toHaveLength(1);
  expect(container.textContent).toContain("PR #799 review");
  expect(container.textContent).toContain("Working");
  expect(container.textContent).not.toContain("Unrelated");
  await render([{ ...child, busy: false }]);
  expect(container.textContent).not.toContain("Working");
  // The sidebar's own card, naming the project its sessions span.
  expect(container.querySelector("[data-session-card]")?.textContent).toContain(
    "repo",
  );
  await act(async () =>
    container.querySelector<HTMLElement>("[data-session-card]")!.click(),
  );
  expect(onOpenSession).toHaveBeenCalledWith(launch.sessionId);
});

it("loads closed sessions across projects", async () => {
  vi.mocked(listSessionsByProject).mockImplementation(async (cwd) => [
    {
      id: cwd === "/repo" ? launch.sessionId : "app-second",
      cwd,
      harness: "codex",
      model: "gpt-6",
      runtimeMode: "default",
      title: cwd === "/repo" ? "Saved review" : "Another project",
      archived: cwd === "/repo",
      createdAt: 1,
      updatedAt: 2,
    } as SessionSummary,
  ]);
  await render(
    [],
    [launch, { ...launch, sessionId: "app-second", cwd: "/other" }],
  );
  expect(listSessionsByProject).toHaveBeenCalledWith("/repo");
  expect(listSessionsByProject).toHaveBeenCalledWith("/other");
  expect(container.textContent).toContain("Saved review");
  expect(container.textContent).toContain("Another project");
  expect(container.querySelectorAll("[data-session-card]")).toHaveLength(2);
});

it("keeps the link usable after a session moves to another project", async () => {
  vi.mocked(getSession).mockResolvedValue({
    ...newSession("codex", "/moved"),
    id: launch.sessionId,
    title: "Moved review",
  });
  await render();
  expect(getSession).toHaveBeenCalledWith(launch.sessionId);
  expect(container.textContent).toContain("Moved review");
  expect(container.textContent).toContain("moved");
  await act(async () =>
    container.querySelector<HTMLElement>("[data-session-card]")!.click(),
  );
  expect(onOpenSession).toHaveBeenCalledWith(launch.sessionId);
});

it("hides deleted launches until filtered to, then shows them as cards that do not open", async () => {
  await render();
  expect(container.querySelector("[data-session-card]")).toBeNull();
  expect(container.textContent).toContain("have since been deleted");
  await render([], [launch], {
    filters: {
      showArchived: false,
      showUnavailable: true,
      hiddenHarnesses: [],
      time: "all",
      status: { working: false, needsApproval: false, done: false },
    },
  });
  const card = container.querySelector<HTMLElement>("[data-session-card]")!;
  expect(card.textContent).toContain(launch.title);
  expect(card.textContent).toContain("Unavailable");
  expect(card.closest("[inert]")).not.toBeNull();
  act(() => card.click());
  expect(onOpenSession).not.toHaveBeenCalled();
});

it("allows opening when status lookup fails and reports navigation errors", async () => {
  vi.mocked(listSessionsByProject).mockRejectedValue(new Error("Offline"));
  onOpenSession.mockRejectedValue(new Error("Could not restore session"));
  await render();
  expect(container.textContent).toContain("Status unavailable");
  await act(async () =>
    container.querySelector<HTMLElement>("[data-session-card]")!.click(),
  );
  expect(container.querySelector('[role="alert"]')?.textContent).toBe(
    "Could not restore session",
  );
});

it("shows a draft and then an approval without treating either as completed", async () => {
  const child = {
    ...newSession("codex", "/repo"),
    id: launch.sessionId,
    blocks: [
      { id: "draft", role: "user" as const, text: "Review", draft: true },
    ],
  };
  await render([child]);
  expect(container.textContent).toContain("Draft");
  await render([
    {
      ...child,
      blocks: [
        {
          id: "approval",
          role: "tool",
          text: "Run command",
          approval: { requestId: 42 },
        },
      ],
    },
  ]);
  expect(container.textContent).toContain("Need approval");
});

it("holds a loading card's place invisibly, and shows a known one at once when remounted", async () => {
  let resolve!: (rows: SessionSummary[]) => void;
  vi.mocked(listSessionsByProject).mockReturnValue(
    new Promise((done) => (resolve = done)),
  );
  const cached = { ...launch, sessionId: "app-cached" };
  await render([], [cached]);
  // No stand-in card that the real one would swap in for.
  expect(container.textContent).not.toContain("Loading");
  expect(
    container.querySelector('[data-mono-session-loading="app-cached"]'),
  ).not.toBeNull();
  await act(async () =>
    resolve([
      {
        id: "app-cached",
        cwd: "/repo",
        harness: "codex",
        model: "gpt-6",
        runtimeMode: "auto",
        title: "Cached review",
        createdAt: 1,
        updatedAt: 2,
      },
    ]),
  );
  expect(container.textContent).toContain("Cached review");
  // Switching Monos remounts the list; the card comes straight back.
  act(() => root.unmount());
  root = createRoot(container);
  vi.mocked(listSessionsByProject).mockReturnValue(new Promise(() => {}));
  await render([], [cached]);
  expect(container.querySelector("[data-session-card]")?.textContent).toContain(
    "Cached review",
  );
});

it("narrows its cards by search, project and the sidebar's filters", async () => {
  vi.mocked(listSessionsByProject).mockImplementation(async (cwd) => [
    {
      id: cwd === "/repo" ? launch.sessionId : "app-second",
      cwd,
      harness: cwd === "/repo" ? "codex" : "claude",
      model: "gpt-6",
      runtimeMode: "default",
      title: cwd === "/repo" ? "Saved review" : "Fix login",
      archived: cwd === "/repo",
      createdAt: 1,
      updatedAt: 2,
    } as SessionSummary,
  ]);
  const launches = [
    launch,
    { ...launch, sessionId: "app-second", cwd: "/other", harness: "claude" },
  ] as MonoSpawnedSession[];
  const show = (props: Record<string, unknown>) =>
    act(async () =>
      root.render(
        createElement(MonoSessionList, {
          agent,
          launches,
          sessions: [],
          onOpenSession,
          ...props,
        }),
      ),
    );
  const cards = () =>
    [...container.querySelectorAll("[data-session-card]")].map(
      (card) => card.textContent ?? "",
    );

  await show({ query: "login" });
  expect(cards()).toHaveLength(1);
  expect(cards()[0]).toContain("Fix login");
  // The project's name is on the card, so it matches too.
  await show({ query: "other" });
  expect(cards()[0]).toContain("Fix login");
  await show({
    filters: {
      showArchived: true,
      hiddenHarnesses: [],
      time: "all",
      status: { working: false, needsApproval: false, done: false },
    },
  });
  expect(cards()).toHaveLength(1);
  expect(cards()[0]).toContain("Saved review");
  await show({ query: "nothing like it" });
  expect(cards()).toHaveLength(0);
  expect(container.textContent).toContain("No sessions match this search.");
});

it("mounts a page of cards at a time and the next as the list nears its end", async () => {
  let intersect: ((entries: { isIntersecting: boolean }[]) => void) | undefined;
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      constructor(callback: typeof intersect) {
        intersect = callback;
      }
      observe() {}
      disconnect() {}
    },
  );
  const launches = Array.from({ length: 40 }, (_, index) => ({
    ...launch,
    sessionId: `app-${index}`,
    title: `Session ${index}`,
  }));
  vi.mocked(listSessionsByProject).mockResolvedValue(
    launches.map(
      (entry) =>
        ({
          id: entry.sessionId,
          cwd: entry.cwd,
          harness: entry.harness,
          model: entry.model,
          runtimeMode: "default",
          title: entry.title,
          createdAt: 1,
          updatedAt: 2,
        }) as SessionSummary,
    ),
  );
  await render([], launches);
  expect(container.querySelectorAll("[data-mono-session]")).toHaveLength(32);
  await act(async () => intersect?.([{ isIntersecting: true }]));
  expect(container.querySelectorAll("[data-mono-session]")).toHaveLength(40);
});
