// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { readFileSync } from "node:fs";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { copyMessage } from "../../../platform/tauri/clipboard";
import type { Block } from "../model/session";
import { AgentTranscript, MonoActivityTrail } from "./AgentTranscript";
import { PACED_REVEAL_KEY, WORD_FADE_MS } from "./wordFade";

vi.mock("../../../platform/tauri/clipboard", () => ({
  copyMessage: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("../../settings/model/sounds", () => ({ playCue: vi.fn() }));

let container: HTMLDivElement;
let root: Root;
let resultStyles: HTMLStyleElement;

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(copyMessage).mockResolvedValue(undefined);
  vi.useFakeTimers({
    toFake: [
      "Date",
      "setTimeout",
      "clearTimeout",
      "setInterval",
      "clearInterval",
      "requestAnimationFrame",
      "cancelAnimationFrame",
      "performance",
    ],
  });
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
  // Exercise the shipped visibility rule with the real Markdown reveal.
  resultStyles = document.createElement("style");
  resultStyles.textContent = [
    ...readFileSync("src/styles/index.css", "utf8").matchAll(
      /\.transcript-turn(?::has\(\[data-artifact-results\]\)| > :has\(\.word-fading\) ~ \[data-launched-sessions\])[^{]*\{[^}]+\}/g,
    ),
  ]
    .map((rule) => rule[0])
    .join("\n");
  document.head.append(resultStyles);
  root = createRoot(container);
});

afterEach(() => {
  localStorage.removeItem(PACED_REVEAL_KEY);
  act(() => root.unmount());
  container.remove();
  resultStyles.remove();
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

it("shows a document below its reply in the originating turn and opens it", () => {
  const onOpenArtifact = vi.fn();
  const card = {
    id: "artifact-report",
    kind: "document" as const,
    title: "PR review",
    summary: "Merge queue and blockers",
  };
  render(
    [
      {
        id: "old",
        role: "user",
        text: "Review",
        durationMs: 1000,
        artifactCards: [card],
      },
      {
        id: "old-answer",
        role: "assistant",
        text: "I created a document for you.",
      },
      { id: "new", role: "user", text: "Thanks", durationMs: 1000 },
      { id: "new-answer", role: "assistant", text: "You're welcome." },
    ],
    { onOpenArtifact },
  );
  const button = container.querySelector<HTMLButtonElement>(
    '[data-artifact-card="artifact-report"]',
  )!;
  expect(
    button
      .closest("[data-transcript-turn]")
      ?.getAttribute("data-transcript-turn"),
  ).toBe("old");
  expect(container.querySelectorAll("[data-artifact-card]")).toHaveLength(1);
  expect(button.textContent).not.toContain(card.summary);
  act(() => button.click());
  expect(onOpenArtifact).toHaveBeenCalledWith(card.id);
});

it("shows launched sessions as cards with live status, only in their turn", () => {
  const onOpenSession = vi.fn();
  const launches = [
    {
      sessionId: "app-mono-review",
      cwd: "/repo/web",
      title: "PR review",
      harness: "codex" as const,
      model: "gpt-6",
    },
    {
      sessionId: "app-mono-fix",
      cwd: "/repo/api",
      title: "Fix the build",
      harness: "claude" as const,
      model: "opus",
    },
  ];
  const blocks: Block[] = [
    { id: "old", role: "user", text: "Earlier", durationMs: 1000 },
    { id: "old-reply", role: "assistant", text: "Earlier answer" },
    {
      id: "user",
      role: "user",
      text: "Review",
      durationMs: 1000,
      monoSpawnedSessions: launches,
    },
    { id: "answer", role: "assistant", text: "Started your reviewer." },
  ];
  const launchedSessions = new Map([
    ["app-mono-review", { state: "working" as const, title: "Review #42" }],
    ["app-mono-fix", { state: "done" as const, title: "Fix the build" }],
  ]);
  render(blocks, {
    onOpenSession,
    launchedSessions,
    onShowWork: vi.fn(),
    onSaveNote: vi.fn(),
  });
  const cards = [
    ...container.querySelectorAll<HTMLButtonElement>("[data-launched-session]"),
  ];
  expect(cards).toHaveLength(2);
  expect(
    cards[0]
      .closest("[data-transcript-turn]")
      ?.getAttribute("data-transcript-turn"),
  ).toBe("user");
  // The live title, its state and project; the rail no longer lists them.
  expect(cards.map((card) => card.dataset.state)).toEqual(["working", "done"]);
  expect(cards[0].textContent).toContain("Review #42");
  expect(cards[0].textContent).toContain("web");
  expect(cards[0].querySelector(".animate-spin")).not.toBeNull();
  expect(cards[1].getAttribute("aria-label")).toBe(
    "Open session Fix the build, done",
  );
  expect(container.querySelector('[aria-label="Show sessions"]')).toBeNull();
  act(() => cards[1].click());
  expect(onOpenSession).toHaveBeenCalledWith(
    "user",
    blocks.slice(2),
    "app-mono-fix",
  );
  render(blocks, {
    onOpenSession,
    launchedSessions,
    openSessionId: "app-mono-fix",
  });
  expect(
    container
      .querySelector('[data-launched-session="app-mono-fix"]')
      ?.getAttribute("aria-pressed"),
  ).toBe("true");
});

it("keeps launched sessions under the reply that started them when the Mono reports back", () => {
  const launch = {
    sessionId: "app-mono-article",
    cwd: "/repo",
    title: "Write an article",
    harness: "codex" as const,
    model: "gpt-6",
  };
  const blocks: Block[] = [
    {
      id: "user",
      role: "user",
      text: "Write two articles",
      startedAt: 1000,
      durationMs: 1000,
      monoSpawnedSessions: [launch],
    },
    tool("start"),
    { id: "started", role: "assistant", text: "Both are running." },
    // The app's hidden prompt when the session finishes; the Mono's report
    // back joins the same message.
    {
      id: "completion",
      role: "user",
      text: "Session finished",
      internal: true,
      appRequestId: "mono-completion-app-mono-article",
      startedAt: 5000,
    },
    tool("read"),
    { id: "report", role: "assistant", text: "Both finished." },
  ];
  render(blocks, { onOpenSession: vi.fn(), agentName: "MonoSkull" });
  expect(container.querySelectorAll("[data-transcript-turn]")).toHaveLength(1);
  const started = container.querySelector('[data-chat-message="started"]')!;
  const cards = container.querySelector("[data-launched-sessions]")!;
  const report = container.querySelector('[data-chat-message="report"]')!;
  expect(container.querySelectorAll("[data-launched-sessions]")).toHaveLength(
    1,
  );
  expect(
    started.compareDocumentPosition(cards) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(
    cards.compareDocumentPosition(report) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
});

it("keeps launched session cards in place while the Mono's report back arrives", () => {
  const launch = {
    sessionId: "app-mono-article",
    cwd: "/repo",
    title: "Write an article",
    harness: "codex" as const,
    model: "gpt-6",
  };
  const user: Block = {
    id: "user",
    role: "user",
    text: "Write two articles",
    startedAt: 1000,
    monoSpawnedSessions: [launch],
  };
  const started: Block = {
    id: "started",
    role: "assistant",
    text: "Both are running.",
  };
  const completion: Block = {
    id: "completion",
    role: "user",
    text: "Session finished",
    internal: true,
    appRequestId: "mono-completion-app-mono-article",
    startedAt: 5000,
  };
  const props = { onOpenSession: vi.fn(), agentName: "MonoCrab" };
  const cards = () => container.querySelector("[data-launched-sessions]");

  // While the reply streams, the cards wait under it.
  render([user, tool("start"), { ...started, streaming: true }], {
    ...props,
    busy: true,
  });
  expect(cards()).toBeNull();
  render([{ ...user, durationMs: 2000 }, tool("start"), started], props);
  const shown = cards();
  expect(shown).not.toBeNull();

  // The session finishing reopens the same message: the cards stay put
  // through the hidden prompt, the Mono's work and its report.
  const reopened = [{ ...user, durationMs: 2000 }, tool("start"), started];
  render([...reopened, completion], { ...props, busy: true });
  expect(cards()).toBe(shown);
  render([...reopened, completion, tool("read", "in_progress")], {
    ...props,
    busy: true,
  });
  expect(cards()).toBe(shown);
  render(
    [
      ...reopened,
      { ...completion, durationMs: 3000 },
      tool("read"),
      { id: "report", role: "assistant", text: "Both finished." },
    ],
    props,
  );
  expect(cards()).toBe(shown);
  expect(
    cards()!.compareDocumentPosition(
      container.querySelector('[data-chat-message="report"]')!,
    ) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
});

it("shows reused sessions in the requesting Mono after its handoff reply, before its completion report", () => {
  const launches = [
    {
      id: "app-other-mono-one",
      title: "Meaning in everyday life",
      harness: "codex",
      model: "gpt-6",
    },
    {
      id: "app-other-mono-two",
      title: "Growth through change",
      harness: "codex",
      model: "gpt-6",
    },
  ];
  const receipt = (id: string, action: string, result: unknown): Block => ({
    ...tool(id),
    text: `monocode app ${action}`,
    tool: { status: "completed", detail: JSON.stringify({ ok: true, result }) },
  });
  const blocks: Block[] = [
    { id: "old", role: "user", text: "Earlier task", durationMs: 1000 },
    { id: "old-answer", role: "assistant", text: "Earlier answer" },
    {
      id: "user",
      role: "user",
      text: "Write two articles",
      startedAt: 1000,
      durationMs: 1000,
    },
    receipt("list", "sessions.list", {
      cwd: "/repo/formshare",
      sessions: launches,
    }),
    // This prose splits the listing and sends into separate folded groups.
    {
      id: "assigning",
      role: "assistant",
      text: "Assigning two existing sessions.",
    },
    ...launches.map((entry) =>
      receipt(`send-${entry.id}`, "sessions.send", {
        sessionId: entry.id,
        submitted: true,
        notifyOnComplete: true,
      }),
    ),
    {
      id: "started",
      role: "assistant",
      text: "Both article sessions are running.",
    },
    {
      id: "completion",
      role: "user",
      text: "Both finished",
      internal: true,
      appRequestId: "mono-completion-batch-articles",
      startedAt: 5000,
      durationMs: 1000,
    },
    { id: "report", role: "assistant", text: "Both articles are finished." },
  ];
  const onOpenSession = vi.fn();
  render(blocks, { onOpenSession, agentName: "MonoShroom" });
  expect(container.querySelectorAll("[data-transcript-turn]")).toHaveLength(2);
  const cards = container.querySelector("[data-launched-sessions]")!;
  expect(cards).not.toBeNull();
  expect(container.querySelectorAll("[data-launched-session]")).toHaveLength(2);
  expect(
    cards
      .closest("[data-transcript-turn]")
      ?.getAttribute("data-transcript-turn"),
  ).toBe("user");
  expect(
    container
      .querySelector('[data-chat-message="started"]')!
      .compareDocumentPosition(cards) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(
    cards.compareDocumentPosition(
      container.querySelector('[data-chat-message="report"]')!,
    ) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  act(() =>
    cards.querySelector<HTMLButtonElement>("[data-launched-session]")!.click(),
  );
  expect(onOpenSession).toHaveBeenCalledWith(
    "user",
    blocks.slice(2),
    launches[0].id,
  );
});

it("waits for the final reply's reveal, then shows its artifact before the actions", () => {
  localStorage.setItem(PACED_REVEAL_KEY, "1");
  const user: Block = {
    id: "user",
    role: "user",
    text: "Review",
    startedAt: 1000,
    artifactCards: [{ id: "review", kind: "document", title: "PR review" }],
    monoSpawnedSessions: [
      {
        sessionId: "app-review",
        cwd: "/repo",
        title: "Review",
        harness: "codex",
        model: "gpt-6",
      },
    ],
  };
  const actions = {
    onOpenSession: vi.fn(),
    onShowWork: vi.fn(),
    agentName: "MonoInvader",
  };
  render([user, tool("call", "in_progress")], { ...actions, busy: true });
  expect(container.querySelector("[data-turn-actions]")).toBeNull();
  expect(container.querySelector("[data-artifact-results]")).toBeNull();
  // Session cards wait for the reply, so it never lands above them.
  expect(container.querySelector("[data-launched-sessions]")).toBeNull();
  expect(
    container.querySelector('[data-mono-work] [aria-label="Show activity"]'),
  ).not.toBeNull();

  const reply: Block = {
    id: "answer",
    role: "assistant",
    text: "Started your reviewer.",
    streaming: true,
  };
  // The reply and completion can arrive together after the artifact was saved.
  render([user, tool("call"), { ...reply, text: "" }], {
    ...actions,
    busy: true,
  });
  expect(container.querySelector("[data-turn-actions]")).toBeNull();
  expect(container.querySelector("[data-artifact-results]")).toBeNull();
  expect(container.textContent).not.toContain(reply.text);

  render(
    [
      { ...user, durationMs: 17000 },
      tool("call"),
      { ...reply, streaming: false },
    ],
    actions,
  );
  const answer = container.querySelector('[data-chat-message="answer"]')!;
  const cards = container.querySelector("[data-artifact-results]")!;
  const sessions = container.querySelector("[data-launched-sessions]")!;
  const footer = container.querySelector("[data-turn-actions]")!;
  expect(answer.textContent).toBe("");
  expect(getComputedStyle(cards).display).toBe("none");
  expect(getComputedStyle(sessions).display).toBe("none");
  expect(getComputedStyle(footer).display).toBe("none");
  act(() => vi.advanceTimersByTime(100));
  expect(answer.textContent!.length).toBeGreaterThan(0);
  expect(answer.textContent!.length).toBeLessThan(reply.text.length);
  expect(getComputedStyle(cards).display).toBe("none");
  act(() => vi.advanceTimersByTime(2500));
  expect(answer.textContent).toBe(reply.text);
  expect(getComputedStyle(cards).display).toBe("none");
  act(() => vi.advanceTimersByTime(WORD_FADE_MS));
  expect(container.querySelector(".word-fading")).toBeNull();
  // happy-dom caches :has matches after descendant changes; evaluate the
  // finished DOM afresh to check the shipped rule after the fade ends.
  const finished = cards.parentElement!.cloneNode(true) as HTMLElement;
  document.body.append(finished);
  expect(
    getComputedStyle(finished.querySelector("[data-artifact-results]")!)
      .display,
  ).not.toBe("none");
  expect(
    getComputedStyle(finished.querySelector("[data-launched-sessions]")!)
      .display,
  ).not.toBe("none");
  expect(
    getComputedStyle(finished.querySelector("[data-turn-actions]")!).display,
  ).not.toBe("none");
  finished.remove();
  // Reply, then its sessions, then its artifacts.
  expect(
    answer.compareDocumentPosition(sessions) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(
    sessions.compareDocumentPosition(cards) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(footer.querySelector('[aria-label="Show activity"]')).not.toBeNull();
  expect(
    answer.compareDocumentPosition(cards) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(
    cards.compareDocumentPosition(footer) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
});

it("shows only the final reply, with no separate work summary or opening narration", () => {
  render([
    { id: "user", role: "user", text: "Inspect" },
    { id: "intro", role: "assistant", text: "I will inspect the files." },
    tool("first"),
    { id: "progress", role: "assistant", text: "Trying another approach." },
    tool("second"),
    { id: "answer", role: "assistant", text: "Everything passed." },
  ]);
  expect(status()).toBeUndefined();
  const work = container.querySelector("[data-mono-work]")!;
  expect(work.querySelector("button")).toBeNull();
  act(() => work.dispatchEvent(new MouseEvent("click", { bubbles: true })));
  expect(container.textContent).not.toContain("Trying another approach.");
  expect(container.textContent).not.toContain("Inspect first");
  expect(container.textContent).not.toContain("I will inspect the files.");
  expect(container.textContent).toContain("Everything passed.");
  expect(container.querySelectorAll("[data-mono-work]")).toHaveLength(1);
});

it("opens the full turn activity from the footer beside copy and notes", () => {
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
  render(blocks, {
    onShowWork,
    onSaveNote: vi.fn(),
    agentName: "Captain Awesome",
  });
  const turn = container.querySelector('[data-transcript-turn="user"]')!;
  const activity = turn.querySelector<HTMLButtonElement>(
    '[aria-label="Show activity"]',
  )!;
  expect(activity.closest("[data-turn-actions]")).not.toBeNull();
  expect(
    [...activity.parentElement!.querySelectorAll("button")].map((button) =>
      button.getAttribute("aria-label"),
    ),
  ).toEqual(["Copy response", "Save as note", "Show activity"]);
  expect(turn.querySelector("[data-mono-work] button")).toBeNull();
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
  expect(status()).toBe("Pondering…");
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
  expect(status()).toBe("Considering…");
  expect(container.querySelector("[data-mono-work]")).toBe(group);
  expect(container.querySelector('[data-chat-message="answer"]')).toBeNull();
  render(worked.map((block) => ({ ...block, streaming: false })));
  settleTicker();
  expect(status()).toBeUndefined();
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

it("opens live activity from the header ticker before any tool, while a direct answer waits for completion", () => {
  const user: Block = { id: "user", role: "user", text: "Hello" };
  const props = {
    busy: true,
    agentName: "Captain",
    agentMascot: { mascot: "cat" as const, color: "#6ba" },
    onShowWork: vi.fn(),
  };
  render([user], props);
  const header = container.querySelector("[data-mono-work]")!;
  expect(header.querySelector(".pixel-mascot")).not.toBeNull();
  expect(header.textContent).toContain("Captain");
  expect(status()).toBe("Thinking…");
  expect(container.textContent).not.toContain("working for");
  const activity = header.querySelector<HTMLButtonElement>(
    '[aria-label="Show activity"]',
  )!;
  expect(activity.contains(header.querySelector('[role="status"]'))).toBe(true);
  expect(activity.getAttribute("aria-expanded")).toBe("false");
  act(() => activity.click());
  expect(props.onShowWork).toHaveBeenCalledWith("user", [user]);
  render([user], { ...props, activeWorkTurnId: "user" });
  expect(activity.getAttribute("aria-label")).toBe("Hide activity");
  expect(activity.getAttribute("aria-expanded")).toBe("true");
  act(() => activity.click());
  expect(props.onShowWork).toHaveBeenCalledTimes(2);

  const answer: Block = {
    id: "answer",
    role: "assistant",
    text: "Hello there!",
    streaming: true,
  };
  render([user, answer], props);
  settleTicker();
  expect(container.querySelector("[data-mono-work]")).toBe(header);
  expect(container.textContent).not.toContain(answer.text);
  expect(container.querySelector('[aria-label="Copy response"]')).toBeNull();
  act(() => activity.click());
  expect(props.onShowWork).toHaveBeenLastCalledWith("user", [user, answer]);

  render(
    [
      { ...user, durationMs: 1200 },
      { ...answer, streaming: false },
    ],
    { ...props, busy: false },
  );
  expect(container.textContent).toContain(answer.text);
  expect(header.querySelector("button")).toBeNull();
  expect(
    container.querySelector('[data-turn-actions] [aria-label="Show activity"]'),
  ).not.toBeNull();
});

it("copies and saves only the final answer while the activity trail retains every step", async () => {
  const onSaveNote = vi.fn();
  const blocks: Block[] = [
    { id: "user", role: "user", text: "Inspect", durationMs: 2000 },
    { id: "intro", role: "assistant", text: "I will inspect the files." },
    { id: "reasoning", role: "reasoning", text: "Weighing the options." },
    tool("first"),
    { id: "progress", role: "assistant", text: "Checking more files." },
    tool("second"),
    { id: "answer", role: "assistant", text: "Everything passed." },
    { id: "detail", role: "assistant", text: "The checks are complete." },
  ];
  render(blocks, { onSaveNote, onShowWork: vi.fn() });
  await act(async () => {
    container
      .querySelector<HTMLButtonElement>('[aria-label="Copy response"]')!
      .click();
    container
      .querySelector<HTMLButtonElement>(
        '[data-turn-actions] [aria-label="Save as note"]',
      )!
      .click();
  });
  const answer = "Everything passed.\n\nThe checks are complete.";
  expect(copyMessage).toHaveBeenCalledWith(answer, undefined);
  expect(onSaveNote).toHaveBeenCalledWith(answer);
  expect(container.textContent).not.toContain("Weighing the options.");
  expect(container.textContent).not.toContain("Checking more files.");

  act(() => root.render(createElement(MonoActivityTrail, { blocks })));
  expect(
    [
      ...container.querySelectorAll<HTMLElement>("[data-mono-activity-block]"),
    ].map((row) => row.dataset.monoActivityBlock),
  ).toEqual([
    "intro",
    "reasoning",
    "first",
    "progress",
    "second",
    "answer",
    "detail",
  ]);
  expect(container.textContent).toContain("I will inspect the files.");
  expect(container.textContent).toContain("Checking more files.");
});

it("keeps interrupted work inspectable without presenting its opening as a final answer", () => {
  const onShowWork = vi.fn();
  const blocks: Block[] = [
    { id: "user", role: "user", text: "Inspect" },
    { id: "intro", role: "assistant", text: "I will inspect the files." },
    tool("first", "cancelled"),
    {
      id: "stopped",
      role: "system",
      notice: "interrupt",
      text: "Turn stopped.",
    },
  ];
  render(blocks, { onShowWork });
  expect(container.textContent).toContain("Turn stopped.");
  expect(container.textContent).not.toContain("I will inspect the files.");
  expect(container.querySelector('[aria-label="Copy response"]')).toBeNull();
  act(() =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Show activity"]')!
      .click(),
  );
  expect(onShowWork).toHaveBeenCalledWith("user", blocks);
});

it("uses the header to show questions and background work while answers are hidden", () => {
  const blocks: Block[] = [
    { id: "user", role: "user", text: "Inspect" },
    {
      id: "intro",
      role: "assistant",
      text: "I have a question.",
      streaming: true,
    },
  ];
  render(blocks, { busy: true, pendingQuestion: true });
  expect(status()).toBe("Waiting for answers…");
  expect(container.textContent).not.toContain("I have a question.");
  render(blocks, { busy: true, backgroundTasks: ["Review"] });
  settleTicker();
  expect(status()).toBe("Waiting for background task…");
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
  expect(current[0].hasAttribute("data-mono-work")).toBe(true);
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
  expect(rows[3].hasAttribute("data-mono-work")).toBe(true);
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

it("shows answers to mid-turn follow-ups immediately and retains them during later work", () => {
  const props = { busy: true, agentName: "MonoInvader" };
  const blocks: Block[] = [
    { id: "user", role: "user", text: "Review the PR", startedAt: 1000 },
    { id: "intro", role: "assistant", text: "I will inspect the files." },
    tool("first"),
    {
      id: "follow-up",
      role: "user",
      text: "What are you doing?",
      sentAt: 1100,
    },
    {
      id: "status-reply",
      role: "assistant",
      text: "I am checking browser security.",
      streaming: true,
    },
  ];
  render(blocks, props);
  expect(container.textContent).toContain("I am checking browser security.");
  expect(container.textContent).not.toContain("I will inspect the files.");
  const continued: Block[] = [
    ...blocks.map((block) =>
      block.id === "status-reply" ? { ...block, streaming: false } : block,
    ),
    tool("second", "in_progress"),
    { id: "stop", role: "user", text: "You can stop", sentAt: 1200 },
    { id: "stop-reply", role: "assistant", text: "Stopping the review now." },
    tool("cancel", "in_progress"),
  ];
  render(continued, props);
  act(() => vi.advanceTimersByTime(1000));
  expect(container.textContent).toContain("I am checking browser security.");
  expect(container.textContent).toContain("Stopping the review now.");
  expect(container.textContent).not.toContain("Inspect second");
  expect(container.querySelectorAll("[data-mono-work]")).toHaveLength(1);
  render(continued, { ...props, busy: false });
  expect(container.textContent).toContain("I am checking browser security.");
  expect(container.textContent).toContain("Stopping the review now.");
});

it.each(["pending", "paused", "failed"] as const)(
  "keeps narration hidden until a %s follow-up is delivered",
  (delivery) => {
    const blocks: Block[] = [
      { id: "user", role: "user", text: "Review the PR", startedAt: 1000 },
      { id: "intro", role: "assistant", text: "I will inspect the files." },
      tool("first"),
      { id: "follow-up", role: "user", text: "Status?", sentAt: 1100 },
      { id: "reply", role: "assistant", text: "Checking browser security." },
    ];
    render(blocks, {
      busy: true,
      messageDeliveries: new Map([["follow-up", { status: delivery }]]),
    });
    expect(container.textContent).not.toContain("Checking browser security.");
    render(blocks, { busy: true, messageDeliveries: new Map() });
    expect(container.textContent).toContain("Checking browser security.");
    expect(container.textContent).not.toContain("I will inspect the files.");
  },
);

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

it("does not repeat a thought's first paragraph above it once opened", () => {
  act(() =>
    root.render(
      createElement(MonoActivityTrail, {
        blocks: [
          tool("before"),
          {
            id: "thought",
            role: "reasoning",
            text: "Check the config first.\n\nThen run the tests.",
          },
          tool("after"),
        ],
      }),
    ),
  );
  const row = container.querySelector<HTMLButtonElement>(
    '[aria-label="Show thinking: Check the config first."]',
  )!;

  act(() => row.click());

  expect(row.getAttribute("aria-label")).toBe("Hide thinking");
  expect(row.textContent).toBe("Thinking");
  expect(container.textContent?.split("Check the config first.")).toHaveLength(
    2,
  );
  expect(container.textContent).toContain("Then run the tests.");
});
