// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  createAutomationTrigger,
  listAutomations,
  newAutomationDraft,
  notifyAutomationsChanged,
  peekAutomations,
  type Automation,
} from "../model/automations";
import { SUPPORTED_INBOX_TRIGGER_EVENTS } from "../model/automationEvents";
import {
  AutomationsView,
  EventTriggerSentence,
  TRIGGER_EVENTS,
} from "./AutomationsView";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/core")>()),
  invoke,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isMaximized: async () => false,
    onResized: async () => () => {},
  }),
}));

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const storage = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
    removeItem: (key: string) => storage.delete(key),
  });
  notifyAutomationsChanged();
  invoke.mockReset();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

it("shows the cached automation list immediately and refreshes it without a loading screen", async () => {
  const automation: Automation = {
    ...newAutomationDraft("/work/project", "codex", "model"),
    id: "test-automation",
    name: "Daily review",
    nextRunAt: 0,
    createdAt: 1,
    updatedAt: 1,
  };
  invoke.mockResolvedValue([automation]);
  await listAutomations();
  let finish!: (automations: Automation[]) => void;
  const refresh = new Promise<Automation[]>((resolve) => {
    finish = resolve;
  });
  invoke.mockReturnValue(refresh);
  await act(async () =>
    root.render(
      createElement(AutomationsView, {
        cwd: "/work/project",
        recents: [],
        onClose: vi.fn(),
        onLaunch: vi.fn(),
        onOpenSession: vi.fn(),
      }),
    ),
  );
  expect(
    container.querySelector('[aria-label="Open Daily review"]'),
  ).not.toBeNull();
  expect(container.querySelector(".animate-spin")).toBeNull();

  await act(async () => finish([{ ...automation, name: "Updated review" }]));
  expect(
    container.querySelector('[aria-label="Open Updated review"]'),
  ).not.toBeNull();
  expect(
    container.querySelector('[aria-label="Open Daily review"]'),
  ).toBeNull();
});

it("invalidates the cached list when an automation changes", async () => {
  invoke.mockResolvedValue([]);
  await listAutomations();
  expect(peekAutomations()).toEqual([]);
  notifyAutomationsChanged();
  expect(peekAutomations()).toBeNull();
});

it("offers exactly the inbox events that can fire, each under its own label", () => {
  for (const [kind, supported] of Object.entries(SUPPORTED_INBOX_TRIGGER_EVENTS)) {
    const offered = TRIGGER_EVENTS[kind as keyof typeof TRIGGER_EVENTS];
    expect(offered.map((event) => event.value).sort()).toEqual(
      [...supported].sort(),
    );
    const labels = offered.map((event) => event.label);
    expect(labels.every((label) => label.trim())).toBe(true);
    expect(new Set(labels).size).toBe(labels.length);
  }
});

it("names the GitHub state-change triggers for what happened", () => {
  const labels = Object.fromEntries(
    TRIGGER_EVENTS.github.map((event) => [event.value, event.label]),
  );
  expect(labels).toMatchObject({
    issue_reopened: "Issue reopened",
    issue_closed: "Issue closed",
    pull_request_reopened: "Pull request reopened",
    pull_request_closed: "Pull request closed",
    pull_request_merged: "Pull request merged",
    pull_request_ready_for_review: "Pull request ready for review",
    issue_labeled: "Issue labeled",
    pull_request_labeled: "Pull request labeled",
  });
});

async function renderSentence(
  trigger: ReturnType<typeof createAutomationTrigger>,
  onChange = vi.fn(),
) {
  await act(async () =>
    root.render(
      createElement(EventTriggerSentence, {
        trigger,
        branchOptions: [],
        projectChosen: true,
        onChange,
      }),
    ),
  );
  return onChange;
}

it("lets a label-added trigger name the label it waits for", async () => {
  const trigger = createAutomationTrigger("github", "issue_labeled");
  const onChange = await renderSentence(trigger);
  const input = container.querySelector<HTMLInputElement>(
    'input[aria-label="Label"]',
  );
  expect(input).not.toBeNull();
  expect(input!.value).toBe("");
  expect(input!.placeholder).toBe("any label");
  expect(container.textContent).toContain("added to issue");

  const setValue = Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    "value",
  )!.set!;
  await act(async () => {
    setValue.call(input, "auto-fix");
    input!.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(onChange).toHaveBeenCalledWith({ ...trigger, label: "auto-fix" });
});

it("shows the saved label and words the sentence for pull requests", async () => {
  await renderSentence(
    createAutomationTrigger("github", "pull_request_labeled", {
      label: "needs-review",
    }),
  );
  expect(
    container.querySelector<HTMLInputElement>('input[aria-label="Label"]')
      ?.value,
  ).toBe("needs-review");
  expect(container.textContent).toContain("added to pull request");
});

it("has no label field on triggers that are not about labels", async () => {
  await renderSentence(createAutomationTrigger("github", "issue_reopened"));
  expect(container.querySelector('input[aria-label="Label"]')).toBeNull();
  expect(container.textContent).toContain("Issue reopened");
});

