// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  appendUser,
  applyHarnessEvent,
  stopStreaming,
} from "../../../integrations/harness/core/apply";
import { copyMessage } from "../../../platform/tauri/clipboard";
import { acknowledgeMonoMessage } from "../../monos/model/monoMessaging";
import {
  enqueueMonoSessionCompletion,
  monoSessionCompletionMessage,
} from "../../monos/model/monoSessionCompletion";
import { sanitizeSessionForPersist } from "../data/sessionStore";
import { newSession, type Block } from "../model/session";
import { AgentTranscript } from "./AgentTranscript";

vi.mock("../../../platform/tauri/clipboard", () => ({
  copyMessage: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("../../settings/model/sounds", () => ({ playCue: vi.fn() }));

let container: HTMLDivElement;
let root: Root;
const onSaveNote = vi.fn();
const postedAt = new Date(2026, 9, 6, 9, 30).getTime();
const report: Block = {
  id: "report",
  role: "assistant",
  text: "CI failed on main.",
  monoHabit: { id: "habit", name: "Morning check", at: postedAt },
};

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("matchMedia", () => ({ matches: true }));
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

function render(blocks: Block[], busy = false) {
  act(() =>
    root.render(
      createElement(AgentTranscript, {
        blocks,
        busy,
        inlineWork: true,
        bottomAligned: true,
        agentName: "Captain Awesome",
        agentMascot: { mascot: "cat", color: "#f97316" },
        daySeparators: true,
        hideTurnMetrics: true,
        onSaveNote,
      }),
    ),
  );
}

it("gives a standalone habit report its mascot, name, posting time and response actions", async () => {
  render([report]);
  const turn = container.querySelector('[data-transcript-turn="report"]')!;
  expect(turn.querySelector(".pixel-mascot")).not.toBeNull();
  expect(turn.textContent).toContain("Captain Awesome");
  expect(turn.textContent!.indexOf("Captain Awesome")).toBeLessThan(
    turn.textContent!.indexOf(report.text),
  );
  expect(turn.querySelector("[data-day-separator]")).not.toBeNull();
  expect(turn.textContent).toContain(
    new Intl.DateTimeFormat(undefined, {
      hour: "numeric",
      minute: "2-digit",
    }).format(postedAt),
  );
  expect(turn.textContent).not.toContain("worked");
  expect(turn.textContent).not.toContain("Thought");

  await act(async () => {
    turn
      .querySelector<HTMLButtonElement>('[aria-label="Copy response"]')!
      .click();
    turn
      .querySelector<HTMLButtonElement>('[aria-label="Save as note"]')!
      .click();
  });
  expect(copyMessage).toHaveBeenCalledWith(report.text, undefined);
  expect(onSaveNote).toHaveBeenCalledWith(report.text);
});

it.each(["current", "older"])(
  "continues the Mono's reply with a delivered completion report in %s saved chats",
  async (version) => {
    let session = appendUser(newSession("codex", "/tmp"), "Review the API");
    session = applyHarnessEvent(session, {
      type: "message.delta",
      text: "I have started the review. I will let you know when it finishes.",
    });
    session = stopStreaming(session);
    const notification = monoSessionCompletionMessage({
      requestId: "review",
      sessionId: "worker",
      project: "/tmp",
      prompt: "Review the API",
      outcome: { status: "completed", text: "The API fix passed all tests." },
    });
    session = enqueueMonoSessionCompletion(session, notification);
    session = acknowledgeMonoMessage(session, notification, {
      mode: "new-turn",
      cards: {
        internal: true,
        appRequestId: notification.id,
        monoSessionCompletion: notification.monoSessionCompletion,
      },
    });
    session = applyHarnessEvent(session, {
      type: "message.delta",
      text: "The API fix passed all tests.",
    });
    session = stopStreaming(session);
    if (version === "older") {
      session = {
        ...session,
        blocks: session.blocks.map((block) =>
          block.id === notification.id
            ? { ...block, monoSessionCompletion: undefined }
            : block,
        ),
      };
    }
    render(sanitizeSessionForPersist(session).blocks);
    const turns = container.querySelectorAll("[data-transcript-turn]");
    expect(turns).toHaveLength(1);
    expect(turns[0].textContent).toContain("I have started the review.");
    expect(turns[0].textContent).toContain("The API fix passed all tests.");
    expect(
      turns[0].querySelectorAll("[data-mono-work]"),
    ).toHaveLength(1);
    expect(container.textContent).not.toContain(notification.text);
    expect(session.queuedMessages).toBeUndefined();
    expect(
      container.querySelectorAll('[aria-label="Copy response"]'),
    ).toHaveLength(1);
    await act(async () => {
      turns[0]
        .querySelector<HTMLButtonElement>('[aria-label="Copy response"]')!
        .click();
    });
    expect(copyMessage).toHaveBeenCalledWith(
      expect.stringContaining("The API fix passed all tests."),
      undefined,
    );
  },
);

it.each(["current", "older"])(
  "keeps the previous reply mounted throughout a completion turn with %s markers",
  (version) => {
    const previous: Block[] = [
      {
        id: "request",
        role: "user",
        text: "Start a quick review",
        startedAt: postedAt - 20_000,
        durationMs: 20_000,
      },
      { id: "opening", role: "assistant", text: "I will start a review." },
      {
        id: "launch",
        role: "tool",
        text: "Start the review session",
        tool: { kind: "shell", status: "completed" },
      },
      {
        id: "started",
        role: "assistant",
        text: "The review has started. I will report back when it finishes.",
      },
    ];
    let session = { ...newSession("codex", "/tmp"), blocks: previous };
    render(session.blocks);
    const reply = container.querySelector('[data-chat-message="started"]')!;
    const paragraph = reply.querySelector("p");
    const previousTurn = container.querySelector(
      '[data-transcript-turn="request"]',
    )!;
    const actions = previousTurn.querySelector('[aria-label="Copy response"]');
    expect(paragraph?.textContent).toContain("The review has started.");
    const expectPreviousUnchanged = () => {
      expect(container.querySelector('[data-chat-message="started"]')).toBe(
        reply,
      );
      expect(reply.isConnected).toBe(true);
      expect(reply.querySelector("p")).toBe(paragraph);
      expect(previousTurn.querySelector('[aria-label="Copy response"]')).toBe(
        actions,
      );
      expect(previousTurn.textContent).toContain("Captain Awesome");
      expect(previousTurn.textContent).not.toContain("worked for");
    };
    const notification = monoSessionCompletionMessage({
      requestId: "review",
      sessionId: "worker",
      project: "/tmp",
      prompt: "Quick review",
      outcome: { status: "completed", text: "The checkout is clean." },
    });
    session = acknowledgeMonoMessage(
      enqueueMonoSessionCompletion(session, notification),
      notification,
      {
        mode: "new-turn",
        cards: {
          internal: true,
          appRequestId: notification.id,
          ...(version === "current"
            ? { monoSessionCompletion: notification.monoSessionCompletion }
            : {}),
        },
      },
    );
    render(session.blocks, session.busy);
    expectPreviousUnchanged();
    const completionTurn = container.querySelector(
      `[data-transcript-turn="${notification.id}"]`,
    )!;
    expect(completionTurn).not.toBeNull();
    expect(
      completionTurn.querySelector('[role="status"]')?.textContent,
    ).toContain("Thinking…");
    expect(
      completionTurn.querySelector("[data-mono-work]")?.textContent,
    ).toContain("Captain Awesome");
    const events = [
      { type: "reasoning.delta", text: "Checking the session report" },
      {
        type: "message.delta",
        text: "The review finished. I am checking the result.",
      },
      { type: "message.completed" },
      {
        type: "tool.started",
        callId: "read",
        title: "Read the session",
        kind: "shell",
        status: "in_progress",
      },
      { type: "tool.updated", callId: "read", status: "completed" },
      {
        type: "message.delta",
        text: "The checkout is clean; no issues were found.",
      },
    ] satisfies Parameters<typeof applyHarnessEvent>[1][];
    for (const event of events) {
      session = applyHarnessEvent(session, event);
      render(session.blocks, session.busy);
      expectPreviousUnchanged();
      expect(
        container.querySelector(`[data-transcript-turn="${notification.id}"]`),
      ).toBe(completionTurn);
      expect(container.textContent).not.toContain(notification.text);
    }
    session = stopStreaming(session);
    render(session.blocks, session.busy);
    expectPreviousUnchanged();
    expect(
      container.querySelector(`[data-transcript-turn="${notification.id}"]`),
    ).toBe(completionTurn);
    expect(
      container.querySelectorAll('[aria-label="Copy response"]'),
    ).toHaveLength(2);
  },
);

it("keeps a posted report actionable while the conversation is still running", async () => {
  render(
    [
      {
        id: "user",
        role: "user",
        text: "Inspect",
        startedAt: postedAt - 1_000,
      },
      {
        id: "answer",
        role: "assistant",
        text: "I am investigating.",
        streaming: true,
      },
      report,
    ],
    true,
  );
  const chat = container.querySelector('[data-transcript-turn="user"]')!;
  const habit = container.querySelector('[data-transcript-turn="report"]')!;
  expect(chat.querySelector("[data-mono-work]")?.textContent).toContain(
    "Captain Awesome",
  );
  expect(chat.querySelector('[role="status"]')?.textContent).toContain(
    "Thinking…",
  );
  expect(chat.textContent).not.toContain(report.text);
  expect(habit.querySelector('[role="status"]')).toBeNull();
  await act(async () => {
    habit
      .querySelector<HTMLButtonElement>('[aria-label="Save as note"]')!
      .click();
  });
  expect(onSaveNote).toHaveBeenCalledWith(report.text);
});

it("renders the identity and actions for older standalone reports missing their habit tag", () => {
  const { monoHabit: _habit, ...legacy } = report;
  render([legacy]);
  expect(container.querySelector(".pixel-mascot")).not.toBeNull();
  expect(container.textContent).toContain("Captain Awesome");
  expect(
    container.querySelector('[aria-label="Copy response"]'),
  ).not.toBeNull();
  expect(container.querySelector('[aria-label="Save as note"]')).not.toBeNull();
});

it("shows an emoji-only message large, without a bubble, time or actions", () => {
  render([
    { id: "fire", role: "user", text: "🔥", startedAt: postedAt },
    { id: "words", role: "user", text: "🔥 nice", startedAt: postedAt },
  ]);
  const emoji = container.querySelector<HTMLElement>(
    '[data-chat-message="fire"]',
  )!;
  expect(emoji.classList.contains("user-message-bubble")).toBe(false);
  expect(emoji.className).toContain("text-6xl");
  const row = container.querySelector('[data-prompt-anchor="fire"]')!;
  expect(row.querySelector('[aria-label="Copy message"]')).toBeNull();
  expect(row.querySelector("time")).toBeNull();
  expect(
    container
      .querySelector('[data-chat-message="words"]')!
      .classList.contains("user-message-bubble"),
  ).toBe(true);
});

it("shows sent images above the bubble, and alone without one", () => {
  const image = {
    id: "img",
    name: "shot.png",
    mimeType: "image/png",
    kind: "image" as const,
    size: 10,
    previewUrl: "blob:shot",
  };
  const file = {
    ...image,
    id: "doc",
    name: "notes.txt",
    kind: "text" as const,
    previewUrl: undefined,
  };
  render([
    {
      id: "both",
      role: "user",
      text: "can you see this",
      attachments: [image, file],
    },
    { id: "alone", role: "user", text: "", attachments: [image] },
  ]);
  const both = container.querySelector('[data-chat-message="both"]')!;
  const bubble = both.querySelector(".user-message-bubble")!;
  const photo = both.querySelector('[aria-label="Open shot.png full screen"]')!;
  expect(bubble.contains(photo)).toBe(false);
  expect(
    photo.compareDocumentPosition(bubble) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(bubble.textContent).toContain("notes.txt");
  expect(bubble.textContent).toContain("can you see this");

  const alone = container.querySelector('[data-chat-message="alone"]')!;
  expect(
    alone.querySelector('[aria-label="Open shot.png full screen"]'),
  ).not.toBeNull();
  expect(alone.querySelector(".user-message-bubble")).toBeNull();
});
