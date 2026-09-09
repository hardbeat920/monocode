export const LOCAL_PREVIEW_EVENT = "monocode:local-preview";

export type LocalPreview = { cwd: string; url: string };

/** Pages are web content; never treat app, file or script URLs as previews. */
export function previewUrl(value: string): string | undefined {
  try {
    const url = new URL(value.trim());
    if (!/^https?:$/.test(url.protocol) || url.username || url.password)
      return undefined;
    if (
      url.hostname.endsWith(".localhost") ||
      (typeof location !== "undefined" && url.origin === location.origin)
    )
      return undefined;
    return url.href;
  } catch {
    return undefined;
  }
}

export function localPreviewUrls(text: string): string[] {
  const clean = text.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "");
  const urls = new Set<string>();
  for (const match of clean.matchAll(/https?:\/\/[^\s<>"'`]+/g)) {
    const value = previewUrl(match[0].replace(/[),.;]+$/, ""));
    if (!value) continue;
    const url = new URL(value);
    if (
      !url.port ||
      !["localhost", "127.0.0.1", "[::1]", "0.0.0.0"].includes(url.hostname)
    )
      continue;
    if (url.hostname === "0.0.0.0") url.hostname = "127.0.0.1";
    urls.add(url.href);
  }
  return [...urls];
}

/** Keep split terminal chunks until a complete line arrives. */
export function localPreviewScanner(onUrl: (url: string) => void) {
  let rest = "";
  return (chunk: string, flush = false) => {
    const lines = (rest + chunk).split(/\r?\n/);
    rest = flush ? "" : (lines.pop() ?? "").slice(-8192);
    for (const line of lines)
      for (const url of localPreviewUrls(line)) onUrl(url);
  };
}

export function announceLocalPreview(cwd: string, url: string) {
  window.dispatchEvent(
    new CustomEvent<LocalPreview>(LOCAL_PREVIEW_EVENT, {
      detail: { cwd, url },
    }),
  );
}

/** Remember an automatic open even after its tab closes, until the workspace closes. */
export function claimLocalPreview(
  url: string,
  seenOrigins: Set<string>,
): string | undefined {
  const target = localPreviewUrls(url)[0];
  if (!target) return undefined;
  const origin = new URL(target).origin;
  if (seenOrigins.has(origin)) return undefined;
  seenOrigins.add(origin);
  return target;
}
