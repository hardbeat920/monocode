import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));

async function loadStore() {
  vi.resetModules();
  return import("./sessionStore");
}

// Roughly the reporter's workload: a 3,643-block Claude-shaped transcript.
// ~1KB per block is ~4 MB over the wire for a full save.
const BLOCK_TEXT = "x".repeat(1024);
const TRANSCRIPT_BLOCKS = 3643;

function transcript(size = TRANSCRIPT_BLOCKS) {
  return Array.from({ length: size }, (_, index) => ({
    id: `b${index}`,
    role: index % 2 ? ("assistant" as const) : ("user" as const),
    text: BLOCK_TEXT,
  }));
}

function record(id: string) {
  return {
    id,
    cwd: "/tmp/project",
    harness: "claude",
    model: "claude-sonnet",
    modelSettings: {},
    runtimeMode: "supervised",
    title: "Busy session",
    createdAt: 1,
    updatedAt: 2,
    blocks: transcript(),
  };
}

function summary(session: { id: string; title: string }) {
  return {
    id: session.id,
    cwd: "/tmp/project",
    harness: "claude",
    model: "claude-sonnet",
    runtimeMode: "supervised",
    title: session.title,
    createdAt: 1,
    updatedAt: 2,
  };
}

function invokeBytes(): number {
  return mocks.invoke.mock.calls
    .filter(([command]) => command.startsWith("session_upsert"))
    .reduce(
      (total, [, args]) => total + JSON.stringify(args).length,
      0,
    );
}

afterEach(() => {
  mocks.invoke.mockReset();
});

describe("regular-session persist payload size", () => {
  it("sends far less than a full transcript per busy-minute of saves", async () => {
    const { readSession, upsertSession } = await loadStore();
    mocks.invoke.mockResolvedValueOnce(record("s1"));
    const session = (await readSession("s1"))!;

    // 30 saves in a busy minute: each appends one new streamed block.
    const saves = 30;
    for (let index = 0; index < saves; index += 1) {
      session.blocks = [
        ...session.blocks,
        { id: `n${index}`, role: "assistant" as const, text: BLOCK_TEXT },
      ];
      mocks.invoke.mockResolvedValueOnce(summary(session));
      await upsertSession(session);
    }

    const actual = invokeBytes();
    // What the full-write code shipped for the same 30 saves: the complete
    // sanitized block array in every `session_upsert` call.
    const before = transcript(TRANSCRIPT_BLOCKS + saves);
    const equivalentFullWrites =
      saves *
      JSON.stringify({
        session: { ...summary(session), blocks: before },
      }).length;
    expect(actual).toBeLessThan(equivalentFullWrites / 10);
  });

  it("sends only headers when a save leaves the transcript unchanged", async () => {
    const { readSession, upsertSession } = await loadStore();
    mocks.invoke.mockResolvedValueOnce(record("s1"));
    const session = (await readSession("s1"))!;

    mocks.invoke.mockResolvedValueOnce(summary(session));
    await upsertSession({ ...session, title: "Renamed" });

    const actual = invokeBytes();
    const fullWrite = JSON.stringify({
      session: { ...summary(session), blocks: transcript() },
    }).length;
    expect(actual).toBeLessThan(fullWrite / 10);
  });
});
