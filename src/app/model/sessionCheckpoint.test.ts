// @vitest-environment happy-dom
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  applySessionCheckpoint,
  flushSessionCheckpoint,
} from "../../features/sessions/model/checkpoint";
import type { HarnessEvent } from "../../integrations/harness/core/types";
import {
  prepareWorkerCheckpoint,
  settleWorkerCheckpoint,
  trackSessionEdits,
} from "./sessionCheckpoint";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const checkoutCwd = "/worktrees/worker";
const workspace = {
  id: "checkout:/worktrees/worker",
  projectCwd: "/repo",
  checkoutCwd,
  kind: "worktree",
} satisfies NonNullable<
  Parameters<typeof prepareWorkerCheckpoint>[0]["workspace"]
>;
const started = {
  type: "tool.started",
  callId: "edit-1",
  title: "Edit src/app.ts",
  kind: "edit",
  paths: ["src/app.ts", "src/app.ts", "src/test.ts"],
} satisfies HarnessEvent;
const completed = {
  ...started,
  type: "tool.updated",
  status: "completed",
} satisfies HarnessEvent;

beforeEach(() => {
  vi.mocked(invoke).mockReset().mockResolvedValue(undefined);
});

describe("worker checkpoint lifecycle", () => {
  it("waits for the fresh checkout baseline before preparation completes", async () => {
    let release!: () => void;
    vi.mocked(invoke).mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
    const ready = vi.fn();
    const preparation = prepareWorkerCheckpoint(
      { sessionId: "worker" },
      checkoutCwd,
    ).then(ready);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledOnce());
    expect(invoke).toHaveBeenCalledWith("worker_checkpoint_ensure", {
      sessionId: "worker",
      cwd: checkoutCwd,
    });
    expect(ready).not.toHaveBeenCalled();
    release();
    await preparation;
    expect(ready).toHaveBeenCalledOnce();
  });

  it("propagates initialization failures instead of allowing preparation to succeed", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(
      new Error("Cannot persist baseline"),
    );
    await expect(
      prepareWorkerCheckpoint({ sessionId: "worker" }, checkoutCwd),
    ).rejects.toThrow("Cannot persist baseline");
  });

  it("does not initialize a retained checkout after it may have been edited", async () => {
    await prepareWorkerCheckpoint(
      { sessionId: "worker", workspace },
      checkoutCwd,
    );
    expect(invoke).not.toHaveBeenCalled();
  });

  it("does not rebaseline a legacy shared worker without workspace metadata", async () => {
    await prepareWorkerCheckpoint(
      { sessionId: "worker", workspacePolicy: "shared" },
      "/repo",
    );
    expect(invoke).not.toHaveBeenCalled();
  });

  it("queues worker edit capture before review applies the checkpoint", async () => {
    await prepareWorkerCheckpoint({ sessionId: "worker" }, checkoutCwd);
    trackSessionEdits("worker", checkoutCwd, started, "lead");
    trackSessionEdits("worker", checkoutCwd, completed, "lead");
    await applySessionCheckpoint("worker", checkoutCwd, "/repo");
    expect(vi.mocked(invoke).mock.calls).toEqual([
      ["worker_checkpoint_ensure", { sessionId: "worker", cwd: checkoutCwd }],
      [
        "session_checkpoint_prepare",
        {
          sessionId: "worker",
          cwd: checkoutCwd,
          paths: ["src/app.ts", "src/test.ts"],
        },
      ],
      [
        "session_checkpoint_capture",
        {
          sessionId: "worker",
          cwd: checkoutCwd,
          paths: ["src/app.ts", "src/test.ts"],
        },
      ],
      [
        "session_checkpoint_apply",
        {
          sessionId: "worker",
          fromCwd: checkoutCwd,
          toCwd: "/repo",
        },
      ],
    ]);
  });

  it("continues recording edits for a resumed worker without replacing its baseline", async () => {
    await prepareWorkerCheckpoint(
      { sessionId: "worker", workspace },
      checkoutCwd,
    );
    trackSessionEdits("worker", checkoutCwd, started, "lead");
    trackSessionEdits("worker", checkoutCwd, completed, "lead");
    await flushSessionCheckpoint("worker");
    expect(vi.mocked(invoke).mock.calls.map(([command]) => command)).toEqual([
      "session_checkpoint_prepare",
      "session_checkpoint_capture",
    ]);
  });

  it("keeps orchestration lead edits out of the worker checkpoint path", async () => {
    trackSessionEdits("lead", "/repo", started, "lead");
    trackSessionEdits("lead", "/repo", completed, "lead");
    await flushSessionCheckpoint("lead");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("continues recording ordinary session edits", async () => {
    trackSessionEdits("ordinary", "/repo", started);
    trackSessionEdits("ordinary", "/repo", completed);
    await flushSessionCheckpoint("ordinary");
    expect(vi.mocked(invoke).mock.calls.map(([command]) => command)).toEqual([
      "session_checkpoint_prepare",
      "session_checkpoint_capture",
    ]);
  });

  it("removes only a checkout created by the failed preparation", async () => {
    const remove = vi.fn().mockResolvedValue(undefined);
    vi.mocked(invoke).mockRejectedValueOnce(new Error("disk full"));
    await expect(
      prepareWorkerCheckpoint({ sessionId: "fresh" }, checkoutCwd, remove),
    ).rejects.toThrow("disk full");
    expect(remove).toHaveBeenCalledOnce();
    remove.mockClear();
    await prepareWorkerCheckpoint(
      { sessionId: "retained", workspace },
      checkoutCwd,
      remove,
    );
    await prepareWorkerCheckpoint(
      { sessionId: "shared", workspacePolicy: "shared" },
      "/repo",
      remove,
    );
    expect(remove).not.toHaveBeenCalled();
  });

  it("preserves the failure and reports a checkout that could not be removed", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("disk full"));
    await expect(
      prepareWorkerCheckpoint({ sessionId: "fresh" }, checkoutCwd, async () => {
        throw new Error("busy");
      }),
    ).rejects.toThrow("disk full. Checkout cleanup failed: Error: busy");
  });

  it("captures shell changes only after the worker exits", async () => {
    let exit!: () => void;
    const stop = () =>
      new Promise<void>((resolve) => {
        exit = resolve;
      });
    const task = { sessionId: "shell-worker", files: ["src"] };
    trackSessionEdits(
      task.sessionId,
      checkoutCwd,
      {
        type: "tool.updated",
        callId: "shell-1",
        kind: "execute",
        title: "Run shell command",
        status: "completed",
      },
      "lead",
    );
    const finished = settleWorkerCheckpoint(task, checkoutCwd, stop);
    expect(invoke).not.toHaveBeenCalled();
    exit();
    await finished;
    expect(invoke).toHaveBeenCalledExactlyOnceWith(
      "worker_checkpoint_capture",
      {
        sessionId: task.sessionId,
        cwd: checkoutCwd,
        scopes: ["src"],
      },
    );
  });

  it("flushes a completion-only event before the final whole-worker capture", async () => {
    const task = { sessionId: "late-path", files: ["src"] };
    trackSessionEdits(task.sessionId, checkoutCwd, completed, "lead");
    await settleWorkerCheckpoint(task, checkoutCwd, async () => {});
    expect(vi.mocked(invoke).mock.calls.map(([command]) => command)).toEqual([
      "session_checkpoint_capture",
      "worker_checkpoint_capture",
    ]);
  });

  it("blocks integration and removal when process shutdown fails", async () => {
    const applyOrRemove = vi.fn();
    await expect(
      settleWorkerCheckpoint(
        { sessionId: "stuck", files: ["."] },
        checkoutCwd,
        async () => {
          throw new Error("exit not confirmed");
        },
      ).then(applyOrRemove),
    ).rejects.toThrow("exit not confirmed");
    expect(invoke).not.toHaveBeenCalled();
    expect(applyOrRemove).not.toHaveBeenCalled();
  });

  it("blocks integration when the final capture fails", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("outside assignment"));
    const applyOrRemove = vi.fn();
    await expect(
      settleWorkerCheckpoint(
        { sessionId: "blocked", files: ["src"] },
        checkoutCwd,
        async () => {},
      ).then(applyOrRemove),
    ).rejects.toThrow("outside assignment");
    expect(applyOrRemove).not.toHaveBeenCalled();
  });
});
