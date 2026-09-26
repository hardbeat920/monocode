// @vitest-environment happy-dom
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, expect, it, vi } from "vitest";
import {
  applySessionCheckpoint,
  ensureSessionCheckpoint,
  flushSessionCheckpoint,
  trackSessionEdits,
} from "./checkpoint";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockResolvedValue(undefined);
});

it("records a worker's checkpoint before and after each edit", async () => {
  const cwd = "/repo-worktrees/mc-orch-1";
  await ensureSessionCheckpoint("worker", cwd);
  trackSessionEdits("worker", cwd, {
    type: "tool.started",
    callId: "c1",
    title: "Edit",
    kind: "edit",
    paths: ["tracked.ts", "new.ts"],
  });
  trackSessionEdits("worker", cwd, {
    type: "tool.updated",
    callId: "c1",
    kind: "edit",
    status: "completed",
    paths: ["tracked.ts", "new.ts"],
  });
  await flushSessionCheckpoint("worker");

  expect(vi.mocked(invoke).mock.calls).toEqual([
    ["session_checkpoint_ensure", { sessionId: "worker", cwd }],
    [
      "session_checkpoint_prepare",
      { sessionId: "worker", cwd, paths: ["tracked.ts", "new.ts"] },
    ],
    [
      "session_checkpoint_capture",
      { sessionId: "worker", cwd, paths: ["tracked.ts", "new.ts"] },
    ],
  ]);
});

it("ignores tools that do not edit files", async () => {
  trackSessionEdits("worker", "/repo", {
    type: "tool.started",
    callId: "c2",
    title: "Read",
    kind: "read",
    paths: ["a.ts"],
  });
  await flushSessionCheckpoint("worker");
  expect(invoke).not.toHaveBeenCalled();
});

it("marks an isolated worker's checkpoint and passes its write scopes to apply", async () => {
  const cwd = "/repo-worktrees/mc-orch-2";
  await ensureSessionCheckpoint("worker", cwd, true);
  await applySessionCheckpoint("worker", cwd, "/repo", [`${cwd}/src`]);
  await ensureSessionCheckpoint("shared", "/repo");

  expect(vi.mocked(invoke).mock.calls).toEqual([
    ["session_checkpoint_ensure", { sessionId: "worker", cwd, isolated: true }],
    [
      "session_checkpoint_apply",
      {
        sessionId: "worker",
        fromCwd: cwd,
        toCwd: "/repo",
        writeScopes: [`${cwd}/src`],
      },
    ],
    ["session_checkpoint_ensure", { sessionId: "shared", cwd: "/repo" }],
  ]);
});
