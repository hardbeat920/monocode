// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

let checkpoint: typeof import("./checkpoint");
beforeEach(async () => {
  vi.resetModules();
  vi.mocked(invoke).mockReset().mockResolvedValue(undefined);
  checkpoint = await import("./checkpoint");
});

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

it("waits for the baseline before begin resolves and for cancellation before capture", async () => {
  const baseline = deferred();
  const stopped = deferred();
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "session_checkpoint_begin_turn") await baseline.promise;
    return undefined;
  });
  let ready = false;
  const begin = checkpoint.beginSessionTurn("s", "/repo").then((id) => {
    ready = true;
    return id;
  });
  await Promise.resolve();
  expect(ready).toBe(false);
  baseline.resolve();
  const turnId = await begin;
  const finish = checkpoint.finishSessionTurn("s", "/repo", {
    turnId,
    after: stopped.promise,
  });
  const status = checkpoint.sessionCheckpointStatus("s", "/repo");
  await Promise.resolve();
  expect(vi.mocked(invoke).mock.calls.map(([command]) => command)).toEqual([
    "session_checkpoint_begin_turn",
  ]);
  stopped.resolve();
  await Promise.all([finish, status]);
  expect(vi.mocked(invoke).mock.calls.map(([command]) => command)).toEqual([
    "session_checkpoint_begin_turn",
    "session_checkpoint_finish_turn",
    "session_checkpoint_status",
  ]);
  expect(invoke).toHaveBeenCalledWith("session_checkpoint_finish_turn", {
    sessionId: "s",
    cwd: "/repo",
    turnId,
  });
});

it("an old completion cannot clear the newer turn's active token", async () => {
  const first = await checkpoint.beginSessionTurn("s", "/repo");
  const second = await checkpoint.beginSessionTurn("s", "/repo");
  await checkpoint.finishSessionTurn("s", "/repo", { turnId: first });
  await checkpoint.finishSessionTurn("s", "/repo");
  expect(invoke).toHaveBeenLastCalledWith("session_checkpoint_finish_turn", {
    sessionId: "s",
    cwd: "/repo",
    turnId: second,
  });
  expect(second).not.toBe(first);
});

it("does not capture a workspace when no local turn was started", async () => {
  await checkpoint.finishSessionTurn("s", "/repo");
  expect(invoke).not.toHaveBeenCalled();
});

it("passes persisted edit evidence when resuming and reviewing a restored session", async () => {
  await checkpoint.beginSessionTurn("s", "/repo", ["README.md"]);
  expect(invoke).toHaveBeenCalledWith(
    "session_checkpoint_begin_turn",
    expect.objectContaining({
      sessionId: "s",
      cwd: "/repo",
      editedPaths: ["README.md"],
    }),
  );
  await checkpoint.sessionCheckpointStatus("s", "/repo", ["README.md"]);
  expect(invoke).toHaveBeenLastCalledWith("session_checkpoint_status", {
    sessionId: "s",
    cwd: "/repo",
    editedPaths: ["README.md"],
  });
});
