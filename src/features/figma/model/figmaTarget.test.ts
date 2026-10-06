import { afterEach, describe, expect, it, vi } from "vitest";
import { newSession, type Session } from "../../sessions/model/session";
import {
  figmaSessionTargetFor,
  figmaSessionTargetFrom,
  setFigmaSessionTarget,
  subscribeFigmaSessionTarget,
} from "./figmaTarget";

function session(cwd: string, changes: Partial<Session> = {}): Session {
  return {
    ...newSession("codex", cwd, "gpt-5"),
    title: "Checkout",
    ...changes,
  };
}

afterEach(() => setFigmaSessionTarget(null));

describe("figma session target", () => {
  it("describes the selected session the component will be generated in", () => {
    const selected = session("/work/app", { busy: true });
    expect(figmaSessionTargetFrom(selected)).toEqual({
      sessionId: selected.id,
      cwd: "/work/app",
      workCwd: "/work/app",
      title: "Checkout",
      harness: "codex",
      model: selected.model,
      modelSettings: selected.modelSettings,
      busy: true,
    });
  });

  it("names the session the way its tab does", () => {
    expect(
      figmaSessionTargetFrom(newSession("claude", "/work/app", "opus"))?.title,
    ).toBe("New session");
    expect(
      figmaSessionTargetFrom(
        session("/work/app", { title: "codex · Checkout" }),
      )?.title,
    ).toBe("Checkout");
  });

  it("works in the session's worktree when it has one", () => {
    expect(
      figmaSessionTargetFrom(
        session("/work/app", {
          workspaceMode: "worktree",
          worktreeCwd: "/work/app-worktrees/figma",
        }),
      )?.workCwd,
    ).toBe("/work/app-worktrees/figma");
  });

  it("skips sessions a design generation must not land in", () => {
    expect(figmaSessionTargetFrom(null)).toBeNull();
    expect(
      figmaSessionTargetFrom(
        session("/work/app", { orchestrationLeadId: "lead" }),
      ),
    ).toBeNull();
    expect(
      figmaSessionTargetFrom(session("/work/app", { worktreeRemoved: true })),
    ).toBeNull();
    expect(
      figmaSessionTargetFrom(
        session("/work/app", { workspaceMode: "worktree" }),
      ),
    ).toBeNull();
    expect(
      figmaSessionTargetFrom(
        session("/work/app", {
          workspaceMode: "worktree",
          worktreeCwd: "/work/app-worktrees/figma",
          worktreePreparing: true,
        }),
      ),
    ).toBeNull();
    expect(
      figmaSessionTargetFrom(session("remote://host/home/me/app")),
    ).toBeNull();
  });

  it("only offers the target to its own project", () => {
    setFigmaSessionTarget(figmaSessionTargetFrom(session("/work/app")));
    expect(figmaSessionTargetFor("/work/app")?.title).toBe("Checkout");
    expect(figmaSessionTargetFor("/work/site")).toBeNull();
  });

  it("notifies only when the target actually changes", () => {
    const listener = vi.fn();
    const stop = subscribeFigmaSessionTarget(listener);
    const selected = session("/work/app");
    setFigmaSessionTarget(figmaSessionTargetFrom(selected));
    setFigmaSessionTarget(figmaSessionTargetFrom({ ...selected }));
    setFigmaSessionTarget(figmaSessionTargetFrom({ ...selected, busy: true }));
    setFigmaSessionTarget(null);
    stop();
    expect(listener).toHaveBeenCalledTimes(3);
    expect(figmaSessionTargetFor("/work/app")).toBeNull();
  });
});
