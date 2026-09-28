import { readdir, stat } from "node:fs/promises";
import { homedir } from "node:os";
import { dirname, isAbsolute, parse, resolve } from "node:path";
import type { HostDirectory } from "../src/features/connections/model/protocol";

/** Lists host directories for the project picker without reading file contents. */
export async function browseHostDirectories(
  rawPath: unknown,
): Promise<HostDirectory> {
  if (
    rawPath !== undefined &&
    (typeof rawPath !== "string" ||
      rawPath.length > 4096 ||
      rawPath.includes("\0"))
  )
    throw new Error("Invalid directory path");
  const requested =
    typeof rawPath === "string" && rawPath.trim() ? rawPath : homedir();
  if (!isAbsolute(requested))
    throw new Error("Choose an absolute directory path");
  const path = resolve(requested);
  if (!(await stat(path)).isDirectory())
    throw new Error("Path is not a directory");
  const entries = (await readdir(path, { withFileTypes: true }))
    .filter((entry) => entry.isDirectory())
    .sort((a, b) => a.name.localeCompare(b.name))
    .slice(0, 500)
    .map((entry) => ({ name: entry.name, path: resolve(path, entry.name) }));
  return {
    path,
    parent: path === parse(path).root ? null : dirname(path),
    entries,
  };
}
