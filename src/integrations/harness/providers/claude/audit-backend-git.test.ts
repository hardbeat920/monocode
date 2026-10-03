import { expect, it, vi } from "vitest";
const runText = vi.fn(async () => "audit-branch");
vi.mock("./claudeText", () => ({ runClaudeTextPrompt: runText }));
vi.mock("../../../../platform/tauri/fs", () => ({
  gitStagedContext: async () => ({
    branch: "main",
    summary: "one file",
    patch: "dummy patch",
  }),
  gitRangeContext: async () => ({
    base: "main",
    head: "work",
    commitSummary: "dummy commit",
    diffSummary: "one file",
    diffPatch: "dummy patch",
  }),
}));
const {
  generateClaudeBranchName,
  generateClaudeCommitMessage,
  generateClaudePrContent,
} = await import("./claudeGit");
it("passes the selected account to every Git helper", async () => {
  await generateClaudeBranchName("/repo", "work", "account-work");
  await generateClaudeCommitMessage("/repo", undefined, "account-work").catch(
    () => undefined,
  );
  await generateClaudePrContent("/repo", "account-work");
  expect(runText).toHaveBeenCalledTimes(3);
  for (const [input] of runText.mock.calls as unknown as [
    Record<string, unknown>,
  ][]) {
    expect(input.cwd).toBe("/repo");
    expect(input.providerAccountId).toBe("account-work");
  }
});
