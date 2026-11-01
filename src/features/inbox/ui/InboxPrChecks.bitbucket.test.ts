// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { InboxPrChecks, PrChecksTab } from "./InboxPrChecks";
import type { GithubPrChecksView } from "../hooks/useGithubPrChecks";
import type {
  GithubPrCheck,
  GithubPrChecksOverall,
} from "../model/githubPrChecks";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const PIPELINE_URL = "https://bitbucket.org/acme/web/pipelines/results/42";

function view(overrides: Partial<GithubPrChecksView> = {}): GithubPrChecksView {
  return {
    checks: null,
    loading: false,
    refreshing: false,
    error: null,
    stale: false,
    refresh: () => {},
    ...overrides,
  };
}

function check(overrides: Partial<GithubPrCheck> = {}): GithubPrCheck {
  return {
    name: "Pipeline",
    workflow: "",
    state: "pass",
    url: PIPELINE_URL,
    startedAt: null,
    completedAt: null,
    ...overrides,
  };
}

let root: Root;
let container: HTMLDivElement;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  invoke.mockReset();
  invoke.mockResolvedValue({ steps: [], annotations: [], notice: null });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

const show = async (
  checks: GithubPrCheck[],
  provider: "bitbucket" | "github" = "bitbucket",
) =>
  act(async () =>
    root.render(
      createElement(InboxPrChecks, {
        provider,
        // Bitbucket details do not depend on a local checkout.
        cwd: provider === "github" ? "/tmp/web" : "",
        repo: "acme/web",
        onRefresh() {},
        view: view({ checks: { headOid: "abc", checks } }),
      }),
    ),
  );

const byLabel = (label: string) =>
  container.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);

it("labels the tab Builds for Bitbucket and Checks for GitHub", () => {
  const overall: GithubPrChecksOverall = {
    kind: "pass",
    description: "2 passed",
  };
  const tab = (provider?: "bitbucket" | "github") =>
    act(() =>
      root.render(
        createElement(PrChecksTab, {
          overall,
          selected: false,
          onSelect() {},
          provider,
        }),
      ),
    );

  tab("bitbucket");
  expect(container.textContent).toContain("Builds");
  expect(container.textContent).not.toContain("Checks");
  expect(container.querySelector("button")?.getAttribute("aria-label")).toBe(
    "Builds: 2 passed",
  );

  tab();
  expect(container.textContent).toContain("Checks");
  expect(container.querySelector("button")?.getAttribute("aria-label")).toBe(
    "Checks: 2 passed",
  );
});

it("says builds throughout the body, and checks for GitHub", async () => {
  await show([check({ state: "fail" })]);
  expect(container.textContent).toContain("1 build needs a fix");
  expect(byLabel("Refresh builds")).not.toBeNull();
  expect(byLabel("All builds: 1")).not.toBeNull();
  expect(container.querySelector("section")?.getAttribute("aria-label")).toBe(
    "Pull request builds",
  );
  expect(container.textContent).not.toMatch(/\bchecks?\b/i);

  await show([check({ state: "pass" })]);
  expect(container.textContent).toContain("Builds passed");

  await show([]);
  expect(container.textContent).toContain("No builds reported");

  await show([check({ url: null, state: "fail" })], "github");
  expect(container.textContent).toContain("1 check needs a fix");
  expect(byLabel("Refresh checks")).not.toBeNull();
});

it("expands a Pipelines build into its steps without a local checkout", async () => {
  invoke.mockResolvedValue({
    steps: [
      {
        name: "Build",
        state: "pass",
        startedAt: "2026-10-07T10:00:00Z",
        completedAt: "2026-10-07T10:02:00Z",
      },
      { name: "Test", state: "fail", startedAt: null, completedAt: null },
    ],
    annotations: [],
    notice: null,
  });
  await show([check({ state: "pass" })]);

  await act(async () => byLabel("Pipeline details")!.click());

  expect(invoke).toHaveBeenCalledWith("bitbucket_build_details", {
    repo: "acme/web",
    build: "42",
  });
  expect(invoke).not.toHaveBeenCalledWith(
    "git_github_check_details",
    expect.anything(),
  );
  expect(container.textContent).toContain("View run steps");
  const steps = Array.from(container.querySelectorAll("ol li")).map(
    (li) => li.textContent,
  );
  expect(steps[0]).toContain("Build");
  expect(steps[1]).toContain("Test");
  expect(container.textContent).not.toContain("No steps reported");
});

it("still expands without the pipeline scope and says which scope to add", async () => {
  const notice =
    "Build steps need the read:pipeline:bitbucket scope. Atlassian API tokens cannot be changed after they are created, so create a new token that includes it, then reconnect in Settings.";
  invoke.mockResolvedValue({ steps: [], annotations: [], notice });
  // A failed build opens by itself, so the missing scope is explained up front.
  await show([check({ state: "fail" })]);

  expect(byLabel("Pipeline details")?.getAttribute("aria-expanded")).toBe(
    "true",
  );
  const status = container.querySelector('[role="status"]');
  expect(status?.textContent).toBe(notice);
  // Standing alone it must sit evenly between the container's top and bottom
  // padding, so it carries no margin of its own.
  expect(status?.className).not.toMatch(/\bm[tby]?-/);
  // The scope notice replaces, rather than sits beside, "no steps".
  expect(container.textContent).not.toContain("No steps reported");
  expect(container.querySelector('[role="alert"]')).toBeNull();
});

it("says there are no steps when a build really has none", async () => {
  invoke.mockResolvedValue({ steps: [], annotations: [], notice: null });
  await show([check()]);

  await act(async () => byLabel("Pipeline details")!.click());

  expect(container.textContent).toContain("No steps reported for this job.");
});

it("offers a retry when loading steps fails outright", async () => {
  invoke.mockRejectedValue(new Error("Could not reach Bitbucket"));
  await show([check()]);

  await act(async () => byLabel("Pipeline details")!.click());

  expect(container.querySelector('[role="alert"]')?.textContent).toContain(
    "Could not reach Bitbucket",
  );
  expect(container.textContent).toContain("Retry details");
});

it("links other builds out instead of expanding them", async () => {
  await show([
    check({
      name: "SonarCloud",
      url: "https://sonarcloud.io/dashboard?id=acme_web&pullRequest=12",
    }),
  ]);

  expect(byLabel("SonarCloud details")).toBeNull();
  expect(byLabel("Expand SonarCloud details")).toBeNull();
  expect(byLabel("View SonarCloud on Bitbucket")).not.toBeNull();
  expect(invoke).not.toHaveBeenCalled();
});

it("points the log link at Bitbucket, and at GitHub for GitHub", async () => {
  await show([check()]);
  expect(byLabel("View Pipeline on Bitbucket")?.title).toBe(
    "View full log on Bitbucket",
  );

  await show(
    [check({ url: "https://github.com/acme/web/actions/runs/1/job/2" })],
    "github",
  );
  expect(byLabel("View Pipeline on GitHub")?.title).toBe(
    "View full log on GitHub",
  );
});

it("centers the status label with the name and duration instead of riding the text baseline", async () => {
  await show([
    check({
      startedAt: "2026-10-07T10:00:00Z",
      completedAt: "2026-10-07T10:11:23Z",
    }),
  ]);

  const status = Array.from(container.querySelectorAll("span")).find(
    (el) => el.textContent === "Passed" && el.children.length === 0,
  );
  // A flex parent centers the label; a plain inline span in a block would sit
  // on the line box and land a few pixels below its neighbours.
  expect(status?.parentElement?.className).toContain("flex");
  expect(status?.parentElement?.className).toContain("items-center");
  expect(container.textContent).toContain("11m 23s");
});
