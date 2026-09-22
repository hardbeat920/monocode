/** Visual-only: turn a bash argv into Read / Find / List when the intent is obvious. */

export type ShellIntent = {
  verb: "Read" | "Find" | "List" | "Edit" | "Write";
  path?: string;
  query?: string;
  startLine?: number;
};

/** Long scripts stay as the command. This runs on every transcript row. */
const MAX_COMMAND_CHARS = 2000;

const READABLE_BIN =
  /\b(?:cat|bat|batcat|nl|less|more|tac|head|tail|sed|grep|egrep|fgrep|rgrep|rg|ag|ack|find|ls|tree|tee)\b/;

/**
 * Best-effort label for a shell command. Returns undefined when the command
 * should stay as typed (git, npm, scripts, mixed opaque work).
 */
export function inferShellIntent(command: string): ShellIntent | undefined {
  const text = unwrapShellCommand(command);
  if (!text || text.length > MAX_COMMAND_CHARS) return undefined;
  if (/^(Read|Find|List|Edit|Write)\s+\S/.test(text)) return undefined;
  if (looksUnsafe(text)) return undefined;
  const write = extractWriteRedirect(text);
  if (!READABLE_BIN.test(text) && !write) return undefined;

  const intents: ShellIntent[] = [];
  for (const chain of splitTopLevel(text, chainSep)) {
    const stages = splitTopLevel(chain, pipeSep);
    let pipeIntent: ShellIntent | undefined;
    for (const stage of stages) {
      const tokens = tokenize(stage);
      if (!tokens || tokens.length === 0) return undefined;
      const argv = tokens.map(({ value }) => value);
      const classified = classifyArgv(argv);
      if (classified === "opaque") return undefined;
      if (classified === "noise") continue;
      pipeIntent = classified;
    }
    if (pipeIntent) intents.push(pipeIntent);
  }

  if (write?.path) {
    return { verb: write.append ? "Edit" : "Write", path: write.path };
  }
  if (intents.length === 0) return undefined;
  const ranked = [...intents].reverse();
  return (
    ranked.find((item) => item.verb === "Edit" || item.verb === "Write") ??
    ranked.find((item) => item.verb === "Read" || item.verb === "Find") ??
    ranked[0]
  );
}

/**
 * Codex may expose a command through the argv used to launch the user's shell,
 * for example `/bin/zsh -lc "cat package.json"` or
 * `pwsh.exe -Command "Get-Content package.json"`. The launcher is transport
 * noise for this visual-only classifier; inspect the script it was given.
 */
export function unwrapShellCommand(command: string): string {
  let current = command.trim();
  for (let depth = 0; depth < 2; depth += 1) {
    const tokens = tokenize(current);
    if (!tokens || tokens.length < 3) break;
    const executableToken = tokens[0];
    const rawExecutable = current.slice(
      executableToken.start,
      executableToken.end,
    );
    const executableQuote = current[executableToken.start];
    const executable =
      (executableQuote === '"' || executableQuote === "'") &&
      rawExecutable[0] === rawExecutable[rawExecutable.length - 1]
        ? rawExecutable.slice(1, -1)
        : rawExecutable;
    const wrapper = SHELL_WRAPPERS.find(({ executables }) =>
      executables.has(binName(executable)),
    );
    if (!wrapper) break;
    let flagIndex = -1;
    for (let index = 1; index < tokens.length; index += 1) {
      const token = tokens[index];
      if (
        "optionBoundary" in wrapper &&
        wrapper.optionBoundary.test(token.value)
      ) {
        break;
      }
      if (wrapper.commandFlag.test(token.value)) {
        flagIndex = index;
        break;
      }
    }
    if (flagIndex < 0) break;
    const commandToken = tokens[flagIndex + 1];
    if (!commandToken) break;
    const remainder = current.slice(commandToken.start).trim();
    const commandQuote = current[commandToken.start];
    const commandIsSoleQuotedToken =
      (commandQuote === '"' || commandQuote === "'") &&
      current[commandToken.end - 1] === commandQuote &&
      tokens.length === flagIndex + 2;
    const script =
      wrapper.consumeRemainder && !commandIsSoleQuotedToken
        ? remainder
        : commandToken.value.trim();
    if (!script || script === current) break;
    current = script;
  }
  return current;
}

const SHELL_WRAPPERS = [
  {
    executables: new Set(["sh", "bash", "zsh", "dash", "ksh"]),
    commandFlag: /^(?:--command|-[a-z]*c[a-z]*)$/i,
    consumeRemainder: false,
  },
  {
    executables: new Set(["powershell", "powershell.exe", "pwsh", "pwsh.exe"]),
    commandFlag: /^-(?:command|c)$/i,
    optionBoundary: /^-(?:file|f)$/i,
    consumeRemainder: true,
  },
  {
    executables: new Set(["cmd", "cmd.exe"]),
    commandFlag: /^\/c$/i,
    consumeRemainder: true,
  },
] as const;

export function formatShellIntent(
  intent: ShellIntent,
  path?: string,
  query?: string,
): string | undefined {
  if (intent.verb === "Find") {
    const q = query || intent.query;
    return q ? `Find ${q}` : undefined;
  }
  const target = path || intent.path;
  if (!target) return undefined;
  return `${intent.verb} ${target}`;
}

/** Re-apply a stored Read/Find/List/Edit title when the row already went through ingest. */
export function rewriteReadableTitle(
  title: string,
  path?: string,
  query?: string,
): string | undefined {
  const match = title.match(/^(Read|Find|List|Edit|Write)\s+(.+)$/i);
  if (!match) return undefined;
  const verb =
    match[1].slice(0, 1).toUpperCase() + match[1].slice(1).toLowerCase();
  if (verb === "Find") return `Find ${query || match[2]}`;
  return `${verb} ${path || match[2]}`;
}

type Classified = ShellIntent | "noise" | "opaque";

function classifyArgv(argv: string[]): Classified {
  const bin = binName(argv[0]);
  if (NOISE_BINS.has(bin)) return "noise";
  if (READ_BINS.has(bin)) return readIntent(argv);
  if (bin === "head" || bin === "tail") return headTailIntent(argv);
  if (bin === "sed") return sedIntent(argv);
  if (bin === "tee") return teeIntent(argv);
  if (bin === "rg" && argv.includes("--files")) {
    return { verb: "Find", query: "files" };
  }
  if (SEARCH_BINS.has(bin)) return grepIntent(argv);
  if (bin === "find") return findIntent(argv);
  if (bin === "ls" || bin === "tree") return listIntent(argv);
  return "opaque";
}

const NOISE_BINS = new Set([
  "cd",
  "echo",
  "printf",
  "pwd",
  "true",
  "false",
  "clear",
  ":",
  "wc",
  "sleep",
  "export",
  "unset",
  "alias",
  "wait",
]);

const READ_BINS = new Set([
  "cat",
  "bat",
  "batcat",
  "nl",
  "less",
  "more",
  "tac",
]);

const SEARCH_BINS = new Set([
  "grep",
  "egrep",
  "fgrep",
  "rgrep",
  "rg",
  "ag",
  "ack",
]);

const GREP_VALUE_FLAGS = new Set([
  "-e",
  "--regexp",
  "-f",
  "--file",
  "-A",
  "--after-context",
  "-B",
  "--before-context",
  "-C",
  "--context",
  "-m",
  "--max-count",
  "-d",
  "--directories",
  "-D",
  "--devices",
  "--include",
  "--exclude",
  "--exclude-dir",
  "-g",
  "--glob",
  "-t",
  "--type",
  "-j",
  "--threads",
  "--max-filesize",
  "--max-depth",
  "--max-columns",
]);

const FIND_VALUE_FLAGS = new Set([
  "-name",
  "-iname",
  "-path",
  "-ipath",
  "-wholename",
  "-iwholename",
  "-regex",
  "-iregex",
  "-type",
  "-mtime",
  "-mmin",
  "-ctime",
  "-cmin",
  "-atime",
  "-amin",
  "-size",
  "-maxdepth",
  "-mindepth",
  "-user",
  "-group",
  "-perm",
  "-exec",
  "-execdir",
  "-ok",
  "-printf",
]);

function readIntent(argv: string[]): Classified {
  const files = positionalArgs(argv, new Set());
  const path = lastFile(files);
  return path ? { verb: "Read", path } : "noise";
}

function headTailIntent(argv: string[]): Classified {
  const files: string[] = [];
  for (let i = 1; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--") {
      files.push(...argv.slice(i + 1));
      break;
    }
    if (/^-\d+$/.test(arg) || arg === "-n" || arg === "-c" || arg === "-q" || arg === "-v") {
      if (arg === "-n" || arg === "-c") i += 1;
      continue;
    }
    if (arg.startsWith("-n") || arg === "--lines" || arg === "--bytes") {
      if (arg === "--lines" || arg === "--bytes") i += 1;
      continue;
    }
    if (arg.startsWith("-") && arg !== "-") continue;
    files.push(arg);
  }
  const path = lastFile(files);
  return path ? { verb: "Read", path } : "noise";
}

function sedIntent(argv: string[]): Classified {
  if (argv.some(isSedInPlace)) {
    const path = lastSourceFile(argv.slice(1));
    return path ? { verb: "Edit", path } : "opaque";
  }
  let script: string | undefined;
  const files: string[] = [];
  for (let i = 1; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--") {
      files.push(...argv.slice(i + 1));
      break;
    }
    if (arg === "-e" || arg === "--expression") {
      script = argv[i + 1];
      i += 1;
      continue;
    }
    if (arg === "-f" || arg === "--file") return "opaque";
    if (
      arg === "-n" ||
      arg === "--quiet" ||
      arg === "--silent" ||
      arg === "-E" ||
      arg === "-r" ||
      arg === "--regexp-extended" ||
      arg === "-l" ||
      arg === "--line-length"
    ) {
      continue;
    }
    if (arg.startsWith("-") && arg !== "-") continue;
    if (!script) script = arg;
    else files.push(arg);
  }
  const path = lastFile(files);
  if (!path) return "noise";
  return { verb: "Read", path, startLine: sedStartLine(script) };
}

function teeIntent(argv: string[]): Classified {
  const append = argv.includes("-a") || argv.includes("--append");
  const files = positionalArgs(argv, new Set());
  const path = lastFile(files);
  if (!path) return "opaque";
  return { verb: append ? "Edit" : "Write", path };
}

function grepIntent(argv: string[]): Classified {
  let query: string | undefined;
  const files: string[] = [];
  for (let i = 1; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--") {
      const rest = argv.slice(i + 1);
      if (!query && rest[0]) {
        query = rest[0];
        files.push(...rest.slice(1));
      } else {
        files.push(...rest);
      }
      break;
    }
    if (arg === "-e" || arg === "--regexp") {
      query = argv[i + 1];
      i += 1;
      continue;
    }
    if (GREP_VALUE_FLAGS.has(arg)) {
      i += 1;
      continue;
    }
    if (arg.startsWith("-") && arg !== "-") {
      if (arg.startsWith("-e") && arg.length > 2) query = arg.slice(2);
      continue;
    }
    if (!query) query = arg;
    else files.push(arg);
  }
  if (!query) return "opaque";
  return { verb: "Find", query, path: lastFile(files) };
}

function findIntent(argv: string[]): Classified {
  let query: string | undefined;
  let path: string | undefined;
  for (let i = 1; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "-name" || arg === "-iname" || arg === "-path" || arg === "-ipath") {
      query = argv[i + 1];
      i += 1;
      continue;
    }
    if (arg.startsWith("-")) {
      if (FIND_VALUE_FLAGS.has(arg)) i += 1;
      continue;
    }
    if (!path) path = tidyPath(arg);
  }
  if (!query) return "opaque";
  return { verb: "Find", query, path };
}

function listIntent(argv: string[]): Classified {
  const files = positionalArgs(argv, new Set(["--ignore", "-I", "--hide"]));
  const path = lastFile(files);
  return path ? { verb: "List", path } : "noise";
}

function positionalArgs(argv: string[], valueFlags: Set<string>): string[] {
  const out: string[] = [];
  for (let i = 1; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--") {
      out.push(...argv.slice(i + 1));
      break;
    }
    if (arg.startsWith("-") && arg !== "-") {
      if (valueFlags.has(arg)) i += 1;
      continue;
    }
    out.push(arg);
  }
  return out;
}

function lastFile(files: string[]): string | undefined {
  for (let i = files.length - 1; i >= 0; i -= 1) {
    const path = tidyPath(files[i]);
    if (path) return path;
  }
  return undefined;
}

function lastSourceFile(args: string[]): string | undefined {
  for (let i = args.length - 1; i >= 0; i -= 1) {
    const arg = args[i];
    if (!arg || arg === "-" || (arg.startsWith("-") && arg !== "-")) continue;
    if (isSedScript(arg)) continue;
    const path = tidyPath(arg);
    if (path) return path;
  }
  return undefined;
}

function isSedScript(value: string): boolean {
  return /^\d+(,\d+)?[spd]$/.test(value) || /^s([^A-Za-z0-9]).+\1/.test(value);
}

function tidyPath(value: string | undefined): string | undefined {
  if (!value || value === "-" || value === "/dev/stdin" || value === "/dev/stdout") {
    return undefined;
  }
  if (value.includes(">") || value.includes("<")) return undefined;
  const trimmed = value.replace(/\\/g, "/").replace(/\/+$/, "");
  if (!trimmed || trimmed === "." || trimmed === "./" || trimmed === "/dev/null") {
    return undefined;
  }
  if (/^\d+$/.test(trimmed)) return undefined;
  return trimmed;
}

function binName(token: string): string {
  const base = token.replace(/\\/g, "/").split("/").pop() ?? token;
  return base.toLowerCase();
}

function isSedInPlace(arg: string): boolean {
  return arg === "--in-place" || arg === "-i" || arg.startsWith("-i") || arg.startsWith("--in-place=");
}

function sedStartLine(script: string | undefined): number | undefined {
  const match = script?.match(/^(\d+)(?:,\d+)?p$/);
  if (!match) return undefined;
  const line = Number(match[1]);
  return line > 0 ? line : undefined;
}

function looksUnsafe(command: string): boolean {
  let quote: "'" | '"' | null = null;
  for (let i = 0; i < command.length; i += 1) {
    const c = command[i];
    if (quote) {
      if (quote === '"' && c === "\\" && i + 1 < command.length) {
        i += 1;
        continue;
      }
      if (c === quote) quote = null;
      continue;
    }
    if (c === "'" || c === '"') {
      quote = c;
      continue;
    }
    if (c === "\\" && i + 1 < command.length) {
      i += 1;
      continue;
    }
    if (c === "`") return true;
    if (c === "$" && (command[i + 1] === "(" || command[i + 1] === "{")) {
      return true;
    }
  }
  return quote != null;
}

function extractWriteRedirect(
  command: string,
): { path: string; append: boolean } | undefined {
  let quote: "'" | '"' | null = null;
  let found: { path: string; append: boolean } | undefined;
  for (let i = 0; i < command.length; i += 1) {
    const c = command[i];
    if (quote) {
      if (quote === '"' && c === "\\" && i + 1 < command.length) {
        i += 1;
        continue;
      }
      if (c === quote) quote = null;
      continue;
    }
    if (c === "'" || c === '"') {
      quote = c;
      continue;
    }
    if (c === "\\" && i + 1 < command.length) {
      i += 1;
      continue;
    }
    if (c !== ">") continue;
    let append = false;
    let j = i + 1;
    if (command[j] === ">") {
      append = true;
      j += 1;
    }
    while (command[j] === " " || command[j] === "\t") j += 1;
    if (command[j] === "&") continue;
    const target = readUnquotedToken(command, j);
    if (target === "/dev/null" || target === "-") continue;
    const path = tidyPath(target);
    if (path) found = { path, append };
    i = Math.max(i, j + Math.max(target.length, 1) - 1);
  }
  return found;
}

function readUnquotedToken(text: string, start: number): string {
  let i = start;
  while (i < text.length && !/[\s|&;<>]/.test(text[i])) i += 1;
  return text.slice(start, i);
}

function chainSep(text: string, i: number): number {
  if (text.startsWith("&&", i) || text.startsWith("||", i)) return 2;
  if (text.startsWith("\r\n", i)) return 2;
  if (text[i] === "\n" || text[i] === "\r") return 1;
  if (text[i] === ";") return 1;
  return 0;
}

function pipeSep(text: string, i: number): number {
  if (text[i] === "|" && text[i + 1] !== "|") return 1;
  return 0;
}

function splitTopLevel(
  command: string,
  sepAt: (text: string, i: number) => number,
): string[] {
  const parts: string[] = [];
  let start = 0;
  let quote: "'" | '"' | null = null;
  for (let i = 0; i < command.length; i += 1) {
    const c = command[i];
    if (quote) {
      if (quote === '"' && c === "\\" && i + 1 < command.length) {
        i += 1;
        continue;
      }
      if (c === quote) quote = null;
      continue;
    }
    if (c === "'" || c === '"') {
      quote = c;
      continue;
    }
    if (c === "\\" && i + 1 < command.length) {
      i += 1;
      continue;
    }
    const len = sepAt(command, i);
    if (len > 0) {
      const part = command.slice(start, i).trim();
      if (part) parts.push(part);
      i += len - 1;
      start = i + 1;
    }
  }
  const tail = command.slice(start).trim();
  if (tail) parts.push(tail);
  return parts;
}

type ShellToken = {
  value: string;
  start: number;
  end: number;
};

function tokenize(stage: string): ShellToken[] | null {
  const tokens: ShellToken[] = [];
  let i = 0;
  while (i < stage.length) {
    // Treat redirects (`2>&1`) as separators so `&` cannot stall the scan.
    while (i < stage.length && /[ \t|&;<>()]/.test(stage[i])) i += 1;
    if (i >= stage.length) break;
    if (stage[i] === "#") break;
    const start = i;
    let token = "";
    while (i < stage.length && !/[ \t|&;<>()]/.test(stage[i])) {
      const c = stage[i];
      if (c === "'") {
        const end = stage.indexOf("'", i + 1);
        if (end < 0) return null;
        token += stage.slice(i + 1, end);
        i = end + 1;
        continue;
      }
      if (c === '"') {
        i += 1;
        while (i < stage.length && stage[i] !== '"') {
          if (stage[i] === "\\" && i + 1 < stage.length) {
            token += stage[i + 1];
            i += 2;
            continue;
          }
          token += stage[i];
          i += 1;
        }
        if (i >= stage.length) return null;
        i += 1;
        continue;
      }
      if (c === "\\" && i + 1 < stage.length) {
        token += stage[i + 1];
        i += 2;
        continue;
      }
      token += c;
      i += 1;
    }
    if (token) {
      tokens.push({
        value: token,
        start,
        end: i,
      });
    }
    if (i <= start) i += 1;
  }
  return tokens;
}
