/** Bundled MonoCode skill: teach the agent how to write portable SKILL.md files. */
export const CREATE_SKILL_NAME = "create-skill";

export const CREATE_SKILL_DESCRIPTION =
  "Create a MonoCode skill as a SKILL.md in .agents/skills. Use when the user wants to author, write, save, or scaffold a skill, or asks about skill format.";

export const CREATE_SKILL_BODY = `---
name: create-skill
description: Create a MonoCode skill as a SKILL.md in .agents/skills. Use when the user wants to author, write, save, or scaffold a skill, or asks about skill format.
---

# Create a MonoCode skill

Write a portable Agent Skill so every harness (Claude, Cursor, Codex, OpenCode, Pi, fx, Amp) can load it.

## Storage (required)

| Scope | Path |
|-------|------|
| This project | \`.agents/skills/<name>/SKILL.md\` |
| Personal (all projects) | \`~/.agents/skills/<name>/SKILL.md\` |

Never write skills to \`.claude/skills\`, \`.cursor/skills\`, \`~/.cursor/skills-cursor\`, \`.fx/skills\`, \`~/.config/amp/skills\`, or any other harness-only folder. One copy under \`.agents/skills\` is enough.

Ask where it should live if the user did not say. Default to the project when a project folder is open.

## Gather before writing

1. **Purpose** — what task this skill owns
2. **Scope** — project vs personal
3. **Triggers** — when the agent should use it (put these in \`description\`)
4. **Domain knowledge** the model would not already know
5. **Output format** — templates, names, style
6. **Verbatim text** — if the user gave exact wording, use it unchanged

Infer from the conversation when it is already clear. Do not ask questions you can answer from context.

## File layout

\`\`\`
skill-name/
├── SKILL.md           # required
├── reference.md       # optional, details the agent reads only when needed
├── examples.md        # optional
└── scripts/           # optional helpers
\`\`\`

## SKILL.md format

\`\`\`markdown
---
name: skill-name
description: What it does and when to use it. Include trigger terms.
---

# Title

## Instructions
...
\`\`\`

- \`name\`: lowercase letters, numbers, hyphens; max 64 characters; must match the folder name
- \`description\`: third person; WHAT + WHEN; max 1024 characters. This is how the picker and the model decide to use the skill.
- Keep SKILL.md under 500 lines. Put long reference material in sibling files and link them one level deep.

### Description

Good: "Review pull requests against team standards. Use when asked to review a PR, diff, or code change."
Bad: "Helps with code." / "I can review PRs."

## Authoring rules

- The model is already smart. Only write what it cannot guess.
- Prefer concrete steps, checklists, and examples over essays.
- One default approach, plus an escape hatch. Do not list five equivalent tools.
- POSIX paths only (\`scripts/helper.py\`, never \`scripts\\\\helper.py\`).
- No dates that will rot. Put legacy notes in an "Old patterns" section.

## After writing

1. Confirm the file path
2. Confirm name + description
3. Do not copy the skill into harness-specific folders
`;
