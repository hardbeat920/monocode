import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearLinearIssueCache,
  linearTeamIdsForFetch,
  linearTeamKeys,
  peekLinearIssue,
} from "./linear";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => {
  clearLinearIssueCache();
  vi.mocked(invoke).mockReset();
});

describe("linearTeamKeys", () => {
  it("caches upper-case team keys and treats an empty list as unknown", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([
      { id: "t1", key: "eng", name: "Engineering" },
    ]);
    await expect(linearTeamKeys()).resolves.toEqual(new Set(["ENG"]));
    await expect(linearTeamKeys()).resolves.toEqual(new Set(["ENG"]));
    expect(invoke).toHaveBeenCalledTimes(1);

    clearLinearIssueCache();
    vi.mocked(invoke).mockResolvedValueOnce([]);
    await expect(linearTeamKeys()).resolves.toBeNull();
  });

  it("drops a team list that arrives after the caches were cleared", async () => {
    let settle: (teams: unknown) => void = () => {};
    vi.mocked(invoke).mockReturnValueOnce(
      new Promise((resolve) => {
        settle = resolve;
      }),
    );
    const pending = linearTeamKeys();
    clearLinearIssueCache();
    settle([{ id: "t1", key: "OLD", name: "Old workspace" }]);
    await expect(pending).resolves.toEqual(new Set(["OLD"]));

    vi.mocked(invoke).mockResolvedValueOnce([
      { id: "t2", key: "NEW", name: "New workspace" },
    ]);
    await expect(linearTeamKeys()).resolves.toEqual(new Set(["NEW"]));
    expect(peekLinearIssue("OLD-1")).toBeNull();
  });
});

describe("linearTeamIdsForFetch", () => {
  const teams = [
    { id: "t1", key: "ENG", name: "Engineering" },
    { id: "t2", key: "DES", name: "Design" },
  ];

  it("sends no team filter when nothing is hidden", () => {
    expect(linearTeamIdsForFetch(teams, [])).toBeNull();
  });

  it("keeps visible team ids", () => {
    expect(linearTeamIdsForFetch(teams, ["t2"])).toEqual(["t1"]);
  });

  it("returns empty when every known team is hidden", () => {
    expect(linearTeamIdsForFetch(teams, ["t1", "t2"])).toEqual([]);
  });

  it("sends no team filter when hidden ids match no team", () => {
    expect(linearTeamIdsForFetch(teams, ["gone"])).toBeNull();
  });
});
