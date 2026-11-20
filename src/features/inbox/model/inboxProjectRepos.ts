import { invoke } from "@tauri-apps/api/core";

// Reuse local Git reads while switching issues. No remote servers are contacted.
const CACHE_MS = 30_000;
const cache = new Map<
  string,
  { expiresAt: number; names: Promise<string[]> }
>();
export function clearProjectRepoNames() {
  cache.clear();
}
export function projectRepoNames(path: string): Promise<string[]> {
  const existing = cache.get(path);
  if (existing && existing.expiresAt > Date.now()) return existing.names;
  const names = (async () => {
    try {
      const names = await invoke<string[]>("git_repository_names", {
        cwd: path,
      });
      return Array.isArray(names) ? names : [];
    } catch {
      return [];
    }
  })();
  if (cache.size >= 100) cache.delete(cache.keys().next().value!);
  cache.set(path, { expiresAt: Date.now() + CACHE_MS, names });
  return names;
}
