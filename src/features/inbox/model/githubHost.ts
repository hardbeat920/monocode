import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const DEFAULT_GITHUB_HOST = "github.com";
const HOST_CHANGED_EVENT = "github-host-changed";

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

/**
 * The server a GitHub web URL lives on, so an item keeps acting on its own
 * server after the configured host changes. Falls back to the configured host.
 */
export function githubHostOf(url: string | null | undefined): string {
  if (!url) return host;
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== "https:" || parsed.port) return host;
    const value = parsed.hostname.toLowerCase();
    return value === `www.${DEFAULT_GITHUB_HOST}` ? DEFAULT_GITHUB_HOST : value;
  } catch {
    return host;
  }
}

/**
 * Prefix for item identities and cache keys. Empty for github.com, so keys
 * (and seen state) saved before Enterprise support keep matching.
 */
export function githubHostScope(itemHost: string): string {
  const value = itemHost.trim().toLowerCase();
  return !value || value === DEFAULT_GITHUB_HOST ? "" : `${value}/`;
}

export async function loadGithubHost(): Promise<string> {
  setGithubHost(await invoke<string>("github_host_get"));
  // Another window may save a new host; the backend broadcasts it.
  void listen<string>(HOST_CHANGED_EVENT, (event) => {
    setGithubHost(event.payload);
  }).catch(() => undefined);
  return host;
}

export async function saveGithubHost(next: string): Promise<string> {
  setGithubHost(await invoke<string>("github_host_set", { host: next.trim() }));
  return host;
}
