import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  ClaudeContextUsage,
  compactionWindowFromContextUsage,
} from "./claudeContextUsage";

const nativePayload = {
  rawMaxTokens: 400_000,
  maxTokens: 400_000,
  autoCompactThreshold: 367_000,
  isAutoCompactEnabled: true,
  model: "claude-opus-5-5",
};

function reply(requestId: string, response: Record<string, unknown>) {
  return {
    type: "control_response",
    response: { subtype: "success", request_id: requestId, response },
  };
}

function failure(requestId: string, error: string) {
  return {
    type: "control_response",
    response: { subtype: "error", request_id: requestId, error },
  };
}

function setup(send?: (request: Record<string, unknown>) => Promise<void>) {
  const requests: Record<string, unknown>[] = [];
  const published: (number | null)[] = [];
  const meter = new ClaudeContextUsage(
    send ??
      (async (request) => {
        requests.push(request);
      }),
    (window) => published.push(window),
  );
  return { meter, requests, published };
}

const idOf = (request: Record<string, unknown>) =>
  request.request_id as string;

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("compactionWindowFromContextUsage", () => {
  it("reads the raw window of the native payload", () => {
    expect(compactionWindowFromContextUsage(nativePayload)).toBe(400_000);
  });

  it.each([
    [{ ...nativePayload, isAutoCompactEnabled: false }],
    [{ rawMaxTokens: 400_000 }],
    [{ ...nativePayload, rawMaxTokens: undefined }],
    [{ ...nativePayload, rawMaxTokens: "400000" }],
    [{ ...nativePayload, rawMaxTokens: Number.NaN }],
    [{ ...nativePayload, rawMaxTokens: Number.POSITIVE_INFINITY }],
    [{ ...nativePayload, rawMaxTokens: 0 }],
    [{ ...nativePayload, rawMaxTokens: -1 }],
    [null],
  ])("has no window for %j", (payload) => {
    expect(compactionWindowFromContextUsage(payload)).toBeNull();
  });
});

describe("ClaudeContextUsage", () => {
  it("asks for context usage and publishes only the window", async () => {
    const { meter, requests, published } = setup();
    meter.refresh();
    await vi.advanceTimersByTimeAsync(0);
    expect(requests).toHaveLength(1);
    expect(requests[0]).toMatchObject({
      type: "control_request",
      request: { subtype: "get_context_usage" },
    });
    expect(idOf(requests[0])).toMatch(/^monocode_context_/);
    expect(meter.handleResponse(reply(idOf(requests[0]), nativePayload))).toBe(
      true,
    );
    expect(published).toEqual([400_000]);
  });

  it("publishes null when disabled", async () => {
    const { meter, requests, published } = setup();
    meter.refresh();
    await vi.advanceTimersByTimeAsync(0);
    meter.handleResponse(
      reply(idOf(requests[0]), { ...nativePayload, isAutoCompactEnabled: false }),
    );
    expect(published).toEqual([null]);
  });

  it("ignores unrelated and stale replies", async () => {
    const { meter, requests, published } = setup();
    meter.refresh();
    meter.refresh();
    await vi.advanceTimersByTimeAsync(0);
    expect(new Set(requests.map(idOf)).size).toBe(2);
    expect(meter.handleResponse(reply("monocode_2", {}))).toBe(false);
    expect(meter.handleResponse({ type: "system" })).toBe(false);
    expect(meter.handleResponse(reply(idOf(requests[0]), nativePayload))).toBe(
      true,
    );
    expect(published).toEqual([]);
    meter.handleResponse(reply(idOf(requests[1]), nativePayload));
    expect(published).toEqual([400_000]);
    meter.handleResponse(reply(idOf(requests[1]), nativePayload));
    expect(published).toEqual([400_000]);
  });

  it("publishes null on timeout and accepts a later request", async () => {
    const { meter, requests, published } = setup();
    meter.refresh();
    await vi.advanceTimersByTimeAsync(4_999);
    expect(published).toEqual([]);
    await vi.advanceTimersByTimeAsync(1);
    expect(published).toEqual([null]);
    meter.refresh();
    await vi.advanceTimersByTimeAsync(0);
    meter.handleResponse(reply(idOf(requests[1]), nativePayload));
    expect(published).toEqual([null, 400_000]);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(published).toEqual([null, 400_000]);
  });

  it("clears quietly when the send is rejected", async () => {
    const { meter, published } = setup(async () => {
      throw new Error("closed");
    });
    meter.refresh();
    await vi.advanceTimersByTimeAsync(0);
    expect(published).toEqual([null]);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(published).toEqual([null]);
  });

  it.each([
    "Unknown request subtype",
    "Unsupported control request: get_context_usage",
    "Unrecognized subtype",
  ])("stops asking after an explicit unsupported error (%s)", async (error) => {
    const { meter, requests, published } = setup();
    meter.refresh();
    await vi.advanceTimersByTimeAsync(0);
    meter.handleResponse(failure(idOf(requests[0]), error));
    expect(published).toEqual([null]);
    meter.refresh();
    await vi.advanceTimersByTimeAsync(0);
    expect(requests).toHaveLength(1);
  });

  it.each(["temporarily busy", "Unknown error while reading context"])(
    "retries after a transient error (%s)",
    async (error) => {
      const { meter, requests, published } = setup();
      meter.refresh();
      await vi.advanceTimersByTimeAsync(0);
      meter.handleResponse(failure(idOf(requests[0]), error));
      expect(published).toEqual([null]);
      meter.refresh();
      await vi.advanceTimersByTimeAsync(0);
      expect(requests).toHaveLength(2);
    },
  );

  it("cancels timers and ignores late replies once disposed", async () => {
    const { meter, requests, published } = setup();
    meter.refresh();
    await vi.advanceTimersByTimeAsync(0);
    meter.dispose();
    expect(vi.getTimerCount()).toBe(0);
    expect(meter.handleResponse(reply(idOf(requests[0]), nativePayload))).toBe(
      true,
    );
    meter.refresh();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(requests).toHaveLength(1);
    expect(published).toEqual([]);
  });
});
