import { describe, expect, it, vi } from "vitest";

vi.mock("../fs", () => ({
  gitStagedContext: async () => ({
    branch: "main",
    summary: "1 file changed, 10 insertions(+)",
    patch: "diff --git a/src/index.ts b/src/index.ts\n+console.log('antigravity');",
  }),
  gitRangeContext: async () => ({
    base: "main",
    head: "feat/antigravity",
    commitSummary: "Add Antigravity CLI support",
    diffSummary: "2 files changed",
    diffPatch: "+antigravity",
  }),
}));

vi.mock("./antigravityText", () => ({
  runAntigravityTextPrompt: async ({ prompt }: { prompt: string }) => {
    if (prompt.includes("git commit messages")) {
      return JSON.stringify({
        subject: "feat: add antigravity support",
        body: "Integrate Antigravity CLI into MonoCode",
      });
    }
    if (prompt.includes("source control change request content")) {
      return JSON.stringify({
        title: "feat: add Antigravity integration",
        body: "## Summary\n- Adds official Antigravity support.\n\n## Testing\n- Not run",
      });
    }
    if (prompt.includes("git branch names")) {
      return JSON.stringify({
        branch: "feat/antigravity-support",
      });
    }
    return "ok";
  },
}));

const {
  generateAntigravityCommitMessage,
  generateAntigravityPrContent,
  generateAntigravityBranchName,
} = await import("./antigravityGit");

describe("Antigravity git helpers", () => {
  it("generates a formatted commit message", async () => {
    await expect(generateAntigravityCommitMessage("/fake/repo")).resolves.toBe(
      "feat: add antigravity support\n\nIntegrate Antigravity CLI into MonoCode",
    );
  });

  it("generates pull request content", async () => {
    await expect(generateAntigravityPrContent("/fake/repo")).resolves.toEqual({
      title: "feat: add Antigravity integration",
      body: "## Summary\n- Adds official Antigravity support.\n\n## Testing\n- Not run",
      base: "main",
      head: "feat/antigravity",
    });
  });

  it("generates a branch name", async () => {
    await expect(
      generateAntigravityBranchName("/fake/repo", "add antigravity support"),
    ).resolves.toBe("feat/antigravity-support");
  });
});
