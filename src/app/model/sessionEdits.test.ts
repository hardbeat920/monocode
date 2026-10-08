import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  flushSessionCheckpoint,
  notifyReviewChanged,
} from "../../features/sessions/model/checkpoint";
import { mapCodexNotification } from "../../integrations/harness/providers/codex/codexProtocol";
import type { HarnessEvent } from "../../integrations/harness/core/types";
import { createSessionEditTracker } from "./sessionEdits";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../features/sessions/model/checkpoint", async (original) => ({
  ...(await original<
    typeof import("../../features/sessions/model/checkpoint")
  >()),
  notifyReviewChanged: vi.fn(),
}));

const path = "src/app.ts";
const diff = "@@ -1 +1 @@\n-old\n+new\n";

function edit(
  completed: boolean,
  callId: string,
  checkpointDiffs?: Record<string, string>,
  paths = [path],
): HarnessEvent {
  return {
    type: completed ? "tool.updated" : "tool.started",
    callId,
    title: "Edit",
    kind: "edit",
    status: completed ? "completed" : "in_progress",
    paths,
    checkpointDiffs,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(invoke).mockResolvedValue(undefined);
});

it.each([undefined, "", "not a complete modification diff"])(
  "captures the start-time diff when completion has no qualifying diff (%s)",
  async (completedDiff) => {
    const track = createSessionEditTracker("session", "/repo");
    for (const completed of [false, true]) {
      const { events } = mapCodexNotification(
        completed ? "item/completed" : "item/started",
        {
          item: {
            id: "edit",
            type: "fileChange",
            status: completed ? "completed" : "inProgress",
            changes: [
              { path, kind: "update", diff: completed ? completedDiff : diff },
            ],
          },
        },
      );
      expect(events).toHaveLength(1);
      track(events[0]);
    }
    await flushSessionCheckpoint("session");

    // No racing prepare snapshot: capture must receive the saved pre-edit diff.
    expect(invoke).toHaveBeenCalledExactlyOnceWith(
      "session_checkpoint_capture",
      {
        sessionId: "session",
        cwd: "/repo",
        paths: [path],
        diffs: { [path]: diff },
      },
    );
    await vi.waitFor(() => {
      expect(notifyReviewChanged).toHaveBeenCalledWith("session");
    });
  },
);

it("falls back per path while preferring completion diffs and ignoring progress", async () => {
  const track = createSessionEditTracker("session", "/repo");
  const other = "src/other.ts";
  const completedDiff = "@@ -1 +1 @@\n-old\n+final\n";
  track(edit(false, "edit", { [path]: diff, [other]: diff }, [path, other]));
  track({
    ...edit(true, "edit", { [other]: completedDiff }, [path, other]),
    status: "in_progress",
  });
  track({
    ...edit(true, "edit", { [path]: completedDiff }, [path, other]),
    status: "success",
  });
  await flushSessionCheckpoint("session");

  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_checkpoint_capture", {
    sessionId: "session",
    cwd: "/repo",
    paths: [path, other],
    diffs: { [path]: completedDiff, [other]: diff },
  });
});

it("keeps overlapping call IDs separate and consumes each fallback on completion", async () => {
  const track = createSessionEditTracker("session", "/repo");
  const secondDiff = "@@ -1 +1 @@\n-new\n+second\n";
  track(edit(false, "first", { [path]: diff }));
  track(edit(false, "second", { [path]: secondDiff }));
  track(edit(true, "second"));
  track(edit(true, "first"));
  track(edit(true, "first"));
  await flushSessionCheckpoint("session");

  const capture = { sessionId: "session", cwd: "/repo", paths: [path] };
  expect(vi.mocked(invoke).mock.calls).toEqual([
    [
      "session_checkpoint_capture",
      { ...capture, diffs: { [path]: secondDiff } },
    ],
    ["session_checkpoint_capture", { ...capture, diffs: { [path]: diff } }],
    ["session_checkpoint_capture", capture],
  ]);
});

it("does not reuse unfinished diffs in another turn or session", async () => {
  const first = createSessionEditTracker("session", "/repo");
  first(edit(false, "edit", { [path]: diff }));
  const nextTurn = createSessionEditTracker("session", "/repo");
  const otherSession = createSessionEditTracker("other", "/repo");
  nextTurn(edit(true, "edit"));
  otherSession(edit(true, "edit"));
  await Promise.all([
    flushSessionCheckpoint("session"),
    flushSessionCheckpoint("other"),
  ]);

  expect(invoke).toHaveBeenCalledTimes(2);
  for (const sessionId of ["session", "other"]) {
    expect(invoke).toHaveBeenCalledWith("session_checkpoint_capture", {
      sessionId,
      cwd: "/repo",
      paths: [path],
    });
  }
});

it("keeps the snapshot path for edits without provider diffs", async () => {
  const track = createSessionEditTracker("session", "/repo");
  track(edit(false, "edit"));
  track(edit(true, "edit"));
  await flushSessionCheckpoint("session");

  const args = { sessionId: "session", cwd: "/repo", paths: [path] };
  expect(vi.mocked(invoke).mock.calls).toEqual([
    ["session_checkpoint_prepare", args],
    ["session_checkpoint_capture", args],
  ]);
});
