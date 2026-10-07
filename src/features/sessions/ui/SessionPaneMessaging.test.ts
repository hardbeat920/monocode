// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { SessionPane, type SessionPaneProps } from "./SessionPane";
import { clearComposerDraft, setComposerDraft } from "../model/draftCache";

const probes = vi.hoisted(() => ({
  pick: vi.fn(),
  transcript: vi.fn(),
  runs: [],
}));
vi.mock("./Composer", () => ({ Composer: () => null }));
vi.mock("../hooks/useFileDrop", () => ({ useFileDrop: () => false }));
vi.mock("../model/attachments", async (original) => ({
  ...(await original<typeof import("../model/attachments")>()),
  pickAttachments: probes.pick,
}));
vi.mock("./AgentTranscript", () => ({
  AgentTranscript: (props: { onAddToChat: (text: string) => void }) => {
    probes.transcript(props);
    return createElement(
      "button",
      { onClick: () => props.onAddToChat("Selected response") },
      "Quote response",
    );
  },
}));
vi.mock("../../orchestration/model/orchestration", async (original) => ({
  ...(await original<
    typeof import("../../orchestration/model/orchestration")
  >()),
  orchestrator: {
    subscribe: () => () => {},
    snapshot: () => probes.runs,
    hydrate: async () => {},
  },
}));
vi.mock("../../monos/model/mono", async (original) => ({
  ...(await original<typeof import("../../monos/model/mono")>()),
  monoForSession: () => ({
    id: "mono",
    sessionId: "chat",
    name: "Captain",
    mascot: "cat",
    color: "#6ba",
    projects: [],
  }),
}));
let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  vi.stubGlobal(
    "requestAnimationFrame",
    vi.fn(() => 1),
  );
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  localStorage.clear();
  probes.transcript.mockClear();
  clearComposerDraft("chat");
  probes.pick.mockReset().mockResolvedValue([
    {
      id: "file",
      name: "note.txt",
      kind: "file",
      mimeType: "text/plain",
      size: 12,
      path: "/tmp/note.txt",
    },
  ]);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  clearComposerDraft("chat");
  vi.unstubAllGlobals();
});
const noop = () => {};
function props(): SessionPaneProps {
  return {
    session: {
      id: "chat",
      title: "Chat",
      cwd: "/repo",
      harness: "codex",
      model: "",
      modelSettings: {},
      runtimeMode: "supervised",
      blocks: [{ id: "user", role: "user", text: "Hello" }],
    },
    visible: true,
    focused: true,
    inSplit: false,
    composerFocused: false,
    recents: [],
    onFocus: noop,
    onClose: noop,
    onCwdChange: noop,
    onBranchChange: noop,
    onWorkspaceModeChange: noop,
    onWorktreeBaseChange: noop,
    onModelChange: noop,
    onModelSettingsChange: noop,
    onRuntimeModeChange: noop,
    onSubmit: noop,
    onSaveDraft: noop,
    onRemoveDraft: noop,
    onStop: noop,
    onCompactContext: () => false,
    onPlaceSessionInFolder: noop,
    onDeleteQueuedMessage: noop,
    onEditQueuedMessage: noop,
    onQueuedMessageEditingChange: noop,
    onSteerQueuedMessage: noop,
    onResumeQueue: noop,
    onUsageLimitResume: noop,
    onUsageLimitResumeAtReset: noop,
    onUsageLimitDismiss: noop,
    onApproval: noop,
    onQuestionReply: noop,
    onOpenFile: noop,
    onOpenDiff: noop,
    onOpenPlan: noop,
    onBuildPlan: noop,
    onNewTerminal: noop,
  };
}
function render(pane: SessionPaneProps) {
  act(() => root.render(createElement(SessionPane, pane)));
}
function field() {
  return container.querySelector<HTMLTextAreaElement>(
    '[aria-label="Message Captain"]',
  )!;
}

it("keeps the same input, current draft and attachments when a question appears and closes", async () => {
  setComposerDraft("chat", "Initial draft");
  const pane = props();
  pane.onSubmit = vi.fn();
  render(pane);
  const input = field();
  const inputProps = Object.entries(input).find(([key]) =>
    key.startsWith("__reactProps"),
  )![1];
  act(() => inputProps.onChange({ target: { value: "Fresh draft" } }));
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Attach files"]')!
      .click(),
  );
  const questioned = {
    ...pane,
    session: {
      ...pane.session,
      pendingQuestion: {
        requestId: 1,
        questions: [
          {
            id: "q",
            prompt: "Which project?",
            multiSelect: false,
            allowCustom: true,
            options: [],
          },
        ],
      },
    },
  };
  render(questioned);
  expect(container.querySelector("[data-question-form]")).not.toBeNull();
  expect(field()).toBe(input);
  expect(field().value).toBe("Fresh draft");
  expect(container.querySelector('[title="/tmp/note.txt"]')).not.toBeNull();
  render(pane);
  expect(field()).toBe(input);
  act(() =>
    container.querySelector<HTMLButtonElement>('[aria-label="Send"]')!.click(),
  );
  expect(pane.onSubmit).toHaveBeenCalledWith("chat", "Fresh draft", [
    expect.objectContaining({ id: "file" }),
  ]);
});

it("routes transcript quotes to the Mono draft", () => {
  render(props());
  const quote = [
    ...container.querySelectorAll<HTMLButtonElement>("button"),
  ].find((button) => button.textContent === "Quote response")!;
  act(() => quote.click());
  expect(field().value).toBe("> Selected response\n\n");
});

it("shows usage recovery above the Mono input and routes the reset option", () => {
  setComposerDraft("chat", "Keep my draft");
  const pane = props();
  pane.session.usageLimit = { resetsAt: Date.now() + 3600_000 };
  pane.onUsageLimitResumeAtReset = vi.fn();
  render(pane);
  expect(container.querySelector("[data-usage-limit]")?.textContent).toContain("usage limit reached");
  expect(container.querySelector('[aria-label="Choose another model"]')).not.toBeNull();
  expect(field().value).toBe("Keep my draft");
  const reset = [...container.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent?.trim() === "Resume at reset")!;
  act(() => reset.click());
  expect(pane.onUsageLimitResumeAtReset).toHaveBeenCalledWith("chat", true);
  render({ ...pane, session: { ...pane.session, usageLimit: undefined } });
  expect(container.querySelector("[data-usage-limit]")).toBeNull();
  expect(field().value).toBe("Keep my draft");
});

it("shows pending messages in the conversation and routes retry from the bubble", () => {
  const pane = props();
  pane.onResumeQueue = vi.fn();
  pane.session = {
    ...pane.session,
    busy: true,
    queueStatus: "paused",
    queuedMessages: [
      {
        id: "first",
        text: "Follow up",
        attachments: [],
        error: "Connection lost",
      },
    ],
  };
  render(pane);
  expect(container.querySelector("[data-message-queue]")).toBeNull();
  const transcript = probes.transcript.mock.calls.at(-1)![0];
  expect(
    transcript.blocks.map((block: { text: string }) => block.text),
  ).toEqual(["Hello", "Follow up"]);
  expect(transcript.messageDeliveries.get("first")).toEqual({
    status: "failed",
    error: "Connection lost",
  });
  act(() => transcript.onRetryMessage("first"));
  expect(pane.onResumeQueue).toHaveBeenCalledWith("chat");
  expect(field()).not.toBeNull();
});

it("keeps a completion notification queued beside the Mono's composer", () => {
  const pane = props();
  pane.session = {
    ...pane.session,
    busy: true,
    queuedMessages: [
      {
        id: "notification",
        text: "Hidden completion review prompt",
        attachments: [],
        monoSessionCompletion: {
          sessionId: "worker",
          title: "API fix",
          status: "completed",
        },
      },
    ],
  };
  render(pane);
  const queue = container.querySelector("[data-message-queue]")!;
  expect(queue.textContent).toContain("Session completed: API fix");
  expect(queue.textContent).not.toContain("Hidden completion review prompt");
  expect(queue.textContent).not.toContain("Steer");
  expect(field()).not.toBeNull();
});

it("keeps rapid sends out of the queue and steer interface", () => {
  const pane = props();
  pane.session = {
    ...pane.session,
    busy: true,
    blocks: [
      ...pane.session.blocks,
      { id: "first", role: "user", text: "Also this", sentAt: 1000 },
      { id: "second", role: "user", text: "And that", sentAt: 1100 },
    ],
    queuedMessages: [
      { id: "first", blockId: "first", text: "Also this", attachments: [] },
      { id: "second", blockId: "second", text: "And that", attachments: [] },
    ],
  };
  render(pane);
  const transcript = probes.transcript.mock.calls.at(-1)![0];
  expect(transcript.blocks).toBe(pane.session.blocks);
  expect(transcript.messageDeliveries.size).toBe(2);
  expect(container.querySelector("[data-message-queue]")).toBeNull();
  expect(
    container.querySelector('[aria-label="Edit queued message"]'),
  ).toBeNull();
  expect(container.textContent).not.toContain("Steer");
});
