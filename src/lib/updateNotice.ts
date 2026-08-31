const INSTALLED_UPDATE_KEY = "monocode.installedUpdate";

export type UpdateNoticeStore = Pick<
  Storage,
  "getItem" | "setItem" | "removeItem"
>;

export type InstalledUpdate = {
  version: string;
};

export function rememberInstalledUpdate(
  version: string,
  store?: UpdateNoticeStore,
): void {
  const normalized = version.trim();
  if (!normalized) return;
  try {
    const target = store ?? window.localStorage;
    target.setItem(
      INSTALLED_UPDATE_KEY,
      JSON.stringify({ version: normalized }),
    );
  } catch {
    return;
  }
}

export function consumeInstalledUpdate(
  store?: UpdateNoticeStore,
): InstalledUpdate | null {
  try {
    const target = store ?? window.localStorage;
    const stored = target.getItem(INSTALLED_UPDATE_KEY);
    if (stored == null) return null;
    target.removeItem(INSTALLED_UPDATE_KEY);
    return parseInstalledUpdate(JSON.parse(stored));
  } catch {
    return null;
  }
}

function parseInstalledUpdate(raw: unknown): InstalledUpdate | null {
  if (!raw || typeof raw !== "object") return null;
  const version = (raw as Record<string, unknown>).version;
  if (typeof version !== "string" || !version.trim()) return null;
  return { version: version.trim() };
}
