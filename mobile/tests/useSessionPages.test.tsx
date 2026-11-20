import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import {
  useSessionPages,
  visibleSessions,
} from "@/features/sessions/useSessionPages";
import type { SessionPage } from "@/features/workspace/WorkspaceProvider";
import type { SessionSummary } from "@/features/workspace/types";

const desktop = vi.hoisted(() => ({ listSessions: vi.fn() }));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({ listSessions: desktop.listSessions }),
}));

const session = (id: string): SessionSummary => ({
  id,
  projectId: "project",
  title: id,
  harness: "grok",
  model: "grok-4.7",
  branch: "main",
  updatedAt: 1,
});
const page = (...ids: string[]): SessionPage => ({
  sessions: ids.map(session),
  hasMore: false,
});
let project = 0;
let latest: ReturnType<typeof useSessionPages>;
let root: ReturnType<typeof createRoot>;
let container: HTMLDivElement;
function Probe({ active, query }: { active: boolean; query: string }) {
  const pages = useSessionPages(`project-${project}`, query, active);
  latest = pages;
  return (
    <output>
      {visibleSessions(pages, [], query, (session) => session)
        .map((session) => session.id)
        .join(",")}
    </output>
  );
}
const render = (active: boolean, query = "") =>
  act(() => root.render(<Probe active={active} query={query} />));
const fetchPage = () =>
  act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
beforeEach(() => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  desktop.listSessions.mockReset();
  project += 1;
  container = document.createElement("div");
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
});

it("lists new handoffs after returning to the same project without switching projects", async () => {
  desktop.listSessions.mockResolvedValue(page("parent"));
  render(true);
  await fetchPage();
  expect(container.textContent).toBe("parent");
  render(false);
  desktop.listSessions.mockResolvedValue(
    page("handoff-two", "handoff-one", "parent"),
  );
  await fetchPage();
  expect(desktop.listSessions).toHaveBeenCalledTimes(1);
  render(true);
  await fetchPage();
  expect(desktop.listSessions).toHaveBeenLastCalledWith(
    `project-${project}`,
    "",
    0,
  );
  expect(container.textContent).toBe("handoff-two,handoff-one,parent");
});

it("ignores an old page response from the covered route and preserves search on return", async () => {
  let resolveOld!: (page: SessionPage) => void;
  desktop.listSessions.mockReturnValueOnce(
    new Promise<SessionPage>((resolve) => {
      resolveOld = resolve;
    }),
  );
  render(true, "handoff");
  await act(async () => {
    await vi.advanceTimersByTimeAsync(250);
  });
  render(false, "handoff");
  await act(async () => resolveOld(page("stale-handoff")));
  expect(container.textContent).toBe("");
  desktop.listSessions.mockResolvedValue(page("new-handoff"));
  render(true, "handoff");
  await act(async () => {
    await vi.advanceTimersByTimeAsync(250);
  });
  expect(desktop.listSessions).toHaveBeenLastCalledWith(
    `project-${project}`,
    "handoff",
    0,
  );
  expect(container.textContent).toBe("new-handoff");
});

it("ignores a next page that arrives after returning and refreshing the first page", async () => {
  let resolveMore!: (page: SessionPage) => void;
  desktop.listSessions.mockResolvedValueOnce({
    ...page("parent"),
    hasMore: true,
  });
  render(true);
  await fetchPage();
  desktop.listSessions.mockReturnValueOnce(
    new Promise<SessionPage>((resolve) => {
      resolveMore = resolve;
    }),
  );
  act(() => latest!.loadMore());
  expect(latest!.loadingMore).toBe(true);
  render(false);
  desktop.listSessions.mockResolvedValueOnce(
    page("handoff-two", "handoff-one", "parent"),
  );
  render(true);
  await fetchPage();
  await act(async () => resolveMore({ ...page("stale-tail"), hasMore: true }));
  expect(container.textContent).toBe("handoff-two,handoff-one,parent");
  expect(latest!.hasMore).toBe(false);
  expect(latest!.loadingMore).toBe(false);
});

it("discards a next page received while covered and permits pagination on the next visit", async () => {
  let resolveOld!: (page: SessionPage) => void;
  desktop.listSessions.mockResolvedValueOnce({
    ...page("parent"),
    hasMore: true,
  });
  render(true);
  await fetchPage();
  desktop.listSessions.mockReturnValueOnce(
    new Promise<SessionPage>((resolve) => {
      resolveOld = resolve;
    }),
  );
  act(() => latest!.loadMore());
  render(false);
  await act(async () => resolveOld(page("stale-tail")));
  expect(container.textContent).toBe("parent");
  act(() => latest!.loadMore());
  expect(desktop.listSessions).toHaveBeenCalledTimes(2);
  desktop.listSessions.mockResolvedValueOnce({
    ...page("handoff", "parent"),
    hasMore: true,
  });
  render(true);
  await fetchPage();
  desktop.listSessions.mockResolvedValueOnce(page("valid-tail"));
  await act(async () => latest!.loadMore());
  expect(desktop.listSessions).toHaveBeenLastCalledWith(
    `project-${project}`,
    "",
    2,
  );
  expect(container.textContent).toBe("handoff,parent,valid-tail");
});

it("does not let an obsolete pagination completion clear a newer loading state", async () => {
  let resolveOld!: (page: SessionPage) => void;
  let resolveNew!: (page: SessionPage) => void;
  desktop.listSessions.mockResolvedValueOnce({
    ...page("parent"),
    hasMore: true,
  });
  render(true);
  await fetchPage();
  desktop.listSessions.mockReturnValueOnce(
    new Promise<SessionPage>((resolve) => {
      resolveOld = resolve;
    }),
  );
  act(() => latest!.loadMore());
  render(false);
  desktop.listSessions.mockResolvedValueOnce({
    ...page("handoff", "parent"),
    hasMore: true,
  });
  render(true);
  await fetchPage();
  desktop.listSessions.mockReturnValueOnce(
    new Promise<SessionPage>((resolve) => {
      resolveNew = resolve;
    }),
  );
  act(() => latest!.loadMore());
  await act(async () => resolveOld(page("stale-tail")));
  expect(latest!.loadingMore).toBe(true);
  expect(container.textContent).toBe("handoff,parent");
  act(() => latest!.loadMore());
  expect(desktop.listSessions).toHaveBeenCalledTimes(4);
  await act(async () => resolveNew(page("valid-tail")));
  expect(latest!.loadingMore).toBe(false);
  expect(container.textContent).toBe("handoff,parent,valid-tail");
});
