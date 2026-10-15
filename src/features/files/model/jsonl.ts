import { basename } from "../../../platform/tauri/fs";

/** Records past this stay in Source, so a huge log still opens instantly. */
export const JSONL_RECORD_LIMIT = 1000;

const SUMMARY_CHARS = 160;
const SUMMARY_STRING_CHARS = 60;

export type JsonlRecord =
  | { line: number; text: string; value: unknown; summary: string }
  | { line: number; text: string; error: string };

export function isJsonlPath(path: string): boolean {
  const name = basename(path).toLowerCase();
  return name.endsWith(".jsonl") || name.endsWith(".ndjson");
}

/**
 * One record per non-blank line, keeping the file's line numbers so a record
 * can be found in Source. A line that fails to parse stays in the list with
 * its error instead of disappearing. Only the first `limit` records are
 * parsed; the rest are counted.
 */
export function parseJsonl(
  text: string,
  limit = JSONL_RECORD_LIMIT,
): { records: JsonlRecord[]; total: number } {
  const records: JsonlRecord[] = [];
  let total = 0;
  let line = 0;
  let start = 0;
  while (start <= text.length) {
    const newline = text.indexOf("\n", start);
    const end = newline < 0 ? text.length : newline;
    line += 1;
    const raw = text.slice(start, end).trim();
    start = end + 1;
    if (!raw) continue;
    total += 1;
    if (records.length >= limit) continue;
    try {
      const value: unknown = JSON.parse(raw);
      records.push({ line, text: raw, value, summary: summarizeJson(value) });
    } catch (error) {
      records.push({
        line,
        text: raw,
        error: error instanceof Error ? error.message : String(error),
      });
    }
  }
  return { records, total };
}

/**
 * A one-line digest of a record: an object's leading `key: value` pairs or an
 * array's leading items, with nested containers reduced to their size.
 */
export function summarizeJson(value: unknown): string {
  if (Array.isArray(value)) {
    return `[${joinWithinBudget(value.map(shallow))}]`;
  }
  if (isObject(value)) {
    const entries = Object.entries(value);
    if (entries.length === 0) return "{}";
    return joinWithinBudget(
      entries.map(([key, item]) => `${key}: ${shallow(item)}`),
    );
  }
  return shallow(value);
}

function shallow(value: unknown): string {
  if (Array.isArray(value)) {
    return value.length === 0 ? "[]" : `[${count(value.length, "item")}]`;
  }
  if (isObject(value)) {
    const size = Object.keys(value).length;
    return size === 0 ? "{}" : `{${count(size, "key")}}`;
  }
  if (typeof value === "string") {
    return JSON.stringify(
      value.length > SUMMARY_STRING_CHARS
        ? `${value.slice(0, SUMMARY_STRING_CHARS)}…`
        : value,
    );
  }
  return String(value);
}

function joinWithinBudget(parts: string[]): string {
  let summary = "";
  for (const [index, part] of parts.entries()) {
    if (index > 0 && summary.length + part.length > SUMMARY_CHARS) {
      return `${summary}, …`;
    }
    summary += index > 0 ? `, ${part}` : part;
  }
  return summary;
}

function count(size: number, noun: string): string {
  return `${size} ${noun}${size === 1 ? "" : "s"}`;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}
