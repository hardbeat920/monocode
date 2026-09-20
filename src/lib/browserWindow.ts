import { invoke } from "@tauri-apps/api/core";

/** Schemes a browser window may load. Anything else can reach local files. */
const ALLOWED = new Set(["http:", "https:"]);

const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "0.0.0.0", "[::1]"]);

/**
 * Turn typed text into a URL the browser window will accept.
 *
 * A bare host is assumed to be `https:`, except for loopback and anything
 * carrying an explicit port — those are dev servers and almost never have TLS.
 * Returns null when the text is empty or names a scheme we refuse to open.
 */
export function normalizeBrowserUrl(raw: string): string | null {
  const text = raw.trim();
  if (!text) return null;

  // `host:port` looks like a scheme to a naive regex, so only treat the prefix
  // as one when what follows the colon is not purely a port number.
  const explicitScheme = text.match(/^([a-z][a-z0-9+.-]*):([^/?#]*)/i);
  if (explicitScheme && !/^\d+$/.test(explicitScheme[2] ?? "")) {
    try {
      const url = new URL(text);
      return ALLOWED.has(url.protocol) ? url.toString() : null;
    } catch {
      return null;
    }
  }

  const host = text.split(/[/?#]/, 1)[0] ?? "";
  const bare = host.replace(/:\d+$/, "");
  const scheme =
    LOCAL_HOSTS.has(bare) || /:\d+$/.test(host) ? "http" : "https";
  try {
    return new URL(`${scheme}://${text}`).toString();
  } catch {
    return null;
  }
}

/** Open `url` as a real top-level window — not an iframe inside the app. */
export function openBrowserWindow(url: string): Promise<void> {
  return invoke<void>("open_browser_window", { url });
}
