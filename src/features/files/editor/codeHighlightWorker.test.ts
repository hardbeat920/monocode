import { afterEach, expect, it, vi } from "vitest";
import type { HighlightOptions, HighlightResult } from "@streamdown/code";

class TestWorker {
  static instances: TestWorker[] = [];
  sent: { id: number; options: HighlightOptions }[] = [];
  onmessage?: (event: {
    data: { id: number; result: HighlightResult };
  }) => void;
  onerror?: () => void;
  terminate = vi.fn();
  constructor() {
    TestWorker.instances.push(this);
  }
  postMessage(job: { id: number; options: HighlightOptions }) {
    this.sent.push(job);
  }
  finish() {
    const job = this.sent.at(-1)!;
    this.onmessage?.({
      data: {
        id: job.id,
        result: {
          tokens: [[{ content: job.options.code, offset: 0 }]],
        } as HighlightResult,
      },
    });
  }
}
const options = (code: string): HighlightOptions => ({
  code,
  language: "typescript",
  themes: ["github-light", "github-dark"],
});
afterEach(() => {
  vi.unstubAllGlobals();
  TestWorker.instances = [];
  vi.resetModules();
});

it("coalesces equal fences, removes obsolete queued work, and prioritizes live output", async () => {
  vi.stubGlobal("Worker", TestWorker);
  const { requestCodeHighlight } = await import("./codeHighlightPlugin");
  const first = vi.fn();
  const cancelled = vi.fn();
  const shared = vi.fn();
  const obsolete = vi.fn();
  const live = vi.fn();
  const history = vi.fn();
  requestCodeHighlight(options("first"), first);
  const cancel = requestCodeHighlight(options("first"), cancelled);
  requestCodeHighlight(options("first"), shared);
  cancel();
  requestCodeHighlight(options("old"), obsolete)();
  requestCodeHighlight(options("history"), history, 1);
  requestCodeHighlight(options("live"), live, 0);
  const worker = TestWorker.instances[0];
  expect(worker.sent).toHaveLength(1);
  worker.finish();
  expect(first).toHaveBeenCalledOnce();
  expect(shared).toHaveBeenCalledOnce();
  expect(cancelled).not.toHaveBeenCalled();
  expect(worker.sent.at(-1)?.options.code).toBe("live");
  worker.finish();
  expect(worker.sent.at(-1)?.options.code).toBe("history");
  worker.finish();
  expect(obsolete).not.toHaveBeenCalled();
  const cached = vi.fn();
  requestCodeHighlight(options("first"), cached);
  expect(cached).toHaveBeenCalledOnce();
  expect(worker.sent).toHaveLength(3);
});

it("discards cancelled active results and recovers from worker failure", async () => {
  vi.stubGlobal("Worker", TestWorker);
  const { requestCodeHighlight } = await import("./codeHighlightPlugin");
  const callback = vi.fn();
  requestCodeHighlight(options("obsolete"), callback)();
  const worker = TestWorker.instances[0];
  worker.finish();
  expect(callback).not.toHaveBeenCalled();
  requestCodeHighlight(options("next"), callback);
  worker.onerror?.();
  expect(worker.terminate).toHaveBeenCalledOnce();
  requestCodeHighlight(options("recovered"), callback);
  expect(TestWorker.instances).toHaveLength(2);
  TestWorker.instances[1].finish();
  expect(callback).toHaveBeenCalledOnce();
});

it("releases an abandoned tokenizer so another session does not wait for it", async () => {
  vi.stubGlobal("Worker", TestWorker);
  const { requestCodeHighlight } = await import("./codeHighlightPlugin");
  const obsolete = vi.fn();
  const visible = vi.fn();
  const cancel = requestCodeHighlight(options("old session"), obsolete, 1);
  const oldWorker = TestWorker.instances[0];
  cancel();
  requestCodeHighlight(options("visible session"), visible, 1);
  await Promise.resolve();

  expect(oldWorker.terminate).toHaveBeenCalledOnce();
  expect(TestWorker.instances).toHaveLength(2);
  const currentWorker = TestWorker.instances[1];
  expect(currentWorker.sent[0].options.code).toBe("visible session");
  oldWorker.finish();
  oldWorker.onerror?.();
  expect(obsolete).not.toHaveBeenCalled();
  expect(visible).not.toHaveBeenCalled();
  expect(currentWorker.terminate).not.toHaveBeenCalled();
  currentWorker.finish();
  expect(visible).toHaveBeenCalledOnce();
});

it("retains a streaming checkpoint until its replacement also loses its readers", async () => {
  vi.stubGlobal("Worker", TestWorker);
  const { requestCodeHighlight } = await import("./codeHighlightPlugin");
  const obsolete = vi.fn();
  const cancel = requestCodeHighlight(options("const value ="), obsolete);
  const worker = TestWorker.instances[0];
  cancel();
  const cancelReplacement = requestCodeHighlight(
    options("const value = 1;"),
    vi.fn(),
  );
  await Promise.resolve();
  expect(worker.terminate).not.toHaveBeenCalled();
  expect(worker.sent).toHaveLength(1);

  cancelReplacement();
  await Promise.resolve();
  expect(worker.terminate).toHaveBeenCalledOnce();
  worker.finish();
  expect(obsolete).not.toHaveBeenCalled();
});
