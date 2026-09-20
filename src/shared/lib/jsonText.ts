export function limitSection(value: string, maxChars: number): string {
  if (value.length <= maxChars) return value;
  return `${value.slice(0, maxChars)}\n\n[truncated]`;
}

export function extractJsonObject(raw: string): string | null {
  const trimmed = raw.trim();
  const start = trimmed.indexOf("{");
  if (start < 0) return null;

  let depth = 0;
  let inString = false;
  let escaping = false;
  for (let index = start; index < trimmed.length; index += 1) {
    const char = trimmed[index];
    if (inString) {
      if (escaping) escaping = false;
      else if (char === "\\") escaping = true;
      else if (char === '"') inString = false;
      continue;
    }
    if (char === '"') {
      inString = true;
      continue;
    }
    if (char === "{") {
      depth += 1;
      continue;
    }
    if (char === "}") {
      depth -= 1;
      if (depth === 0) return trimmed.slice(start, index + 1);
    }
  }
  return null;
}

const GIT_TEXT_KEYS = ["subject", "title", "message", "body", "branch"] as const;

export function parseJsonObject(raw: string): Record<string, unknown> | null {
  const trimmed = raw.trim();
  let searchFrom = 0;
  let fallback: Record<string, unknown> | null = null;
  while (searchFrom < trimmed.length) {
    const start = trimmed.indexOf("{", searchFrom);
    if (start < 0) break;
    const json = extractJsonObject(trimmed.slice(start));
    if (!json) break;
    searchFrom = start + 1;
    try {
      const parsed: unknown = JSON.parse(json);
      if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
        continue;
      }
      const rec = parsed as Record<string, unknown>;
      if (GIT_TEXT_KEYS.some((key) => typeof rec[key] === "string")) return rec;
      fallback ??= rec;
    } catch {
      // Models often mention `{` in preamble; keep scanning for real JSON.
    }
  }
  return fallback;
}

export function stringField(rec: Record<string, unknown>, key: string): string {
  const value = rec[key];
  return typeof value === "string" ? value : "";
}
