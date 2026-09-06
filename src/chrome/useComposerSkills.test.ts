vi.mock("../lib/harness/registry", () => ({
  getHarness: (id: string) =>
    id === "pi" || id === "omp"
      ? {
          commands: {
            discover: async () => [],
            rawSlashCommands: id === "omp",
          },
        }
      : undefined,
}));

import { describe, expect, it, vi } from "vitest";
import type { Skill } from "../lib/skills";
import {
  nextComposerSkillContextToken,
  pickerSkillLoadOptions,
  visibleComposerSkills,
} from "./useComposerSkills";

const piSkill: Skill = {
  kind: "native",
  name: "architect",
  description: "Design first.",
  invocation: "skill:architect",
  source: "pi",
};

const cachedSkill: Skill = {
  kind: "native",
  name: "cached",
  description: "Cached current context.",
  invocation: "skill:cached",
  source: "pi",
};

describe("composer skill catalog policies", () => {
  it("uses loaded rows only for their owning context", () => {
    expect(
      visibleComposerSkills(
        { key: "pi\0/a", skills: [piSkill] },
        "pi\0/a",
        null,
        [],
      ),
    ).toEqual([piSkill]);
    expect(
      visibleComposerSkills(
        { key: "pi\0/a", skills: [piSkill] },
        "pi\0/b",
        [cachedSkill],
        [],
      ),
    ).toEqual([cachedSkill]);
  });

  it("preserves filesystem refresh while Pi uses its TTL", () => {
    expect(pickerSkillLoadOptions("pi")).toBeUndefined();
    expect(pickerSkillLoadOptions("omp")).toBeUndefined();
    expect(pickerSkillLoadOptions("claude")).toEqual({ refresh: true });
  });

  it("does not reuse a context token after A to B to A", () => {
    const firstA = nextComposerSkillContextToken(null, "pi\0/a");
    const sameA = nextComposerSkillContextToken(firstA, "pi\0/a");
    const b = nextComposerSkillContextToken(sameA, "pi\0/b");
    const secondA = nextComposerSkillContextToken(b, "pi\0/a");

    expect(sameA).toBe(firstA);
    expect(secondA.key).toBe(firstA.key);
    expect(secondA).not.toBe(firstA);
    expect(secondA.generation).toBeGreaterThan(firstA.generation);
  });
});
