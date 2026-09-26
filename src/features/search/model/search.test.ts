import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  cancelProjectSearch,
  createProjectSearchId,
  searchProject,
} from "./search";
import {
  cancelSessionSearch,
  searchSessions,
} from "../../sessions/data/sessionStore";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("project search cancellation", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("gives each mounted search a distinct id", () => {
    const first = createProjectSearchId();
    const second = createProjectSearchId();
    expect(second).not.toBe(first);
  });

  it("passes the owner id with search and cancel commands", async () => {
    const options = {
      cwd: "/repo",
      query: "needle",
      searchId: "search-7",
    };
    vi.mocked(invoke).mockResolvedValueOnce({ matches: [], truncated: false });
    vi.mocked(invoke).mockResolvedValueOnce(undefined);

    await searchProject(options);
    await cancelProjectSearch(options.cwd, options.searchId);

    expect(invoke).toHaveBeenNthCalledWith(1, "search_project", { options });
    expect(invoke).toHaveBeenNthCalledWith(2, "cancel_project_search", {
      cwd: "/repo",
      searchId: "search-7",
    });
  });

  it("passes the owner id with session search and cancel commands", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ hits: [], truncated: false });
    vi.mocked(invoke).mockResolvedValueOnce(undefined);

    await searchSessions({ query: "needle", searchOwner: "owner-1" });
    await cancelSessionSearch("owner-1");

    expect(invoke).toHaveBeenNthCalledWith(1, "session_search", {
      options: { query: "needle", searchOwner: "owner-1" },
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "cancel_session_search", {
      searchOwner: "owner-1",
    });
  });
});
