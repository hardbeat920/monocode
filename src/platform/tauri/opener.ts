/**
 * App-wide stand-in for `@tauri-apps/plugin-opener`. vite.config.ts resolves
 * every import of that package to this file (this file alone gets the real
 * one), so links a terminal opens with plain `openUrl` follow the in-app link
 * setting without the terminal code knowing about the browser. Everything
 * else passes straight through.
 */
import * as opener from "@tauri-apps/plugin-opener";
import { openLink } from "../../features/browser/model/openLink";

export * from "@tauri-apps/plugin-opener";

/** The real opener: always the system default app. */
export const openExternalUrl: typeof opener.openUrl = (url, openWith) =>
  opener.openUrl(url, openWith);

/** The mouse event behind this call, when a terminal link was clicked. */
export function terminalLinkClick(
  event: Event | undefined,
): MouseEvent | null {
  if (typeof MouseEvent === "undefined" || !(event instanceof MouseEvent)) {
    return null;
  }
  const target = event.target;
  if (typeof Element === "undefined" || !(target instanceof Element)) {
    return null;
  }
  return target.closest(".xterm") ? event : null;
}

export const openUrl: typeof opener.openUrl = (url, openWith) => {
  const click =
    openWith || typeof window === "undefined"
      ? null
      : // xterm activates links synchronously inside the DOM click.
        terminalLinkClick(window.event);
  if (click) return openLink(String(url), click);
  return openExternalUrl(url, openWith);
};
