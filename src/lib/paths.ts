import { IS_WIN } from "./platform";

function windowsPath(path: string): boolean {
  return /^[A-Za-z]:[\\/]/.test(path) || path.startsWith("\\\\") || path.startsWith("//");
}

export function slash(path: string): string {
  return windowsPath(path) || (IS_WIN && !path.startsWith("/"))
    ? path.replace(/\\/g, "/") : path;
}

function trimSlash(path: string): string {
  return slash(path).replace(/\/+$/, "") || "/";
}

/** Stable comparison key for Windows paths without changing their display case. */
export function pathKey(path: string): string {
  const normalized = trimSlash(path);
  return /^[A-Za-z]:(?:\/|$)/.test(normalized) || normalized.startsWith("//")
    ? normalized.toLowerCase()
    : normalized;
}

export function prettyCwd(cwd: string): string {
  const trimmed = trimSlash(cwd);
  if (trimmed === "~") return "~";

  const parts = trimmed.split("/").filter(Boolean);
  if (parts.length >= 2 && (parts[0] === "Users" || parts[0] === "home")) {
    const rest = parts.slice(2).join("/");
    return rest ? `~/${rest}` : "~";
  }
  if (
    parts.length >= 3 &&
    /^[A-Za-z]:$/.test(parts[0]) &&
    parts[1] === "Users"
  ) {
    const rest = parts.slice(3).join("/");
    return rest ? `~/${rest}` : "~";
  }
  return trimmed;
}

export function parentPath(path: string): string {
  const trimmed = trimSlash(path);
  if (/^\/\/[^/]+\/[^/]+$/.test(trimmed)) return trimmed;
  if (/^[A-Za-z]:$/.test(trimmed)) return `${trimmed}/`;
  const i = trimmed.lastIndexOf("/");
  if (i <= 0) return "/";
  const parent = trimmed.slice(0, i);
  if (/^[A-Za-z]:$/.test(parent)) return `${parent}/`;
  return parent;
}

export function rebasePath(path: string, from: string, to: string): string {
  const normalized = trimSlash(path);
  const source = trimSlash(from);
  const dest = trimSlash(to);
  const key = pathKey(normalized);
  const sourceKey = pathKey(source);
  if (key === sourceKey) return /^[A-Za-z]:$/.test(dest) ? `${dest}/` : dest;
  if (key.startsWith(`${sourceKey}/`)) {
    return `${dest}${normalized.slice(source.length)}`;
  }
  return slash(path);
}

export function isEqualOrInside(path: string, root: string): boolean {
  const normalized = trimSlash(path);
  const base = trimSlash(root);
  const key = pathKey(normalized);
  const baseKey = pathKey(base);
  return key === baseKey || key.startsWith(`${baseKey}/`);
}

export function joinPath(parent: string, relative: string): string {
  const base = trimSlash(parent);
  const parts = relative
    .split(windowsPath(parent) ? /[/\\]/ : /\//)
    .filter((part) => part && part !== ".");
  let out = base;
  for (const part of parts) {
    if (part === "..") {
      out = parentPath(out);
      continue;
    }
    out = out === "/" ? `/${part}` : `${out}/${part}`;
  }
  return out;
}

/** Absolute path for a workspace file href, or `undefined` if it is not a local file. */
export function resolveWorkspacePath(
  href: string,
  cwd?: string,
): string | undefined {
  let value = href.trim();
  if (!value || /^(https?:|mailto:|tel:)/i.test(value)) return undefined;

  if (value.startsWith("file://")) {
    try {
      value = decodeURIComponent(value.slice("file://".length));
    } catch {
      value = value.slice("file://".length);
    }
  }

  value = slash(value).replace(/(?::\d+(?::\d+)?|#L\d+(?:-L\d+)?)$/, "");
  if (!value || value === "." || value.startsWith("#") || value.startsWith("?") || value.includes("://")) {
    return undefined;
  }
  if (!looksLikeFilePath(value)) return undefined;

  if (/^[A-Za-z]:\//.test(value)) return value;
  if (value.startsWith("/")) {
    return /^\/[A-Za-z]:\//.test(value) ? value.slice(1) : value;
  }
  if (!cwd || cwd === "~") return undefined;
  return joinPath(cwd, value);
}

function looksLikeFilePath(value: string): boolean {
  if (value.startsWith("/") || /^[A-Za-z]:\//.test(value)) return true;
  if (value.includes("/")) return true;
  return /\.[A-Za-z][A-Za-z0-9+]{0,11}$/.test(value);
}

export function prettyParent(path: string): string {
  return prettyCwd(parentPath(path));
}

/** Path relative to cwd when it lives under the project, otherwise unchanged. */
export function displayPath(path: string, cwd?: string): string {
  const normalized = trimSlash(path);
  const base = cwd ? trimSlash(cwd) : undefined;
  if (base && base !== "~") {
    const key = pathKey(normalized);
    const baseKey = pathKey(base);
    if (key === baseKey) {
      return normalized.split("/").filter(Boolean).pop() || normalized;
    }
    const prefix = `${base}/`;
    if (key.startsWith(`${baseKey}/`)) {
      return normalized.slice(prefix.length);
    }
  }
  return normalized;
}

/** Folder name for tab labels — `~` when the cwd is home. */
export function projectName(cwd: string): string {
  if (!cwd || prettyCwd(cwd) === "~") return "~";
  const trimmed = trimSlash(cwd);
  if (/^[A-Za-z]:$/.test(trimmed)) return trimmed;
  const parts = trimmed.split("/").filter(Boolean);
  return parts[parts.length - 1] ?? trimmed;
}
