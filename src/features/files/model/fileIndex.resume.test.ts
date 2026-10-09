// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { listProjectFiles, type ProjectFile } from "../../../platform/tauri/fs";
import {
  invalidateProjectFiles,
  loadProjectFiles,
  peekProjectFiles,
} from "./fileIndex";

vi.mock("../../../platform/tauri/fs", () => ({ listProjectFiles: vi.fn() }));
const list = vi.mocked(listProjectFiles);
const files: ProjectFile[] = [
  { name: "a.ts", relative: "a.ts", path: "/repo/a.ts" },
];

beforeEach(() => {
  vi.useFakeTimers();
  vi.spyOn(document, "hidden", "get").mockReturnValue(false);
  invalidateProjectFiles();
  list.mockReset().mockResolvedValue(files);
});

afterEach(() => {
  invalidateProjectFiles();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

it.each(["focus", "visibilitychange"])(
  "revalidates indexes after %s",
  async (event) => {
    await loadProjectFiles("/repo/first");
    await loadProjectFiles("/repo/second");
    const changed = [
      ...files,
      { name: "new.ts", relative: "new.ts", path: "/repo/new.ts" },
    ];
    list.mockResolvedValue(changed);
    (event === "focus" ? window : document).dispatchEvent(new Event(event));
    expect(peekProjectFiles("/repo/first")).toBeNull();
    await vi.runAllTimersAsync();
    expect(peekProjectFiles("/repo/second")).toBe(changed);
    expect(list).toHaveBeenCalledTimes(3);
  },
);

it("does not refresh or evict indexes while the window is hidden", async () => {
  await loadProjectFiles("/repo/first");
  await loadProjectFiles("/repo/second");
  vi.spyOn(document, "hidden", "get").mockReturnValue(true);
  document.dispatchEvent(new Event("visibilitychange"));
  await vi.runAllTimersAsync();
  expect(peekProjectFiles("/repo/first")).toBe(files);
  expect(peekProjectFiles("/repo/second")).toBe(files);
  expect(list).toHaveBeenCalledTimes(2);
});
