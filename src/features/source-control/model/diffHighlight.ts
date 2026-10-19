import {
  highlightDiffFile,
  type SyntaxToken,
} from "../../files/editor/syntaxTokens";
import type { UnifiedBlock, UnifiedLine } from "./unifiedDiff";

type HighlightFile = { path: string; blocks: UnifiedBlock[] };
export type HighlightRequest = {
  id: number;
  file: HighlightFile;
};
type Job = HighlightRequest & {
  signal: AbortSignal;
  resolve: (tokens: Map<UnifiedLine, SyntaxToken[]> | null) => void;
  cleanup: () => void;
};

// Token colours are `--syntax-*` variables, so one result serves both schemes.
const cache = new WeakMap<
  UnifiedBlock[],
  Map<string, Map<UnifiedLine, SyntaxToken[]>>
>();
const queued: Job[] = [];
let worker: Worker | undefined;
let active: Job | undefined;
let sequence = 0;

export function cachedDiffHighlight(file: HighlightFile) {
  return cache.get(file.blocks)?.get(file.path);
}

/** Decorative parsing is serialized off the UI thread. Abandoned queued files
 * are dropped; an already-running parse may finish and populate the cache. */
export function highlightDiffInBackground(
  file: HighlightFile,
  signal: AbortSignal,
): Promise<Map<UnifiedLine, SyntaxToken[]> | null> {
  const cached = cachedDiffHighlight(file);
  if (cached) return Promise.resolve(cached);
  if (signal.aborted) return Promise.resolve(null);
  if (typeof Worker === "undefined") return highlightDiffFile(file);
  return new Promise((resolve) => {
    const job: Job = {
      id: ++sequence,
      file,
      signal,
      resolve,
      cleanup: () => signal.removeEventListener("abort", cancel),
    };
    const cancel = () => {
      const index = queued.indexOf(job);
      if (index !== -1) {
        queued.splice(index, 1);
        job.cleanup();
      }
      resolve(null);
    };
    signal.addEventListener("abort", cancel, { once: true });
    queued.push(job);
    pump();
  });
}

function pump() {
  if (active || queued.length === 0) return;
  active = queued.shift()!;
  try {
    if (!worker) {
      worker = new Worker(
        new URL("./diffHighlight.worker.ts", import.meta.url),
        { type: "module" },
      );
      worker.onmessage = ({
        data,
      }: MessageEvent<{ id: number; tokens: SyntaxToken[][] }>) => {
        const job = active;
        if (!job || job.id !== data.id) return;
        const tokens = new Map<UnifiedLine, SyntaxToken[]>();
        let index = 0;
        for (const block of job.file.blocks)
          for (const line of block.lines) {
            const pieces = data.tokens[index++];
            if (pieces?.length) tokens.set(line, pieces);
          }
        let byPath = cache.get(job.file.blocks);
        if (!byPath) cache.set(job.file.blocks, (byPath = new Map()));
        byPath.set(job.file.path, tokens);
        active = undefined;
        job.cleanup();
        job.resolve(tokens);
        pump();
      };
      const currentWorker = worker;
      worker.onerror = () => {
        if (worker === currentWorker) fail();
      };
    }
    worker.postMessage({
      id: active.id,
      file: active.file,
    });
  } catch {
    fail();
  }
}

function fail() {
  worker?.terminate();
  worker = undefined;
  const jobs = [...(active ? [active] : []), ...queued.splice(0)];
  active = undefined;
  for (const job of jobs) {
    job.cleanup();
    job.resolve(null);
  }
}
