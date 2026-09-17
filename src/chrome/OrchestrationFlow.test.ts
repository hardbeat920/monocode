// @vitest-environment happy-dom
import { act, createElement, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => []),
  isTauri: () => false,
  convertFileSrc: (path: string) => path,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ onFocusChanged: async () => () => {} }),
}));
vi.mock("../lib/harness/availability", () => ({
  getHarnessAvailabilitySnapshot: () => 0,
  hasProbedHarnessAvailability: () => true,
  isHarnessAvailable: () => true,
  harnessUnavailableHint: () => "",
  probeHarnessAvailability: async () => {},
  subscribeHarnessAvailability: () => () => {},
}));
vi.mock("../lib/harness/registry", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/harness/registry")>()),
  refreshHarnessCatalogs: async () => {},
  isLiveHarness: () => true,
}));
vi.mock("./ModelPicker", () => ({ ModelPicker: () => null }));
vi.mock("./SessionReview", () => ({
  SessionReview: ({ undoLocked }: { undoLocked: boolean }) =>
    createElement("div", { "data-review-undo-locked": String(undoLocked) }),
}));
vi.mock("../lib/orchestration", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/orchestration")>()),
  orchestrator: {
    subscribe: () => () => {},
    snapshot: () => emptyRuns,
    hydrate: async () => {},
    waitingFor: () => undefined,
    resumeBlocker: vi.fn(() => undefined),
    resumeLeadBusy: vi.fn(() => false),
    start: vi.fn(async () => {}),
    cancelTask: vi.fn(async () => {}),
  },
}));

import { Composer } from "./Composer";
import { AgentTranscript } from "../surfaces/AgentTranscript";
import { SessionPane } from "../surfaces/SessionPane";
import {
  OrchestrationActions,
  OrchestrationWorkers,
} from "./OrchestrationActions";
import { newSession } from "../lib/session";
import {
  orchestrator,
  type OrchestrationRun,
  type OrchestrationTask,
} from "../lib/orchestration";
import type { OrchestrationSummary } from "../lib/orchestrationSummary";
import { OrchestrationSidebarAgents } from "./OrchestrationSidebarAgents";
import {
  modelsFor,
  setHarnessModels,
  resetHarnessModelOverlays,
} from "../lib/models";
import type { OrchestrationProposal } from "../lib/orchestrationPlan";
import { invoke } from "@tauri-apps/api/core";

const emptyRuns: OrchestrationRun[] = [];
let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  vi.mocked(orchestrator.resumeBlocker).mockReturnValue(undefined);
  vi.mocked(orchestrator.resumeLeadBusy).mockReturnValue(false);
  setHarnessModels("codex", [
    {
      id: "codex:one",
      harness: "codex",
      name: "Worker One",
      settings: [
        {
          id: "reasoningEffort",
          label: "Reasoning",
          kind: "select",
          value: "high",
          options: [
            { value: "xhigh", label: "Extra High" },
            { value: "high", label: "High" },
          ],
        },
      ],
    },
    { id: "codex:two", harness: "codex", name: "Worker Two" },
  ]);
  setHarnessModels("claude", [
    {
      id: "claude:two",
      harness: "claude",
      name: "Worker Two",
      settings: [
        {
          id: "effort",
          label: "Effort",
          kind: "select",
          value: "high",
          options: [
            { value: "xhigh", label: "Extra High" },
            { value: "high", label: "High" },
          ],
        },
      ],
    },
  ]);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  resetHarnessModelOverlays();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  emptyRuns.length = 0;
});
const button = (text: string) =>
  [...document.querySelectorAll("button")].find((element) =>
    element.textContent?.includes(text),
  )!;
async function click(element: Element) {
  expect(element).toBeTruthy();
  await act(async () => {
    (element as HTMLElement).click();
  });
}
async function press(element: Element, key: string) {
  await act(async () => {
    element.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
  });
}
async function hover(element: Element) {
  await act(async () => {
    element.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
  });
}
async function input(
  element: HTMLInputElement | HTMLTextAreaElement,
  value: string,
) {
  const prototype =
    element instanceof HTMLTextAreaElement
      ? HTMLTextAreaElement.prototype
      : HTMLInputElement.prototype;
  await act(async () => {
    Object.getOwnPropertyDescriptor(prototype, "value")!.set!.call(
      element,
      value,
    );
    element.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("orchestration composer and card", () => {
  it("toggles a badge through the plus menu and submits without a team setup or execution", async () => {
    const model = modelsFor("codex")[0];
    const submit = vi.fn();
    await act(async () =>
      root.render(
        createElement(Composer, {
          focused: false,
          harness: "codex",
          model: model.id,
          runtimeMode: "supervised",
          cwd: "/repo",
          executionCwd: "/repo",
          sessionId: "lead",
          initialDraft: "Build settings",
          hideProjectPicker: true,
          hideBranchPicker: true,
          onFocus: () => {},
          onCwdChange: () => {},
          onModelChange: () => {},
          onRuntimeModeChange: () => {},
          onSubmit: submit,
        }),
      ),
    );
    await click(
      document.querySelector(
        'button[aria-label="Add files or choose a mode"]',
      )!,
    );
    expect(button("Plan mode")).toBeTruthy();
    await click(button("Orchestrator"));
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(
      document.querySelector('[aria-label="Turn off Orchestrator mode"]'),
    ).not.toBeNull();
    await click(
      document.querySelector('[aria-label="Turn off Orchestrator mode"]')!,
    );
    expect(
      document.querySelector('[aria-label="Turn off Orchestrator mode"]'),
    ).toBeNull();
    await click(
      document.querySelector(
        'button[aria-label="Add files or choose a mode"]',
      )!,
    );
    await click(button("Plan mode"));
    expect(
      document.querySelector('[title="Turn off Plan mode"]'),
    ).not.toBeNull();
    await click(
      document.querySelector(
        'button[aria-label="Add files or choose a mode"]',
      )!,
    );
    await click(button("Orchestrator"));
    expect(document.querySelector('[title="Turn off Plan mode"]')).toBeNull();
    const textarea = container.querySelector("textarea")!;
    await act(async () =>
      textarea.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      ),
    );
    expect(submit).toHaveBeenCalledWith("Build settings", [], {
      intent: "orchestrate",
    });
    expect(
      document.querySelector('[aria-label="Turn off Orchestrator mode"]'),
    ).toBeNull();
    expect(
      vi
        .mocked(invoke)
        .mock.calls.some(([command]) => command === "control_enable"),
    ).toBe(false);
  });
  it("preserves the draft and selected mode when the app rejects submission", async () => {
    const model = modelsFor("codex")[0];
    const submit = vi.fn(() => false);
    await act(async () =>
      root.render(
        createElement(Composer, {
          focused: false,
          harness: "codex",
          model: model.id,
          runtimeMode: "supervised",
          cwd: "/repo",
          executionCwd: "/repo",
          sessionId: "lead",
          initialDraft: "Do not lose this message",
          hideProjectPicker: true,
          hideBranchPicker: true,
          onFocus: () => {},
          onCwdChange: () => {},
          onModelChange: () => {},
          onRuntimeModeChange: () => {},
          onSubmit: submit,
        }),
      ),
    );
    await click(
      document.querySelector(
        'button[aria-label="Add files or choose a mode"]',
      )!,
    );
    await click(button("Orchestrator"));
    const textarea = container.querySelector("textarea")!;
    await press(textarea, "Enter");

    expect(submit).toHaveBeenCalled();
    expect(textarea.value).toBe("Do not lose this message");
    expect(
      document.querySelector('[aria-label="Turn off Orchestrator mode"]'),
    ).not.toBeNull();
  });
  it("lets the user change an assignment's model and waits for explicit confirmation", async () => {
    const choices = [
      { harness: "codex" as const, model: "codex:one", name: "Worker One" },
      { harness: "claude" as const, model: "claude:two", name: "Worker Two" },
    ];
    const initial: OrchestrationProposal = {
      version: 1,
      leadId: "lead",
      cwd: "/repo",
      request: "Build",
      author: choices[0],
      settings: { choices, maxWorkers: 2 },
      status: "ready",
      title: "Build settings",
      summary: "Split UI and persistence",
      tasks: [
        {
          id: "ui",
          title: "Settings UI",
          prompt: "Build the form",
          harness: "codex",
          model: "codex:one",
          files: ["src/settings"],
          dependsOn: [],
        },
      ],
    };
    const confirm = vi.fn(async (_proposal: OrchestrationProposal) => {});
    function Card() {
      const [proposal, setProposal] = useState(initial);
      return createElement(
        OrchestrationActions.Provider,
        {
          value: {
            update: (_id, _block, edited) => setProposal(edited),
            confirm: async () => confirm(proposal),
            retry: () => {},
            open: () => {},
          },
        },
        createElement(AgentTranscript, {
          blocks: [
            {
              id: "card",
              role: "plan",
              text: "Readable plan",
              orchestration: proposal,
            },
          ],
        }),
      );
    }
    await act(async () => root.render(createElement(Card)));
    expect(container.textContent).toContain("Settings UI");
    expect(
      container.querySelector("[data-orchestration-review]"),
    ).not.toBeNull();
    expect(container.textContent).not.toContain("Readable plan");
    // The lead's summary stays out of the card; the task rows carry the plan.
    expect(container.textContent).not.toContain("Split UI and persistence");
    expect(confirm).not.toHaveBeenCalled();
    await click(
      document.querySelector('[aria-label="Model for Settings UI"]')!,
    );
    expect(document.activeElement).toBe(
      document.querySelector('[aria-label="Search assignment models"]'),
    );
    // Marks the surface as a picker, which is what keeps the composer from
    // pulling focus straight back out of the search field.
    expect(
      document
        .querySelector('[aria-label="Search assignment models"]')
        ?.closest("[data-model-picker]"),
    ).not.toBeNull();
    await input(
      document.querySelector('[aria-label="Search assignment models"]')!,
      "Claude",
    );
    expect(
      document.querySelector("[data-popover-side]")?.textContent,
    ).not.toContain("Worker One");
    // Arrows walk the list, then Enter opens and chooses its effort.
    await input(
      document.querySelector('[aria-label="Search assignment models"]')!,
      "Worker",
    );
    await press(
      document.querySelector('[aria-label="Search assignment models"]')!,
      "ArrowDown",
    );
    await press(
      document.querySelector('[aria-label="Search assignment models"]')!,
      "Enter",
    );
    expect(
      document.querySelector('[role="menu"][aria-label="Worker Two effort"]'),
    ).not.toBeNull();
    await press(
      document.querySelector('[aria-label="Search assignment models"]')!,
      "ArrowUp",
    );
    await press(
      document.querySelector('[aria-label="Search assignment models"]')!,
      "Enter",
    );
    expect(container.textContent).toContain("Worker Two · Extra High");
    // The pointer reaches the same rows.
    await click(
      document.querySelector('[aria-label="Model for Settings UI"]')!,
    );
    const workerTwo = [...document.querySelectorAll('[role="option"]')].find(
      (row) => row.textContent?.includes("Worker Two"),
    )!;
    await hover(workerTwo);
    const effortMenu = document.querySelector(
      '[role="menu"][aria-label="Worker Two effort"]',
    )!;
    expect(effortMenu).toBeTruthy();
    await click(
      [...effortMenu.querySelectorAll('[role="menuitemradio"]')].find(
        (option) => option.textContent === "High",
      )!,
    );
    expect(container.textContent).toContain("Worker Two · High");
    expect(container.querySelector("textarea")).toBeNull();
    await click(
      document.querySelector('[aria-label="Details for Settings UI"]')!,
    );
    await input(
      document.querySelector('[aria-label="Instructions for task 1"]')!,
      "Build the accessible form and check keyboard navigation",
    );
    await click(
      [
        ...document.querySelectorAll('[aria-label="Parallel workers"] button'),
      ].find((option) => option.textContent === "1")!,
    );
    expect(confirm).not.toHaveBeenCalled();
    await click(button("Confirm & start"));
    expect(confirm).toHaveBeenCalledWith(
      expect.objectContaining({
        settings: expect.objectContaining({ maxWorkers: 1 }),
        tasks: [
          expect.objectContaining({
            harness: "claude",
            model: "claude:two",
            modelSettings: { effort: "high" },
            prompt: "Build the accessible form and check keyboard navigation",
          }),
        ],
      }),
    );
  });
  it("never offers confirmation for a proposal that is still being generated", async () => {
    const proposal: OrchestrationProposal = {
      version: 1,
      leadId: "lead",
      cwd: "/repo",
      request: "Build",
      author: { harness: "codex", model: "test", name: "Lead" },
      settings: { choices: [], maxWorkers: 2 },
      status: "planning",
      title: "Planning",
      summary: "",
      tasks: [],
    };
    await act(async () =>
      root.render(
        createElement(AgentTranscript, {
          busy: true,
          blocks: [
            {
              id: "draft",
              role: "plan",
              text: "",
              orchestration: proposal,
            },
          ],
        }),
      ),
    );
    expect(container.querySelector("[data-orchestration-review]")).toBeNull();
    expect(container.textContent).not.toContain("Confirm & start");
  });
  it("opens worker panes from View agents instead of the sidebar", async () => {
    const open = vi.fn();
    const openAgents = vi.fn();
    const task: OrchestrationTask = {
      id: "engine",
      sessionId: "worker-a",
      title: "Audit engine",
      harness: "codex",
      model: "codex:two",
      prompt: "Review the engine",
      files: [],
      scopes: [],
      dependsOn: [],
      status: "cancelled",
      accepted: false,
      delivered: false,
      result: "",
    };
    emptyRuns.push({
      version: 1,
      leadId: "lead",
      cwd: "/repo",
      status: "stopped",
      allowedHarnesses: ["codex", "cursor"],
      proposalId: "card",
      maxWorkers: 2,
      cli: "monocode",
      tasks: [
        task,
        {
          ...task,
          id: "ui",
          sessionId: "worker-b",
          title: "Audit UI",
          harness: "cursor",
          model: "cursor:composer-2.5",
          status: "completed",
        },
      ],
      continuations: 0,
      requests: {},
    });
    await act(async () =>
      root.render(
        createElement(
          OrchestrationActions.Provider,
          {
            value: {
              update: () => {},
              confirm: async () => {},
              retry: () => {},
              open,
              openAgents,
            },
          },
          createElement(AgentTranscript, {
            blocks: [
              {
                id: "card",
                role: "plan",
                text: "",
                orchestration: {
                  version: 1,
                  leadId: "lead",
                  cwd: "/repo",
                  request: "Review",
                  author: {
                    harness: "codex",
                    model: "codex:one",
                    name: "Lead",
                  },
                  settings: { choices: [], maxWorkers: 2 },
                  status: "approved",
                  title: "Review local-agent orchestration branch",
                  summary: "",
                  tasks: [
                    {
                      id: "engine",
                      title: "Audit engine",
                      prompt: "Review the engine",
                      harness: "codex",
                      model: "codex:two",
                      files: [],
                      dependsOn: [],
                    },
                    {
                      id: "ui",
                      title: "Audit UI",
                      prompt: "Review the UI",
                      harness: "cursor",
                      model: "cursor:composer-2.5",
                      files: [],
                      dependsOn: [],
                    },
                  ],
                },
              },
            ],
          }),
        ),
      ),
    );
    await click(button("View agents"));
    expect(open).not.toHaveBeenCalled();
    expect(openAgents).toHaveBeenCalledWith([
      {
        sessionId: "worker-a",
        leadId: "lead",
        title: "Audit engine",
        harness: "codex",
      },
      {
        sessionId: "worker-b",
        leadId: "lead",
        title: "Audit UI",
        harness: "cursor",
      },
    ]);
  });
  it("gives the lead's transcript and composer the whole pane", async () => {
    const lead = {
      ...newSession("codex", "/repo", "codex:one"),
      id: "lead",
      blocks: [
        { id: "user", role: "user" as const, text: "Build the feature" },
        {
          id: "answer",
          role: "assistant" as const,
          text: "I am coordinating the work.",
        },
      ],
    };
    const task: OrchestrationTask = {
      id: "task",
      sessionId: "worker",
      title: "UI worker",
      harness: "codex",
      model: "codex:two",
      prompt: "Build UI",
      files: ["ui"],
      scopes: ["/repo/ui"],
      dependsOn: [],
      status: "running",
      accepted: false,
      delivered: false,
      result: "",
    };
    emptyRuns.push({
      version: 1,
      leadId: lead.id,
      cwd: lead.cwd,
      status: "active",
      allowedHarnesses: ["codex"],
      maxWorkers: 2,
      cli: "monocode",
      tasks: [
        task,
        {
          ...task,
          id: "second-task",
          sessionId: "second",
          title: "Check worker",
        },
      ],
      continuations: 0,
      requests: {},
    });
    const submit = vi.fn();
    const approve = vi.fn();
    const reply = vi.fn();
    const open = vi.fn();
    const noop = () => {};
    function LeadPane({ id = lead.id, undoLocked = false } = {}) {
      const [selectedId, inspect] = useState<string | null>(null);
      return createElement(
        OrchestrationActions.Provider,
        { value: { update: noop, confirm: async () => {}, retry: noop, open } },
        createElement(
          OrchestrationWorkers.Provider,
          { value: { selectedId, inspect } },
          createElement(SessionPane, {
            session: { ...lead, id },
            reviewUndoLocked: undoLocked,
            visible: true,
            focused: true,
            inSplit: false,
            composerFocused: true,
            recents: [],
            onFocus: noop,
            onClose: noop,
            onCwdChange: noop,
            onBranchChange: noop,
            onModelChange: noop,
            onModelSettingsChange: noop,
            onRuntimeModeChange: noop,
            onSubmit: submit,
            onStop: noop,
            onCompactContext: () => false,
            onPlaceSessionInFolder: noop,
            onDeleteQueuedMessage: noop,
            onEditQueuedMessage: noop,
            onQueuedMessageEditingChange: noop,
            onSteerQueuedMessage: noop,
            onResumeQueue: noop,
            onApproval: approve,
            onQuestionReply: reply,
            onOpenFile: noop,
            onOpenDiff: noop,
            onOpenPlan: noop,
            onBuildPlan: noop,
            onNewTerminal: noop,
          }),
        ),
      );
    }
    await act(async () => root.render(createElement(LeadPane)));
    // Agents live on the sidebar card now; nothing narrows the lead's pane.
    expect(container.querySelector("[data-orchestration-agents]")).toBeNull();
    expect(container.textContent).toContain("I am coordinating the work.");
    expect(container.textContent).not.toContain("UI worker");
    expect(open).not.toHaveBeenCalled();
    const composer = container.querySelector("textarea")!;
    await input(composer, "Ask the UI worker to check keyboard navigation.");
    await act(async () => {
      composer.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    });
    expect(submit).toHaveBeenCalledWith(
      "lead",
      "Ask the UI worker to check keyboard navigation.",
      [],
      { intent: "default" },
    );
    for (const status of ["active", "paused", "finished", "stopped"] as const) {
      emptyRuns[0] = { ...emptyRuns[0], status };
      for (const id of ["lead", "worker", "unrelated"]) {
        await act(async () => root.render(createElement(LeadPane, { id })));
        expect(
          container
            .querySelector("[data-review-undo-locked]")
            ?.getAttribute("data-review-undo-locked"),
        ).toBe(
          String(
            id !== "unrelated" && (status === "active" || status === "paused"),
          ),
        );
      }
    }
    await act(async () =>
      root.render(createElement(LeadPane, { undoLocked: true })),
    );
    expect(
      container
        .querySelector("[data-review-undo-locked]")
        ?.getAttribute("data-review-undo-locked"),
    ).toBe("true");
  });

  it("shows a blocked agent as the lead's to answer, not the user's", async () => {
    const summary: OrchestrationSummary = {
      status: "active",
      live: true,
      tasks: [
        {
          sessionId: "worker",
          title: "UI worker",
          harness: "codex",
          model: "codex:two",
          status: "running",
          needsInput: true,
        },
        {
          sessionId: "second",
          title: "Check worker",
          harness: "codex",
          model: "codex:two",
          status: "running",
          needsInput: true,
        },
      ],
    };
    function Card() {
      const [selectedId, inspect] = useState<string | null>(null);
      return createElement(
        OrchestrationWorkers.Provider,
        { value: { selectedId, inspect } },
        createElement(OrchestrationSidebarAgents, { leadId: "lead", summary }),
      );
    }
    await act(async () => root.render(createElement(Card)));
    expect(container.textContent).toContain("UI worker");
    // A blocked agent expands itself so its model is visible, but the
    // approval still belongs to the lead, not to this card.
    expect(
      container.querySelector(
        '[data-orchestration-agent="worker"] [aria-expanded="true"]',
      ),
    ).not.toBeNull();
    expect(container.textContent).toContain("Worker Two");
    expect(button("Allow")).toBeUndefined();
    expect(button("Deny")).toBeUndefined();
    expect(container.textContent).not.toContain("Which check should I run?");
  });

  it("inspects an agent without taking the card's click or its tab", async () => {
    const summary: OrchestrationSummary = {
      status: "active",
      live: true,
      tasks: [
        {
          sessionId: "worker",
          title: "UI worker",
          harness: "codex",
          model: "codex:two",
          status: "running",
        },
      ],
    };
    const selectCard = vi.fn();
    const openDetails = vi.fn();
    function Card() {
      const [selectedId, inspect] = useState<string | null>(null);
      return createElement(
        "div",
        { onClick: selectCard },
        createElement(
          OrchestrationWorkers.Provider,
          { value: { selectedId, inspect, openDetails } },
          createElement(OrchestrationSidebarAgents, {
            leadId: "lead",
            summary,
          }),
        ),
      );
    }
    await act(async () => root.render(createElement(Card)));
    await click(
      container.querySelector('[aria-label="Agent details: UI worker"]')!,
    );
    // Expanding a row is not a request to open the lead's tab.
    expect(selectCard).not.toHaveBeenCalled();
    await click(button("See details"));
    expect(openDetails).toHaveBeenCalledWith({
      sessionId: "worker",
      leadId: "lead",
      title: "UI worker",
      harness: "codex",
    });
    expect(selectCard).not.toHaveBeenCalled();
  });

  it("expands any number of agents at once", async () => {
    const summary: OrchestrationSummary = {
      status: "active",
      live: true,
      tasks: ["one", "two", "three"].map((id) => ({
        sessionId: id,
        title: `Agent ${id}`,
        harness: "codex" as const,
        model: "codex:two",
        status: "running" as const,
      })),
    };
    function Card() {
      const [selectedId, inspect] = useState<string | null>(null);
      return createElement(
        OrchestrationWorkers.Provider,
        { value: { selectedId, inspect } },
        createElement(OrchestrationSidebarAgents, { leadId: "lead", summary }),
      );
    }
    await act(async () => root.render(createElement(Card)));
    const row = (id: string) =>
      container.querySelector<HTMLElement>(
        `[data-orchestration-agent="${id}"] button`,
      )!;
    const openIds = () =>
      [...container.querySelectorAll("[data-orchestration-agent]")]
        .filter((entry) => entry.querySelector("[aria-expanded=true]"))
        .map((entry) => entry.getAttribute("data-orchestration-agent"));
    await click(row("one"));
    await click(row("three"));
    expect(openIds()).toEqual(["one", "three"]);
    // Collapsing one leaves the other where it was.
    await click(row("one"));
    expect(openIds()).toEqual(["three"]);
  });

  it("explains paused recovery and opens the conversation blocking Resume", async () => {
    const task: OrchestrationTask = {
      id: "task",
      sessionId: "worker",
      title: "Interrupted worker",
      harness: "codex",
      model: "codex:two",
      prompt: "Implement",
      files: ["src"],
      scopes: ["/repo/src"],
      dependsOn: [],
      status: "cancelled",
      accepted: false,
      delivered: false,
      result: "",
    };
    emptyRuns.push({
      version: 1,
      leadId: "lead",
      cwd: "/repo",
      status: "paused",
      allowedHarnesses: ["codex"],
      maxWorkers: 2,
      cli: "monocode",
      tasks: [task],
      continuations: 0,
      requests: {},
    });
    vi.mocked(orchestrator.resumeBlocker).mockReturnValue({
      ...newSession("codex", "/repo"),
      id: "investigation",
      title: "Investigating the failure",
      busy: true,
    });
    const open = vi.fn();
    const summary: OrchestrationSummary = {
      status: "paused",
      live: true,
      tasks: [
        {
          sessionId: task.sessionId,
          title: task.title,
          harness: task.harness,
          model: task.model,
          status: task.status,
        },
      ],
    };

    await act(async () =>
      root.render(
        createElement(
          OrchestrationActions.Provider,
          {
            value: {
              update: () => {},
              confirm: async () => {},
              retry: () => {},
              open,
            },
          },
          createElement(OrchestrationSidebarAgents, {
            leadId: "lead",
            summary,
          }),
        ),
      ),
    );

    expect(container.textContent).toContain(
      "Interrupted tasks stay stopped for the lead to review.",
    );
    expect(container.textContent).toContain(
      "Investigating the failure is still running in this project.",
    );
    expect(button("Resume").hasAttribute("disabled")).toBe(true);
    await click(button("Open blocker"));
    expect(open).toHaveBeenCalledWith("investigation");
  });
});
