import type { ToolPreview, ToolPreviewKind, ToolPreviewLine } from "../session";
import { displayPath } from "../paths";
import {
  formatShellIntent,
  inferShellIntent,
  rewriteReadableTitle,
} from "./shellIntent";

export const MAX_PREVIEW_LINES = 6;
export const MAX_LINE_CHARS = 120;

export function extractToolPreview(
  update: Record<string, unknown>,
  tool: Record<string, unknown>,
): ToolPreview | undefined {
  const title =
    coerceString(update.title) ??
    coerceString(tool.title) ??
    coerceString(update.name) ??
    coerceString(tool.name);
  const rawKind = (
    coerceString(update.kind) ??
    coerceString(tool.kind) ??
    ""
  ).toLowerCase();
  const inputs = inputRecords(
    update.rawInput ?? update.raw_input ?? update.input,
    tool.rawInput ?? tool.raw_input ?? tool.input,
    update.arguments ?? update.args ?? update.params,
    tool.arguments ?? tool.args ?? tool.params,
    update,
    tool,
  );
  const content = update.content ?? tool.content;
  const diff = extractDiff(content);
  const path = extractPath(update, tool, inputs, diff?.path);
  const startLine =
    locationLine(update.locations ?? update.location) ??
    locationLine(tool.locations ?? tool.location) ??
    firstNumber(inputs, "line") ??
    firstNumber(inputs, "offset");
  const kind = previewKind(rawKind, title, !!diff, !!path);
  const query = extractSearchQuery(inputs);

  if (
    kind === "search" ||
    (query &&
      kind !== "write" &&
      rawKind !== "read" &&
      !/^read\b/i.test(title ?? ""))
  ) {
    return {
      kind: "search",
      title,
      query,
    };
  }

  if (kind === "shell") return shellPreview(title, update, tool);

  const fileName = path ? basename(path) : undefined;

  if (kind === "write" && diff) {
    if (diff.lines?.length) {
      return {
        kind: "write",
        title,
        path,
        fileName,
        startLine,
        additions: diff.additions,
        deletions: diff.deletions,
        lines: diff.lines,
      };
    }
    if (diff.oldText != null || diff.newText) {
      const built = compactDiff(diff.oldText, diff.newText ?? "");
      return {
        kind: "write",
        title,
        path,
        fileName,
        startLine,
        ...built,
      };
    }
  }

  if (!path && kind !== "read" && kind !== "write") return undefined;

  return {
    kind,
    title,
    path,
    fileName,
    startLine,
  };
}

function shellPreview(
  title: string | undefined,
  update: Record<string, unknown>,
  tool: Record<string, unknown>,
): ToolPreview | undefined {
  const command = extractShellCommand(update, tool);
  const inferred = command ? inferShellIntent(command) : undefined;
  if (!inferred) return undefined;
  const inferredPath = inferred.path ? normalizePath(inferred.path) : undefined;
  if (inferred.verb === "Find" && inferred.query) {
    return {
      kind: "shell",
      title,
      path: inferredPath,
      fileName: inferredPath ? basename(inferredPath) : undefined,
      query: inferred.query,
    };
  }
  if (!inferredPath) return undefined;
  return {
    kind: "shell",
    title,
    path: inferredPath,
    fileName: basename(inferredPath),
    startLine: inferred.startLine,
  };
}

export function isEditTool(
  kind?: string,
  title?: string,
  preview?: ToolPreview,
): boolean {
  if (preview?.kind === "write") return true;
  const key = kind?.trim().toLowerCase() ?? "";
  if (["edit", "write", "delete", "move"].includes(key)) return true;
  if (key && key !== "other") return false;
  return /^(edit|write|delete|update)\b/i.test(title?.trim() ?? "");
}

export function isReadTool(
  kind?: string,
  title?: string,
  preview?: ToolPreview,
): boolean {
  if (preview?.kind === "read") return true;
  const key = kind?.trim().toLowerCase() ?? "";
  if (key === "read") return true;
  if (key && key !== "other") return false;
  return /^read\b/i.test(title?.trim() ?? "");
}

export function isSearchTool(
  kind?: string,
  title?: string,
  preview?: ToolPreview,
): boolean {
  if (preview?.kind === "search") return true;
  const key = kind?.trim().toLowerCase() ?? "";
  if (key === "search") return true;
  if (key && key !== "other") return false;
  return /^(find|search|grep|glob)\b/i.test(title?.trim() ?? "");
}

export function isFileTool(
  kind?: string,
  title?: string,
  preview?: ToolPreview,
): boolean {
  return (
    isReadTool(kind, title, preview) || isEditTool(kind, title, preview)
  );
}

export function isExecuteTool(kind?: string, title?: string): boolean {
  const key = kind?.trim().toLowerCase() ?? "";
  if (key === "execute" || key === "shell" || key === "bash") return true;
  if (key && key !== "other") return false;
  return /^(bash|shell|run(?:ning)?(?:\s+command)?)\b/i.test(
    title?.trim() ?? "",
  );
}

/** The argv / script a shell tool is about to run, if the harness sent it. */
export function extractShellCommand(...values: unknown[]): string | undefined {
  for (const raw of inputRecords(...values)) {
    for (const key of ["command", "cmd", "script"]) {
      const found = commandField(raw[key]);
      if (found) return found;
    }
  }
  return undefined;
}

/** The skill a Skill tool is invoking, if the harness sent it. */
export function extractSkillName(...values: unknown[]): string | undefined {
  for (const raw of inputRecords(...values)) {
    for (const key of ["skill", "skill_name", "skillName", "skill_id", "skillId"]) {
      const found = skillNameField(raw[key]);
      if (found) return found;
    }
    const named = skillNameField(raw.name);
    if (named && looksLikeSkillName(named)) return named;
  }
  return undefined;
}

export function isSkillTool(kind?: string, title?: string): boolean {
  const key = kind?.trim().toLowerCase() ?? "";
  if (key === "skill" || key === "skills") return true;
  if (key && key !== "other") return false;
  return /^skill\b/i.test(title?.trim() ?? "");
}

/** Claude Code's Agent tool (named Task before v2.1.63), plus Codex subagents. */
export function isAgentToolName(name: string): boolean {
  const normalized = name.trim().toLowerCase();
  return (
    normalized === "agent" ||
    normalized === "task" ||
    normalized === "subagent" ||
    normalized === "taskcreate"
  );
}

export function isAgentTool(kind?: string, title?: string): boolean {
  const key = kind?.trim().toLowerCase() ?? "";
  if (key === "agent" || key === "task" || key === "subagent") return true;
  if (key && key !== "other") return false;
  return /^(agent|task|subagent)\b/i.test(title?.trim() ?? "");
}

/** Human label for a spawned subagent: the agent's description, else its type. */
export function agentToolTitle(
  input: Record<string, unknown>,
  fallback = "Subagent",
): string {
  const description = coerceString(input.description)?.trim();
  if (description) return description;
  const type =
    coerceString(input.subagent_type) ??
    coerceString(input.subagentType) ??
    coerceString(input.agent_type) ??
    coerceString(input.agentType);
  if (type) {
    const label = formatAgentType(type);
    return /subagent/i.test(label) ? label : `${label} subagent`;
  }
  if (
    fallback &&
    !isAgentToolName(fallback) &&
    !isWeakToolTitle(fallback)
  ) {
    return fallback;
  }
  return "Subagent";
}

/**
 * What an Agent call's input says about the subagent it spawns: its type, the
 * brief it was handed, and whether it was sent to the background.
 */
export function subagentMetaFromInput(
  value: unknown,
): { agentType?: string; prompt?: string; background?: boolean } | undefined {
  const input = asRecord(value);
  if (!input) return undefined;
  const agentType =
    coerceString(input.subagent_type) ??
    coerceString(input.subagentType) ??
    coerceString(input.agent_type) ??
    coerceString(input.agentType);
  const prompt =
    typeof input.prompt === "string" && input.prompt.trim()
      ? input.prompt.trim()
      : undefined;
  const background =
    input.run_in_background === true || input.runInBackground === true;
  if (!agentType && !prompt && !background) return undefined;
  return {
    ...(agentType ? { agentType } : {}),
    ...(prompt ? { prompt } : {}),
    ...(background ? { background } : {}),
  };
}

export function formatAgentType(value: string): string {
  const text = value.trim().replace(/[_-]+/g, " ");
  if (!text) return "Subagent";
  return text.replace(/\b[a-z]/g, (char) => char.toUpperCase());
}

export function extractSearchQuery(value: unknown): string | undefined {
  const keys = [
    "pattern",
    "query",
    "glob",
    "glob_pattern",
    "globPattern",
    "search_term",
    "searchTerm",
    "regex",
  ];
  for (const raw of inputRecords(value)) {
    for (const key of keys) {
      const found = coerceString(raw[key]);
      if (found) return found;
    }
  }
  return undefined;
}

/** Shared title builder used by Claude, Pi, and OpenCode tool mappers. */
export function titleFromToolInput(
  name: string,
  kind: string,
  input: Record<string, unknown>,
): string {
  if (isAgentToolName(name) || isAgentTool(kind)) {
    return agentToolTitle(input, name);
  }
  const preview = extractToolPreview(
    { title: name, name, kind, rawInput: input, input },
    { title: name, name, kind, rawInput: input },
  );
  return (
    composeToolTitle({
      kind,
      title: name,
      command: extractShellCommand(input),
      skill: extractSkillName(input),
      path: preview?.path,
      query: preview?.query,
      previewKind: preview?.kind,
    }) || name
  );
}

export function composeToolTitle(opts: {
  kind?: string;
  title?: string;
  path?: string;
  query?: string;
  command?: string;
  skill?: string;
  previewKind?: ToolPreviewKind;
  cwd?: string;
}): string {
  const kind = opts.kind?.trim().toLowerCase() ?? "";
  const title = opts.title?.trim() ?? "";
  const path = opts.path?.trim();
  const query = opts.query?.trim();
  const previewKind = opts.previewKind;
  const command = firstLine(opts.command);
  const skill = formatSkillName(opts.skill);

  if (isAgentTool(kind, title)) {
    const rest = title.replace(/^(agent|task|subagent)\b\s*/i, "").trim();
    if (rest && !isWeakToolTitle(rest)) return rest;
    return "Subagent";
  }

  if (
    isSkillTool(kind, title) ||
    (skill &&
      !isExecuteTool(kind, title) &&
      !isReadTool(kind, title) &&
      !isSearchTool(kind, title) &&
      !isEditTool(kind, title, undefined))
  ) {
    if (skill) return `Skill ${skill}`;
    const rest = title.replace(/^skill\b\s*/i, "").trim();
    if (rest && !isWeakToolTitle(rest)) {
      return `Skill ${formatSkillName(rest) || rest}`;
    }
    return "Skill";
  }

  // Shell rows are one line. The command has to live in the title
  // itself: Codex already does this, Claude/Pi used to label the row "Bash"
  // and hide the argv in detail the activity stack never shows.
  if (previewKind === "shell" || isExecuteTool(kind, title)) {
    const rewritten = rewriteReadableTitle(title, path, query);
    if (rewritten) return rewritten;
    const script = command || stripExecutePrefix(title);
    const inferred = inferShellIntent(script);
    if (inferred) {
      const inferredPath =
        path ||
        (inferred.path
          ? displayPath(inferred.path, opts.cwd)
          : undefined);
      const readable = formatShellIntent(inferred, inferredPath, query);
      if (readable) return readable;
    }
    if (command) return command;
    const rest = stripExecutePrefix(title);
    if (rest && !isWeakToolTitle(rest)) return rest;
    if (title && !isWeakToolTitle(title)) return title;
    return "Shell";
  }

  // A title that already names a different action is finished; re-prefixing it
  // with "Read" produces nonsense like "Read List ." when only a stale
  // read-shaped preview is pulling us into the branch below.
  if (!path && /^(list|write|edit|run|delete|move|fetch)\s+\S/i.test(title)) {
    return title;
  }

  if (previewKind === "read" || isReadTool(kind, title)) {
    if (path) return `Read ${path}`;
    // \b keeps "Reading" from being sliced into "Read ing".
    const rest = title.replace(/^read(?:ing)?(?:\s+file)?\b\s*/i, "").trim();
    if (rest && !isWeakToolTitle(rest)) return `Read ${rest}`;
    return "Read";
  }

  if (previewKind === "search" || isSearchTool(kind, title)) {
    const q =
      query ||
      title.replace(/^(?:find|search|grep|glob)(?:ing)?\b\s*/i, "").trim();
    if (q && !isWeakToolTitle(q)) return `Find ${q}`;
    return "Find";
  }

  return title;
}

export function stubFilePreview(
  kind?: string,
  title?: string,
): ToolPreview {
  const inferred = previewKind(kind ?? "", title, false, false);
  return {
    kind: inferred === "write" ? "write" : "read",
    title,
  };
}

export function isWeakToolTitle(value: string): boolean {
  return /^(tool|shell|bash|execute|command|skill|read|edit|search|find|grep|glob|fetch|other|write|delete|move|think|run|list|working|reading|editing|searching|writing|running|listing|fetching|thinking|deleting|moving|mcp:\s*tool|read file|edit file|write file|run command|ran command|unnamed)$/i.test(
    value.trim(),
  );
}

export function contextLines(
  text: string,
  startLine?: number,
): ToolPreviewLine[] {
  const raw = text.replace(/\r\n/g, "\n").replace(/\s+$/, "").split("\n");
  const start = Math.max(1, startLine ?? 1);
  const from = Math.min(Math.max(0, start - 1), Math.max(0, raw.length - 1));
  return raw.slice(from, from + MAX_PREVIEW_LINES).map((line, index) => ({
    number: from + index + 1,
    kind: "context" as const,
    text: capLine(line),
  }));
}

export function mergeToolPreview(
  next?: ToolPreview,
  prev?: ToolPreview,
): ToolPreview | undefined {
  if (!next) return prev;
  if (!prev) return next;
  return {
    kind: next.kind || prev.kind,
    title: pickStrong(next.title, prev.title),
    path: next.path || prev.path,
    fileName: next.fileName || prev.fileName,
    startLine: next.startLine ?? prev.startLine,
    additions: next.additions ?? prev.additions,
    deletions: next.deletions ?? prev.deletions,
    query: next.query || prev.query,
    lines:
      next.kind === "read" || next.kind === "search"
        ? undefined
        : next.lines?.length
          ? next.lines
          : prev.kind === "read" || prev.kind === "search"
            ? undefined
            : prev.lines,
    output: next.output || prev.output,
  };
}

function pickStrong(
  next?: string,
  prev?: string,
): string | undefined {
  if (next && !isWeakToolTitle(next)) return next;
  if (prev && !isWeakToolTitle(prev)) return prev;
  return next || prev;
}

function previewKind(
  raw: string,
  title: string | undefined,
  hasDiff: boolean,
  hasPath: boolean,
): ToolPreviewKind {
  switch (raw) {
    case "read":
      return "read";
    case "search":
      return "search";
    case "edit":
    case "write":
    case "delete":
    case "move":
      return "write";
    case "execute":
    case "shell":
      return "shell";
    default:
      if (hasDiff) return "write";
      if (/^(edit|write|delete|update)\b/i.test(title ?? "")) return "write";
      if (/^(find|search|grep|glob)\b/i.test(title ?? "")) return "search";
      if (/^read\b/i.test(title ?? "") || hasPath) return "read";
      return "shell";
  }
}

function extractPath(
  update: Record<string, unknown>,
  tool: Record<string, unknown>,
  inputs: Record<string, unknown>[],
  diffPath?: string,
): string | undefined {
  return (
    locationPath(update.locations ?? update.location) ??
    locationPath(tool.locations ?? tool.location) ??
    firstInputPath(inputs) ??
    diffPath ??
    contentPath(update.content ?? tool.content) ??
    findPathInUnknown(update, 0) ??
    findPathInUnknown(tool, 0) ??
    pathFromTitle(
      coerceString(update.title) ?? coerceString(tool.title),
    )
  );
}

function firstInputPath(
  inputs: Record<string, unknown>[],
): string | undefined {
  for (const raw of inputs) {
    const found = inputPath(raw);
    if (found) return found;
  }
  return undefined;
}

function firstNumber(
  inputs: Record<string, unknown>[],
  key: string,
): number | undefined {
  for (const raw of inputs) {
    const found = numberField(raw, key);
    if (found) return found;
  }
  return undefined;
}

function inputPath(rawInput: Record<string, unknown>): string | undefined {
  const keys = [
    "path",
    "filePath",
    "file_path",
    "targetFile",
    "target_file",
    "relative_workspace_path",
    "relativeWorkspacePath",
    "uri",
    "file",
    "absolutePath",
  ];
  for (const key of keys) {
    const value = coerceString(rawInput[key]);
    if (value && looksLikeToolPath(value)) return normalizePath(value);
  }
  return undefined;
}

function pathFromTitle(title?: string): string | undefined {
  if (!title) return undefined;
  const match = title.match(
    /^(?:Read|Edit|Write|Delete|Update)\s+(?:file\s+)?(.+)$/i,
  );
  const rest = match?.[1]?.trim();
  if (!rest) return undefined;
  return looksLikeToolPath(rest) ? rest : undefined;
}

function locationPath(locations: unknown): string | undefined {
  const items = Array.isArray(locations)
    ? locations
    : locations
      ? [locations]
      : [];
  for (const item of items) {
    if (typeof item === "string" && looksLikeToolPath(item)) {
      return normalizePath(item);
    }
    const rec = asRecord(item);
    const path =
      rec &&
      (coerceString(rec.path) ??
        coerceString(rec.filePath) ??
        coerceString(rec.uri) ??
        coerceString(rec.file));
    if (path && looksLikeToolPath(path)) return normalizePath(path);
  }
  return undefined;
}

function locationLine(locations: unknown): number | undefined {
  const items = Array.isArray(locations)
    ? locations
    : locations
      ? [locations]
      : [];
  for (const item of items) {
    const rec = asRecord(item);
    const line = rec && numberField(rec, "line");
    if (line && line > 0) return line;
  }
  return undefined;
}

function contentPath(content: unknown): string | undefined {
  for (const block of contentBlocks(content)) {
    const path = coerceString(block.path);
    if (path && looksLikeToolPath(path)) return normalizePath(path);
    const change = firstChange(block);
    const changePath = change && coerceString(change.path);
    if (changePath && looksLikeToolPath(changePath)) return normalizePath(changePath);
  }
  return undefined;
}

function extractDiff(content: unknown): {
  path?: string;
  oldText?: string;
  newText?: string;
  lines?: ToolPreviewLine[];
  additions?: number;
  deletions?: number;
} | undefined {
  for (const block of contentBlocks(content)) {
    const type = (coerceString(block.type) ?? "").toLowerCase();
    if (type !== "diff") continue;
    const change = firstChange(block);
    const path =
      coerceString(block.path) ??
      (change ? coerceString(change.path) : undefined);
    const patch = asRecord(block.patch);
    const patchText = coerceString(patch?.text) ?? coerceString(block.patch);
    if (patchText && /^(diff --git|@@ )/.test(patchText.trim())) {
      const parsed = parseGitPatch(patchText);
      return {
        path: parsed.path ?? path ?? undefined,
        lines: parsed.lines,
        additions: parsed.additions,
        deletions: parsed.deletions,
      };
    }
    return {
      path: path ?? undefined,
      oldText: coerceString(block.oldText) ?? coerceString(block.old_text),
      newText: coerceString(block.newText) ?? coerceString(block.new_text),
    };
  }
  return undefined;
}

function firstChange(
  block: Record<string, unknown>,
): Record<string, unknown> | null {
  if (!Array.isArray(block.changes) || block.changes.length === 0) return null;
  return asRecord(block.changes[0]);
}

function contentBlocks(content: unknown): Record<string, unknown>[] {
  if (Array.isArray(content)) {
    return content.flatMap((item) => {
      const rec = asRecord(item);
      if (!rec) return [];
      const nested = asRecord(rec.content);
      return nested ? [nested, rec] : [rec];
    });
  }
  const rec = asRecord(content);
  if (!rec) return [];
  const nested = asRecord(rec.content);
  return nested ? [nested, rec] : [rec];
}

function parseGitPatch(text: string): {
  path?: string;
  lines: ToolPreviewLine[];
  additions: number;
  deletions: number;
} {
  const plus = text.match(/^\+\+\+\s+(?:b\/)?(.+)$/m);
  const git = text.match(/^diff --git\s+\S+\s+(\S+)/m);
  let path = (plus?.[1] ?? git?.[1])?.replace(/^b\//, "").trim();
  if (path === "/dev/null") path = undefined;

  const hunks: ToolPreviewLine[] = [];
  let additions = 0;
  let deletions = 0;
  let newNum = 0;
  for (const line of text.replace(/\r\n/g, "\n").split("\n")) {
    const header = line.match(/^@@\s+-(\d+)(?:,\d+)?\s+\+(\d+)/);
    if (header) {
      newNum = Number(header[2]);
      continue;
    }
    if (
      line.startsWith("diff ") ||
      line.startsWith("index ") ||
      line.startsWith("--- ") ||
      line.startsWith("+++ ")
    ) {
      continue;
    }
    if (line.startsWith("+")) {
      additions += 1;
      hunks.push({ number: newNum, kind: "add", text: line.slice(1) });
      newNum += 1;
    } else if (line.startsWith("-")) {
      deletions += 1;
      hunks.push({ number: newNum, kind: "del", text: line.slice(1) });
    } else if (line.startsWith("\\")) {
      continue;
    } else {
      const body = line.startsWith(" ") ? line.slice(1) : line;
      hunks.push({ number: newNum, kind: "context", text: body });
      newNum += 1;
    }
  }

  const first = hunks.findIndex((line) => line.kind !== "context");
  const start = Math.max(0, first === -1 ? 0 : first - 1);
  return {
    path: path && looksLikePath(path) ? normalizePath(path) : path,
    lines: hunks.slice(start, start + MAX_PREVIEW_LINES).map((line) => ({
      ...line,
      text: capLine(line.text),
    })),
    additions,
    deletions,
  };
}

function compactDiff(
  oldText: string | undefined,
  newText: string,
): { lines: ToolPreviewLine[]; additions: number; deletions: number } {
  const oldLines = (oldText ?? "").replace(/\r\n/g, "\n").split("\n");
  const newLines = newText.replace(/\r\n/g, "\n").split("\n");
  const cappedOld = oldLines.slice(0, 800);
  const cappedNew = newLines.slice(0, 800);
  const hunks =
    oldText == null || oldText === ""
      ? cappedNew.map((text, index) => ({
          number: index + 1,
          kind: "add" as const,
          text,
        }))
      : greedyDiff(cappedOld, cappedNew);

  const additions = hunks.filter((line) => line.kind === "add").length;
  const deletions = hunks.filter((line) => line.kind === "del").length;
  const first = hunks.findIndex((line) => line.kind !== "context");
  const start = Math.max(0, first === -1 ? 0 : first - 1);
  return {
    lines: hunks.slice(start, start + MAX_PREVIEW_LINES).map((line) => ({
      ...line,
      text: capLine(line.text),
    })),
    additions,
    deletions,
  };
}

function greedyDiff(oldLines: string[], newLines: string[]): ToolPreviewLine[] {
  const out: ToolPreviewLine[] = [];
  let i = 0;
  let j = 0;
  while (i < oldLines.length || j < newLines.length) {
    if (i < oldLines.length && j < newLines.length && oldLines[i] === newLines[j]) {
      out.push({ number: j + 1, kind: "context", text: oldLines[i] });
      i += 1;
      j += 1;
      continue;
    }
    const sync = findSync(oldLines, newLines, i, j, 8);
    if (sync) {
      while (i < sync.i) {
        out.push({ number: i + 1, kind: "del", text: oldLines[i] });
        i += 1;
      }
      while (j < sync.j) {
        out.push({ number: j + 1, kind: "add", text: newLines[j] });
        j += 1;
      }
      continue;
    }
    if (i < oldLines.length) {
      out.push({ number: i + 1, kind: "del", text: oldLines[i] });
      i += 1;
    } else {
      out.push({ number: j + 1, kind: "add", text: newLines[j] });
      j += 1;
    }
  }
  return out;
}

function findSync(
  oldLines: string[],
  newLines: string[],
  i: number,
  j: number,
  window: number,
): { i: number; j: number } | null {
  for (let di = 0; di <= window; di += 1) {
    for (let dj = 0; dj <= window; dj += 1) {
      if (di === 0 && dj === 0) continue;
      const oi = i + di;
      const nj = j + dj;
      if (
        oi < oldLines.length &&
        nj < newLines.length &&
        oldLines[oi] === newLines[nj]
      ) {
        return { i: oi, j: nj };
      }
    }
  }
  return null;
}

function findPathInUnknown(value: unknown, depth: number): string | undefined {
  if (depth > 4 || value == null) return undefined;
  if (typeof value === "string") {
    const text = value.trim();
    if (!text) return undefined;
    if (text.startsWith("{") || text.startsWith("[")) {
      try {
        return findPathInUnknown(JSON.parse(text), depth + 1);
      } catch {
        return undefined;
      }
    }
    return looksLikeToolPath(text) ? normalizePath(text) : undefined;
  }
  if (Array.isArray(value)) {
    for (const item of value) {
      const found = findPathInUnknown(item, depth + 1);
      if (found) return found;
    }
    return undefined;
  }
  const rec = asRecord(value);
  if (!rec) return undefined;
  const skip = new Set([
    "toolCallId",
    "tool_call_id",
    "sessionUpdate",
    "status",
    "kind",
    "title",
    "content",
    "text",
    "rawOutput",
    "raw_output",
    "output",
    "result",
    "cwd",
    "workingDirectory",
    "working_directory",
  ]);
  for (const [key, nested] of Object.entries(rec)) {
    if (skip.has(key)) continue;
    const found = findPathInUnknown(nested, depth + 1);
    if (found) return found;
  }
  return undefined;
}

function looksLikeToolPath(value: string): boolean {
  const text = value.trim();
  if (!text || text.length > 400 || /[\n\r]/.test(text)) return false;
  if (isWeakToolTitle(text)) return false;
  if (/^(\/\/|\/\*|\*)/.test(text)) return false;
  if (/^https?:/i.test(text)) return false;
  return looksLikePath(text);
}

function looksLikePath(value: string): boolean {
  const text = value.trim();
  return (
    looksLikeAbsPath(text) ||
    (text.includes("/") && !/^(\/\/|\/\*)/.test(text)) ||
    /\.[a-z0-9]{1,8}$/i.test(text)
  );
}

function looksLikeAbsPath(value: string): boolean {
  const text = value.trim();
  if (text.startsWith("file://")) return true;
  if (/^(\/\/|\/\*)/.test(text)) return false;
  if (text.startsWith("/")) return /\/[^/\s]/.test(text) && text.length < 512;
  return /^[A-Za-z]:[\\/]/.test(text);
}

function capLine(text: string): string {
  if (text.length <= MAX_LINE_CHARS) return text;
  return `${text.slice(0, MAX_LINE_CHARS - 1)}…`;
}

function normalizePath(path: string): string {
  if (path.startsWith("file://")) {
    try {
      return decodeURIComponent(path.slice("file://".length));
    } catch {
      return path.slice("file://".length);
    }
  }
  return path;
}

function basename(path: string): string {
  const trimmed = path.replace(/[/\\]+$/, "") || path;
  const parts = trimmed.split(/[/\\]/).filter(Boolean);
  return parts[parts.length - 1] ?? trimmed;
}

function parseRecord(value: unknown): Record<string, unknown> {
  if (typeof value === "string") {
    const text = value.trim();
    if (!text.startsWith("{") && !text.startsWith("[")) return {};
    try {
      return parseRecord(JSON.parse(text));
    } catch {
      return {};
    }
  }
  return asRecord(value) ?? {};
}

/** Flatten the nested argument bags ACP agents stuff under args/input/rawInput. */
function inputRecords(...values: unknown[]): Record<string, unknown>[] {
  const out: Record<string, unknown>[] = [];
  const seen = new Set<object>();
  const add = (value: unknown) => {
    if (Array.isArray(value)) {
      for (const item of value) add(item);
      return;
    }
    const rec = parseRecord(value);
    if (Object.keys(rec).length === 0 || seen.has(rec)) return;
    seen.add(rec);
    out.push(rec);
    add(rec.arguments);
    add(rec.args);
    add(rec.params);
    add(rec.input);
    add(rec.rawInput);
    add(rec.raw_input);
  };
  for (const value of values) add(value);
  return out;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

function commandField(value: unknown): string | undefined {
  if (typeof value === "string" && value.trim()) return value.trim();
  if (
    Array.isArray(value) &&
    value.length > 0 &&
    value.every((item) => typeof item === "string")
  ) {
    const joined = value.join(" ").trim();
    if (joined) return joined;
  }
  return undefined;
}

function firstLine(value: string | undefined): string {
  const text = value?.trim() ?? "";
  if (!text) return "";
  return text.split(/\r?\n/)[0]?.trim() ?? "";
}

function stripExecutePrefix(title: string): string {
  return title.replace(/^(?:bash|shell|execute)\s*[:\-]\s+/i, "").trim();
}

function skillNameField(value: unknown): string | undefined {
  if (typeof value !== "string" || !value.trim()) return undefined;
  const text = value.trim().replace(/^\/+/, "");
  if (!text || isWeakToolTitle(text) || text.length > 80 || /[\n\r]/.test(text)) {
    return undefined;
  }
  return text;
}

function looksLikeSkillName(value: string): boolean {
  const text = value.trim().replace(/^\/+/, "");
  if (!text) return false;
  if (text.includes("/") || text.includes("\\")) return false;
  if (/\.[a-z0-9]{1,8}$/i.test(text)) return false;
  return /^[a-zA-Z][\w.-]*$/.test(text);
}

function formatSkillName(value: string | undefined): string {
  const text = value?.trim().replace(/^\/+/, "") ?? "";
  return text ? `/${text}` : "";
}

function coerceString(value: unknown): string | undefined {
  if (typeof value === "string" && value.trim()) return value.trim();
  if (typeof value === "number" && Number.isFinite(value)) return String(value);
  const rec = asRecord(value);
  if (!rec) return undefined;
  return (
    coerceString(rec.path) ??
    coerceString(rec.text) ??
    coerceString(rec.uri) ??
    coerceString(rec.value)
  );
}

function numberField(
  rec: Record<string, unknown>,
  key: string,
): number | undefined {
  const value = rec[key];
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && /^\d+$/.test(value.trim())) {
    return Number(value.trim());
  }
  return undefined;
}
