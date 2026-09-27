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

    // The button stays enabled when the same harness has another model.
    expect(markup).toContain('aria-label="Second opinion"');
    expect(markup).not.toContain(
      'aria-label="No different model available for a second opinion"',
    );
  });
});
