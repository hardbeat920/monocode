import { openExternalUrl } from "../../../platform/tauri/opener";
import { IS_MAC } from "../../../platform/tauri/platform";
import { loadOpenLinksInApp } from "../../settings/model/displayPrefs";
import { isBrowsableUrl } from "./browserUrl";
import { openBrowserTab } from "./browserStore";

type Modifiers = Pick<MouseEvent, "metaKey" | "ctrlKey">;

export type LinkTarget = "browser" | "external";

/**
 * Where a clicked link goes. The setting picks the usual target; holding the
 * platform modifier (⌘ on macOS, Ctrl elsewhere) picks the other one. Only
 * web pages can open in the built-in browser.
 */
export function linkTarget(
  url: string,
  preferBrowser: boolean,
  modifiers?: Modifiers | null,
  mac = IS_MAC,
): LinkTarget {
  if (!/^https?:/i.test(url) || !isBrowsableUrl(url)) return "external";
  const flipped = !!modifiers && (mac ? modifiers.metaKey : modifiers.ctrlKey);
  return preferBrowser !== flipped ? "browser" : "external";
}

/** Open a link the user clicked in app content (chat, terminal). */
export function openLink(url: string, modifiers?: Modifiers | null): Promise<void> {
  if (linkTarget(url, loadOpenLinksInApp(), modifiers) === "browser") {
    openBrowserTab(url);
    return Promise.resolve();
  }
  return openExternalUrl(url);
}
