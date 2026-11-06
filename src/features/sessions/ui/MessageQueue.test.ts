// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { expect, it, vi } from "vitest";
import { monoSessionCompletionMessage } from "../../monos/model/monoSessionCompletion";
import type { QueuedMessage } from "../model/session";
import { MessageQueue } from "./MessageQueue";

it("labels a pending completion and offers removal without editing or steering it", () => {
  const message = monoSessionCompletionMessage({
    sessionId: "worker",
    requestId: "app-mono-monitor",
    project: "/code/project",
    prompt: "Review",
    session: undefined,
    outcome: { status: "completed", text: "Done" },
  });
  const container = document.createElement("div");
  container.innerHTML = renderToStaticMarkup(
    createElement(MessageQueue, {
      messages: [message],
      onSteer: vi.fn(),
      onEdit: vi.fn(),
      onDelete: vi.fn(),
      variant: "messages",
    }),
  );
  expect(container.textContent).toContain("Session completed: Agent session");
  expect(container.textContent).not.toContain(
    "MonoCode completion notification",
  );
  expect(container.textContent).not.toContain("Steer");
  expect(
    container.querySelector<HTMLButtonElement>(
      '[aria-label="Edit queued message"]',
    )?.disabled,
  ).toBe(true);
  expect(
    container.querySelector('[aria-label="Remove queued message"]'),
  ).not.toBeNull();
});

it("labels a combined report without exposing the app prompt", () => {
  const message = monoSessionCompletionMessage({
    sessionId: "worker",
    requestId: "request",
    project: "/code/project",
    prompt: "Review",
    outcome: { status: "completed", text: "Done" },
  });
  message.monoSessionCompletion!.sessionCount = 3;
  const markup = renderToStaticMarkup(
    createElement(MessageQueue, { messages: [message] }),
  );
  expect(markup).toContain("3 sessions finished");
  expect(markup).not.toContain("MonoCode completion notification");
});

it("keeps its hooks when the queue empties and fills again", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const queue = (messages: QueuedMessage[]) =>
    createElement(MessageQueue, { messages });
  try {
    await act(async () => {
      root.render(queue([]));
    });
    await act(async () => {
      root.render(
        queue([{ id: "q1", text: "follow up", attachments: [] }]),
      );
    });
    expect(container.textContent).toContain("follow up");
    await act(async () => {
      root.render(queue([]));
    });
    expect(container.textContent).toBe("");
  } finally {
    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  }
});
