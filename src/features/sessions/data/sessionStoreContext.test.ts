import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));

const { getSession } = await import("./sessionStore");
const { contextRatio } = await import("../model/contextUsage");

/** The stored row a session leaves behind, with only the fields under test. */
function record(overrides: Record<string, unknown> = {}) {
  return {
    id: "s1",
    projectId: "p1",
    cwd: "/repo",
    harness: "codex",
    title: "Session",
    createdAt: 1,
    updatedAt: 2,
    blocks: [],
    ...overrides,
  };
}

describe("restoring a stored context window", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  async function restore(row: Record<string, unknown>) {
    mocks.invoke.mockResolvedValueOnce(row);
    return getSession("s1");
  }

  it("restores a level and its window", async () => {
    const session = await restore(
      record({ contextUsed: 120_000, contextWindow: 272_000 }),
    );
    expect(session?.context).toEqual({ used: 120_000, window: 272_000 });
    expect(contextRatio(session?.context)).toBeCloseTo(0.4412, 3);
  });

  it("keeps a window stored without a level, marked stale", async () => {
    // `persistableMeta` drops the level at a compaction boundary and keeps the
    // window. Restoring nothing at all would take the meter — and its Compact
    // now action — away with the denominator.
    const session = await restore(record({ contextWindow: 272_000 }));
    expect(session?.context).toEqual({
      used: 0,
      window: 272_000,
      compacted: true,
    });
    // Nothing is claimed about the level, so no percentage is rendered.
    expect(contextRatio(session?.context)).toBeNull();
  });

  it("restores a level with no window as before", async () => {
    const session = await restore(record({ contextUsed: 30_000 }));
    expect(session?.context).toEqual({ used: 30_000 });
    expect(contextRatio(session?.context)).toBeNull();
  });

  it("has no context when neither half was stored", async () => {
    const session = await restore(record());
    expect(session?.context).toBeUndefined();
  });

  it("ignores a nonsense window rather than trusting it", async () => {
    for (const contextWindow of [0, -1, Number.NaN, "272000"]) {
      const session = await restore(record({ contextWindow }));
      expect(session?.context).toBeUndefined();
    }
  });
});
