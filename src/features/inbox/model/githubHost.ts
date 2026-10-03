import { invoke } from "@tauri-apps/api/core";

export const DEFAULT_GITHUB_HOST = "github.com";

// Mirrors the backend setting so URL helpers can stay synchronous.
let host = DEFAULT_GITHUB_HOST;

export function githubHost(): string {
  return host;
}

export function setGithubHost(next: string | null | undefined): void {
  host = next?.trim().toLowerCase() || DEFAULT_GITHUB_HOST;
}

/** Web origin of the configured GitHub, e.g. `https://github.example.com`. */
export function githubOrigin(): string {
  return `https://${host}`;
}

/** Whether a URL host is the configured GitHub (`www.` only for github.com). */
export function isGithubHost(candidate: string): boolean {
  const value = candidate.toLowerCase();
  if (host === DEFAULT_GITHUB_HOST) {
    return (
      value === DEFAULT_GITHUB_HOST || value === `www.${DEFAULT_GITHUB_HOST}`
    );
  }
  return value === host;
}

export async function loadGithubHost(): Promise<string> {
  setGithubHost(await invoke<string>("github_host_get"));
  return host;
}

export async function saveGithubHost(next: string): Promise<string> {
  setGithubHost(await invoke<string>("github_host_set", { host: next.trim() }));
  return host;
}
