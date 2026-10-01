import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, expect, it } from "vitest";
import { listHostSkills, parseFrontmatter } from "./skills";

// Ports the main cases of src-tauri/src/skills.rs, which a local project uses.

const dirs: string[] = [];
afterEach(() => {
  for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
});

function tmp(label: string) {
  const dir = realpathSync(mkdtempSync(join(tmpdir(), `monocode-skills-${label}-`)));
  dirs.push(dir);
  return dir;
}

function writeSkill(root: string, folder: string, body: string) {
  mkdirSync(join(root, folder), { recursive: true });
  writeFileSync(join(root, folder, "SKILL.md"), body);
}

const skill = (name: string, description: string) =>
  `---\nname: ${name}\ndescription: ${description}\n---\n`;

// No machine-wide managed settings in tests.
const list = (project: string, home: string, disabled?: string[]) =>
  listHostSkills(project, home, disabled, undefined);

it("reads a folded description and falls back to the folder name", () => {
  expect(
    parseFrontmatter(
      "---\nname: review-pr\ndescription: >\n  Review pull requests.\n  Use when asked to review.\n---\n\n# hi\n",
      "fallback",
    ),
  ).toEqual({
    name: "review-pr",
    description: "Review pull requests. Use when asked to review.",
  });
  expect(parseFrontmatter("# no yaml\n", "create-skill")).toEqual({
    name: "create-skill",
    description: "",
  });
});

it("prefers .agents skills over provider folders and keeps both scopes", () => {
  const project = tmp("project");
  const home = tmp("home");
  writeSkill(join(project, ".agents/skills"), "ship", skill("ship", "MonoCode ship"));
  writeSkill(join(project, ".claude/skills"), "ship", skill("ship", "Claude ship"));
  writeSkill(join(home, ".agents/skills"), "greet", skill("greet", "Hello"));
  writeSkill(join(home, ".claude/skills"), "polish", skill("polish", "Personal"));
  writeSkill(join(project, ".cursor/skills"), "cursor-only", skill("cursor-only", "Cursor"));
  mkdirSync(join(home, ".claude/skills/no-skill-md"), { recursive: true });

  const skills = list(project, home);
  expect(skills.find((entry) => entry.name === "ship")).toMatchObject({
    description: "MonoCode ship",
    source: "agents",
    scope: "project",
  });
  expect(skills.find((entry) => entry.name === "greet")).toMatchObject({
    source: "agents",
    scope: "user",
  });
  expect(skills.find((entry) => entry.name === "polish")).toMatchObject({
    source: "claude",
    scope: "user",
    path: join(home, ".claude/skills/polish/SKILL.md"),
  });
  expect(skills.find((entry) => entry.name === "cursor-only")?.source).toBe("cursor");
  expect(skills.some((entry) => entry.name === "no-skill-md")).toBe(false);
});

it("lists enabled Claude plugin skills under their plugin's name", () => {
  const project = tmp("project");
  const other = tmp("other");
  const home = tmp("home");
  const userPlugin = join(home, ".claude/plugins/cache/community/workflow-kit/1.2.3");
  const projectPlugin = join(home, ".claude/plugins/cache/community/delivery/2.0.0");
  writeSkill(join(userPlugin, "skills"), "quick-plan", skill("quick-plan", "Plan from plugin"));
  writeSkill(join(projectPlugin, "skills"), "ship-it", skill("ship-it", "Deliver"));
  writeSkill(join(home, ".claude/skills"), "quick-plan", skill("quick-plan", "Personal plan"));
  mkdirSync(join(home, ".claude/plugins"), { recursive: true });
  writeFileSync(
    join(home, ".claude/plugins/installed_plugins.json"),
    JSON.stringify({
      version: 2,
      plugins: {
        "workflow-kit@community": [
          {
            scope: "user",
            installPath: "~/.claude/plugins/cache/community/workflow-kit/1.2.3",
          },
        ],
        "delivery@community": [
          { scope: "project", projectPath: project, installPath: projectPlugin },
        ],
      },
    }),
  );

  const nested = join(project, "src");
  mkdirSync(nested);
  const skills = list(nested, home);
  expect(skills.find((entry) => entry.name === "workflow-kit:quick-plan")).toMatchObject({
    description: "Plan from plugin",
    source: "claude",
    scope: "user",
  });
  expect(skills.some((entry) => entry.name === "quick-plan")).toBe(true);
  expect(skills.find((entry) => entry.name === "delivery:ship-it")?.scope).toBe("project");
  // A project-scoped plugin applies only inside its project.
  expect(list(other, home).some((entry) => entry.name === "delivery:ship-it")).toBe(false);

  // A plugin turned off in the project's settings is hidden there.
  mkdirSync(join(project, ".claude"));
  writeFileSync(
    join(project, ".claude/settings.local.json"),
    JSON.stringify({ enabledPlugins: { "workflow-kit@community": false } }),
  );
  expect(list(nested, home).some((entry) => entry.name === "workflow-kit:quick-plan")).toBe(
    false,
  );
});

it("falls back to a same-name personal skill when the project one is disabled", () => {
  const project = tmp("project");
  const home = tmp("home");
  writeSkill(join(project, ".agents/skills"), "review", skill("review", "Project review"));
  writeSkill(join(home, ".agents/skills"), "review", skill("review", "Personal review"));
  const projectSkill = join(project, ".agents/skills/review/SKILL.md");
  const personalSkill = join(home, ".agents/skills/review/SKILL.md");
  const review = (disabled?: string[]) =>
    list(project, home, disabled).find((entry) => entry.name === "review");

  expect(review()?.description).toBe("Project review");
  expect(review([projectSkill])).toMatchObject({
    description: "Personal review",
    scope: "user",
  });
  expect(review([personalSkill])?.description).toBe("Project review");
  expect(review([projectSkill, personalSkill])).toBeUndefined();
});
