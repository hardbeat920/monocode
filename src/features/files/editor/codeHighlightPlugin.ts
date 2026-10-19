import type {
  CodeHighlighterPlugin,
  HighlightOptions,
  HighlightResult,
  ThemeInput,
} from "@streamdown/code";
import { bundledLanguagesInfo } from "shiki/langs";
import type { BundledLanguage } from "shiki";
export {
  createBoundedCodePlugin,
  type BoundedCodePlugin,
} from "./codeHighlightEngine";

type Callback = (result: HighlightResult) => void;
type Job = {
  id: number;
  key: string;
  options: HighlightOptions;
  callbacks: Set<Callback>;
  priority: number;
};
const languages = new Set(bundledLanguagesInfo.map((language) => language.id));
const aliases = new Map(
  bundledLanguagesInfo.flatMap((language) =>
    (language.aliases ?? []).map((alias) => [alias, language.id] as const),
  ),
);
const normalize = (language: string) =>
  aliases.get(language.trim().toLowerCase()) ?? language.trim().toLowerCase();
const MAX_CHARS = 500_000;
const MAX_RESULTS = 100;
const THEMES: [ThemeInput, ThemeInput] = ["light-plus", "dark-plus"];
const results = new Map<string, HighlightResult>();
const jobs = new Map<string, Job>();
let chars = 0;
let sequence = 0;
let active: Job | undefined;
let worker: Worker | undefined;
let cancellationScheduled = false;

function remember(key: string, result: HighlightResult) {
  if (key.length > MAX_CHARS) return;
  if (results.delete(key)) chars -= key.length;
  results.set(key, result);
  chars += key.length;
  while (results.size > MAX_RESULTS || chars > MAX_CHARS) {
    const oldest = results.keys().next().value!;
    chars -= oldest.length;
    results.delete(oldest);
  }
}

function pump() {
  if (active || !jobs.size || typeof Worker === "undefined") return;
  const next = [...jobs.values()]
    .filter((job) => job.callbacks.size)
    .sort((a, b) => a.priority - b.priority || a.id - b.id)[0];
  if (!next) return;
  try {
    if (!worker) {
      worker = new Worker(
        new URL("./codeHighlight.worker.ts", import.meta.url),
        { type: "module" },
      );
      worker.onmessage = ({
        data,
      }: MessageEvent<{
        id: number;
        result?: HighlightResult;
        error?: string;
      }>) => {
        const job = active;
        if (!job || data.id !== job.id) return;
        active = undefined;
        jobs.delete(job.key);
        if (data.result && job.callbacks.size) {
          remember(job.key, data.result);
          for (const callback of job.callbacks) callback(data.result);
        }
        pump();
      };
      const currentWorker = worker;
      worker.onerror = () => {
        if (worker !== currentWorker) return;
        worker?.terminate();
        worker = undefined;
        active = undefined;
        jobs.clear();
      };
    }
    active = next;
    worker.postMessage({ id: next.id, options: next.options });
  } catch {
    worker?.terminate();
    worker = undefined;
    active = undefined;
    jobs.clear();
  }
}

function keyFor(options: HighlightOptions) {
  return `${normalize(options.language)}\0${JSON.stringify(options.themes)}\0${options.code}`;
}

function cancelAbandonedWork() {
  if (cancellationScheduled || !active || active.callbacks.size) return;
  cancellationScheduled = true;
  // React removes the old subscription before subscribing to a growing fence.
  // Let that replacement arrive before deciding whether to discard the worker's
  // grammar checkpoint; otherwise every streamed chunk restarts the tokenizer.
  queueMicrotask(() => {
    cancellationScheduled = false;
    const abandoned = active;
    if (!abandoned || abandoned.callbacks.size) return;
    const continued = [...jobs.values()].some(
      (job) =>
        job !== abandoned &&
        job.callbacks.size > 0 &&
        normalize(job.options.language) ===
          normalize(abandoned.options.language) &&
        JSON.stringify(job.options.themes) ===
          JSON.stringify(abandoned.options.themes) &&
        job.options.code.startsWith(abandoned.options.code),
    );
    if (continued) return;
    worker?.terminate();
    worker = undefined;
    active = undefined;
    jobs.delete(abandoned.key);
    pump();
  });
}

/** Cancel obsolete fences; at most one tokenizer runs, off the UI thread. */
export function requestCodeHighlight(
  options: HighlightOptions,
  callback: Callback,
  priority = 0,
): () => void {
  const key = keyFor(options);
  const cached = results.get(key);
  if (cached) {
    results.delete(key);
    results.set(key, cached);
    callback(cached);
    return () => {};
  }
  if (typeof Worker === "undefined") return () => {};
  let job = jobs.get(key);
  if (!job) {
    job = { id: ++sequence, key, options, callbacks: new Set(), priority };
    jobs.set(key, job);
  }
  job.priority = Math.min(job.priority, priority);
  job.callbacks.add(callback);
  pump();
  return () => {
    job.callbacks.delete(callback);
    if (!job.callbacks.size && active !== job) jobs.delete(key);
    cancelAbandonedWork();
  };
}

export const boundedCode: CodeHighlighterPlugin = {
  name: "shiki",
  type: "code-highlighter",
  supportsLanguage: (language) => languages.has(normalize(language)),
  getSupportedLanguages: () => [...languages] as BundledLanguage[],
  getThemes: () => THEMES,
  highlight(options, callback) {
    const cached = results.get(keyFor(options));
    if (cached) return cached;
    if (callback) requestCodeHighlight(options, callback);
    return null;
  },
};
