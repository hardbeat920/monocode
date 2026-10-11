// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from "vitest";
import type { MonoSkill } from "./monoSkills";

const mocks = vi.hoisted(() => ({
  load: vi.fn(),
  read: vi.fn(),
  write: vi.fn(),
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("./monoFiles", async (original) => ({
  ...(await original<object>()),
  loadMonoFiles: mocks.load,
  readAgentFile: mocks.read,
  writeAgentFile: mocks.write,
}));
import {
  assignMonoSkill,
  createMonoSkill,
  monoSkillMarkdown,
  removeMonoSkill,
  readMonoSkill,
  updateMonoSkill,
} from "./monoSkills";
import { MonoFileConflict } from "./monoFiles";

const shared = {
  name: "review-pr",
  description: "Review PRs",
  path: "/shared/review-pr/SKILL.md",
  scope: "user",
  source: "agents",
};
const skill: MonoSkill = {
  ...shared,
  owned: false,
  available: true,
  hash: "body",
};

beforeEach(() => {
  vi.clearAllMocks();
  mocks.load.mockResolvedValue({ skills: [] });
  mocks.read.mockResolvedValue({ text: null, hash: "empty" });
  mocks.write.mockResolvedValue("saved");
  mocks.invoke.mockResolvedValue(undefined);
});

it("assigns a reference in the chosen Mono without copying or changing the source", async () => {
  await assignMonoSkill("mono-a", shared);
  expect(mocks.write).toHaveBeenCalledWith(
    "mono-a",
    "skills.json",
    expect.any(String),
    "empty",
  );
  expect(JSON.parse(mocks.write.mock.calls[0][2])).toEqual([
    { name: shared.name, description: shared.description, path: shared.path },
  ]);
  expect(mocks.invoke).not.toHaveBeenCalled();
});

it("preserves another assignment when retrying a concurrent config edit", async () => {
  mocks.write.mockRejectedValueOnce(new MonoFileConflict());
  mocks.read
    .mockResolvedValueOnce({ text: null, hash: "empty" })
    .mockResolvedValueOnce({
      text: JSON.stringify([
        { ...shared, name: "other", path: "/shared/other/SKILL.md" },
      ]),
      hash: "new",
    });
  await assignMonoSkill("mono-a", shared);
  expect(
    JSON.parse(mocks.write.mock.calls[1][2]).map(
      (item: { name: string }) => item.name,
    ),
  ).toEqual(["other", "review-pr"]);
  expect(mocks.write.mock.calls[1][3]).toBe("new");
});

it("returns the source path from the same read as the body when an assignment changes", async () => {
  mocks.load.mockResolvedValue({ skills: [skill] });
  const current = {
    text: "New source",
    hash: "new",
    path: "/new/review-pr/SKILL.md",
    owned: false,
  };
  mocks.invoke.mockResolvedValue(current);
  expect(await readMonoSkill("mono-a", "review-pr")).toEqual(current);
});

it("unassigns a shared skill without deleting its original files", async () => {
  mocks.read.mockResolvedValue({
    text: JSON.stringify([shared]),
    hash: "config",
  });
  await removeMonoSkill("mono-a", skill);
  expect(mocks.write).toHaveBeenCalledWith(
    "mono-a",
    "skills.json",
    "[]\n",
    "config",
  );
  expect(mocks.invoke).not.toHaveBeenCalled();
});

it("creates SKILL.md only in the selected Mono, with a create-only hash", async () => {
  await createMonoSkill("mono-a", {
    name: "Review PR",
    description: "Review the team's PRs",
    instructions: "Check the diff.",
  });
  expect(mocks.write).toHaveBeenCalledWith(
    "mono-a",
    "skills/review-pr/SKILL.md",
    expect.stringContaining("Check the diff."),
    "empty",
  );
  expect(
    monoSkillMarkdown("review-pr", "Review the team's PRs", "Check."),
  ).toContain("description: 'Review the team''s PRs'");
});

it("refuses duplicate names and edits to shared skills", async () => {
  mocks.load.mockResolvedValue({ skills: [skill] });
  await expect(
    createMonoSkill("mono-a", {
      name: "review-pr",
      description: "Review PRs",
      instructions: "Check",
    }),
  ).rejects.toThrow("already has");
  await expect(
    assignMonoSkill("mono-a", { ...shared, path: "/other/SKILL.md" }),
  ).rejects.toThrow("already has");
  await expect(
    updateMonoSkill("mono-a", "review-pr", "New", "body"),
  ).rejects.toThrow("Only a skill created");
  expect(mocks.write).not.toHaveBeenCalled();
});

it("keeps a stale delete from overwriting another editor's work", async () => {
  mocks.invoke.mockRejectedValueOnce("conflict");
  await expect(
    removeMonoSkill("mono-a", { ...skill, owned: true }),
  ).rejects.toBeInstanceOf(MonoFileConflict);
  expect(mocks.invoke).toHaveBeenCalledWith("mono_skill_remove", {
    mono: "mono-a",
    name: "review-pr",
    expectedHash: "body",
  });
});
