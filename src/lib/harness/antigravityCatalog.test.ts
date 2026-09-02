import { describe, expect, it, vi } from "vitest";

const execChild = vi.fn(async () =>
  JSON.stringify({
    command: {
      data: {
        models: [
          { id: "gemini-3.7-flash-high", label: "Gemini 3.7 Flash (High)" },
          { id: "gemini-3.7-flash-medium", label: "Gemini 3.7 Flash (Medium)" },
          { id: "gemini-3.7-flash-low", label: "Gemini 3.7 Flash (Low)" },
        ],
      },
    },
  }),
);

vi.mock("./child", () => ({
  resolveAntigravityBinary: async () => ({ path: "/fake/agy" }),
  execChild,
}));

const { discoverAntigravityModels } = await import("./antigravityCatalog");

describe("Antigravity model catalog", () => {
  it("uses the structured models command result and unifies reasoning variants", async () => {
    await expect(discoverAntigravityModels("/repo")).resolves.toEqual([
      {
        id: "antigravity:gemini-3.7-flash",
        harness: "antigravity",
        name: "Gemini 3.7 Flash",
        nativeId: "gemini-3.7-flash",
        contextWindow: 1_000_000,
        settings: [
          {
            id: "effort",
            label: "Reasoning",
            kind: "select",
            value: "high",
            options: [
              { label: "High", value: "high" },
              { label: "Medium", value: "medium" },
              { label: "Low", value: "low" },
            ],
          },
        ],
      },
    ]);
    expect(execChild).toHaveBeenCalledWith(
      "/fake/agy",
      ["--output-format", "json", "models"],
      "/repo",
    );
  });

  it("does not start a workspace-less probe", async () => {
    const callsBefore = execChild.mock.calls.length;
    await expect(discoverAntigravityModels()).resolves.toEqual([]);
    expect(execChild).toHaveBeenCalledTimes(callsBefore);
  });
});
