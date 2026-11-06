import { describe, expect, it } from "vitest";
import { newSession, type Session } from "../../sessions/model/session";
import { mobileHandoff } from "./mobileHandoff";

function source(): Session {
  return {
    ...newSession("claude", "/repo", undefined, "supervised"),
    worktreeCwd: "/repo-worktree",
    branch: "fix",
    blocks: [
      {
        id: "u1",
        role: "user",
        text: "Fix the first bug",
        startedAt: 1,
        durationMs: 10,
        turnModel: {
          harness: "claude",
          id: "claude-sonnet-4-6",
          name: "Sonnet",
        },
      },
      { id: "a1", role: "assistant", text: "The first bug is fixed." },
      {
        id: "u2",
        role: "user",
        text: "Now fix the second bug",
        startedAt: 20,
        durationMs: 10,
      },
      { id: "a2", role: "assistant", text: "The second bug is fixed." },
    ],
  };
}
const target = {
  turnId: "u1",
  harness: "codex",
  model: "gpt-5.4",
  modelSettings: { effort: "high" },
};

describe("mobile reply handoff", () => {
  it("creates a draft in the same working copy, bounded at the selected reply", () => {
    const original = source();
    const before = structuredClone(original);
    const created = mobileHandoff(original, target);
    expect(original).toEqual(before);
    expect(created.id).not.toBe(original.id);
    expect(created).toMatchObject({
      harness: "codex",
      model: target.model,
      runtimeMode: "supervised",
      worktreeCwd: "/repo-worktree",
      branch: "fix",
      blocks: [],
    });
    expect(created.handoffCard).toMatchObject({
      from: "claude",
      to: "codex",
      request: "Fix the first bug",
    });
    expect(created.handoffCard?.brief).toContain("The first bug is fixed.");
    expect(created.handoffCard?.brief).not.toContain("second bug");
  });

  it("rejects missing replies, the same provider, and a removed working copy", () => {
    expect(() =>
      mobileHandoff(source(), { ...target, turnId: "missing" }),
    ).toThrow("no longer available");
    expect(() =>
      mobileHandoff(source(), { ...target, harness: "claude" }),
    ).toThrow("another provider");
    expect(() =>
      mobileHandoff({ ...source(), worktreeRemoved: true }, target),
    ).toThrow("can't be handed off");
  });

  it("allows a completed earlier reply while the latest reply is still streaming", () => {
    const live = { ...source(), busy: true };
    expect(mobileHandoff(live, target).handoffCard?.brief).toContain(
      "first bug",
    );
    expect(() => mobileHandoff(live, { ...target, turnId: "u2" })).toThrow(
      "no longer available",
    );
  });
});
