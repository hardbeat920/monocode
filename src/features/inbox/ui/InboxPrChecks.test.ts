// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { InboxPrChecks, PrChecksTab } from "./InboxPrChecks";
import { InboxDetail, LinkedWorkItemPanel } from "./InboxView";
import type { GithubPrChecksView } from "../hooks/useGithubPrChecks";
import type {
  GithubPrCheck,
  GithubPrChecksOverall,
} from "../model/githubPrChecks";

const { openUrl, invoke } = vi.hoisted(() => ({
  openUrl: vi.fn(),
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isMaximized: async () => false,
    onResized: async () => () => {},
    setTitle: async () => {},
  }),
}));

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
    name: "ci/build",
    workflow: "Build",
    state: "pass",
    url: null,
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
  openUrl.mockReset();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

const render = (element: React.ReactElement) => {
  act(() => root.render(element));
  return container;
};

const buttonByLabel = (label: string) =>
  container.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);

it("filters attention checks without hiding cancelled or unknown outcomes", () => {
  render(
    createElement(InboxPrChecks, {
      view: view({
        checks: {
          headOid: "abc",
          checks: [
            check({ name: "Tests", state: "fail" }),
            check({ name: "Build", state: "pending" }),
            check({ name: "Deploy", state: "cancel" }),
            check({ name: "External scan", state: "unknown" }),
            check({ name: "Lint", state: "pass" }),
            check({ name: "Publish", state: "skipping" }),
          ],
        },
      }),
      onRefresh: () => {},
    }),
  );
  const attention = buttonByLabel("Needs attention: 4");
  expect(attention?.getAttribute("aria-pressed")).toBe("true");
  const visibleNames = () =>
    Array.from(container.querySelectorAll("li:not([hidden]) [data-check-name]"))
      .filter((el) => !el.closest("[hidden]"))
      .map((el) => el.textContent);
  expect(visibleNames()).toEqual(["Tests", "Build", "Deploy", "External scan"]);
  act(() => buttonByLabel("All checks: 6")?.click());
  expect(visibleNames()).toEqual([
    "Tests",
    "Build",
    "Deploy",
    "External scan",
    "Lint",
    "Publish",
  ]);
  act(() => attention?.click());
  expect(visibleNames()).toEqual(["Tests", "Build", "Deploy", "External scan"]);
});

it("shows annotation source from the checked commit and links to that same revision", async () => {
  const headOid = "a".repeat(40);
  invoke.mockImplementation(async (command) => {
    if (command === "git_github_check_details")
      return {
        steps: [],
        notice: null,
        annotations: [
          {
            path: "src/preview test.ts",
            line: 2,
            level: "failure",
            message: "Assertion failed\nExpected: 200\nReceived: 500",
          },
        ],
      };
    if (command === "git_commit_file_diff")
      return {
        current:
          "const status = response.status;\nexpect(status).toBe(200);\nfinish();",
        original: "",
        binary: false,
        tooLarge: false,
      };
    throw new Error("Unexpected command");
  });
  render(
    createElement(InboxPrChecks, {
      cwd: "/tmp/web",
      repo: "acme/web",
      onRefresh: () => {},
      view: view({
        checks: {
          headOid,
          checks: [
            check({
              name: "Tests",
              state: "fail",
              url: "https://github.com/acme/web/actions/runs/9/job/123",
            }),
          ],
        },
      }),
    }),
  );
  await act(async () => {});
  expect(invoke).toHaveBeenCalledWith("git_commit_file_diff", {
    cwd: "/tmp/web",
    sha: headOid,
    relative: "src/preview test.ts",
  });
  expect(container.textContent).toContain("expect(status).toBe(200);");
  expect(container.textContent).toContain("Received: 500");
  await act(async () =>
    buttonByLabel("View src/preview test.ts:2 on GitHub")?.click(),
  );
  expect(openUrl).toHaveBeenCalledWith(
    `https://github.com/acme/web/blob/${headOid}/src/preview%20test.ts#L2`,
  );
});

it("opens the only failed Actions job and shows its failed step and error", async () => {
  invoke.mockResolvedValueOnce({
    steps: [
      { name: "Install", state: "pass", startedAt: null, completedAt: null },
      {
        name: "Run tests",
        state: "fail",
        startedAt: "2030-01-01T10:00:00Z",
        completedAt: "2030-01-01T10:00:12Z",
      },
    ],
    annotations: [
      {
        path: "src/app.test.ts",
        line: 42,
        message: "Expected 2, received 1",
        level: "failure",
      },
    ],
    notice: null,
  });
  render(
    createElement(InboxPrChecks, {
      cwd: "/tmp/web",
      repo: "acme/web",
      view: view({
        checks: {
          headOid: "abc",
          checks: [
            check({
              name: "Windows",
              state: "fail",
              url: "https://github.com/acme/web/actions/runs/9/job/123",
            }),
          ],
        },
      }),
      onRefresh: () => {},
    }),
  );
  await act(async () => {});
  expect(invoke).toHaveBeenCalledWith("git_github_check_details", {
    cwd: "/tmp/web",
    repo: "acme/web",
    jobId: "123",
  });
  expect(container.textContent).toContain("1 failed");
  expect(container.textContent).toContain("Failed at Run tests");
  expect(container.textContent).toContain("Expected 2, received 1");
  expect(container.textContent).toContain("src/app.test.ts:42");
  expect(container.textContent).toContain("12s");
  const toggle = buttonByLabel("Windows details");
  expect(toggle?.getAttribute("aria-expanded")).toBe("true");
  act(() => toggle?.click());
  expect(toggle?.getAttribute("aria-expanded")).toBe("false");
  expect(container.textContent).not.toContain("src/app.test.ts:42");
  expect(container.textContent).toContain("Expected 2, received 1");
  expect(container.textContent).toContain("Failed at Run tests");
});

it("clears the old failed step when a collapsed job is refreshed", async () => {
  invoke.mockResolvedValue({
    steps: [
      {
        name: "Old failure",
        state: "fail",
        startedAt: null,
        completedAt: null,
      },
    ],
    annotations: [],
    notice: null,
  });
  const job = check({
    name: "Windows",
    state: "fail",
    url: "https://github.com/acme/web/actions/runs/9/job/123",
  });
  const props = { cwd: "/tmp/web", repo: "acme/web", onRefresh: () => {} };
  render(
    createElement(InboxPrChecks, {
      ...props,
      view: view({ checks: { headOid: "abc", checks: [job] } }),
    }),
  );
  await act(async () => {});
  act(() => buttonByLabel("Windows details")?.click());
  expect(container.textContent).toContain("Failed at Old failure");
  render(
    createElement(InboxPrChecks, {
      ...props,
      view: view({
        checks: { headOid: "abc", checks: [{ ...job, state: "pending" }] },
      }),
    }),
  );
  expect(container.textContent).not.toContain("Old failure");
  expect(buttonByLabel("Windows details")?.getAttribute("aria-expanded")).toBe(
    "false",
  );
});

it("opens a job that fails during polling and lets the user retry unavailable details", async () => {
  const job = check({
    name: "Linux",
    state: "pending",
    url: "https://github.com/acme/web/actions/runs/9/job/124",
  });
  const props = { cwd: "/tmp/web", repo: "acme/web", onRefresh: () => {} };
  render(
    createElement(InboxPrChecks, {
      ...props,
      view: view({ checks: { headOid: "abc", checks: [job] } }),
    }),
  );
  await act(async () => {});
  expect(invoke).not.toHaveBeenCalled();
  invoke.mockRejectedValueOnce(new Error("Request timed out"));
  render(
    createElement(InboxPrChecks, {
      ...props,
      view: view({
        checks: { headOid: "abc", checks: [{ ...job, state: "fail" }] },
      }),
    }),
  );
  await act(async () => {});
  expect(buttonByLabel("Linux details")?.getAttribute("aria-expanded")).toBe(
    "true",
  );
  expect(container.textContent).toContain("Could not load job details.");
  invoke.mockResolvedValueOnce({
    steps: [
      { name: "Build", state: "fail", startedAt: null, completedAt: null },
    ],
    annotations: [],
    notice: null,
  });
  await act(async () =>
    Array.from(container.querySelectorAll("button"))
      .find((button) => button.textContent === "Retry details")
      ?.click(),
  );
  expect(container.textContent).toContain("Failed at Build");
  expect(container.textContent).toContain("No error annotations reported.");
  expect(container.textContent).not.toContain("Request timed out");
});

it("renders grouped rows with workflow, status, duration and an opener link", async () => {
  const body = render(
    createElement(InboxPrChecks, {
      view: view({
        checks: {
          headOid: "abc123",
          checks: [
            check({
              name: "skip job",
              state: "skipping",
              workflow: "Lint",
            }),
            check({
              name: "test job",
              state: "pass",
              url: "https://github.com/acme/web/actions/runs/9",
              startedAt: "2030-01-01T10:00:00Z",
              completedAt: "2030-01-01T10:01:05Z",
            }),
            check({ name: "failed job", state: "fail", workflow: "" }),
          ],
        },
      }),
      onRefresh: () => {},
    }),
  );

  const text = body.textContent ?? "";
  expect(text.indexOf("failed job")).toBeLessThan(text.indexOf("test job"));
  expect(text.indexOf("test job")).toBeLessThan(text.indexOf("skip job"));
  expect(text).toContain("Build · Passed · 1m 05s");
  expect(text).toContain("Failed");
  expect(text).toContain("Skipped");

  const failedRow = buttonByLabel("failed job · Failed");
  expect(failedRow).toBeNull();
  const linkedRow = buttonByLabel("test job · Passed, took 1m 05s, Build");
  expect(linkedRow).not.toBeNull();
  await act(async () => {
    linkedRow?.click();
  });
  expect(openUrl).toHaveBeenCalledWith(
    "https://github.com/acme/web/actions/runs/9",
  );
});

it("keeps rows without a valid HTTP(S) URL unlinked", async () => {
  render(
    createElement(InboxPrChecks, {
      view: view({
        checks: {
          headOid: "abc123",
          checks: [
            check({ name: "ftp job", url: "ftp://ci/run/1" }),
            check({ name: "no url job", url: null }),
          ],
        },
      }),
      onRefresh: () => {},
    }),
  );
  expect(container.querySelectorAll("li button")).toHaveLength(0);
  expect(buttonByLabel("Refresh checks")).not.toBeNull();
  expect(container.textContent).toContain("ftp job");
  expect(container.textContent).toContain("no url job");
  expect(openUrl).not.toHaveBeenCalled();
});

it("separates the initial loading state from the no-checks state", () => {
  const loading = render(
    createElement(InboxPrChecks, {
      view: view({ loading: true }),
      onRefresh: () => {},
    }),
  );
  expect(loading.querySelector(".animate-spin")).not.toBeNull();
  expect(loading.textContent).not.toContain("No checks reported");

  const empty = render(
    createElement(InboxPrChecks, {
      view: view({ checks: { headOid: "abc", checks: [] } }),
      onRefresh: () => {},
    }),
  );
  expect(empty.querySelector(".animate-spin")).toBeNull();
  expect(empty.textContent).toContain("No checks reported");
});

it("offers a retry after a load error and refreshes on demand", async () => {
  const onRefresh = vi.fn();
  const failed = render(
    createElement(InboxPrChecks, {
      view: view({ error: "rate limited" }),
      onRefresh,
    }),
  );
  expect(failed.querySelector('[role="alert"]')?.textContent).toBe(
    "rate limited",
  );
  const retry = buttonByLabel("Retry loading checks");
  expect(retry).not.toBeNull();
  await act(async () => {
    retry?.click();
  });
  expect(onRefresh).toHaveBeenCalledTimes(1);
});

it("marks kept results as out of date after a failed refresh", () => {
  const kept = render(
    createElement(InboxPrChecks, {
      view: view({
        error: "rate limited",
        stale: true,
        refreshing: false,
        checks: { headOid: "abc", checks: [check()] },
      }),
      onRefresh: () => {},
    }),
  );
  expect(kept.querySelector('[role="status"]')?.textContent).toContain(
    "out of date",
  );
  expect(kept.textContent).toContain("ci/build");
});

it("names the per-state counts on the tab for color-blind-safe reading", () => {
  const fail: GithubPrChecksOverall = {
    kind: "fail",
    failed: 2,
    description: "2 failed, 1 passed",
  };
  const tab = render(
    createElement(PrChecksTab, {
      overall: fail,
      selected: false,
      onSelect: () => {},
    }),
  );
  const button = tab.querySelector('button[role="tab"]');
  expect(button?.getAttribute("aria-label")).toBe("Checks: 2 failed, 1 passed");
  expect(button?.getAttribute("title")).toBe("Checks: 2 failed, 1 passed");
  expect(button?.textContent).toContain("Checks");
  expect(button?.textContent).toContain("2");

  const neutral = render(
    createElement(PrChecksTab, {
      overall: { kind: "neutral", description: "3 skipped" },
      selected: true,
      onSelect: () => {},
    }),
  );
  expect(
    neutral.querySelector('button[role="tab"]')?.getAttribute("aria-label"),
  ).toBe("Checks: 3 skipped");

  const loading = render(
    createElement(PrChecksTab, {
      overall: { kind: "loading", description: "Loading checks" },
      selected: false,
      onSelect: () => {},
    }),
  );
  expect(
    loading.querySelector('button[role="tab"]')?.getAttribute("aria-label"),
  ).toBe("Checks: Loading checks");
});

it("keeps rows compact enough for a 360 px panel", () => {
  const rows = render(
    createElement(InboxPrChecks, {
      view: view({
        checks: {
          headOid: "abc",
          checks: [
            check({
              name: "a very long check name that must truncate instead of pushing the row wide",
              workflow: "A long workflow name here too",
              url: "https://github.com/acme/web/actions/runs/9",
            }),
          ],
        },
      }),
      onRefresh: () => {},
    }),
  );
  const row = rows.querySelector("li button");
  expect(row?.className).toContain("w-full");
  const name = row?.querySelector(".truncate");
  expect(name?.className).toContain("min-w-0");
  expect(row?.textContent).not.toContain(
    "pushes the row wide", // nothing renders untruncated even for long names
  );
});

const prItem = {
  kind: "pr" as const,
  number: 157,
  title: "Ship the checks tab",
  url: "https://github.com/acme/web/pull/157",
  state: "open",
  updatedAt: "2026-09-11T08:00:00Z",
  labels: [],
  assignees: [],
  draft: false,
  repo: "acme/web",
};

const checksAnswer = {
  headOid: "abc123",
  checks: [
    {
      name: "build",
      workflow: "CI",
      state: "pass",
      url: "https://github.com/acme/web/actions/runs/9",
      startedAt: "2026-09-11T08:00:00Z",
      completedAt: "2026-09-11T08:01:00Z",
    },
  ],
};

function mockBackend() {
  invoke.mockImplementation(
    (command: string, args: Record<string, unknown>) => {
      if (command === "git_github_pr_checks") {
        return Promise.resolve(checksAnswer);
      }
      if (command === "git_github_work_item") {
        return Promise.resolve(prItem);
      }
      return Promise.reject(new Error("No native bridge"));
    },
  );
}

const flush = () => act(async () => {});

describe("Checks tab user behavior", () => {
  beforeEach(() => {
    invoke.mockReset();
    openUrl.mockReset();
  });

  it("loads checks on open in the inbox even while Summary is active", async () => {
    mockBackend();
    render(
      createElement(InboxDetail, {
        item: { ...prItem, projectPath: "/tmp/web", provider: "github" },
        cwd: "/tmp/web",
        projects: [],
        revision: 0,
        relatedSessions: [],
        onDiscuss: () => {},
        onStart: () => {},
      }),
    );
    await flush();
    expect(invoke).toHaveBeenCalledWith("git_github_pr_checks", {
      cwd: "/tmp/web",
      repo: "acme/web",
      number: 157,
    });
    const summaryTab = container.querySelector<HTMLButtonElement>(
      'button[role="tab"][aria-selected="true"]',
    );
    expect(summaryTab?.textContent).toContain("Summary");

    const checksTab = container.querySelector<HTMLButtonElement>(
      'button[role="tab"][aria-label="Checks: 1 passed"]',
    );
    expect(checksTab).not.toBeNull();
    await act(async () => {
      checksTab?.click();
    });
    expect(container.textContent).toContain("build");
    expect(container.textContent).toContain("CI · Passed · 1m 00s");
    const row = buttonByLabel("build · Passed, took 1m 00s, CI");
    await act(async () => {
      row?.click();
    });
    expect(openUrl).toHaveBeenCalledWith(
      "https://github.com/acme/web/actions/runs/9",
    );

    // An inbox revision change revalidates the same PR.
    const checksCalls = () =>
      invoke.mock.calls.filter(
        ([command]) => command === "git_github_pr_checks",
      );
    expect(checksCalls()).toHaveLength(1);
    render(
      createElement(InboxDetail, {
        item: { ...prItem, projectPath: "/tmp/web", provider: "github" },
        cwd: "/tmp/web",
        projects: [],
        revision: 1,
        relatedSessions: [],
        onDiscuss: () => {},
        onStart: () => {},
      }),
    );
    await flush();
    expect(checksCalls()).toHaveLength(2);
  });

  it("loads checks in the linked side panel where revision stays 0", async () => {
    mockBackend();
    render(
      createElement(LinkedWorkItemPanel, {
        target: {
          kind: "pr",
          repo: "acme/web",
          number: 157,
          url: "https://github.com/acme/web/pull/157",
        },
        cwd: "/tmp/web",
        recents: [],
        onClose: () => {},
      }),
    );
    await flush();
    await flush();
    expect(invoke).toHaveBeenCalledWith("git_github_pr_checks", {
      cwd: "/tmp/web",
      repo: "acme/web",
      number: 157,
    });
    expect(
      container.querySelector(
        'button[role="tab"][aria-label="Checks: 1 passed"]',
      ),
    ).not.toBeNull();
    await act(async () => {
      (
        container.querySelector(
          'button[role="tab"][aria-label="Checks: 1 passed"]',
        ) as HTMLButtonElement
      )?.click();
    });
    expect(container.textContent).toContain("build");
  });
});
