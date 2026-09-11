import { createElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
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

  it("opens failed subagent work but keeps provider details collapsed", () => {
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

    expect(markup).toContain("Subagent failed");
    expect(markup).not.toContain("Child process disconnected");
    expect(markup).toContain("Show error details for Inspect auth");
    expect(markup).toContain('aria-label="Hide the work"');
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
