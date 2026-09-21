import { describe, expect, it } from "vitest";

import { eventsFromAcpUpdate } from "./opencrabsProtocol";

/**
 * Usage metering end-to-end: the opencrabs ACP server emits
 * `{ sessionUpdate: "usage", usage: { used, size } }` (size is the ACP
 * spec's field for the context window total). The meter must translate
 * `size` into MonoCode's `window` — with `used` alone, `contextRatio`
 * returns null and the context meter renders nothing.
 *
 * Regression: the adapter originally read only window/contextWindow/
 * context_window, while the server sent `size` — the meter was silently
 * dead for every opencrabs session.
 */
describe("eventsFromAcpUpdate usage metering", () => {
  it("maps the opencrabs wire shape (used + size) to a context event", () => {
    const events = eventsFromAcpUpdate({
      update: {
        sessionUpdate: "usage",
        usage: { used: 1234, size: 200000 },
      },
    });
    expect(events).toEqual([
      { type: "context", used: 1234, window: 200000 },
    ]);
  });

  it("still maps the legacy window field names", () => {
    const events = eventsFromAcpUpdate({
      update: {
        sessionUpdate: "usage",
        usage: { used: 500, context_window: 128000 },
      },
    });
    expect(events).toEqual([
      { type: "context", used: 500, window: 128000 },
    ]);
  });

  it("emits nothing when the usage block has no readable numbers", () => {
    const events = eventsFromAcpUpdate({
      update: { sessionUpdate: "usage", usage: {} },
    });
    expect(events).toEqual([]);
  });
});

describe("acpSizeField validation (CodeRabbit: spec is unsigned integer)", () => {
  it("accepts a non-negative integer size", () => {
    const events = eventsFromAcpUpdate({
      update: { sessionUpdate: "usage", usage: { used: 10, size: 0 } },
    });
    expect(events).toEqual([{ type: "context", used: 10, window: 0 }]);
  });

  it("rejects a negative size instead of poisoning the meter window", () => {
    const events = eventsFromAcpUpdate({
      update: { sessionUpdate: "usage", usage: { used: 10, size: -5 } },
    });
    // used survives, window stays undefined — the meter keeps its previous
    // window instead of adopting garbage.
    expect(events).toEqual([{ type: "context", used: 10, window: undefined }]);
  });

  it("rejects a fractional size", () => {
    const events = eventsFromAcpUpdate({
      update: { sessionUpdate: "usage", usage: { used: 10, size: 200000.5 } },
    });
    expect(events).toEqual([{ type: "context", used: 10, window: undefined }]);
  });
});
