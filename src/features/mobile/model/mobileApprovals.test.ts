import { relayApproval, dropRelays } from "../../monos/model/monoHabits";
import { describe, expect, it, vi } from "vitest";
import { newSession } from "../../sessions/model/session";
import { applyHarnessEvent } from "../../../integrations/harness/core/apply";
import {
  markMobileApprovalResolved,
  mobileApprovalDecision,
  mobileApprovals,
  respondMobileApproval,
} from "./mobileApprovals";

const session = () =>
  applyHarnessEvent(newSession("codex", "/tmp"), {
    type: "approval.requested",
    requestId: 7,
    title: "Run command?",
    kind: "shell",
    callId: "call",
    preview: { kind: "shell", title: "rm -rf build", path: "/tmp/build" },
  });
describe("mobile approvals", () => {
  it("exposes context and exactly the JavaScript desktop decisions", () => {
    const live = session();
    const [approval] = mobileApprovals(live);
    expect(approval.detail).toContain("rm -rf build");
    expect(approval.detail).toContain("/tmp/build");
    expect(approval.decisions.map((d) => d.id)).toEqual(["deny", "allow"]);
    expect(mobileApprovals(live)[0].token).toBe(approval.token);
    expect(
      mobileApprovalDecision(live, { ...approval, decision: "allow" }),
    ).toEqual(approval);
    expect(() =>
      mobileApprovalDecision(live, { ...approval, decision: "session" }),
    ).toThrow("not available");
    expect(() =>
      mobileApprovalDecision(
        { ...live, id: "other", blocks: [] },
        { ...approval, decision: "allow" },
      ),
    ).toThrow("resolved or replaced");
  });
  it("rejects desktop-resolved requests and reused IDs, including reuse on the same tool", () => {
    const live = session();
    const [old] = mobileApprovals(live);
    const resolved = applyHarnessEvent(live, {
      type: "approval.resolved",
      requestId: 7,
      decision: "allow",
    });
    expect(mobileApprovals(resolved)).toEqual([]);
    expect(() =>
      mobileApprovalDecision(resolved, { ...old, decision: "deny" }),
    ).toThrow("resolved or replaced");
    const replacement = applyHarnessEvent(live, {
      type: "approval.requested",
      requestId: 7,
      title: "Run command?",
      kind: "shell",
      callId: "call",
    });
    expect(mobileApprovals(replacement)[0].token).not.toBe(old.token);
    expect(() =>
      mobileApprovalDecision(replacement, { ...old, decision: "allow" }),
    ).toThrow("resolved or replaced");
  });
  it("immediately retires a desktop response before the provider resolution event flushes", () => {
    const live = session();
    const [approval] = mobileApprovals(live);
    markMobileApprovalResolved(live, 7);
    expect(mobileApprovals(live)).toEqual([]);
    expect(() =>
      mobileApprovalDecision(live, { ...approval, decision: "allow" }),
    ).toThrow("resolved or replaced");
  });
  it("validates a Mono relay against the exact source approval in its hidden run", () => {
    const run = session();
    const mono = newSession("codex", "/tmp");
    const requestId = relayApproval({
      monoId: mono.id,
      runId: run.id,
      requestId: 7,
      blockId: "relayed",
      approval: run.blocks[0].approval!,
    });
    mono.blocks = [
      {
        ...run.blocks[0],
        id: "relayed",
        approval: { requestId },
        monoHabit: { id: "habit", name: "Check", at: 1 },
      },
    ];
    const [approval] = mobileApprovals(mono, [mono, run]);
    expect(approval.decisions).toHaveLength(2);
    expect(
      mobileApprovalDecision(mono, { ...approval, decision: "allow" }, [
        mono,
        run,
      ]),
    ).toEqual(approval);
    const replaced = applyHarnessEvent(run, {
      type: "approval.requested",
      requestId: 7,
      title: "Run?",
      callId: "call",
    });
    expect(mobileApprovals(mono, [mono, replaced])[0].decisions).toEqual([]);
    expect(() =>
      mobileApprovalDecision(mono, { ...approval, decision: "allow" }, [
        mono,
        replaced,
      ]),
    ).toThrow("no longer available");
    dropRelays(run.id);
  });
  it("disables responses for removed worktrees and remote delivery without acknowledgement", () => {
    for (const live of [
      { ...session(), worktreeRemoved: true },
      { ...session(), cwd: "remote://host/tmp" },
    ]) {
      const [approval] = mobileApprovals(live);
      expect(approval.decisions).toEqual([]);
      expect(approval.unsupported).toBeTruthy();
      expect(() =>
        mobileApprovalDecision(live, { ...approval, decision: "allow" }),
      ).toThrow();
    }
  });
  it("uses the desktop command handler to validate delivery and propagate failures", () => {
    const live = session();
    const [prompt] = mobileApprovals(live);
    const params = { requestId: prompt.requestId, token: prompt.token, decision: "allow" };
    const deliver = vi.fn<() => void>(() => { throw new Error("Provider unavailable"); });
    expect(() => respondMobileApproval(live.id, params, [live], deliver)).toThrow("Provider unavailable");
    expect(mobileApprovals(live)).toHaveLength(1);
    expect(() => respondMobileApproval("missing", params, [live], deliver)).toThrow("no longer available");
    // Device A and B race through the same handler. Only the first current
    // instance can reach the desktop response callback.
    deliver.mockImplementation(() => markMobileApprovalResolved(live, prompt.requestId));
    expect(respondMobileApproval(live.id, params, [live], deliver)).toEqual({ sessionId: live.id });
    expect(() => respondMobileApproval(live.id, params, [live], deliver)).toThrow("resolved or replaced");
    expect(deliver).toHaveBeenCalledTimes(2); // failed attempt plus one delivery
  });
  it("bounds large previews and refuses decisions when the phone cannot show all context", () => {
    const live = session();
    live.blocks[0].tool!.detail = "👋\"\n".repeat(1_000_000);
    const [prompt] = mobileApprovals(live);
    expect(JSON.stringify(prompt).length).toBeLessThan(100_000);
    expect(prompt.unsupported).toContain("too large");
    expect(prompt.decisions).toEqual([]);
    expect(() => mobileApprovalDecision(live, { ...prompt, decision: "allow" })).toThrow("too large");
    live.blocks[0].tool!.detail = "\u0000".repeat(30_000);
    live.blocks = Array.from({ length: 100 }, (_, i) => ({ ...live.blocks[0], approval: { requestId: i } }));
    const prompts = mobileApprovals(live);
    expect(prompts).toHaveLength(65);
    expect(JSON.stringify(prompts).length).toBeLessThan(500_000);
    expect(prompts[64].unsupported).toContain("remaining approvals");
  });

});
