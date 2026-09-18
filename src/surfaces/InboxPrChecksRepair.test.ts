vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { InboxPrChecks } from "./InboxPrChecks";

// @vitest-environment happy-dom
const roots: Root[] = [];
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

it("closes the repair selection when check results refresh", async () => {
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
    document.querySelector('[role="dialog"][aria-label="Fix checks with AI"]'),
  ).not.toBeNull();
  await act(async () =>
    root.render(
      createElement(InboxPrChecks, {
        ...props,
        view: {
          ...props.view,
          checks: { headOid: "abc", checks: [{ ...failed, state: "pass" }] },
        },
      }),
    ),
  );
  expect(
    document.querySelector('[role="dialog"][aria-label="Fix checks with AI"]'),
  ).toBeNull();
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});
