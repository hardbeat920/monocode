import { invoke } from "@tauri-apps/api/core";
import type { DiscoveredSkill } from "../../../platform/tauri/fs";
import { slugSkillName, isValidSkillName } from "../../skills/model/skills";
import {
  loadMonoFiles,
  MonoFileConflict,
  notifyMonoFilesChanged,
  readAgentFile,
  writeAgentFile,
} from "./monoFiles";

export type MonoSkill = {
  name: string;
  description: string;
  path: string;
  hash: string;
  owned: boolean;
  available: boolean;
};

type Assignment = Pick<MonoSkill, "name" | "description" | "path">;

function parseAssignments(text: string | null): Assignment[] {
  if (text === null) return [];
  const value: unknown = JSON.parse(text);
  if (
    !Array.isArray(value) ||
    !value.every(
      (item) =>
        item &&
        typeof item === "object" &&
        typeof item.name === "string" &&
        typeof item.description === "string" &&
        typeof item.path === "string",
    )
  ) {
    throw new Error("Could not read this Mono's skill assignments");
  }
  return value as Assignment[];
}

async function editAssignments(
  monoId: string,
  edit: (items: Assignment[]) => Assignment[],
): Promise<void> {
  for (let attempt = 0; ; attempt++) {
    const current = await readAgentFile(monoId, "skills.json");
    const next = edit(parseAssignments(current.text));
    try {
      await writeAgentFile(
        monoId,
        "skills.json",
        `${JSON.stringify(next, null, 2)}\n`,
        current.hash,
      );
      return;
    } catch (error) {
      if (!(error instanceof MonoFileConflict) || attempt >= 2) throw error;
    }
  }
}

export async function assignMonoSkill(
  monoId: string,
  skill: DiscoveredSkill,
): Promise<void> {
  const files = await loadMonoFiles(monoId);
  const existing = files.skills?.find((item) => item.name === skill.name);
  if (existing && existing.path !== skill.path)
    throw new Error(`This Mono already has a skill named ${skill.name}`);
  await editAssignments(monoId, (items) => {
    if (
      items.some((item) => item.name === skill.name && item.path !== skill.path)
    )
      throw new Error(`This Mono already has a skill named ${skill.name}`);
    if (items.some((item) => item.path === skill.path)) return items;
    return [
      ...items,
      { name: skill.name, description: skill.description, path: skill.path },
    ];
  });
}

export async function removeMonoSkill(
  monoId: string,
  skill: MonoSkill,
): Promise<void> {
  if (!skill.owned) {
    await editAssignments(monoId, (items) =>
      items.filter((item) => item.path !== skill.path),
    );
    return;
  }
  try {
    await invoke("mono_skill_remove", {
      mono: monoId,
      name: skill.name,
      expectedHash: skill.hash,
    });
    notifyMonoFilesChanged();
  } catch (error) {
    if (error === "conflict") throw new MonoFileConflict();
    throw error;
  }
}

export async function readMonoSkill(
  monoId: string,
  name: string,
): Promise<{ text: string; hash: string; path: string; owned: boolean }> {
  const files = await loadMonoFiles(monoId);
  const skill = files.skills?.find((item) => item.name === name);
  if (!skill)
    throw new Error("No skill with that name is assigned to this Mono");
  return invoke<{ text: string; hash: string; path: string; owned: boolean }>(
    "mono_skill_read",
    {
      mono: monoId,
      name,
    },
  );
}

export function monoSkillMarkdown(
  name: string,
  description: string,
  instructions: string,
): string {
  // YAML single quotes preserve punctuation, quotes and backslashes verbatim.
  const quote = (text: string) =>
    `'${text.replace(/'/g, "''").replace(/\r?\n/g, " ")}'`;
  return `---\nname: ${name}\ndescription: ${quote(description.trim())}\n---\n\n${instructions.trim()}\n`;
}

export async function createMonoSkill(
  monoId: string,
  input: { name: string; description: string; instructions: string },
): Promise<void> {
  const name = slugSkillName(input.name);
  if (!isValidSkillName(name) || name.length > 64)
    throw new Error(
      "Use a skill name with lowercase letters, numbers and hyphens (up to 64 characters)",
    );
  if (
    !input.description.trim() ||
    new TextEncoder().encode(input.description.trim()).length > 1024
  )
    throw new Error(
      "Add a description under 1024 bytes explaining when to use this skill",
    );
  if (!input.instructions.trim())
    throw new Error("Add instructions for this skill");
  const files = await loadMonoFiles(monoId);
  if (files.skills?.some((skill) => skill.name === name))
    throw new Error(`This Mono already has a skill named ${name}`);
  const path = `skills/${name}/SKILL.md` as const;
  const current = await readAgentFile(monoId, path);
  if (current.text !== null)
    throw new Error(`This Mono already has a skill named ${name}`);
  await writeAgentFile(
    monoId,
    path,
    monoSkillMarkdown(name, input.description, input.instructions),
    current.hash,
  );
}

export async function updateMonoSkill(
  monoId: string,
  name: string,
  text: string,
  expectedHash: string,
): Promise<void> {
  const files = await loadMonoFiles(monoId);
  if (!files.skills?.some((skill) => skill.name === name && skill.owned))
    throw new Error("Only a skill created for this Mono can be edited here");
  await writeAgentFile(monoId, `skills/${name}/SKILL.md`, text, expectedHash);
}
