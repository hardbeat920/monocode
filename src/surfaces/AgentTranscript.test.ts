import { createElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { Block } from "../lib/session";
import { AgentTranscript } from "./AgentTranscript";

function tool(id: string, approval?: Block["approval"]): Block {
  return {
    id,
    role: "tool",
    text: `Inspect hidden-detail-${id}`,
    tool: { kind: "shell", status: approval ? "pending" : "completed" },
    ...(approval ? { approval } : {}),
  };
}

function render(
  blocks: Block[],
  busy = false,
  latestTurnAccessory?: ReactNode,
) {
  return renderToStaticMarkup(
    createElement(AgentTranscript, { blocks, busy, latestTurnAccessory }),
  );
}

describe("AgentTranscript collapsed work", () => {
  it("keeps each completed turn's recorded model label", () => {
    const blocks: Block[] = [
      {
        id: "user",
        role: "user",
        text: "Remember this",
        durationMs: 9_000,
        turnModel: {
          harness: "claude",
          id: "claude:sonnet-5",
          name: "Claude Sonnet 5",
        },
      },
      { id: "answer", role: "assistant", text: "Remembered." },
    ];
    const markup = renderToStaticMarkup(
      createElement(AgentTranscript, {
        blocks,
        harness: "claude",
        model: "claude:opus-5",
      }),
    );

    expect(markup).toContain("Claude Sonnet 5 worked for 9s");
    expect(markup).not.toContain("Claude Opus 5 worked for 9s");
  });

  it("does not assign the current model to a legacy completed turn", () => {
    const blocks: Block[] = [
      {
        id: "user",
        role: "user",
        text: "Old prompt",
        durationMs: 9_000,
      },
      { id: "answer", role: "assistant", text: "Old answer." },
    ];
    const markup = renderToStaticMarkup(
      createElement(AgentTranscript, {
        blocks,
        harness: "claude",
        model: "claude:opus-5",
      }),
    );

    expect(markup).toContain("Worked for 9s");
    expect(markup).not.toContain("Claude Opus 5 worked for 9s");
  });

  it("renders the summary and answer without mounting a large completed tool trail", () => {
    const blocks: Block[] = [
      { id: "user", role: "user", text: "Check the project" },
      ...Array.from({ length: 1357 }, (_, index) => tool(String(index))),
      { id: "answer", role: "assistant", text: "The project checks passed." },
    ];
    const markup = render(blocks);
    expect(markup).toContain("The project checks passed.");
    expect(markup).toContain("Show the work");
    expect(markup.includes("hidden-detail-")).toBe(false);
    const short = render([blocks[0], tool("one"), tool("two"), blocks.at(-1)!]);
    const tagCount = (html: string) => html.match(/<[a-z]/g)?.length ?? 0;
    expect(tagCount(markup)).toBe(tagCount(short));
  });

  it("keeps live work visible before the assistant answers", () => {
    expect(render([tool("live")], true)).toContain("hidden-detail-live");
  });

  it("keeps an unresolved approval visible even when narration follows it", () => {
    const markup = render(
      [
        tool("approval", { requestId: 1 }),
        {
          id: "answer",
          role: "assistant",
          text: "Please approve the command.",
        },
      ],
      true,
    );
    expect(markup).toContain("hidden-detail-approval");
    expect(markup).toContain("Please approve the command.");
    expect(markup.includes('aria-label="Show the work"')).toBe(false);
  });

  it("opens a failed subagent's own row on its provider reason", () => {
    const markup = render([
      { id: "user", role: "user", text: "Delegate this", startedAt: 1_000 },
      {
        id: "agent",
        role: "tool",
        text: "Inspect auth",
        tool: {
          callId: "agent-1",
          kind: "agent",
          status: "failed",
          detail: "Child process disconnected",
        },
      },
      { id: "answer", role: "assistant", text: "I could not finish." },
    ]);

    expect(markup).toContain("Inspect auth");
    expect(markup).toContain("failed");
    expect(markup).toContain("Child process disconnected");
    expect(markup).toContain("Hide Inspect auth&#x27;s work");
  });

  it("gives each running subagent its own row above the work that folds", () => {
    const markup = render(
      [
        { id: "user", role: "user", text: "Review this", startedAt: 1_000 },
        { id: "lead", role: "assistant", text: "I will run two reviews." },
        {
          id: "a1",
          role: "tool",
          text: "Correctness review",
          tool: { callId: "agent-1", kind: "agent", status: "in_progress" },
          agentRun: {
            name: "Correctness review",
            steps: [
              {
                id: "s1",
                kind: "tool",
                text: "Read src/App.tsx",
                status: "completed",
              },
            ],
          },
        },
        {
          id: "a2",
          role: "tool",
          text: "Quality review",
          tool: { callId: "agent-2", kind: "agent", status: "in_progress" },
        },
        tool("t1"),
        { id: "answer", role: "assistant", text: "Both reviewers agree." },
      ],
      true,
    );

    // A row each, named, hopping while the run is live — no grouped header.
    expect(markup).toContain("Correctness review");
    expect(markup).toContain("Quality review");
    expect(markup).toContain("mascot-active");
    expect(markup).not.toContain("are working");
    // A row counts its agent's work; it does not echo the call in flight,
    // which put a second scrolling command line on every row.
    expect(markup).toContain("1 step");
    expect(markup).not.toContain("Read src/App.tsx");
    expect(markup).not.toContain("starting up");
    // The name carries the shimmer while the run is live, and each row opens
    // on its own.
    expect(markup).toContain("shimmer-text");
    expect(markup).toContain("Show Correctness review&#x27;s work");
    // The rows sit outside the fold, so they stay put as the work collapses.
    expect(markup).toContain("Both reviewers agree.");
  });

  it("keeps the turn's status line at the top of the turn above a stack", () => {
    const markup = render(
      [
        { id: "user", role: "user", text: "Review this", startedAt: 1_000 },
        { id: "lead", role: "assistant", text: "I will run two reviews." },
        tool("t0"),
        { id: "plan", role: "assistant", text: "Splitting the review in two." },
        {
          id: "a1",
          role: "tool",
          text: "Independently review the current repository's recent changes for correctness and regressions. Inspect the uncommitted diff.",
          tool: { callId: "agent-1", kind: "agent", status: "in_progress" },
        },
        tool("t1"),
        { id: "answer", role: "assistant", text: "Both reviewers agree." },
      ],
      true,
    );

    // The line the work folds behind sits above everything it folds, and a
    // stack of delegated runs no longer pushes it down the turn.
    const statusAt = markup.indexOf("Show the work");
    const stackAt = markup.indexOf("Independently review");
    expect(statusAt).toBeGreaterThan(-1);
    expect(stackAt).toBeGreaterThan(statusAt);
    // The work around it is collapsed away, and the stack is still on screen:
    // it is pinned outside the fold's body, not inside it.
    expect(markup).not.toContain("hidden-detail-t1");
    expect(markup).not.toContain("Splitting the review in two.");
    // A row-length name is capped, and the whole brief stays on the hover.
    expect(markup).toContain(
      "Independently review the current repository&#x27;s recent…",
    );
    expect(markup).toContain("Inspect the uncommitted diff.");
  });

  it("groups an opened subagent's trail the way the main transcript does", () => {
    const markup = render([
      { id: "user", role: "user", text: "Review this", durationMs: 4_000 },
      {
        id: "a1",
        role: "tool",
        // A failed run opens itself, which is the only way to see an open
        // panel without a click.
        text: "Correctness review",
        tool: { callId: "agent-1", kind: "agent", status: "failed" },
        agentRun: {
          name: "Correctness review",
          steps: [
            { id: "s1", kind: "message", text: "Reading the diff first." },
            { id: "s2", kind: "tool", text: "Read src/App.tsx" },
            { id: "s3", kind: "tool", text: "Read src/lib/session.ts" },
          ],
        },
      },
      { id: "answer", role: "assistant", text: "It could not finish." },
    ]);

    // The run's own words title a group, with the calls they introduced under
    // it — not one flat dump of every step it took.
    expect(markup).toContain("Reading the diff first.");
    expect(markup).toContain("Show the steps for Reading the diff first.");
    // An open row holds its wash, so the panel reads as hanging off it.
    expect(markup).toContain("hover:bg-content/8 bg-content/8");
    // The panel opens straight onto its phases. A scroll window of its own
    // here would nest one 17.5rem scroller inside the window each phase
    // already keeps, and the inner one could never reach its last row.
    expect(markup).toContain(
      'data-open="true"><div class="flex min-w-0 flex-col pb-1">',
    );
    // A settled group stays folded behind its header, so opening a long run
    // no longer dumps every call it made on screen at once.
    expect(markup).toContain('class="zen-phase-body" data-open="false"');
    expect(markup).not.toContain("src/lib/session.ts");
  });

  it("opens a lone subagent straight into its own transcript", () => {
    const markup = render([
      { id: "user", role: "user", text: "Review this", durationMs: 4_000 },
      {
        id: "a1",
        role: "tool",
        text: "Correctness review",
        tool: { callId: "agent-1", kind: "agent", status: "completed" },
        agentRun: {
          name: "Correctness review",
          steps: [
            { id: "s1", kind: "tool", text: "Read src/App.tsx", status: "completed" },
            { id: "s2", kind: "message", text: "Nothing to flag." },
          ],
        },
      },
      { id: "answer", role: "assistant", text: "Clean." },
    ]);

    // One agent needs no stack header: its own row is the row.
    expect(markup).not.toContain("Show every subagent");
    expect(markup).toContain("Correctness review");
    expect(markup).toContain("1 step");
    expect(markup).not.toContain("done");
    expect(markup).toContain("Show Correctness review&#x27;s work");
  });

  it("places a session accessory after the latest reply and before its action row", () => {
    const markup = render(
      [
        {
          id: "user",
          role: "user",
          text: "Change the files",
          startedAt: 1_000,
          durationMs: 500,
        },
        { id: "answer", role: "assistant", text: "Done changing files." },
      ],
      false,
      createElement("aside", { "data-test-review": true }, "Changed files"),
    );

    expect(markup.indexOf("Done changing files.")).toBeLessThan(
      markup.indexOf("Changed files"),
    );
    expect(markup.indexOf("Changed files")).toBeLessThan(
      markup.indexOf('aria-label="Worked for 1s"'),
    );
  });
});

describe("AgentTranscript compact transcript", () => {
  beforeEach(() => {
    Object.defineProperty(globalThis, "localStorage", {
      value: {
        getItem: (key: string) =>
          key === "monocode.transcriptCompact" ? "1" : null,
      },
      configurable: true,
    });
  });
  afterEach(() => {
    Reflect.deleteProperty(globalThis, "localStorage");
  });

  it("folds live work behind the line that titles it", () => {
    const markup = render([tool("one"), tool("two"), tool("three")], true);
    expect(markup).toContain("Running 3 commands");
    expect(markup.includes("hidden-detail-")).toBe(false);
  });

  it("still opens a group holding a call that waits on an approval", () => {
    const markup = render(
      [tool("one"), tool("two"), tool("approval", { requestId: 1 })],
      true,
    );
    expect(markup).toContain("hidden-detail-approval");
    expect(markup).toContain("hidden-detail-one");
  });
});
