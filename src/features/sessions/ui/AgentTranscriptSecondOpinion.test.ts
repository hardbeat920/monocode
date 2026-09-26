import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { Block } from "../model/session";

// Only "claude" is an installed/available harness in this test, the same
// shape as a user who only enabled one harness in Settings.
vi.mock("../../../integrations/harness/core/availability", () => ({
  getHarnessAvailabilitySnapshot: () => 0,
  hasProbedHarnessAvailability: () => true,
  isHarnessAvailable: (harness: string) => harness === "claude",
  probeHarnessAvailability: () => Promise.resolve(),
  subscribeHarnessAvailability: () => () => undefined,
}));

vi.mock("../../../integrations/harness/core/registry", () => ({
  refreshHarnessCatalogs: () => Promise.resolve(),
}));

import { AgentTranscript } from "./AgentTranscript";

describe("second opinion with only one harness installed", () => {
  it("still let the user ask for a second opinion from a different model of the same harness", () => {
    const blocks: Block[] = [
      { id: "user", role: "user", text: "Ship it", durationMs: 5_000 },
      { id: "answer", role: "assistant", text: "Done." },
    ];
    const markup = renderToStaticMarkup(
      createElement(AgentTranscript, {
        blocks,
        harness: "claude",
        onSecondOpinion: () => {},
      }),
    );

    // Before the fix, the button say "Install another provider for a
    // second opinion" and is disabled, even though the same harness has
    // other models it could use. After the fix, it stay enabled.
    expect(markup).toContain('aria-label="Second opinion"');
    expect(markup).not.toContain(
      'aria-label="Install another provider for a second opinion"',
    );
  });
});
