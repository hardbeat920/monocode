// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { newSession } from "../lib/session";
import { getSession, sanitizeSessionForPersist } from "../lib/sessionStore";
import { AgentTranscript } from "./AgentTranscript";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

it("reopens a saved Cursor placeholder as a named, expandable subagent with a step count", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const saved = newSession("cursor", "/repo");
  saved.providerSessionId = "parent";
  saved.blocks = [
    { id: "user", role: "user", text: "Review the changes" },
    {
      id: "spawn",
      role: "tool",
      text: ": Subagent task",
      tool: {
        callId: "call-1",
        title: ": Subagent task",
        kind: "agent",
        status: "completed",
      },
    },
  ];
  invoke.mockImplementation(async (command: string) => {
    if (command === "session_get")
      return {
        ...sanitizeSessionForPersist(saved),
        createdAt: 1,
        updatedAt: 2,
      };
    if (command === "cursor_subagent_runs")
      return [
        {
          agentId: "child",
          toolCallId: "call-1",
          revision: "5",
          model: "grok-4.6",
          prompt: "Perform a read-only code review of ACP routing in /repo.",
          steps: [
            {
              id: "child:tool:read",
              kind: "tool",
              toolName: "Read",
              args: { path: "/repo/acp.ts" },
              text: "",
              status: "completed",
            },
            {
              id: "child:message:0",
              kind: "message",
              text: "Checked the notification routing.",
            },
          ],
        },
      ];
    throw new Error(`Unexpected command: ${command}`);
  });
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  try {
    const session = await getSession(saved.id);
    expect(invoke).toHaveBeenCalledWith("cursor_subagent_runs", {
      sessionId: "parent",
      toolCallIds: ["call-1"],
      knownRevisions: {},
    });
    act(() =>
      root.render(
        createElement(AgentTranscript, {
          blocks: session!.blocks,
          busy: false,
        }),
      ),
    );
    const button = container.querySelector<HTMLButtonElement>(
      'button[aria-label="Show Review ACP routing\'s work"]',
    );
    expect(button).not.toBeNull();
    expect(button!.textContent).toContain("1 step");
    expect(button!.textContent).toContain("Grok 4.6");
    expect(
      sanitizeSessionForPersist(session!).blocks.find(
        (block) => block.tool?.callId === "call-1",
      )?.agentRun?.model,
    ).toBe("grok-4.6");
    expect(button!.getAttribute("aria-expanded")).toBe("false");
    act(() => button!.click());
    expect(button!.getAttribute("aria-expanded")).toBe("true");
    expect(container.textContent).toContain(
      "Checked the notification routing.",
    );
    expect(container.textContent).not.toContain("Subagent task");
    act(() => button!.click());
    expect(button!.getAttribute("aria-expanded")).toBe("false");
  } finally {
    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  }
});
