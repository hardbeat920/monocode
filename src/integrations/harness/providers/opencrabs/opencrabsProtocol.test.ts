import { describe, expect, it } from "vitest";
import { writeFileSync, rmSync } from "node:fs";

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

describe("eventsFromAcpUpdate image resource_links", () => {
  it("emits image.generated for a disk-backed image resource_link", () => {
    const path = "/tmp/acp-adapter-img-test.png";
    writeFileSync(path, Buffer.from("png"));
    try {
      const events = eventsFromAcpUpdate({
        update: {
          sessionUpdate: "tool_call_update",
          toolCallId: "call-img-1",
          status: "completed",
          rawOutput: "chart saved",
          content: [
            { type: "text", text: "chart saved" },
            { type: "resource_link", uri: `file://${path}`, name: "chart.png" },
          ],
        },
      });
      const img = events.find((e) => e.type === "image.generated");
      expect(img).toMatchObject({
        type: "image.generated",
        itemId: "call-img-1:1",
        path,
        name: "chart.png",
        mimeType: "image/png",
      });
      expect(events.some((e) => e.type === "tool.updated")).toBe(true);
    } finally {
      rmSync(path);
    }
  });

  it("ignores missing files, non-images, and non-file uris", () => {
    const events = eventsFromAcpUpdate({
      update: {
        sessionUpdate: "tool_call_update",
        toolCallId: "call-img-2",
        content: [
          { type: "resource_link", uri: "file:///nonexistent/nope.png" },
          { type: "resource_link", uri: "file:///etc/hosts" },
          { type: "resource_link", uri: "https://example.com/x.png" },
        ],
      },
    });
    expect(events.some((e) => e.type === "image.generated")).toBe(false);
  });
});

describe("eventsFromAcpUpdate subagent classification", () => {
  it("classifies the spawn title prefix as an agent card", () => {
    const events = eventsFromAcpUpdate({
      update: {
        sessionUpdate: "tool_call",
        toolCallId: "call-agent-1",
        kind: "other",
        title: "subagent: fix-auth",
        status: "in_progress",
        rawInput: { label: "fix-auth" },
      },
    });
    const tool = events[0];
    expect(tool.type).toBe("tool.updated");
    expect((tool as { kind?: string }).kind).toBe("agent");
  });

  it("leaves ordinary tools unclassified", () => {
    const events = eventsFromAcpUpdate({
      update: {
        sessionUpdate: "tool_call",
        toolCallId: "call-bash-1",
        kind: "execute",
        title: "bash",
        status: "in_progress",
      },
    });
    expect((events[0] as { kind?: string }).kind).toBe("execute");
  });
});
