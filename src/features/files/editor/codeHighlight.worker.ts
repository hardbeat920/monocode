/// <reference lib="webworker" />
import type { HighlightOptions, HighlightResult } from "@streamdown/code";
import { createBoundedCodePlugin } from "./codeHighlightEngine";

let rejectJob: ((error: unknown) => void) | undefined;
const engine = createBoundedCodePlugin({
  maxEntries: 1,
  onError: (error) => rejectJob?.(error),
});
self.onmessage = async ({
  data,
}: MessageEvent<{ id: number; options: HighlightOptions }>) => {
  try {
    const result = await new Promise<HighlightResult>((resolve, reject) => {
      rejectJob = reject;
      const cached = engine.highlight(data.options, resolve);
      if (cached) resolve(cached);
    });
    // GrammarState contains methods and belongs to the worker's checkpoint.
    // Only presentation tokens cross the structured-clone boundary.
    const { grammarState: _state, ...presentation } = result;
    self.postMessage({ id: data.id, result: presentation });
  } catch (error) {
    self.postMessage({ id: data.id, error: String(error) });
  } finally {
    rejectJob = undefined;
  }
};
