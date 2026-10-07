import { expect, it } from "vitest";
import {
  newSession,
  type Block,
  type MonoSpawnedSession,
} from "../../sessions/model/session";
import {
  monoSpawnedSessions,
  recordMonoSpawnedSession,
  sanitizeMonoSpawnedSessions,
} from "./monoSpawnedSessions";

const launch: MonoSpawnedSession = {
  sessionId: "app-mono-review",
  cwd: "/repo",
  title: "Review latest PR",
  harness: "codex",
  model: "gpt-6",
};

it("attaches accepted launches to their originating turn without mutating or duplicating them", () => {
  const mono = newSession("codex", "/repo");
  mono.blocks = [
    { id: "first", role: "user", text: "Review" },
    { id: "reply", role: "assistant", text: "Started" },
    { id: "second", role: "user", text: "Another task" },
  ];
  const updated = recordMonoSpawnedSession(mono, "first", launch);
  expect(mono.blocks[0].monoSpawnedSessions).toBeUndefined();
  expect(updated.blocks[0].monoSpawnedSessions).toEqual([launch]);
  expect(updated.blocks[2]).toBe(mono.blocks[2]);
  expect(monoSpawnedSessions(updated.blocks.slice(0, 2))).toEqual([launch]);
  expect(monoSpawnedSessions(updated.blocks.slice(2))).toEqual([]);
  expect(recordMonoSpawnedSession(updated, "first", launch)).toBe(updated);
  expect(recordMonoSpawnedSession(mono, "missing", launch)).toBe(mono);
  expect(recordMonoSpawnedSession(mono, "reply", launch)).toBe(mono);
});

function receipt(command = "monocode app sessions.start", ok = true): Block {
  return {
    id: "call",
    role: "tool",
    text: command,
    tool: {
      status: "completed",
      detail: JSON.stringify({
        ok,
        result: { ...launch, id: launch.sessionId },
      }),
    },
  };
}

it("recovers confirmed launches from older chats, including shell wrappers, and deduplicates metadata", () => {
  expect(monoSpawnedSessions([receipt()])).toEqual([launch]);
  expect(
    monoSpawnedSessions([
      receipt("/bin/zsh -lc 'monocode app sessions.start'"),
    ]),
  ).toEqual([launch]);
  expect(
    monoSpawnedSessions([
      {
        id: "user",
        role: "user",
        text: "Review",
        monoSpawnedSessions: [launch],
      },
      receipt(),
      receipt(),
    ]),
  ).toEqual([launch]);
});

it("does not turn failed, pending, unrelated or truncated output into session links", () => {
  const call = receipt();
  expect(
    monoSpawnedSessions([
      receipt(undefined, false),
      receipt("monocode app sessions.read"),
      receipt("echo monocode app sessions.start"),
      { ...call, role: "assistant" },
      { ...call, tool: { ...call.tool, status: "in_progress" } },
      { ...call, tool: { ...call.tool, detail: '{"ok":true,"result":' } },
    ]),
  ).toEqual([]);
});

it("sanitizes saved references and strips extra fields", () => {
  expect(
    sanitizeMonoSpawnedSessions([
      { ...launch, title: "  Review  ", extra: "discard" },
      launch,
      { ...launch, sessionId: "../bad" },
      { ...launch, sessionId: "other", harness: "unknown" },
      { ...launch, sessionId: "empty", cwd: "" },
      null,
    ]),
  ).toEqual([{ ...launch, title: "Review" }]);
  expect(sanitizeMonoSpawnedSessions({})).toEqual([]);
});
