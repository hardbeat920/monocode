/** Unix file-name checks for the explorer: empty, slashes, invalid names. */

export type NameIssue =
  | { severity: "error"; kind: "empty" }
  | { severity: "error"; kind: "slash" }
  | { severity: "error"; kind: "exists"; name: string }
  | { severity: "error"; kind: "invalid"; name: string }
  | { severity: "warning"; kind: "whitespace" };

export function wellFormedFileName(filename: string): string {
  if (!filename) return filename;
  return filename.replace(/^\t+|\t+$/g, "").replace(/[/\\]+$/, "");
}

export function pathSegments(name: string): string[] {
  return wellFormedFileName(name).split(/[/\\]/).filter(Boolean);
}

function isValidBasename(name: string): boolean {
  if (!name || /^\s+$/.test(name)) return false;
  if (name.includes("/") || name.includes("\\")) return false;
  if (name === "." || name === "..") return false;
  if (name.length > 255) return false;
  return true;
}

export function validateFileName(
  raw: string,
  siblingNames: Iterable<string>,
): NameIssue | null {
  const name = wellFormedFileName(raw);

  if (!name || /^\s+$/.test(name)) {
    return { severity: "error", kind: "empty" };
  }

  if (name[0] === "/" || name[0] === "\\") {
    return { severity: "error", kind: "slash" };
  }

  const siblings = new Set(
    [...siblingNames].map((n) => n.toLowerCase()),
  );
  if (siblings.has(name.toLowerCase())) {
    return { severity: "error", kind: "exists", name };
  }

  const names = pathSegments(name);
  if (names.some((segment) => !isValidBasename(segment))) {
    return { severity: "error", kind: "invalid", name };
  }

  if (names.some((segment) => /^\s|\s$/.test(segment))) {
    return { severity: "warning", kind: "whitespace" };
  }

  return null;
}

export function leafName(raw: string): string {
  const names = pathSegments(raw);
  return names[names.length - 1] ?? "";
}
