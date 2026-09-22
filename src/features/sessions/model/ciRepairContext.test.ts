import { expect, it } from "vitest";
import {
  appendReadyHandoff,
  buildDeterministicHandoff,
  userMessagesAfterHandoff,
} from "./handoff";
import { buildSecondOpinionPrompt } from "./secondOpinion";
import { newSession, type Block } from "./session";

const repair: Block = {
  id: "repair",
  role: "user",
  text: "Fix 1 failed CI check for acme/web PR #42.",
  ciContext: `Checked commit: abc123\n${"CI instructions. ".repeat(60)}\nFailed check: lint\nDo not commit or push unless asked.`,
};

it.each([
  [
    "provider handoff",
    () =>
      buildDeterministicHandoff({
        ...newSession("claude", "/web"),
        blocks: [repair],
      }),
  ],
  [
    "retry after a failed handoff",
    () => {
      const session = appendReadyHandoff(
        newSession("claude", "/web"),
        "claude",
        "codex",
        "Repair lint",
      );
      return userMessagesAfterHandoff({
        ...session,
        blocks: [...session.blocks, repair],
      }).join("\n");
    },
  ],
  [
    "second opinion",
    () =>
      buildSecondOpinionPrompt({
        from: "claude",
        userRequest: repair.text,
        report: "Fixed lint",
        files: [],
        ciContext: repair.ciContext,
      }),
  ],
] as const)("preserves CI evidence for %s", (_name, prompt) => {
  expect(prompt()).toContain("Checked commit: abc123");
  expect(prompt()).toContain("Failed check: lint");
  expect(prompt()).toContain("Do not commit or push unless asked.");
});
