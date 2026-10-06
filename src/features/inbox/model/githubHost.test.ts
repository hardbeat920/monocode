import { afterEach, expect, it } from "vitest";
import { githubOrigin, isGithubHost, setGithubHost } from "./githubHost";
import { githubActionsJobId } from "./githubPrChecks";
import { parseGithubWorkItemUrl } from "../../sessions/model/sessionWorkItem";

afterEach(() => setGithubHost(null));

it("defaults to github.com", () => {
  expect(githubOrigin()).toBe("https://github.com");
  expect(isGithubHost("www.github.com")).toBe(true);
});

it("builds and recognizes GitHub Enterprise URLs for the configured host", () => {
  setGithubHost("GitHub.Example.com");
  expect(githubOrigin()).toBe("https://github.example.com");
  expect(isGithubHost("github.example.com")).toBe(true);
  expect(isGithubHost("github.com")).toBe(false);
  expect(
    parseGithubWorkItemUrl("see https://github.example.com/acme/web/pull/7"),
  ).toEqual({
    kind: "pr",
    repo: "acme/web",
    number: 7,
    url: "https://github.example.com/acme/web/pull/7",
  });
  expect(
    parseGithubWorkItemUrl("https://github.com/acme/web/pull/7"),
  ).toBeNull();
  expect(
    githubActionsJobId(
      "https://github.example.com/acme/web/actions/runs/1/job/42",
      "acme/web",
    ),
  ).toBe("42");
});
