import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  bitbucketBuildId,
  fetchBitbucketBuildDetails,
  fetchBitbucketPrChecks,
} from "./bitbucket";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("fetchBitbucketPrChecks", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("asks the backend by repository and number, not by local checkout", async () => {
    const payload = {
      headOid: "abc",
      checks: [
        {
          name: "Pipeline",
          workflow: "",
          state: "pass",
          url: "https://bitbucket.org/acme/web/pipelines/results/1",
          startedAt: null,
          completedAt: null,
        },
      ],
    };
    vi.mocked(invoke).mockResolvedValue(payload);

    await expect(
      fetchBitbucketPrChecks("/tmp/web", "acme/web", 12),
    ).resolves.toEqual(payload);

    expect(invoke).toHaveBeenCalledWith("bitbucket_pr_checks", {
      repo: "acme/web",
      number: 12,
    });
  });

  it("passes backend errors through for the Checks tab to show", async () => {
    vi.mocked(invoke).mockRejectedValue(
      "Bitbucket API token is missing a required scope: read:pullrequest:bitbucket",
    );

    await expect(
      fetchBitbucketPrChecks("/tmp/web", "acme/web", 12),
    ).rejects.toContain("read:pullrequest:bitbucket");
  });
});

describe("bitbucketBuildId", () => {
  const repo = "acme/web";

  it("reads the build number from Pipelines result URLs", () => {
    expect(
      bitbucketBuildId(
        "https://bitbucket.org/acme/web/pipelines/results/42",
        repo,
      ),
    ).toBe("42");
    expect(
      bitbucketBuildId(
        "https://bitbucket.org/acme/web/pipelines/results/42/",
        repo,
      ),
    ).toBe("42");
    expect(
      bitbucketBuildId(
        "https://bitbucket.org/ACME/Web/pipelines/results/7",
        repo,
      ),
    ).toBe("7");
  });

  it("reads the older add-on style URL, where the number is in the hash", () => {
    expect(
      bitbucketBuildId(
        "https://bitbucket.org/acme/web/addon/pipelines/home#!/results/108",
        repo,
      ),
    ).toBe("108");
  });

  it("has nothing for other services, other repos, other hosts or bad numbers", () => {
    for (const url of [
      null,
      "",
      "not a url",
      "https://sonarcloud.io/dashboard?id=acme_web&pullRequest=12",
      "https://bitbucket.org/acme/other/pipelines/results/42",
      "https://bitbucket.org/acme/web/pipelines/results/0",
      "https://bitbucket.org/acme/web/pipelines/results/abc",
      "https://bitbucket.org/acme/web/pipelines/results/42/steps/9",
      "https://bitbucket.org/acme/web/addon/pipelines/home",
      "https://bitbucket.org/acme/web/addon/pipelines/home#!/other/5",
      "https://evil.example/acme/web/pipelines/results/42",
      "http://bitbucket.org/acme/web/pipelines/results/42",
    ]) {
      expect(bitbucketBuildId(url, repo), String(url)).toBeNull();
    }
  });
});

describe("fetchBitbucketBuildDetails", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
  });

  it("asks the backend by repository and build number", async () => {
    const payload = { steps: [], annotations: [], notice: null };
    vi.mocked(invoke).mockResolvedValue(payload);

    await expect(fetchBitbucketBuildDetails("acme/web", "42")).resolves.toEqual(
      payload,
    );

    expect(invoke).toHaveBeenCalledWith("bitbucket_build_details", {
      repo: "acme/web",
      build: "42",
    });
  });
});
