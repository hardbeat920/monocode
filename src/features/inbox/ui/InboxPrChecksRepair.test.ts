vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { InboxPrChecks } from "./InboxPrChecks";
import { buildCiRepairRequest } from "../model/ciRepair";
import { trackCiRepair } from "../model/ciRepairTracking";
import { CheckRepairProgress } from "./CheckRepairProgress";
import type { GithubPrChecksView } from "../hooks/useGithubPrChecks";

// @vitest-environment happy-dom
const roots: Root[] = [];

it("only marks the selected job as repairing when check names repeat", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  roots.push(root);
  const first = {
    name: "tests",
    workflow: "CI",
    state: "fail" as const,
    url: "https://ci.example/jobs/1",
    startedAt: null,
    completedAt: null,
  };
  const second = { ...first, url: "https://ci.example/jobs/2" };
  trackCiRepair(
    "/duplicate-jobs",
    buildCiRepairRequest({
      repo: "acme/web",
      number: 42,
      headOid: "abc",
      evidence: [first],
    }),
    "first-chat",
    () => true,
  );
  await act(async () =>
    root.render(
      createElement(InboxPrChecks, {
        cwd: "/duplicate-jobs",
        repo: "acme/web",
        onRefresh() {},
        repair: { number: 42, sessions: [], onStart() {} },
        view: {
          checks: { headOid: "abc", checks: [first, second] },
          loading: false,
          refreshing: false,
          stale: false,
          error: null,
          refresh() {},
        },
      }),
    ),
  );
  expect(host.querySelectorAll("[data-repair-status]")).toHaveLength(1);
  act(() =>
    trackCiRepair(
      "/duplicate-jobs",
      buildCiRepairRequest({
        repo: "acme/web",
        number: 42,
        headOid: "abc",
        evidence: [second],
      }),
      "second-chat",
      () => true,
    ),
  );
  expect(
    host.querySelectorAll('[aria-label="Repair progress"] > div'),
  ).toHaveLength(2);
});

it("verifies a newer external CI result even when its dashboard URL stays the same", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  roots.push(root);
  const check = {
    name: "External tests",
    workflow: "",
    state: "fail" as const,
    url: "https://ci.example/project/web",
    startedAt: null,
    completedAt: null,
  };
  trackCiRepair(
    "/external-ci",
    buildCiRepairRequest({
      repo: "acme/web",
      number: 42,
      headOid: "old",
      evidence: [check],
    }),
    "external-chat",
    (settle) => {
      settle("completed");
      return true;
    },
  );
  await act(async () =>
    root.render(
      createElement(CheckRepairProgress, {
        cwd: "/external-ci",
        repo: "acme/web",
        repair: { number: 42, sessions: [], onStart() {} },
        view: {
          checks: {
            headOid: "new",
            checks: [
              {
                ...check,
                state: "pass",
                startedAt: new Date(Date.now() + 1000).toISOString(),
              },
            ],
          },
          loading: false,
          refreshing: false,
          stale: false,
          error: null,
          refresh() {},
        },
      }),
    ),
  );
  expect(host.textContent).toContain("CI passed");
});

it("does not use one newer result to verify two different jobs with the same name", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  roots.push(root);
  const check = {
    name: "tests",
    workflow: "CI",
    state: "fail" as const,
    url: "https://ci.example/jobs/1",
    startedAt: null,
    completedAt: null,
  };
  for (const id of [1, 2]) {
    trackCiRepair(
      "/ambiguous-jobs",
      buildCiRepairRequest({
        repo: "acme/web",
        number: 42,
        headOid: "old",
        evidence: [{ ...check, url: `https://ci.example/jobs/${id}` }],
      }),
      `chat-${id}`,
      (settle) => {
        settle("completed");
        return true;
      },
    );
  }
  await act(async () =>
    root.render(
      createElement(CheckRepairProgress, {
        cwd: "/ambiguous-jobs",
        repo: "acme/web",
        repair: { number: 42, sessions: [], onStart() {} },
        view: {
          checks: {
            headOid: "new",
            checks: [
              {
                ...check,
                state: "pass",
                url: "https://ci.example/jobs/3",
                startedAt: new Date(Date.now() + 1000).toISOString(),
              },
            ],
          },
          loading: false,
          refreshing: false,
          stale: false,
          error: null,
          refresh() {},
        },
      }),
    ),
  );
  expect(host.textContent).not.toContain("CI passed");
  expect(host.textContent).toContain("Awaiting new GitHub checks");
});

it.each([1, 0.8])(
  "reveals and expands the repaired check inside its PR scroller at scale %s",
  async (scale) => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValue({
      steps: [],
      annotations: [],
      notice: null,
    });
    const host = document.createElement("div");
    const shell = document.createElement("div");
    shell.style.overflow = "hidden";
    const panel = document.createElement("div");
    panel.setAttribute("data-inbox-detail-scroll", "");
    panel.style.overflowY = "auto";
    shell.append(panel);
    panel.append(host);
    document.body.append(shell);
    panel.scrollTop = 40;
    panel.scrollLeft = 12;
    vi.spyOn(panel, "getBoundingClientRect").mockReturnValue(
      new DOMRect(0, 100 * scale, 500 * scale, 400 * scale),
    );
    Object.defineProperty(panel, "offsetHeight", { value: 400 });
    Object.defineProperty(panel, "clientTop", { value: 2 });
    const root = createRoot(host);
    roots.push(root);
    const linux = {
      name: "Unit tests / Linux",
      workflow: "CI",
      state: "fail" as const,
      url: "https://github.com/acme/web/actions/runs/1/job/1",
      startedAt: null,
      completedAt: null,
    };
    const windows = {
      ...linux,
      name: "Unit tests / Windows",
      url: "https://github.com/acme/web/actions/runs/1/job/2",
    };
    const props = {
      cwd: `/single-followup-${scale}`,
      repo: "acme/web",
      onRefresh() {},
      repair: {
        number: 42,
        sessions: [],
        onStart() {},
        onOpenSession: vi.fn(),
      },
      view: {
        checks: { headOid: "old", checks: [linux, windows] },
        loading: false,
        refreshing: false,
        stale: false,
        error: null,
        refresh() {},
      },
    };
    await act(async () => root.render(createElement(InboxPrChecks, props)));
    expect(
      host
        .querySelector('[aria-label="Unit tests / Linux details"]')
        ?.getAttribute("aria-expanded"),
    ).toBe("true");
    act(() =>
      trackCiRepair(
        props.cwd,
        buildCiRepairRequest({
          repo: props.repo,
          number: 42,
          headOid: "old",
          evidence: [linux],
        }),
        "linux-chat",
        (settle) => {
          settle("completed");
          return true;
        },
      ),
    );
    await act(async () =>
      root.render(
        createElement(InboxPrChecks, {
          ...props,
          view: {
            ...props.view,
            checks: {
              headOid: "new",
              checks: [
                {
                  ...linux,
                  state: "pass",
                  url: "https://github.com/acme/web/actions/runs/2/job/3",
                  startedAt: new Date(Date.now() + 1000).toISOString(),
                },
                windows,
              ],
            },
          },
        }),
      ),
    );
    const card = host.querySelector('[aria-label="Repair progress"]')!;
    expect(card.textContent).toContain("Unit tests / Linux");
    expect(card.textContent).toContain("CI passed");
    expect(
      host
        .querySelector('[aria-label="Unit tests / Windows details"]')
        ?.getAttribute("aria-expanded"),
    ).toBe("false");
    const linuxRow = host
      .querySelector('[aria-label="Unit tests / Linux details"]')!
      .closest("li")!;
    vi.spyOn(linuxRow, "getBoundingClientRect").mockImplementation(
      () =>
        new DOMRect(
          0,
          (782 - panel.scrollTop) * scale,
          480 * scale,
          80 * scale,
        ),
    );
    const unboundedScroll = vi.fn();
    Object.defineProperty(linuxRow, "scrollIntoView", {
      value: unboundedScroll,
    });
    expect(linuxRow.closest("[hidden]")).not.toBeNull();
    const show = [...card.querySelectorAll("button")].find(
      (button) => button.textContent === "Show check",
    )!;
    expect(show).toBeDefined();
    await act(async () => show.click());
    expect(unboundedScroll).not.toHaveBeenCalled();
    expect(panel.scrollTop).toBeCloseTo(680);
    expect(panel.scrollLeft).toBe(12);
    expect(shell.scrollTop).toBe(0);
    expect(document.documentElement.scrollTop).toBe(0);
    expect(linuxRow.closest("[hidden]")).toBeNull();
    expect(
      host
        .querySelector('[aria-label="All checks: 2"]')
        ?.getAttribute("aria-pressed"),
    ).toBe("true");
    expect(document.activeElement).toBe(linuxRow);
    const detailsToggle = host.querySelector<HTMLButtonElement>(
      '[aria-label="Unit tests / Linux details"]',
    )!;
    expect(detailsToggle.getAttribute("aria-expanded")).toBe("true");
    await act(async () => detailsToggle.click());
    expect(detailsToggle.getAttribute("aria-expanded")).toBe("false");
    await act(async () => show.click());
    expect(detailsToggle.getAttribute("aria-expanded")).toBe("true");
  },
);

it("collapses a batch into one conversation card and keeps results in the check rows", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  roots.push(root);
  const checks = Array.from({ length: 10 }, (_, index) => ({
    name: `Test ${index + 1}`,
    workflow: "CI",
    state: "fail" as const,
    url: null,
    startedAt: null,
    completedAt: null,
  }));
  const open = vi.fn();
  let finish!: (outcome: "completed") => void;
  trackCiRepair(
    "/batch",
    buildCiRepairRequest({
      repo: "acme/web",
      number: 42,
      headOid: "old",
      evidence: checks,
    }),
    "batch-chat",
    (settle) => {
      finish = settle;
      return true;
    },
  );
  const props = {
    cwd: "/batch",
    repo: "acme/web",
    onRefresh() {},
    repair: { number: 42, sessions: [], onStart() {}, onOpenSession: open },
    view: {
      checks: { headOid: "old", checks },
      loading: false,
      refreshing: false,
      stale: false,
      error: null,
      refresh() {},
    },
  };
  await act(async () => root.render(createElement(InboxPrChecks, props)));
  const buttons = () =>
    [...host.querySelectorAll("button")].filter(
      (button) => button.textContent === "Open conversation",
    );
  expect(buttons()).toHaveLength(1);
  const toggle = host.querySelector<HTMLButtonElement>(
    'button[aria-label="Repair details for 10 checks"]',
  )!;
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  expect(
    host.querySelector('[aria-label="Repair progress"]')?.textContent,
  ).not.toContain("Test 1");
  expect(host.querySelectorAll("[data-repair-status]")).toHaveLength(10);
  act(() => toggle.click());
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
  expect(
    host.querySelector('[aria-label="Repair progress"]')?.textContent,
  ).toContain("Test 10");
  act(() => buttons()[0].click());
  expect(open).toHaveBeenCalledWith("batch-chat");
  act(() => {
    toggle.click();
    finish("completed");
  });
  await act(async () =>
    root.render(
      createElement(InboxPrChecks, {
        ...props,
        view: {
          ...props.view,
          checks: {
            headOid: "new",
            checks: checks.map((check, index) => ({
              ...check,
              state: index < 8 ? "pass" : "fail",
              startedAt: new Date(Date.now() + 1000).toISOString(),
            })),
          },
        },
      }),
    ),
  );
  const summary = host.querySelector('[aria-label="Repair progress"]')!;
  expect(summary.textContent).toContain("8 passed");
  expect(summary.textContent).toContain("2 still failing");
  expect(
    [...host.querySelectorAll("[data-repair-status]")].filter(
      (row) => row.textContent === "CI passed",
    ),
  ).toHaveLength(8);
  expect(
    [...host.querySelectorAll("[data-repair-status]")].filter(
      (row) => row.textContent === "Still failing",
    ),
  ).toHaveLength(2);
  expect(buttons()).toHaveLength(1);
});

it("shows live repair progress and opens the linked conversation", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  roots.push(root);
  const check = {
    name: "tests",
    workflow: "CI",
    state: "fail" as const,
    url: null,
    startedAt: null,
    completedAt: null,
  };
  const open = vi.fn();
  await act(async () =>
    root.render(
      createElement(InboxPrChecks, {
        cwd: "/progress",
        repo: "acme/web",
        onRefresh() {},
        view: {
          checks: { headOid: "abc", checks: [check] },
          loading: false,
          refreshing: false,
          stale: false,
          error: null,
          refresh() {},
        },
        repair: {
          number: 42,
          sessions: [{ id: "repair-chat", title: "Fix tests" }],
          onStart() {},
          onOpenSession: open,
        },
      }),
    ),
  );
  let finish!: (outcome: "completed") => void;
  act(() =>
    trackCiRepair(
      "/progress",
      buildCiRepairRequest({
        repo: "acme/web",
        number: 42,
        headOid: "abc",
        evidence: [check],
      }),
      "repair-chat",
      (settle) => {
        finish = settle;
        return true;
      },
    ),
  );
  expect(host.textContent).toContain("Repair in progress");
  await act(async () =>
    [...host.querySelectorAll("button")]
      .find((button) => button.textContent === "Open conversation")!
      .click(),
  );
  expect(open).toHaveBeenCalledWith("repair-chat");
  act(() => finish("completed"));
  expect(host.textContent).not.toContain("Agent finished");
  expect(host.textContent).toContain("Awaiting new GitHub checks");
  expect(host.textContent).not.toContain("Repair in progress");
});

it("reports fresh checks for a later PR commit without confirming old or stale results", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  roots.push(root);
  const check = {
    name: "tests",
    workflow: "CI",
    state: "fail" as const,
    url: "https://github.com/acme/web/actions/runs/1/job/2",
    startedAt: null,
    completedAt: null,
  };
  trackCiRepair(
    "/verification",
    buildCiRepairRequest({
      repo: "acme/web",
      number: 42,
      headOid: "original",
      evidence: [check],
    }),
    "chat",
    (settle) => {
      settle("completed");
      return true;
    },
  );
  const passed = {
    ...check,
    state: "pass" as const,
    url: "https://github.com/acme/web/actions/runs/3/job/4",
    startedAt: new Date(Date.now() + 1000).toISOString(),
  };
  const render = async (
    headOid: string,
    checks = [passed],
    state: Partial<GithubPrChecksView> = {},
  ) => {
    await act(async () =>
      root.render(
        createElement(CheckRepairProgress, {
          cwd: "/verification",
          repo: "acme/web",
          repair: { number: 42, sessions: [], onStart() {} },
          view: {
            checks: { headOid, checks },
            stale: false,
            refreshing: false,
            loading: false,
            error: null,
            refresh() {},
            ...state,
          },
        }),
      ),
    );
  };
  await render("original");
  expect(host.textContent).toContain("Awaiting new GitHub checks");
  await render("new-commit", [
    { ...passed, url: check.url, startedAt: "2000-01-01T00:00:00Z" },
  ]);
  expect(host.textContent).toContain("Awaiting new GitHub checks");
  await render("new-commit");
  expect(host.textContent).toContain("CI passed");
  expect(host.textContent).not.toContain("new-com");
  act(() =>
    host
      .querySelector<HTMLButtonElement>(
        'button[aria-label="Repair details for 1 check"]',
      )!
      .click(),
  );
  expect(host.textContent).toContain("new-com");
  await render("new-commit", [passed], { stale: true });
  expect(host.textContent).not.toContain("CI passed");
  expect(host.textContent).toContain("GitHub results are out of date");
  await render("new-commit", [passed], { refreshing: true });
  expect(host.textContent).not.toContain("CI passed");
  await render("another-commit", []);
  expect(host.textContent).toContain("Awaiting new GitHub checks");
  await render("another-commit", [passed, passed]);
  expect(host.textContent).toContain("Awaiting new GitHub checks");
});
afterEach(() => {
  act(() => roots.splice(0).forEach((root) => root.unmount()));
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});
it("starts a repair with all failed checks and the selected project chat", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  roots.push(root);
  const start = vi.fn();
  const check = (name: string, state: "fail" | "pass") => ({
    name,
    state,
    workflow: "CI",
    url: null,
    startedAt: null,
    completedAt: null,
  });
  await act(async () =>
    root.render(
      createElement(InboxPrChecks, {
        cwd: "/web",
        repo: "acme/web",
        onRefresh() {},
        view: {
          checks: {
            headOid: "abc123",
            checks: [
              check("lint", "fail"),
              check("tests", "fail"),
              check("build", "pass"),
            ],
          },
          loading: false,
          refreshing: false,
          stale: false,
          error: null,
          refresh() {},
        },
        repair: {
          number: 42,
          sessions: [
            { id: "chat1", title: "Repair tests" },
            { id: "chat2", title: "Unrelated work" },
          ],
          onStart: start,
        },
      }),
    ),
  );
  const button = (text: string) =>
    [...document.querySelectorAll("button")].find(
      (b) => b.getAttribute("aria-label") === text || b.textContent === text,
    )!;
  expect(button("Fix all failed")).toBeDefined();
  await act(async () => button("Fix all failed").click());
  const search = document.querySelector<HTMLInputElement>(
    'input[placeholder="Search chats..."]',
  );
  expect(search).not.toBeNull();
  expect(document.activeElement).toBe(search);
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(search, "Repair");
    search!.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(
    document.querySelector('[role="option"][aria-label="Unrelated work"]'),
  ).toBeNull();
  await act(async () => {
    search!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
    );
  });
  await act(async () => {
    search!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );
  });
  await act(async () => button("Start fix").click());
  expect(start).toHaveBeenCalledWith(
    {
      text: "Fix 2 failed CI checks for acme/web PR #42.",
      prompt: expect.stringContaining("abc123"),
      target: {
        repo: "acme/web",
        number: 42,
        headOid: "abc123",
        checks: [
          { name: "lint", workflow: "CI", url: null },
          { name: "tests", workflow: "CI", url: null },
        ],
      },
    },
    "chat1",
  );
  expect(start.mock.calls[0][0].prompt).toContain("lint");
  expect(start.mock.calls[0][0].prompt).toContain("tests");
  expect(start.mock.calls[0][0].prompt).not.toContain('"name":"build"');
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

it("does not start a repair after leaving checks while details load", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const { CheckRepairForm } = await import("./CheckRepairForm");
  const { invoke } = await import("@tauri-apps/api/core");
  let resolve!: (value: unknown) => void;
  vi.mocked(invoke).mockImplementation(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  roots.push(root);
  const start = vi.fn();
  await act(async () =>
    root.render(
      createElement(CheckRepairForm, {
        anchor: host,
        checks: [
          {
            name: "lint",
            state: "fail",
            workflow: "CI",
            url: "https://github.com/acme/web/actions/runs/1/job/2",
            startedAt: null,
            completedAt: null,
          },
        ],
        headOid: "abc",
        cwd: "/web",
        repo: "acme/web",
        repair: { number: 42, sessions: [], onStart: start },
        onClose() {},
      }),
    ),
  );
  await act(async () =>
    [...document.querySelectorAll("button")]
      .find((button) => button.textContent === "Start fix")!
      .click(),
  );
  act(() => root.unmount());
  host.remove();
  await act(async () => resolve({ steps: [], annotations: [], notice: null }));
  expect(start).not.toHaveBeenCalled();
  vi.unstubAllGlobals();
});

it("prepares several CI jobs concurrently before starting a repair", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const { CheckRepairForm } = await import("./CheckRepairForm");
  const { invoke } = await import("@tauri-apps/api/core");
  const resolveDetails: Array<(value: unknown) => void> = [];
  vi.mocked(invoke).mockImplementation(
    () =>
      new Promise((resolve) => {
        resolveDetails.push(resolve);
      }),
  );
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  roots.push(root);
  const start = vi.fn();
  await act(async () =>
    root.render(
      createElement(CheckRepairForm, {
        anchor: host,
        checks: Array.from({ length: 4 }, (_, index) => ({
          name: `tests-${index}`,
          state: "fail" as const,
          workflow: "CI",
          url: `https://github.com/acme/web/actions/runs/1/job/${index + 1}`,
          startedAt: null,
          completedAt: null,
        })),
        headOid: "abc",
        cwd: "/web",
        repo: "acme/web",
        repair: { number: 42, sessions: [], onStart: start },
        onClose() {},
      }),
    ),
  );
  await act(async () =>
    [...document.querySelectorAll("button")]
      .find((button) => button.textContent === "Start fix")!
      .click(),
  );
  expect(resolveDetails).toHaveLength(3);
  await act(async () =>
    resolveDetails[0]({ steps: [], annotations: [], notice: null }),
  );
  expect(resolveDetails).toHaveLength(4);
  await act(async () => {
    for (const resolve of resolveDetails.slice(1)) {
      resolve({ steps: [], annotations: [], notice: null });
    }
  });
  expect(start).toHaveBeenCalledTimes(1);
});

it.each(["results", "pr"])(
  "closes the repair selection when %s changes",
  async (change) => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    roots.push(root);
    const failed = {
      name: "lint",
      state: "fail" as const,
      workflow: "CI",
      url: null,
      startedAt: null,
      completedAt: null,
    };
    const props = {
      cwd: "/web",
      repo: "acme/web",
      onRefresh() {},
      repair: { number: 42, sessions: [], onStart: vi.fn() },
      view: {
        checks: { headOid: "abc", checks: [failed] },
        loading: false,
        refreshing: false,
        stale: false,
        error: null,
        refresh() {},
      },
    };
    await act(async () => root.render(createElement(InboxPrChecks, props)));
    await act(async () =>
      host
        .querySelector<HTMLButtonElement>(
          'button[aria-label="Fix lint with AI"]',
        )!
        .click(),
    );
    expect(
      document.querySelector(
        '[role="dialog"][aria-label="Fix checks with AI"]',
      ),
    ).not.toBeNull();
    await act(async () =>
      root.render(
        createElement(InboxPrChecks, {
          ...props,
          view: {
            ...props.view,
            refreshing: change === "refreshing",
            checks:
              change === "results"
                ? { headOid: "abc", checks: [{ ...failed, state: "pass" }] }
                : props.view.checks,
          },
          repair:
            change === "pr" ? { ...props.repair, number: 43 } : props.repair,
        }),
      ),
    );
    expect(
      document.querySelector(
        '[role="dialog"][aria-label="Fix checks with AI"]',
      ),
    ).toBeNull();
    act(() => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  },
);

it.each(["refreshing", "failed"] as const)(
  "keeps a pending repair through a %s checks refresh",
  async (refreshState) => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    const { invoke } = await import("@tauri-apps/api/core");
    let resolveDetails!: (value: unknown) => void;
    vi.mocked(invoke).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveDetails = resolve;
        }),
    );
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    roots.push(root);
    const check = {
      name: "lint",
      state: "fail" as const,
      workflow: "CI",
      url: "https://github.com/acme/web/actions/runs/1/job/2",
      startedAt: null,
      completedAt: null,
    };
    const start = vi.fn();
    const props = {
      cwd: "/web",
      repo: "acme/web",
      onRefresh() {},
      repair: { number: 42, sessions: [], onStart: start },
      view: {
        checks: { headOid: "abc", checks: [check] },
        loading: false,
        refreshing: false,
        stale: false,
        error: null,
        refresh() {},
      },
    };
    await act(async () => root.render(createElement(InboxPrChecks, props)));
    await act(async () =>
      host.querySelector<HTMLButtonElement>('button[aria-label="Fix lint with AI"]')!.click(),
    );
    await act(async () =>
      [...document.querySelectorAll("button")]
        .find((button) => button.textContent === "Start fix")!
        .click(),
    );
    await act(async () =>
      root.render(
        createElement(InboxPrChecks, {
          ...props,
          view: {
            ...props.view,
            refreshing: refreshState === "refreshing",
            stale: refreshState === "failed",
            error: refreshState === "failed" ? "network down" : null,
          },
        }),
      ),
    );
    expect(
      document.querySelector('[role="dialog"][aria-label="Fix checks with AI"]'),
    ).not.toBeNull();
    await act(async () =>
      resolveDetails({ steps: [], annotations: [], notice: null }),
    );
    expect(start).toHaveBeenCalledTimes(1);
  },
);

it("keeps the selected failed job when another check changes", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  roots.push(root);
  const failed = {
    name: "lint",
    state: "fail" as const,
    workflow: "CI",
    url: null,
    startedAt: null,
    completedAt: null,
  };
  const other = { ...failed, name: "build", state: "pending" as const };
  const props = {
    cwd: "/web",
    repo: "acme/web",
    onRefresh() {},
    repair: { number: 42, sessions: [], onStart() {} },
    view: {
      checks: { headOid: "abc", checks: [failed, other] },
      loading: false,
      refreshing: false,
      stale: false,
      error: null,
      refresh() {},
    },
  };
  await act(async () => root.render(createElement(InboxPrChecks, props)));
  await act(async () =>
    host.querySelector<HTMLButtonElement>('button[aria-label="Fix lint with AI"]')!.click(),
  );
  await act(async () =>
    root.render(
      createElement(InboxPrChecks, {
        ...props,
        view: {
          ...props.view,
          checks: {
            headOid: "abc",
            checks: [failed, { ...other, state: "pass" as const }],
          },
        },
      }),
    ),
  );
  expect(
    document.querySelector('[role="dialog"][aria-label="Fix checks with AI"]'),
  ).not.toBeNull();
});
