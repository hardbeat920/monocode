import { beforeEach, describe, expect, it, vi } from "vitest";

const refreshHarnessCatalogs = vi.fn(async () => {});
const refreshProjectOpenCodeCatalog = vi.fn(async () => {});

vi.mock("../../integrations/harness/core/registry", () => ({
  refreshHarnessCatalogs,
}));
vi.mock(
  "../../integrations/harness/providers/opencode/opencodeCatalog",
  () => ({ refreshProjectOpenCodeCatalog }),
);

const { refreshStartupCatalogs } = await import("./startupCatalogs");

describe("refreshStartupCatalogs", () => {
  beforeEach(() => {
    refreshHarnessCatalogs.mockClear();
    refreshProjectOpenCodeCatalog.mockClear();
  });

  it("refreshes the home OpenCode catalog as well as each project list", async () => {
    await refreshStartupCatalogs([
      { harness: "opencode", cwd: "/repo" },
      { harness: "opencode", cwd: "/repo", worktreeCwd: "/repo-tree" },
      { harness: "opencode", cwd: "/repo" },
      { harness: "claude", cwd: "/repo" },
    ]);
    expect(refreshHarnessCatalogs).toHaveBeenCalledWith(["opencode", "claude"]);
    expect(
      refreshProjectOpenCodeCatalog.mock.calls.map(([cwd]) => cwd),
    ).toEqual(["/repo", "/repo-tree"]);
  });

  it("skips project lists when no OpenCode session is open", async () => {
    await refreshStartupCatalogs([{ harness: "codex", cwd: "/repo" }]);
    expect(refreshHarnessCatalogs).toHaveBeenCalledWith(["codex"]);
    expect(refreshProjectOpenCodeCatalog).not.toHaveBeenCalled();
  });
});
