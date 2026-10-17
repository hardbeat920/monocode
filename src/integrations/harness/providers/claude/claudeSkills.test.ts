import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type LineHandler = (line: string) => void;
type ExitHandler = (code?: number | null) => void;

const handlers = new Map<string, { onLine: LineHandler; onExit: ExitHandler }>();
const spawned: string[] = [];
const sent = new Map<string, string[]>();

vi.mock("../../core/child", () => ({
  resolveClaudeBinary: async () => ({ path: "/fake/claude" }),
  spawnChild: async (id: string) => {
    spawned.push(id);
  },
  killChild: async (id: string) => {
    handlers.delete(id);
  },
  unwatchChild: (id: string) => {
    handlers.delete(id);
  },
  watchChild: (id: string, onLine: LineHandler, onExit: ExitHandler) => {
    handlers.set(id, { onLine, onExit });
  },
  writeChild: async (id: string, line: string) => {
    const lines = sent.get(id) ?? [];
    lines.push(line);
    sent.set(id, lines);
  },
}));

vi.mock("../../../../platform/tauri/fs", () => ({
  homeDir: async () => "/home/user",
}));

const { discoverClaudeCommands } = await import("./claudeSkills");

function emit(id: string, rec: Record<string, unknown>) {
  handlers.get(id)?.onLine(JSON.stringify(rec));
}

function requestIdOf(id: string): string {
  const lines = sent.get(id) ?? [];
  const record = JSON.parse(lines[lines.length - 1]!) as {
    request_id: string;
  };
  return record.request_id;
}

function commandsResponse(requestId: string, names: string[]) {
  return {
    type: "control_response",
    response: {
      subtype: "success",
      request_id: requestId,
      response: {
        commands: names.map((name) => ({ name, description: name })),
      },
    },
  };
}

beforeEach(() => {
  handlers.clear();
  spawned.length = 0;
  sent.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("discoverClaudeCommands", () => {
  it("filters out known terminal-only commands, since a cold probe never sees system/init", async () => {
    const call = discoverClaudeCommands("/repo");
    await vi.waitFor(() => expect(spawned).toHaveLength(1));
    const id = spawned[0]!;
    emit(
      id,
      commandsResponse(requestIdOf(id), ["compact", "doctor", "color", "claude-api"]),
    );

    const commands = await call;
    expect(commands.map((c) => c.name)).toEqual(["compact", "claude-api"]);
  });

  it("gives each concurrent probe its own child id, so they cannot cross-deliver", async () => {
    const first = discoverClaudeCommands("/repo-a");
    const second = discoverClaudeCommands("/repo-b");
    await vi.waitFor(() => expect(spawned).toHaveLength(2));
    const [idA, idB] = spawned;
    expect(idA).not.toBe(idB);

    // Answer them out of order: second probe's child resolves first.
    emit(idB!, commandsResponse(requestIdOf(idB!), ["b-command"]));
    emit(idA!, commandsResponse(requestIdOf(idA!), ["a-command"]));

    const [firstResult, secondResult] = await Promise.all([first, second]);
    expect(firstResult.map((c) => c.name)).toEqual(["a-command"]);
    expect(secondResult.map((c) => c.name)).toEqual(["b-command"]);
  });
});
