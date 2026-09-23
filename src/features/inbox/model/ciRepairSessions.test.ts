import { expect, it } from "vitest";
import { summaryFromSession } from "../../sessions/data/sessionHistory";
import { appendPreparingHandoff } from "../../sessions/model/handoff";
import { newSession } from "../../sessions/model/session";
import { ciRepairSessions } from "./ciRepairSessions";

it("excludes removed worktrees from history and live chats", () => {
  const ready = { ...newSession("claude", "/web"), id: "ready" };
  const removed = {
    ...newSession("claude", "/web"),
    id: "removed-live",
    worktreeRemoved: true,
  };
  const history = [
    { ...removed, id: "removed-history" },
    { ...removed, worktreeRemoved: false },
    ready,
  ].map((session) => summaryFromSession(session));
  expect(
    ciRepairSessions(history, [removed]).map((session) => session.id),
  ).toEqual(["ready"]);
});

it("offers available chats without restoring unavailable live chats from history", () => {
  const closed = { ...newSession("claude", "/web"), id: "closed" };
  const ready = {
    ...newSession("claude", "/web"),
    id: "ready",
    title: "Current title",
  };
  const busy = { ...newSession("claude", "/web"), id: "busy", busy: true };
  const switching = {
    ...newSession("codex", "/web"),
    id: "switching",
    pendingSwitch: { from: "claude" as const, fromModel: "sonnet" },
  };
  const preparing = appendPreparingHandoff(
    { ...newSession("claude", "/web"), id: "preparing" },
    "claude",
    "codex",
  );
  const history = [
    closed,
    { ...ready, title: "Saved title" },
    busy,
    switching,
    preparing,
  ].map((session) => summaryFromSession(session));
  const choices = ciRepairSessions(history, [
    ready,
    busy,
    switching,
    preparing,
  ]);
  expect(choices.map((session) => session.id)).toEqual(["closed", "ready"]);
  expect(choices[1].title).toBe("Current title");
});
