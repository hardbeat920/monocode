import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const app = vi.hoisted(() => ({ listSessions: vi.fn() }));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({ listSessions: app.listSessions }),
}));

import { useSessionPages } from "@/features/sessions/useSessionPages";
import type { SessionSummary } from "@/features/workspace/types";

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

const session = (id: string, updatedAt: number) => ({ id, updatedAt }) as SessionSummary;

let root: ReturnType<typeof createRoot>;
let shown: ReturnType<typeof useSessionPages>;
function List({ project, reload }: { project: string; reload: string }) {
  shown = useSessionPages(project, "", true, reload);
  return null;
}
// The first page is read from a timer, so a search can wait for a pause in typing.
const render = async (project: string, reload: string) => {
  await act(async () => root.render(<List project={project} reload={reload} />));
  await act(() => new Promise((resolve) => setTimeout(resolve, 1)));
};
const ids = () => shown?.sessions.map((s) => s.id);

beforeEach(() => {
  app.listSessions.mockReset();
  root = createRoot(document.createElement("div"));
});
afterEach(() => act(() => root.unmount()));

it("reads the first page again when the poll's roster changes, and not otherwise", async () => {
  app.listSessions.mockResolvedValue({ sessions: [session("a", 200)], hasMore: false });
  await render("roster", "a");
  await render("roster", "a");
  expect(app.listSessions).toHaveBeenCalledTimes(1);

  app.listSessions.mockResolvedValue({
    sessions: [session("new", 300), session("a", 200)],
    hasMore: false,
  });
  await render("roster", "a,new");

  expect(app.listSessions).toHaveBeenCalledTimes(2);
  expect(ids()).toEqual(["new", "a"]);
});

it("keeps the pages already scrolled through when the first is read again", async () => {
  app.listSessions.mockResolvedValueOnce({
    sessions: [session("a", 400), session("b", 300)],
    hasMore: true,
  });
  await render("scrolled", "a");
  app.listSessions.mockResolvedValueOnce({
    sessions: [session("c", 200), session("d", 100)],
    hasMore: true,
  });
  await act(async () => shown?.loadMore());
  expect(ids()).toEqual(["a", "b", "c", "d"]);

  app.listSessions.mockResolvedValueOnce({
    sessions: [session("new", 500), session("a", 400)],
    hasMore: true,
  });
  await render("scrolled", "a,new");

  expect(ids()).toEqual(["new", "a", "b", "c", "d"]);
});
