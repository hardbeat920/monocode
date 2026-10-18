import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { afterEach, expect, it, vi } from "vitest";
import {
  githubHost,
  githubHostOf,
  githubHostScope,
  githubOrigin,
  isGithubHost,
  loadGithubHost,
  setGithubHost,
} from "./githubHost";
import { githubActionsJobId } from "./githubPrChecks";
import { parseGithubWorkItemUrl } from "../../sessions/model/sessionWorkItem";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

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

it("reads an item's server from its URL", () => {
  setGithubHost("github.example.com");
  expect(githubHostOf("https://www.github.com/acme/web/pull/1")).toBe(
    "github.com",
  );
  expect(githubHostOf("https://GHE.corp.example/acme/web/pull/1")).toBe(
    "ghe.corp.example",
  );
  // Ports and non-HTTPS URLs never name a GitHub server.
  expect(githubHostOf("https://github.com:8443/acme/web/pull/1")).toBe(
    "github.example.com",
  );
  expect(githubHostOf("not a url")).toBe("github.example.com");
  expect(githubHostScope("github.com")).toBe("");
  expect(githubHostScope("GitHub.Example.com")).toBe("github.example.com/");
});

it("follows host changes saved in another window", async () => {
  let onChange: ((event: { payload: string }) => void) | undefined;
  vi.mocked(invoke).mockResolvedValue("github.com" as never);
  vi.mocked(listen).mockImplementation(async (_event, handler) => {
    onChange = handler as typeof onChange;
    return () => {};
  });
  await loadGithubHost();
  expect(listen).toHaveBeenCalledWith(
    "github-host-changed",
    expect.any(Function),
  );
  onChange?.({ payload: "github.example.com" });
  expect(githubHost()).toBe("github.example.com");
});
