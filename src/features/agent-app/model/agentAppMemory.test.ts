// @vitest-environment happy-dom
import { expect, it, vi } from "vitest";
import { newSession } from "../../sessions/model/session";
import {
  MonoFileConflict,
  type AgentFilePath,
} from "../../monos/model/monoFiles";
import {
  setHabitRunning,
  type Habit,
} from "../../monos/model/monoHabits";
import { handleAgentApp, type AgentAppHost } from "./agentApp";

/** The agent's folder as a map, with a hash that changes on every write. */
function fixture(agent = true) {
  const source = newSession("codex", "/tmp/project", "codex:test");
  source.id = "agent";
  const files = new Map<string, string>([["MEMORY.md", ""]]);
  const versions = new Map<string, number>();
  const hash = (path: string) => `${path}@${versions.get(path) ?? 0}`;
  const write = vi.fn(
    async (_monoId: string, path: AgentFilePath, text: string, expected: string) => {
      if (expected !== hash(path)) throw new MonoFileConflict();
      files.set(path, text);
      versions.set(path, (versions.get(path) ?? 0) + 1);
      return hash(path);
    },
  );
  const host = {
    isMono: (id: string) => agent && id === "agent",
    monoOf: (id: string) =>
      agent && id === "agent" ? { id: "mono-1", projects: [] } : undefined,
    readAgentFile: async (_monoId: string, path: AgentFilePath) => ({
      text: files.get(path) ?? null,
      hash: hash(path),
    }),
    writeAgentFile: write,
    agentFiles: async () => ({
      id: "a",
      dir: "/data/agents/a",
      soul: "",
      soulHash: "",
      memory: files.get("MEMORY.md") ?? "",
      memoryHash: hash("MEMORY.md"),
      memoryPath: "/data/agents/a/MEMORY.md",
      topics: [...files.keys()]
        .filter((path) => path.startsWith("memory/") && path !== "memory/archive.md")
        .map((path) => path.slice(7, -3)),
    }),
    now: () => new Date(2026, 9, 4, 12),
  } as unknown as AgentAppHost;
  const run = (action: string, input: Record<string, unknown> = {}) =>
    handleAgentApp(source, `req-${Math.random()}`, action, input, host);
  return { files, write, versions, run };
}

it("adds, supersedes and removes dated entries in MEMORY.md", async () => {
  const { files, run } = fixture();
  await run("memory.add", { fact: "CI runs on Travis" });
  expect(await run("memory.add", { fact: "CI runs on Travis" })).toMatchObject({
    alreadyRemembered: true,
  });
  await run("memory.replace", { find: "Travis", fact: "CI runs on Actions" });
  await run("memory.add", { fact: "wrong fact" });
  await run("memory.remove", { find: "wrong fact" });
  expect(files.get("MEMORY.md")).toBe(
    "- ~~2026-10-04 · CI runs on Travis~~ · superseded 2026-10-04\n- 2026-10-04 · CI runs on Actions\n",
  );
  expect(await run("memory.read")).toMatchObject({ lines: 2, notLoaded: 0 });
});

it("keeps topic notes in their own file and lists them", async () => {
  const { files, run } = fixture();
  await run("memory.add", { fact: "v2.1 shipped", topic: "Releases" });
  expect(files.get("memory/Releases.md")).toContain("- 2026-10-04 · v2.1 shipped");
  expect(await run("memory.read")).toMatchObject({ topics: ["Releases"] });
  expect(await run("memory.read", { topic: "Releases" })).toMatchObject({
    text: expect.stringContaining("v2.1 shipped"),
  });
  await expect(run("memory.add", { fact: "x", topic: "../SOUL" })).rejects.toThrow(
    "topic",
  );
});

it("retries over an edit the user saved in between", async () => {
  const { files, versions, write, run } = fixture();
  const original = write.getMockImplementation()!;
  write.mockImplementationOnce(async (...args) => {
    // The user saves from the details panel just before the agent's write.
    files.set("MEMORY.md", "User note\n");
    versions.set("MEMORY.md", 9);
    return original(...args);
  });
  await run("memory.add", { fact: "uses pnpm" });
  expect(files.get("MEMORY.md")).toBe("User note\n- 2026-10-04 · uses pnpm\n");
});

it("redacts secrets and refuses sessions that are not the Mono", async () => {
  const { files, run } = fixture();
  await run("memory.add", { fact: "deploy token=abc123secretvalue" });
  expect(files.get("MEMORY.md")).not.toContain("abc123secretvalue");
  await expect(fixture(false).run("memory.read")).rejects.toThrow(
    "Mono",
  );
});

it("searches MEMORY.md, topics and the archive", async () => {
  const { files, run } = fixture();
  await run("memory.add", { fact: "deploys go through Fly" });
  await run("memory.add", { fact: "Fly region is ams", topic: "infra" });
  files.set("memory/archive.md", "- 2026-01-01 · deploys went through Heroku · moved 2026-09-01\n");
  const result = (await run("memory.search", { query: "deploys" })) as {
    hits: { file: string }[];
  };
  expect(result.hits.map((hit) => hit.file)).toEqual(["MEMORY.md", "memory/archive.md"]);
  expect(await run("memory.search", { query: "fly" })).toMatchObject({
    hits: [{ file: "MEMORY.md" }, { file: "memory/infra.md" }],
  });
});

function habitsFixture(mono = true) {
  const source = newSession("codex", "/tmp/project", "codex:test");
  source.id = "agent";
  let stored: Habit[] = [];
  const host = {
    isMono: (id: string) => mono && id === "agent",
    monoOf: (id: string) =>
      mono && id === "agent" ? { id: "mono-1", projects: [] } : undefined,
    habits: {
      load: async () => stored,
      update: async <T,>(
        _monoId: string,
        change: (list: Habit[]) => { habits: Habit[]; result: T },
      ) => {
        const next = change(stored);
        stored = next.habits;
        return next.result;
      },
    },
    now: () => new Date(2026, 9, 5, 8),
  } as unknown as AgentAppHost;
  const run = (action: string, input: Record<string, unknown> = {}) =>
    handleAgentApp(source, `req-${Math.random()}`, action, input, host);
  return { run, habits: () => stored };
}

it("lets the Mono add, pause, try and remove its habits", async () => {
  const { run, habits } = habitsFixture();
  const added = (await run("habits.add", {
    name: "Morning CI check",
    instructions: "Tell me if CI on main is red.",
    schedule: { kind: "weekdays", time: "09:00" },
  })) as { habit: { id: string; schedule: string } };
  expect(added.habit.schedule).toBe("Weekdays at 9:00 AM");
  expect(habits()[0].nextRunAt).toBe(new Date(2026, 9, 5, 9).getTime());
  const id = added.habit.id;
  await run("habits.update", { id, enabled: false });
  expect(habits()[0].enabled).toBe(false);
  await run("habits.run", { id });
  // Runs once now, paused or not, and leaves the schedule alone.
  expect(habits()[0]).toMatchObject({
    runRequested: true,
    enabled: false,
    nextRunAt: new Date(2026, 9, 5, 9).getTime(),
  });
  expect(await run("habits.list")).toMatchObject({
    habits: [{ id, name: "Morning CI check", enabled: false }],
  });
  await run("habits.remove", { id });
  expect(habits()).toEqual([]);
  await expect(run("habits.remove", { id })).rejects.toThrow("No habit");
});

it("keeps habits to the Mono's own conversation", async () => {
  await expect(habitsFixture(false).run("habits.list")).rejects.toThrow(
    "Only a Mono",
  );
});

it("refuses to start a habit that is already running", async () => {
  const { run } = habitsFixture();
  const added = (await run("habits.add", {
    name: "PR check",
    instructions: "Look at open PRs.",
    schedule: { kind: "daily", time: "08:30" },
  })) as { habit: { id: string } };
  setHabitRunning(added.habit.id, Date.now());
  await expect(run("habits.run", { id: added.habit.id })).rejects.toThrow(
    "already running",
  );
  expect(await run("habits.list")).toMatchObject({
    habits: [{ id: added.habit.id, running: true }],
  });
  setHabitRunning(added.habit.id);
});

function projectFixture(projects: string[], cwd = "/Users/me") {
  const source = newSession("codex", cwd, "codex:test");
  source.id = "agent";
  const sessions = vi.fn(async (_cwd: string) => []);
  const host = {
    isMono: (id: string) => id === "agent",
    monoOf: (id: string) =>
      id === "agent" ? { id: "mono-1", projects } : undefined,
    sessions,
  } as unknown as AgentAppHost;
  const run = (action: string, input: Record<string, unknown> = {}) =>
    handleAgentApp(source, `req-${Math.random()}`, action, input, host);
  return { run, sessions };
}

it("lets a Mono choose which of its projects an action works in", async () => {
  const { run, sessions } = projectFixture(["/code/app", "/code/site"]);
  await expect(run("sessions.list")).rejects.toThrow(
    'Pass "project" to choose one. Yours: app (/code/app), site (/code/site)',
  );
  expect(await run("sessions.list", { project: "site" })).toMatchObject({
    cwd: "/code/site",
  });
  expect(await run("sessions.list", { project: "/code/app/" })).toMatchObject({
    cwd: "/code/app",
  });
  await expect(run("sessions.list", { project: "/code/other" })).rejects.toThrow(
    "Not one of your projects",
  );
  expect(sessions.mock.calls.map(([cwd]) => cwd)).toEqual([
    "/code/site",
    "/code/app",
  ]);
});

it("uses a Mono's only project, or the one its chat started in", async () => {
  expect(await projectFixture(["/code/app"]).run("sessions.list")).toMatchObject({
    cwd: "/code/app",
  });
  expect(
    await projectFixture(["/code/app", "/code/site"], "/code/site").run(
      "sessions.list",
    ),
  ).toMatchObject({ cwd: "/code/site" });
  await expect(projectFixture([]).run("sessions.list")).rejects.toThrow(
    "none yet",
  );
});
