import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HarnessEvent } from "../../../integrations/harness";

const sendHarnessTurn = vi.fn();
vi.mock("../../../integrations/harness/core/registry", () => ({
  sendHarnessTurn: (input: unknown) => sendHarnessTurn(input),
  cancelHarnessTurn: vi.fn(),
  respondHarnessApproval: vi.fn(),
  respondHarnessQuestion: vi.fn(),
}));

import { requestOutgoingHandoff } from "./handoffTurn";

const input = {
  harness: "claude" as const,
  sessionId: "s1",
  cwd: "/tmp/project",
  model: "claude:opus",
  userRequest: "look again",
};

function streams(...events: HarnessEvent[]) {
  sendHarnessTurn.mockImplementationOnce(
    async ({ onEvent }: { onEvent: (event: HarnessEvent) => void }) => {
      for (const event of events) onEvent(event);
    },
  );
}

describe("requestOutgoingHandoff", () => {
  beforeEach(() => sendHarnessTurn.mockReset());

  it("returns the recap the outgoing agent streamed", async () => {
    streams({ type: "message.delta", text: "Session so far: s2 not started" });
    await expect(requestOutgoingHandoff(input)).resolves.toBe(
      "Session so far: s2 not started",
    );
  });

  it("drops an error the provider streamed as assistant text", async () => {
    const error =
      "Your organization has disabled Claude subscription access for Claude Code";
    streams(
      { type: "message.delta", text: error },
      { type: "session.error", message: error },
    );
    await expect(requestOutgoingHandoff(input)).resolves.toBe("");
  });

  it("drops partial text when the turn throws", async () => {
    sendHarnessTurn.mockImplementationOnce(
      async ({ onEvent }: { onEvent: (event: HarnessEvent) => void }) => {
        onEvent({ type: "message.delta", text: "Session so far" });
        throw new Error("Claude Code exited");
      },
    );
    await expect(requestOutgoingHandoff(input)).resolves.toBe("");
  });
});
