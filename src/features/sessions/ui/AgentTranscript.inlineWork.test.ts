// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Block } from "../model/session";
import { AgentTranscript } from "./AgentTranscript";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.spyOn(HTMLElement.prototype, "animate").mockImplementation(
    () => ({ cancel: vi.fn() }) as unknown as Animation,
  );
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
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function tool(id: string, status = "completed"): Block {
  return {
    id,
    role: "tool",
    text: `Inspect ${id}`,
    tool: { kind: "shell", status },
  };
}

function render(
  blocks: Block[],
  props: Partial<ComponentProps<typeof AgentTranscript>> = {},
) {
  act(() =>
    root.render(
      createElement(AgentTranscript, { blocks, inlineWork: true, ...props }),
    ),
  );
}

function status() {
  return container.querySelector('[data-mono-work] [role="status"] .sr-only')
    ?.textContent;
}

function settleTicker() {
  act(() => vi.advanceTimersByTime(850));
  act(() => vi.advanceTimersByTime(340));
}

it("keeps the opening and reply outside a noninteractive work summary", () => {
  render([
    { id: "user", role: "user", text: "Inspect" },
    { id: "intro", role: "assistant", text: "I will inspect the files." },
    tool("first"),
    { id: "progress", role: "assistant", text: "Trying another approach." },
    tool("second"),
    { id: "answer", role: "assistant", text: "Everything passed." },
  ]);
  expect(status()).toBe("Ran 2 commands");
  const work = container.querySelector("[data-mono-work]")!;
  expect(work.querySelector("button")).toBeNull();
  act(() => work.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  expect(container.textContent).not.toContain("Trying another approach.");
  expect(container.textContent).not.toContain("Inspect first");
  expect(container.textContent).toContain("I will inspect the files.");
  expect(container.textContent).toContain("Everything passed.");
});

it("opens activity from the work summary while keeping the Mono duration label noninteractive", () => {
  const onShowWork = vi.fn();
  const blocks: Block[] = [
    { id: "earlier", role: "user", text: "Earlier turn" },
    tool("earlier-call"),
    { id: "user", role: "user", text: "Inspect", durationMs: 23000 },
    { id: "intro", role: "assistant", text: "I will inspect the files." },
    tool("first"),
    { id: "progress", role: "assistant", text: "Trying another approach." },
    tool("second"),
    { id: "answer", role: "assistant", text: "Everything passed." },
  ];
  render(blocks, { onShowWork, agentName: "Captain Awesome" });
  const turn = container.querySelector('[data-transcript-turn="user"]')!;
  act(() =>
    turn
      .querySelector<HTMLButtonElement>('[aria-label="Show activity"]')!
      .click(),
  );
  expect(onShowWork).toHaveBeenLastCalledWith("user", blocks.slice(2));
  expect(turn.querySelector('[aria-label="Show the work"]')).toBeNull();
  expect(onShowWork).toHaveBeenCalledOnce();
  expect(container.textContent).not.toContain("Trying another approach.");
  expect(container.textContent).not.toContain("Inspect first");
  render(blocks, {
    onShowWork,
    activeWorkTurnId: "user",
    agentName: "Captain Awesome",
  });
  expect(turn.querySelector('[aria-label="Hide the work"]')).toBeNull();
  expect(
    turn
      .querySelector('[aria-label="Hide activity"]')
      ?.getAttribute("aria-expanded"),
  ).toBe("true");
});

it("keeps streamed progress inside the status row and reveals the final reply when done", () => {
  const opening: Block[] = [
    { id: "user", role: "user", text: "Inspect" },
    { id: "intro", role: "assistant", text: "I will inspect the files." },
  ];
  render([...opening, tool("first", "in_progress")], { busy: true });
  const group = container.querySelector("[data-mono-work]");
  expect(status()).toBe("Running command…");
  const narrated: Block[] = [
    ...opening,
    tool("first"),
    {
      id: "progress",
      role: "assistant",
      text: "Private streamed progress",
      streaming: true,
    },
  ];
  render(narrated, { busy: true });
  settleTicker();
  expect(status()).toBe("Thinking…");
  expect(container.textContent).not.toContain("Private streamed progress");
  const worked: Block[] = [
    ...narrated,
    tool("second"),
    {
      id: "answer",
      role: "assistant",
      text: "Everything passed.",
      streaming: true,
    },
  ];
  render(worked, { busy: true });
  settleTicker();
  expect(status()).toBe("Pondering…");
  expect(container.querySelector("[data-mono-work]")).toBe(group);
  expect(container.querySelector('[data-chat-message="answer"]')).toBeNull();
  render(worked.map((block) => ({ ...block, streaming: false })));
  settleTicker();
  expect(status()).toBe("Ran 2 commands");
  expect(
    container.querySelector('[data-chat-message="answer"]'),
  ).not.toBeNull();
  expect(group?.textContent).not.toContain("Everything passed.");
});

it("keeps pending edit approvals actionable beside the compact status", () => {
  const onApproval = vi.fn();
  render(
    [
      { id: "user", role: "user", text: "Edit the file" },
      tool("first"),
      { id: "progress", role: "assistant", text: "Ready to edit." },
      {
        id: "edit",
        role: "tool",
        text: "Edit src/App.tsx",
        approval: { requestId: 42 },
        tool: {
          kind: "edit",
          status: "pending",
          preview: {
            kind: "write",
            path: "src/App.tsx",
            lines: [{ kind: "add", text: "Approved edit preview" }],
          },
        },
      },
    ],
    { busy: true, onApproval },
  );
  expect(status()).toBe("Waiting for approval…");
  expect(container.textContent).toContain("Approved edit preview");
  expect(container.textContent).not.toContain("Ready to edit.");
  const allow = Array.from(container.querySelectorAll("button")).find(
    (button) => button.textContent === "Allow",
  )!;
  act(() => allow.click());
  expect(onApproval).toHaveBeenCalledWith(42, "allow");
});

it("marks the compact group for search without expanding its commands or narration", () => {
  let navigate: ((blockId: string | null) => boolean) | undefined;
  render(
    [
      { id: "user", role: "user", text: "Inspect" },
      tool("first"),
      { id: "progress", role: "assistant", text: "Trying another approach." },
      tool("second"),
      { id: "answer", role: "assistant", text: "Everything passed." },
    ],
    {
      onNavigateReady: (callback) => {
        navigate = callback;
      },
    },
  );
  act(() => {
    expect(navigate?.("progress")).toBe(true);
  });
  const current = container.querySelectorAll(
    '[data-transcript-search-current="true"]',
  );
  expect(current).toHaveLength(1);
  expect(current[0].querySelector("[data-mono-work]")).not.toBeNull();
  expect(container.textContent).not.toContain("Trying another approach.");
  expect(container.querySelector("[data-mono-work] button")).toBeNull();
});

it("keeps interrupted Mono messages together above one working indicator", () => {
  const blocks: Block[] = [
    {
      id: "first-message",
      role: "user",
      text: "Hey anything new",
      startedAt: 1000,
    },
    tool("first-call", "in_progress"),
    { id: "second-message", role: "user", text: "Let me know", sentAt: 1100 },
    tool("second-call", "in_progress"),
    { id: "third-message", role: "user", text: "Awesome", sentAt: 1200 },
  ];
  const props = {
    busy: true,
    agentName: "Captain",
    agentMascot: { mascot: "cat" as const, color: "#6ba" },
    bottomAligned: true,
  };
  render(blocks, props);
  const turn = container.querySelector(
    '[data-transcript-turn="first-message"]',
  )!;
  expect(container.querySelectorAll(".transcript-turn")).toHaveLength(1);
  expect(
    [...turn.querySelectorAll<HTMLElement>("[data-prompt-anchor]")].map(
      (row) => row.dataset.promptAnchor,
    ),
  ).toEqual(["first-message", "second-message", "third-message"]);
  expect(turn.querySelectorAll('[data-message-stack="true"]')).toHaveLength(2);
  expect(turn.querySelectorAll("[data-mono-work]")).toHaveLength(1);
  const rows = [...turn.querySelectorAll("[data-transcript-search-item]")];
  expect(
    rows.slice(0, 3).every((row) => row.querySelector("[data-prompt-anchor]")),
  ).toBe(true);
  expect(rows[3].querySelector("[data-mono-work]")).not.toBeNull();
  render([...blocks, { id: "answer", role: "assistant", text: "Done" }], {
    ...props,
    busy: false,
  });
  expect(container.querySelectorAll(".transcript-turn")).toHaveLength(1);
  expect(container.querySelectorAll("[data-prompt-anchor]")).toHaveLength(3);
  render(
    [
      ...blocks,
      { id: "answer", role: "assistant", text: "Done" },
      { id: "new-message", role: "user", text: "New request", startedAt: 5000 },
    ],
    props,
  );
  expect(container.querySelectorAll(".transcript-turn")).toHaveLength(2);
});

it.each(["Follow up", "👍"])(
  "keeps a failed optimistic message visible with inline retry: %s",
  (text) => {
    const onRetryMessage = vi.fn();
    render(
      [
        { id: "original", role: "user", text: "Inspect", startedAt: 1000 },
        { id: "pending", role: "user", text, sentAt: 1100 },
      ],
      {
        busy: true,
        agentMascot: { mascot: "cat", color: "#6ba" },
        onRetryMessage,
        messageDeliveries: new Map([
          ["pending", { status: "failed", error: "Offline" }],
        ]),
      },
    );
    const bubble = container.querySelector('[data-prompt-anchor="pending"]')!;
    expect(bubble.getAttribute("data-message-delivery")).toBe("failed");
    expect(bubble.textContent).toContain(text);
    expect(bubble.querySelector('[role="alert"]')?.textContent).toContain(
      "Offline",
    );
    const retry = [
      ...bubble.querySelectorAll<HTMLButtonElement>("button"),
    ].find((button) => button.textContent === "Retry")!;
    act(() => retry.click());
    expect(onRetryMessage).toHaveBeenCalledWith("pending");
    render(
      [
        { id: "original", role: "user", text: "Inspect", startedAt: 1000 },
        { id: "pending", role: "user", text, sentAt: 1100 },
      ],
      {
        busy: true,
        agentMascot: { mascot: "cat", color: "#6ba" },
        messageDeliveries: new Map([["pending", { status: "pending" }]]),
      },
    );
    expect(
      container.querySelectorAll('[data-prompt-anchor="pending"]'),
    ).toHaveLength(1);
    expect(
      container.querySelector('[data-prompt-anchor="pending"] [role="alert"]'),
    ).toBeNull();
    expect(container.textContent).not.toContain("Waiting to send");
    expect(container.textContent).not.toContain("Steer");
  },
);
