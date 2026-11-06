import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { isAbsolute, join, resolve } from "node:path";

// Match the existing MCP JSONC reader's handling of strings and trailing commas.
function stripJsonc(raw: string): string {
  let clean = "";
  let quoted = false;
  for (let index = 0; index < raw.length; index += 1) {
    const char = raw[index];
    if (quoted) {
      clean += char;
      if (char === "\\" && index + 1 < raw.length) clean += raw[++index];
      else if (char === '"') quoted = false;
    } else if (char === '"') {
      quoted = true;
      clean += char;
    } else if (char === "/" && raw[index + 1] === "/") {
      while (index < raw.length && raw[index] !== "\n") index += 1;
      clean += "\n";
    } else if (char === "/" && raw[index + 1] === "*") {
      const end = raw.indexOf("*/", index + 2);
      if (end < 0) throw new Error("Unterminated OpenCode config comment");
      clean += " ";
      index = end + 1;
    } else clean += char;
  }
  let result = "";
  quoted = false;
  for (let index = 0; index < clean.length; index += 1) {
    const char = clean[index];
    if (quoted) {
      result += char;
      if (char === "\\" && index + 1 < clean.length) result += clean[++index];
      else if (char === '"') quoted = false;
    } else if (char === '"') {
      quoted = true;
      result += char;
    } else if (char !== "," || !/^\s*[}\]]/.test(clean.slice(index + 1))) {
      result += char;
    }
  }
  return result;
}

export function parseInheritedOpenCodeConfig(
  raw: string,
  cwd: string,
  environment: NodeJS.ProcessEnv = process.env,
): unknown {
  const env = raw.replace(
    /\{env:([^}]+)\}/g,
    (_token, name: string) => environment[name] || "",
  );
  const expanded = env.replace(
    /\{file:([^}]+)\}/g,
    (token, name: string, index: number) => {
      const lineStart = env.lastIndexOf("\n", index - 1) + 1;
      if (env.slice(lineStart, index).trimStart().startsWith("//"))
        return token;
      const file = name.startsWith("~/")
        ? join(homedir(), name.slice(2))
        : name;
      const path = isAbsolute(file) ? file : resolve(cwd, file);
      return JSON.stringify(readFileSync(path, "utf8").trim()).slice(1, -1);
    },
  );
  return JSON.parse(stripJsonc(expanded));
}

export function serializeOpenCodeConfig(config: unknown): string {
  // OpenCode preprocesses again in the child. Keep materialized tokens literal.
  return JSON.stringify(config).replace(/\{(env|file):/g, "\\u007b$1:");
}
