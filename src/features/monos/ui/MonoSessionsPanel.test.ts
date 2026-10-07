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
import { MonoSessionsPanel } from "./MonoSessionsPanel";

vi.mock("../../sessions/data/sessionStore", () => ({
  getSession: vi.fn(),
  listSessionsByProject: vi.fn(),
}));

let container: HTMLDivElement;
let root: Root;
const onOpenSession = vi.fn();
const onClose = vi.fn();
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

async function render(sessions: Session[] = [], launches = [launch]) {
  await act(async () =>
    root.render(
      createElement(MonoSessionsPanel, {
        agent,
        launches,
        sessions,
        onOpenSession,
        onClose,
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
  expect(container.querySelector("aside")?.getAttribute("aria-label")).toBe(
    "MonoInvader sessions",
  );
  expect(container.querySelectorAll("[data-mono-session]")).toHaveLength(1);
  expect(container.textContent).toContain("PR #799 review");
  expect(container.textContent).toContain("Working");
  expect(container.textContent).not.toContain("Unrelated");
  await render([{ ...child, busy: false }]);
  expect(container.textContent).toContain("Ready");
  await act(async () =>
    container.querySelector<HTMLButtonElement>("[data-mono-session]")!.click(),
  );
  expect(onOpenSession).toHaveBeenCalledWith(launch.sessionId);
  act(() =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Hide sessions"]')!
      .click(),
  );
  expect(onClose).toHaveBeenCalledOnce();
});

it("loads closed sessions across projects and preserves archived cards", async () => {
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
  expect(container.textContent).toContain("Archived");
  expect(container.textContent).toContain("Another project");
  expect(
    container.querySelector<HTMLButtonElement>("[data-mono-session]")!.disabled,
  ).toBe(false);
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
  expect(
    container.querySelector<HTMLButtonElement>("[data-mono-session]")!.disabled,
  ).toBe(false);
});

it("keeps deleted launches visible as unavailable and does not offer to open them", async () => {
  await render();
  expect(container.textContent).toContain(launch.title);
  expect(container.textContent).toContain("No longer available");
  const button = container.querySelector<HTMLButtonElement>(
    "[data-mono-session]",
  )!;
  expect(button.disabled).toBe(true);
  act(() => button.click());
  expect(onOpenSession).not.toHaveBeenCalled();
});

it("allows opening when status lookup fails and reports navigation errors", async () => {
  vi.mocked(listSessionsByProject).mockRejectedValue(new Error("Offline"));
  onOpenSession.mockRejectedValue(new Error("Could not restore session"));
  await render();
  expect(container.textContent).toContain("Status unavailable");
  await act(async () =>
    container.querySelector<HTMLButtonElement>("[data-mono-session]")!.click(),
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
  expect(container.textContent).toContain("Needs input");
});
