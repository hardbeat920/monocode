import { describe, expect, it } from "vitest";
import { RUNTIME_MODES } from "../../sessions/model/session";
import type { AgentModel } from "../../sessions/model/models";
import {
  filterQuickModels,
  filterQuickProjects,
  orderQuickProjects,
  parseQuickLaunch,
} from "./quickComposer";

describe("parseQuickLaunch", () => {
  it("accepts a complete launch", () => {
    expect(
      parseQuickLaunch({
        prompt: "fix the flaky test",
        cwd: "/Users/me/code/app",
        harness: "claude",
        reveal: true,
      }),
    ).toEqual({
      prompt: "fix the flaky test",
      cwd: "/Users/me/code/app",
      harness: "claude",
      reveal: true,
    });
  });

  it("carries the chosen model, and leaves it to the workspace when blank", () => {
    const base = { prompt: "hi", cwd: "/Users/me/code/app", harness: "codex" };
    expect(parseQuickLaunch({ ...base, model: "gpt-6" })?.model).toBe("gpt-6");
    expect(parseQuickLaunch({ ...base, model: "" })).not.toHaveProperty(
      "model",
    );
    expect(parseQuickLaunch({ ...base, model: 7 })).not.toHaveProperty("model");
  });

  it("carries effort and other model settings across the launch boundary", () => {
    const base = { prompt: "hi", cwd: "/Users/me/code/app", harness: "claude" };
    expect(
      parseQuickLaunch({
        ...base,
        modelSettings: { effort: "high", fast: "true", invalid: 7 },
      })?.modelSettings,
    ).toEqual({ effort: "high", fast: "true" });
    expect(
      parseQuickLaunch({ ...base, modelSettings: ["high"] }),
    ).not.toHaveProperty("modelSettings");
    expect(parseQuickLaunch(base)).not.toHaveProperty("modelSettings");
  });

  it.each(RUNTIME_MODES)(
    "carries the selected %s permissions into the session",
    (runtimeMode) => {
      expect(
        parseQuickLaunch({
          prompt: "hi",
          cwd: "/tmp/project",
          harness: "codex",
          runtimeMode,
        })?.runtimeMode,
      ).toBe(runtimeMode);
    },
  );

  it("leaves missing or invalid permissions at the session's supervised default", () => {
    const base = { prompt: "hi", cwd: "/tmp/project", harness: "codex" };
    expect(parseQuickLaunch(base)).not.toHaveProperty("runtimeMode");
    expect(
      parseQuickLaunch({ ...base, runtimeMode: "unknown" }),
    ).not.toHaveProperty("runtimeMode");
  });

  it("starts quietly unless reveal is exactly true", () => {
    expect(
      parseQuickLaunch({
        prompt: "hi",
        cwd: "/Users/me/code/app",
        harness: "codex",
        reveal: "yes",
      })?.reveal,
    ).toBe(false);
  });

  it("drops blank prompts, missing projects, and unknown harnesses", () => {
    const base = { prompt: "hi", cwd: "/Users/me/code/app", harness: "claude" };
    expect(parseQuickLaunch({ ...base, prompt: "   " })).toBeNull();
    expect(parseQuickLaunch({ ...base, cwd: "" })).toBeNull();
    expect(parseQuickLaunch({ ...base, harness: "gemini" })).toBeNull();
    expect(parseQuickLaunch(null)).toBeNull();
  });
});

describe("orderQuickProjects", () => {
  it("puts recents first and fills in the rest of the rail once", () => {
    expect(
      orderQuickProjects(
        ["/Users/me/code/b", "/Users/me/code/a/"],
        ["/Users/me/code/c"],
        ["/Users/me/code/a", "/Users/me/code/d"],
        [],
      ),
    ).toEqual([
      "/Users/me/code/b",
      "/Users/me/code/a",
      "/Users/me/code/c",
      "/Users/me/code/d",
    ]);
  });

  it("leaves out archived projects and non-projects", () => {
    expect(
      orderQuickProjects(
        ["/Users/me/code/a", "~", "/", "/Users/me/code/old"],
        [],
        [],
        ["/Users/me/code/old"],
      ),
    ).toEqual(["/Users/me/code/a"]);
  });
});

describe("filterQuickProjects", () => {
  const projects = ["/Users/me/code/monocode", "/Users/me/work/api"];

  it("matches the project name or its parent folder", () => {
    expect(filterQuickProjects(projects, "mono")).toEqual([
      "/Users/me/code/monocode",
    ]);
    expect(filterQuickProjects(projects, "work")).toEqual([
      "/Users/me/work/api",
    ]);
  });

  it("returns everything for an empty query", () => {
    expect(filterQuickProjects(projects, "  ")).toEqual(projects);
  });
});

describe("filterQuickModels", () => {
  const models: AgentModel[] = [
    { id: "claude-opus", harness: "claude", name: "Claude Opus" },
    {
      id: "opencode/kimi",
      harness: "opencode",
      name: "Kimi K3",
      provider: { id: "moonshot", name: "Moonshot" },
    },
  ];

  it("matches the model name, its provider, or the harness", () => {
    expect(filterQuickModels(models, "opus").map((m) => m.id)).toEqual([
      "claude-opus",
    ]);
    expect(filterQuickModels(models, "moonshot").map((m) => m.id)).toEqual([
      "opencode/kimi",
    ]);
    expect(filterQuickModels(models, "opencode").map((m) => m.id)).toEqual([
      "opencode/kimi",
    ]);
  });
});
