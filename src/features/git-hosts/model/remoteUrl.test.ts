import { describe, expect, it } from "vitest";
import {
  parseRemoteUrl,
  planCheckoutFolder,
  remoteMatches,
  repoFolderName,
  validRepoSlug,
} from "./remoteUrl";

describe("parseRemoteUrl", () => {
  it("reads https, ssh and scp-like remotes the same way", () => {
    const expected = { host: "github.com", path: "owner/repo" };
    expect(parseRemoteUrl("https://GitHub.com/owner/repo.git")).toEqual(expected);
    expect(parseRemoteUrl("https://token@github.com/owner/repo/")).toEqual(expected);
    expect(parseRemoteUrl("ssh://git@github.com:22/owner/repo.git")).toEqual(expected);
    expect(parseRemoteUrl("git@github.com:owner/repo.GIT")).toEqual(expected);
    expect(parseRemoteUrl("github.com:owner/repo")).toEqual(expected);
  });

  it("keeps the path's case and nested groups", () => {
    expect(parseRemoteUrl("git@gitlab.com:Group/sub/Repo.git")).toEqual({
      host: "gitlab.com",
      path: "Group/sub/Repo",
    });
  });

  it("does not decode escapes", () => {
    expect(parseRemoteUrl("https://github.com/o/r%")).toEqual({ host: "github.com", path: "o/r%" });
  });

  it("rejects local paths", () => {
    expect(parseRemoteUrl("")).toBeNull();
    expect(parseRemoteUrl("/srv/git/repo.git")).toBeNull();
    expect(parseRemoteUrl("C:\\repos\\repo")).toBeNull();
    expect(parseRemoteUrl("../repo")).toBeNull();
  });
});

describe("remoteMatches", () => {
  it("compares host and path case-insensitively", () => {
    expect(remoteMatches("git@github.com:Owner/Repo.git", "github.com", "owner/repo")).toBe(true);
    expect(remoteMatches("https://github.com/owner/repo", "github.com", "Owner/Repo.git")).toBe(true);
    expect(remoteMatches("git@github.com:owner/other.git", "github.com", "owner/repo")).toBe(false);
    expect(remoteMatches("git@gitlab.com:owner/repo.git", "github.com", "owner/repo")).toBe(false);
  });
});

describe("validRepoSlug", () => {
  it("accepts owner/name only", () => {
    expect(validRepoSlug("hardbeat920/monocode")).toBe(true);
    expect(validRepoSlug("a.b/c_d-e")).toBe(true);
    expect(validRepoSlug("monocode")).toBe(false);
    expect(validRepoSlug("a/b/c")).toBe(false);
    expect(validRepoSlug("../x")).toBe(false);
    expect(validRepoSlug("a/..")).toBe(false);
    expect(validRepoSlug("-o/x")).toBe(false);
    expect(validRepoSlug("o/x y")).toBe(false);
  });

  it("rejects names that cannot be a folder everywhere", () => {
    for (const slug of ["o/.git", "o/..git", "o/...git", "o/repo.", "o/con", "o/NUL.txt", "o/com1"])
      expect(validRepoSlug(slug)).toBe(false);
    expect(repoFolderName("o/Repo.GIT")).toBe("Repo");
    expect(repoFolderName("o/.github")).toBe(".github");
  });
});

describe("planCheckoutFolder", () => {
  it("clones into the repository name when it is free", async () => {
    expect(await planCheckoutFolder("o/repo", () => "free")).toEqual({ name: "repo", reuse: false });
  });

  it("reuses a matching checkout", async () => {
    expect(await planCheckoutFolder("o/repo", () => "match")).toEqual({ name: "repo", reuse: true });
  });

  it("steps past folders that hold something else", async () => {
    const states: Record<string, "taken" | "match" | "free"> = { repo: "taken", "repo-2": "taken" };
    expect(await planCheckoutFolder("o/repo", (name) => states[name] ?? "free")).toEqual({
      name: "repo-3",
      reuse: false,
    });
    states["repo-2"] = "match";
    expect(await planCheckoutFolder("o/repo", (name) => states[name] ?? "free")).toEqual({
      name: "repo-2",
      reuse: true,
    });
  });
});
