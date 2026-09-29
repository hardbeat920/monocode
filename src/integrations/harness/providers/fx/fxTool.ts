import type { ToolPreview } from "../../../../features/sessions/model/session";

/**
 * fx tool metadata recovery.
 *
 * Unlike every other ACP harness we speak to, fx sends no `rawInput`, no
 * `locations` and no `diff` content on `tool_call` / `tool_call_update`. All we
 * get is a gerund title ("Reading"), a kind, a status, and — once the call
 * completes — a free-text result blob. Everything the transcript needs (path,
 * query, command) has to be mined back out of that text.
 *
 * The shapes below are taken from `fx acp` 0.0.5 wire captures.
 */

export type FxToolInfo = {
  title?: string;
  /** Overrides fx's reported kind when it mislabels the call. */
  kind?: string;
  preview?: ToolPreview;
  detail?: string;
  /**
   * True once we have understood the call. The generic ACP extraction must not
   * run afterwards: on a shell call it happily mistakes `command_result.cwd`
   * for a read target and relabels `echo hi` as "Read /tmp/project".
   */
  resolved: boolean;
};

/** fx labels calls with a gerund; the transcript wants the plain verb. */
const VERBS: Record<string, string> = {
  listing: "List",
  reading: "Read",
  writing: "Write",
  editing: "Edit",
  searching: "Find",
  running: "Run",
  fetching: "Fetch",
  thinking: "Think",
  deleting: "Delete",
  moving: "Move",
};

export function fxToolVerb(title: string | undefined): string | undefined {
  const key = title?.trim().toLowerCase();
  return key ? VERBS[key] : undefined;
}

export function fxToolInfo(
  update: Record<string, unknown>,
  tool: Record<string, unknown>,
): FxToolInfo {
  const rawTitle = stringField(update, "title") ?? stringField(tool, "title");
  const verb = fxToolVerb(rawTitle);
  const kind = (
    stringField(update, "kind") ??
    stringField(tool, "kind") ??
    ""
  ).toLowerCase();
  const text = resultText(update, tool);

  const command = commandResult(update, tool);
  if (command) {
    const detail = [shellOutput(text) ?? text, command.output]
      .filter((part): part is string => !!part?.trim())
      .join("\n");
    return {
      title: command.command || verb || rawTitle,
      detail: detail || undefined,
      resolved: true,
    };
  }

  if (kind === "execute") {
    const shell = shellOutput(text);
    return {
      title: verb ?? rawTitle,
      detail: shell ?? text ?? undefined,
      resolved: true,
    };
  }

  const search = grepResult(text);
  if (search) {
    return {
      title: verb ?? rawTitle,
      preview: { kind: "search", query: search.query },
      detail: search.body,
      resolved: true,
    };
  }

  const read = readResult(text);
  if (read) {
    return {
      title: verb ?? rawTitle,
      preview: {
        kind: "read",
        path: read.path,
        fileName: basename(read.path),
        startLine: read.startLine,
      },
      detail: read.body,
      resolved: true,
    };
  }

  const listing = listingResult(text);
  if (listing) {
    // fx reports a directory listing as kind "read", which the shared title
    // builder renders as "Read .". Report it as a plain call with no preview:
    // the transcript only builds a file chip for "Read"/"Find" labels anyway,
    // so a read-shaped preview would buy nothing but the wrong label.
    return {
      title: `List ${listing.path}`,
      kind: "other",
      detail: text ?? undefined,
      resolved: true,
    };
  }

  const wrote = writeResult(text);
  if (wrote) {
    const name = basename(wrote.path);
    return {
      title: `${wrote.verb} ${name}`,
      preview: { kind: "write", path: wrote.path, fileName: name },
      detail: text ?? undefined,
      resolved: true,
    };
  }

  return {
    title: verb ?? rawTitle,
    detail: text ?? undefined,
    resolved: false,
  };
}

/** `exit_code=0\n<stdout>\nhi\n</stdout>\n` */
function shellOutput(text: string | undefined): string | undefined {
  if (!text) return undefined;
  const out = section(text, "stdout");
  const err = section(text, "stderr");
  const joined = [out, err].filter((part) => part?.trim()).join("\n");
  return joined.trim() ? joined.trim() : undefined;
}

/** fx extends ACP with a `command_result` object on shell calls. */
function commandResult(
  update: Record<string, unknown>,
  tool: Record<string, unknown>,
): { command: string; output?: string } | null {
  const rec =
    asRecord(update.command_result) ??
    asRecord(update.commandResult) ??
    asRecord(tool.command_result) ??
    asRecord(tool.commandResult);
  if (!rec) return null;
  const command = stringField(rec, "command");
  if (!command) return null;
  const code = rec.exit_code ?? rec.exitCode;
  const suffix = typeof code === "number" && code !== 0 ? `\nexit ${code}` : "";
  return { command, output: suffix ? suffix.trim() : undefined };
}

/** `[grep] 2 matches for export\n - app.ts:1: …` */
function grepResult(
  text: string | undefined,
): { query: string; body?: string } | null {
  if (!text) return null;
  const match = /^\[(?:grep|search|glob)\]\s+.*?\bfor\s+(.+?)\s*$/m.exec(text);
  if (!match) return null;
  const query = match[1].trim();
  if (!query) return null;
  const body = text.slice(match.index + match[0].length).trim();
  return { query, body: body || undefined };
}

/** `<path>notes.txt</path>\n<content>\n1\thello\n</content>` */
function readResult(
  text: string | undefined,
): { path: string; startLine?: number; body?: string } | null {
  if (!text) return null;
  const path = section(text, "path")?.trim();
  if (!path) return null;
  const body = section(text, "content");
  const first = body ? /^\s*(\d+)\t/.exec(body) : null;
  return {
    path,
    startLine: first ? Number(first[1]) : undefined,
    body: body?.trim() || undefined,
  };
}

/** `.:\n- notes.txt\n` — a directory listing keyed by its header line. */
function listingResult(text: string | undefined): { path: string } | null {
  if (!text) return null;
  const match = /^(\S.*?):\s*$/m.exec(text);
  if (!match || match.index !== 0) return null;
  if (!/^\s*[-*]\s+\S/m.test(text)) return null;
  return { path: match[1].trim() };
}

/** `wrote out.txt (4 bytes)` / `edited app.ts (41 bytes)` */
function writeResult(
  text: string | undefined,
): { verb: string; path: string } | null {
  if (!text) return null;
  const match =
    /^\s*(wrote|edited|created|updated|deleted|moved|removed)\s+(\S+?)(?:\s+\(|\s*$)/i.exec(
      text,
    );
  if (!match) return null;
  const verb = match[1].toLowerCase();
  const label =
    verb === "wrote" || verb === "created"
      ? "Write"
      : verb === "deleted" || verb === "removed"
        ? "Delete"
        : verb === "moved"
          ? "Move"
          : "Edit";
  return { verb: label, path: match[2] };
}

/** fx nests result text as `content[].content.text`. */
function resultText(
  update: Record<string, unknown>,
  tool: Record<string, unknown>,
): string | undefined {
  const raw = update.content ?? tool.content;
  const text = flattenText(raw);
  return text.trim() ? text : undefined;
}

function flattenText(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) {
    return value.map(flattenText).filter(Boolean).join("\n");
  }
  const rec = asRecord(value);
  if (!rec) return "";
  if (typeof rec.text === "string") return rec.text;
  if (rec.content != null) return flattenText(rec.content);
  return "";
}

function section(text: string, tag: string): string | undefined {
  const match = new RegExp(`<${tag}>\\n?([\\s\\S]*?)\\n?</${tag}>`).exec(text);
  return match ? match[1] : undefined;
}

function basename(path: string): string {
  const trimmed = path.replace(/[/\\]+$/, "") || path;
  const parts = trimmed.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] ?? trimmed;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

function stringField(
  rec: Record<string, unknown>,
  key: string,
): string | undefined {
  const value = rec[key];
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}
