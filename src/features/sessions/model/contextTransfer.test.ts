import { describe, expect, it, vi } from "vitest";
import { prepareContextTransferInput } from "./contextTransfer";
import { buildPortableContext } from "./portableContext";
import { newSession } from "./session";
import type { SendTurnInput } from "../../../integrations/harness/core/types";

function input(): SendTurnInput {
  const session = { ...newSession("claude", "/repo"), blocks: [{ id: "u1", role: "user" as const, text: "Prior instruction" }] };
  return {
    sessionId: session.id, cwd: "/repo", model: "claude:opus", runtimeMode: "supervised",
    text: "Current unique request", onEvent: vi.fn(), onAccepted: vi.fn(),
    contextTransfer: { context: buildPortableContext(session), onDelivered: vi.fn() },
  };
}

describe("shared context delivery", () => {
  it("uses inline context for text-only adapters and acknowledges it only after target evidence", () => {
    const source = input();
    const prepared = prepareContextTransferInput(source);
    expect(prepared.text).toContain("Prior instruction");
    expect(prepared.text.match(/Current unique request/g)).toHaveLength(1);
    expect(prepared.contextTransfer).toBeUndefined();
    prepared.onEvent({ type: "session.started" });
    prepared.onEvent({ type: "session.error", message: "Auth failed" });
    expect(source.onAccepted).not.toHaveBeenCalled();
    expect(source.contextTransfer?.onDelivered).not.toHaveBeenCalled();
    prepared.onEvent({ type: "session.providerBound", providerSessionId: "native-1" });
    prepared.onEvent({ type: "message.delta", text: "Response" });
    prepared.onAccepted?.();
    expect(source.onAccepted).toHaveBeenCalledOnce();
    expect(source.contextTransfer?.onDelivered).toHaveBeenCalledOnce();
    expect(source.contextTransfer?.onDelivered).toHaveBeenCalledWith(expect.objectContaining({ mode: "inline", providerSessionId: "native-1" }));
  });

  it("leaves native messages to capable adapters and waits for their acceptance callback", () => {
    const source = input();
    const prepared = prepareContextTransferInput(source, { nativeMessages: true, resumedAppend: true });
    expect(prepared.text).toBe(source.text);
    expect(prepared.contextTransfer).toBe(source.contextTransfer);
    prepared.onEvent({ type: "turn.started", providerTurnId: "previous-thread-turn" });
    expect(source.onAccepted).not.toHaveBeenCalled();
    prepared.onAccepted?.();
    prepared.onAccepted?.();
    expect(source.onAccepted).toHaveBeenCalledOnce();
    expect(source.contextTransfer?.onDelivered).not.toHaveBeenCalled();
  });
});
