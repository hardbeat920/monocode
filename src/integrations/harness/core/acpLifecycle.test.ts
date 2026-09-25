import { describe, expect, it, vi } from "vitest";
import { recoverAcpSession, unknownAcpRequest } from "./acpLifecycle";

describe("ACP lifecycle", () => {
  it("resumes before loading and creates only after both recovery paths fail", async () => {
    const calls: string[] = [];
    const setup = (sessionId: string) => ({ sessionId });
    await expect(recoverAcpSession("saved", {
      resume: async () => { calls.push("resume"); throw new Error("unsupported"); },
      load: async () => { calls.push("load"); throw new Error("missing"); },
      create: async () => { calls.push("create"); return setup("new"); },
      sessionId: (value) => value.sessionId,
    })).resolves.toEqual({ sessionId: "new", setup: { sessionId: "new" }, restored: false });
    expect(calls).toEqual(["resume", "load", "create"]);
  });

  it("does not stack load or create after a resume timeout", async () => {
    const calls: string[] = [];
    await expect(recoverAcpSession("saved", {
      resume: async () => { calls.push("resume"); throw new Error("resume timed out"); },
      load: async () => { calls.push("load"); return { sessionId: "loaded" }; },
      create: async () => { calls.push("create"); return { sessionId: "new" }; },
      sessionId: (value) => value.sessionId,
      isTimeout: (error) => error instanceof Error && error.message.includes("timed out"),
    })).rejects.toThrow("timed out");
    expect(calls).toEqual(["resume"]);
  });

  it("preserves raw IDs when rejecting unknown methods", async () => {
    const respondError = vi.fn(async () => undefined);
    await unknownAcpRequest(respondError, "raw-7", "future/request");
    expect(respondError).toHaveBeenCalledWith("raw-7", {
      code: -32601,
      message: "Method not found: future/request",
    });
  });
});
