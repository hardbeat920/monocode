import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { SkillPicker } from "./SkillPicker";
import { ompCommandsFromRpcData } from "../lib/harness/piSkills";
import { PLAN_COMMAND } from "../lib/plan";
import { COMPACT_COMMAND } from "../lib/compact";
import type { Skill } from "../lib/skills";

describe("native command picker", () => {
  it("renders native commands and argument hints alongside MonoCode shortcuts", () => {
    const native: Skill[] = ompCommandsFromRpcData({
      commands: [
        { name: "plan", source: "builtin", description: "OMP planning" },
        {
          name: "compact",
          source: "builtin",
          input: { hint: "[instructions]" },
        },
        {
          name: "workflow",
          source: "custom",
          description: "Choose planners and reviewers",
          input: { hint: "<reviewer> [path]" },
        },
        {
          name: "mcp",
          source: "builtin",
          subcommands: [{ name: "list", usage: "list --all" }],
        },
      ],
    }).map((command) => ({ ...command, kind: "native" }));
    const html = renderToStaticMarkup(
      createElement(SkillPicker, {
        skills: [PLAN_COMMAND, COMPACT_COMMAND, ...native],
        query: "",
        active: 0,
        creating: false,
        cwd: "/repo",
        onActive: vi.fn(),
        onPick: vi.fn(),
        onStartCreate: vi.fn(),
        onCancelCreate: vi.fn(),
        onCreate: vi.fn(),
      }),
    );
    expect(html).toContain("/omp:plan");
    expect(html).toContain("/omp:compact");
    expect(html).toContain("/plan");
    expect(html).toContain("/compact");
    expect(html).toContain("/workflow");
    expect(html).toContain("Choose planners and reviewers");
    expect(html).toContain("&lt;reviewer&gt; [path]");
    expect(html).toContain("omp · custom");
    expect(html).toContain("list --all");
  });
});
