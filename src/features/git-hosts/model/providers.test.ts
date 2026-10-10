import { describe, expect, it } from "vitest";
import { gitHostUi, parseRepoInput } from "./providers";

describe("parseRepoInput", () => {
  const github = gitHostUi("github");

  it("accepts slugs and GitHub URLs, keeping their case", () => {
    expect(parseRepoInput(github, " Owner/Repo ")).toBe("Owner/Repo");
    expect(parseRepoInput(github, "https://github.com/Owner/Repo")).toBe("Owner/Repo");
    expect(parseRepoInput(github, "https://github.com/Owner/Repo.git")).toBe("Owner/Repo");
    expect(parseRepoInput(github, "https://github.com/Owner/Repo/")).toBe("Owner/Repo");
    expect(parseRepoInput(github, "github.com/Owner/Repo/tree/main?x=1")).toBe("Owner/Repo");
    expect(parseRepoInput(github, "https://github.com/o/r/pull/12#discussion")).toBe("o/r");
    expect(parseRepoInput(github, "git@github.com:Owner/Repo.git")).toBe("Owner/Repo");
    expect(parseRepoInput(github, "ssh://git@github.com/o/r.git")).toBe("o/r");
  });

  it("ignores ports and escapes in URLs", () => {
    expect(parseRepoInput(github, "ssh://git@github.com:22/o/r.git")).toBe("o/r");
    expect(parseRepoInput(github, "https://github.com:443/o/r")).toBe("o/r");
    expect(parseRepoInput(github, "https://github.com/o/r/tree/feature/50%-off")).toBe("o/r");
    expect(parseRepoInput(github, "github.com/o/r%")).toBeNull();
    expect(parseRepoInput(github, "https://gitlab.com/%")).toBeNull();
  });

  it("rejects other hosts and incomplete input", () => {
    expect(parseRepoInput(github, "https://gitlab.com/o/r")).toBeNull();
    expect(parseRepoInput(github, "https://github.com/o")).toBeNull();
    expect(parseRepoInput(github, "github.com/owner")).toBeNull();
    expect(parseRepoInput(github, "github.com/")).toBeNull();
    expect(parseRepoInput(github, "repo")).toBeNull();
    expect(parseRepoInput(github, "")).toBeNull();
  });
});
