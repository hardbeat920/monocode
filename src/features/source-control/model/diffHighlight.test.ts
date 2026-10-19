import { afterEach, expect, it, vi } from "vitest";
import type { HighlightRequest } from "./diffHighlight";
import type { SyntaxToken } from "../../files/editor/syntaxTokens";
import { buildUnifiedFile } from "./unifiedDiff";

class TestWorker {
  static instances: TestWorker[] = [];
  sent: HighlightRequest[] = [];
  onmessage?: (event: {
    data: { id: number; tokens: SyntaxToken[][] };
  }) => void;
  onerror?: () => void;
  terminate = vi.fn();
  constructor() {
    TestWorker.instances.push(this);
  }
  postMessage(request: HighlightRequest) {
    this.sent.push(request);
  }
  finish() {
    const request = this.sent.at(-1)!;
    this.onmessage?.({
      data: {
        id: request.id,
        tokens: request.file.blocks.flatMap((block) =>
          block.lines.map((line) => [{ text: line.text, color: "#abcdef" }]),
        ),
      },
    });
  }
}
afterEach(() => {
  vi.unstubAllGlobals();
  vi.resetModules();
  TestWorker.instances = [];
});
const file = (path: string) => ({
  path,
  blocks: buildUnifiedFile("old\n", "new\n").blocks,
});

it("maps worker results to original source lines and caches the unchanged snapshot", async () => {
  vi.stubGlobal("Worker", TestWorker);
  const { highlightDiffInBackground } = await import("./diffHighlight");
  const model = file("a.ts");
  const controller = new AbortController();
  const result = highlightDiffInBackground(model, controller.signal);
  const worker = TestWorker.instances[0];
  worker.finish();
  const tokens = await result;
  const line = model.blocks[0].lines[0];
  expect(tokens?.get(line)?.[0]).toEqual({ text: line.text, color: "#abcdef" });
  expect(await highlightDiffInBackground({ ...model }, controller.signal)).toBe(
    tokens,
  );
  expect(worker.sent).toHaveLength(1);
});

it("drops abandoned queued parses without interrupting the active visible file", async () => {
  vi.stubGlobal("Worker", TestWorker);
  const { highlightDiffInBackground } = await import("./diffHighlight");
  const first = highlightDiffInBackground(
    file("first.ts"),
    new AbortController().signal,
  );
  const obsolete = new AbortController();
  const queued = highlightDiffInBackground(file("old.ts"), obsolete.signal);
  const next = highlightDiffInBackground(
    file("next.ts"),
    new AbortController().signal,
  );
  obsolete.abort();
  expect(await queued).toBeNull();
  const worker = TestWorker.instances[0];
  expect(worker.terminate).not.toHaveBeenCalled();
  worker.finish();
  await first;
  expect(worker.sent.map((job) => job.file.path)).toEqual([
    "first.ts",
    "next.ts",
  ]);
  worker.finish();
  expect(await next).toBeInstanceOf(Map);
});

it("falls back to plain text on worker failure and recovers without stale events", async () => {
  vi.stubGlobal("Worker", TestWorker);
  const { highlightDiffInBackground } = await import("./diffHighlight");
  const first = highlightDiffInBackground(
    file("first.ts"),
    new AbortController().signal,
  );
  const previous = TestWorker.instances[0];
  previous.onerror?.();
  expect(await first).toBeNull();
  const next = highlightDiffInBackground(
    file("next.ts"),
    new AbortController().signal,
  );
  const worker = TestWorker.instances[1];
  previous.onerror?.();
  expect(worker.terminate).not.toHaveBeenCalled();
  worker.finish();
  expect(await next).toBeInstanceOf(Map);
});
