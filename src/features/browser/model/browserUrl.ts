const LOCAL_HOST =
  /^(localhost|127\.\d+\.\d+\.\d+|\[::1\]|0\.0\.0\.0)(:\d+)?(\/|$)/i;
const HAS_SCHEME = /^[a-z][a-z\d+.-]*:/i;
const LOOKS_LIKE_HOST = /^[^\s/]+\.[^\s/]+(\/|$)|^[^\s/]+:\d+(\/|$)/;

export const BLANK_URL = "about:blank";

/** Whether the embedded browser may load this URL. */
export function isBrowsableUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return (
      url.protocol === "http:" ||
      url.protocol === "https:" ||
      url.href === BLANK_URL
    );
  } catch {
    return false;
  }
}

/**
 * What the address bar should load for typed text: a URL as-is, a bare host
 * over https (http for local dev servers), anything else as a web search.
 */
export function browserUrlFromInput(input: string): string {
  const text = input.trim();
  if (!text) return BLANK_URL;
  if (HAS_SCHEME.test(text) && !isHostWithPort(text)) {
    return isBrowsableUrl(text) ? new URL(text).href : searchUrl(text);
  }
  if (LOCAL_HOST.test(text)) return new URL(`http://${text}`).href;
  if (!/\s/.test(text) && LOOKS_LIKE_HOST.test(text)) {
    try {
      return new URL(`https://${text}`).href;
    } catch {
      return searchUrl(text);
    }
  }
  return searchUrl(text);
}

// `localhost:3000` parses as a URL with scheme `localhost:`.
function isHostWithPort(text: string): boolean {
  return /^[^\s/:]+:\d+(\/|$)/.test(text);
}

function searchUrl(text: string): string {
  return `https://duckduckgo.com/?q=${encodeURIComponent(text)}`;
}

/** Short tab label for a page with no title yet. */
export function browserUrlLabel(value: string): string {
  if (!value || value === BLANK_URL) return "New Tab";
  try {
    const url = new URL(value);
    return url.host || value;
  } catch {
    return value;
  }
}
