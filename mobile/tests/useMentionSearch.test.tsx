import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import { useMentionSearch } from "@/features/connection/useMentionSearch";
const link = vi.hoisted(() => ({ command: vi.fn(), fingerprint: "desktop" }));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({
    command: link.command,
    pairing: { desktop: { fingerprint: link.fingerprint } },
  }),
}));
let root: ReturnType<typeof createRoot>;
let value: ReturnType<typeof useMentionSearch>;
function Probe({
  query,
  worktree,
  project = "project",
}: {
  query: string | null;
  worktree?: string;
  project?: string;
}) {
  value = useMentionSearch(project, query, worktree);
  return null;
}
const render = (query: string | null, worktree?: string, project?: string) =>
  act(() =>
    root.render(<Probe query={query} worktree={worktree} project={project} />),
  );
const flush = () =>
  act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
const tick = () =>
  act(async () => {
    await vi.advanceTimersByTimeAsync(60);
  });
function deferred() {
  let resolve!: (value: object) => void;
  const promise = new Promise<object>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
beforeEach(() => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  link.command = vi.fn();
  link.fingerprint = "desktop";
  link.command.mockImplementation(async (method: string) =>
    method === "project.files"
      ? {
          entries: [
            { name: "docs", isDir: true },
            { name: "README.md", isDir: false },
            { name: ".git", isDir: true },
          ],
        }
      : { results: [] },
  );
  root = createRoot(document.createElement("div"));
});
afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
});
it("preloads roots before @ and opens them immediately without empty-query quick open or debounce", async () => {
  render(null, "/desktop/worktree");
  expect(link.command).toHaveBeenCalledWith("project.files", {
    projectId: "project",
    path: "",
    worktreePath: "/desktop/worktree",
  });
  await flush();
  render("", "/desktop/worktree");
  expect(value.results.map((file) => file.path)).toEqual(["docs", "README.md"]);
  expect(value.searching).toBe(false);
  expect(link.command).toHaveBeenCalledTimes(1);
});
it("uses released desktop search replies without labels, including nested folders and match highlighting", async () => {
  link.command.mockImplementation(async (method: string) =>
    method === "project.files"
      ? { entries: [] }
      : { results: [{ name: "view.ts", path: "src/view.ts" }] },
  );
  render("view");
  await tick();
  expect(value.results[0]).toMatchObject({
    path: "src/view.ts",
    label: "src/view.ts",
    positions: [4, 5, 6, 7],
    isDir: false,
  });
  render("src");
  // Derived folders and cached files are searchable in the same render.
  expect(value.results.some((file) => file.path === "src" && file.isDir)).toBe(
    true,
  );
});
it("locally filters roots and cached search hits while typing, with no round trip when returning to a cached query", async () => {
  link.command.mockImplementation(async (method: string) =>
    method === "project.files"
      ? { entries: [{ name: "docs", isDir: true }] }
      : { results: [{ name: "document.ts", path: "src/document.ts" }] },
  );
  render("");
  await flush();
  render("doc");
  expect(value.results[0].path).toBe("docs");
  await tick();
  const requests = link.command.mock.calls.length;
  render("document");
  expect(value.results[0].path).toBe("src/document.ts");
  render("doc");
  await tick();
  expect(link.command).toHaveBeenCalledTimes(requests);
});
it("does not let out-of-order replies or other worktrees/desktops supply unrelated results", async () => {
  const first = deferred();
  const second = deferred();
  link.command.mockImplementation(
    (method: string, params: { query: string }) =>
      method === "project.files"
        ? Promise.resolve({ entries: [] })
        : params.query === "a"
          ? first.promise
          : second.promise,
  );
  render("a");
  await tick();
  render("new");
  await tick();
  await act(async () => {
    second.resolve({ results: [{ name: "new.ts", path: "new.ts" }] });
  });
  expect(value.results[0].path).toBe("new.ts");
  await act(async () => {
    first.resolve({ results: [{ name: "ancient.ts", path: "ancient.ts" }] });
  });
  expect(value.results.map((file) => file.path)).toEqual(["new.ts"]);
  render("new", "/other");
  expect(value.results).toEqual([]);
  render(null);
  expect(value.results).toEqual([]);
  link.fingerprint = "other-desktop";
  render("new");
  expect(value.results).toEqual([]);
  await flush();
});
it("reports root/search errors instead of disguising protocol failures as empty matches", async () => {
  link.command.mockRejectedValue(new Error("Desktop unavailable"));
  render("");
  await flush();
  expect(value.error).toBe("Desktop unavailable");
  render("fail");
  await tick();
  expect(value.error).toBe("Desktop unavailable");
});
it("keeps canonical labels from newer desktops and safely marks legacy spaced paths for literal sending", async () => {
  link.command.mockImplementation(async (method: string) =>
    method === "project.files"
      ? { entries: [] }
      : {
          results: [
            { name: "read me.md", path: "docs/read me.md" },
            {
              name: "read me.ts",
              path: "src/read me.ts",
              label: "src/read-me.ts~2",
            },
          ],
        },
  );
  render("read");
  await tick();
  expect(
    value.results.find((file) => file.path === "docs/read me.md"),
  ).toMatchObject({ label: "docs/read%20me.md", literalPath: true });
  expect(
    value.results.find((file) => file.path === "src/read me.ts"),
  ).toMatchObject({ label: "src/read-me.ts~2" });
});
