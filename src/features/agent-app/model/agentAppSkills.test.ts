// @vitest-environment happy-dom
import { expect, it, vi } from "vitest";
import { newSession } from "../../sessions/model/session";
import { MonoFileConflict } from "../../monos/model/monoFiles";
import { handleAgentApp, type AgentAppHost } from "./agentApp";

function fixture(kind: "mono" | "habit" | "session" = "mono") {
  const source = newSession("codex", "/Users/me", "codex:test");
  const skill = {
    name: "review-pr",
    description: "Review PRs",
    path: "/skills/review-pr/SKILL.md",
    hash: "h1",
    owned: true,
    available: true,
  };
  const skills = {
    read: vi.fn(async () => ({
      text: "Full instructions",
      hash: "h1",
      path: skill.path,
      owned: true,
    })),
    create: vi.fn(async () => {}),
    update: vi.fn(async () => {}),
  };
  const host = {
    isMono: () => kind === "mono",
    isHabitRun: () => kind === "habit",
    monoOf: () => ({ id: "mono-a", projects: [] }),
    agentFiles: vi.fn(async () => ({ skills: [skill] })),
    skills,
  } as unknown as AgentAppHost;
  return {
    skills,
    host,
    run: (action: string, input: Record<string, unknown> = {}) =>
      handleAgentApp(source, "req", action, input, host),
  };
}

it.each(["mono", "habit"] as const)(
  "lets a %s read only its own catalog through the host",
  async (kind) => {
    const { run, skills, host } = fixture(kind);
    expect(await run("skills.list")).toMatchObject({
      skills: [{ name: "review-pr" }],
    });
    expect(host.agentFiles).toHaveBeenCalledWith("mono-a");
    expect(await run("skills.read", { name: "review-pr" })).toMatchObject({
      text: "Full instructions",
      hash: "h1",
    });
    expect(skills.read).toHaveBeenCalledWith("mono-a", "review-pr");
    await expect(
      run("skills.read", { name: "review-pr", monoId: "mono-b" }),
    ).rejects.toThrow("Unknown skills.read fields");
  },
);

it("creates and edits skills in its own conversation", async () => {
  const { run, skills } = fixture();
  const input = {
    name: "review-pr",
    description: "Review PRs",
    instructions: "Check the diff",
  };
  expect(await run("skills.create", input)).toEqual({
    created: true,
    name: "review-pr",
  });
  expect(skills.create).toHaveBeenCalledWith("mono-a", input);
  await run("skills.update", {
    name: "review-pr",
    text: "Updated Markdown",
    expectedHash: "h1",
  });
  expect(skills.update).toHaveBeenCalledWith(
    "mono-a",
    "review-pr",
    "Updated Markdown",
    "h1",
  );
  skills.update.mockRejectedValueOnce(new MonoFileConflict());
  await expect(
    run("skills.update", {
      name: "review-pr",
      text: "Markdown",
      expectedHash: "h1",
    }),
  ).rejects.toThrow("Run skills.read");
});

it.each(["habit", "session"] as const)(
  "refuses skill mutations from a %s",
  async (kind) => {
    const { run, skills } = fixture(kind);
    await expect(
      run("skills.create", {
        name: "review-pr",
        description: "Review PRs",
        instructions: "Check",
      }),
    ).rejects.toThrow("Only a Mono");
    await expect(
      run("skills.update", {
        name: "review-pr",
        text: "Markdown",
        expectedHash: "h1",
      }),
    ).rejects.toThrow("Only a Mono");
    expect(skills.create).not.toHaveBeenCalled();
    expect(skills.update).not.toHaveBeenCalled();
  },
);

it("refuses ordinary sessions access even if a host supplies a Mono id", async () => {
  const { run, skills } = fixture("session");
  await expect(run("skills.list")).rejects.toThrow("Only a Mono");
  await expect(run("skills.read", { name: "review-pr" })).rejects.toThrow(
    "Only a Mono",
  );
  expect(skills.read).not.toHaveBeenCalled();
});
