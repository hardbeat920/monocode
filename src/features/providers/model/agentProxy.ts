const STORAGE_KEY = "monocode.agentProxyUrl";

export function parseAgentProxyUrl(value: string): string | null {
  const address = value.trim();
  if (!address || address.length > 256) return null;
  try {
    const url = new URL(address);
    if (
      (url.protocol !== "http:" && url.protocol !== "https:") ||
      !url.hostname ||
      url.username ||
      url.password ||
      url.pathname !== "/" ||
      url.search ||
      url.hash
    ) {
      return null;
    }
    return url.origin;
  } catch {
    return null;
  }
}

export function loadAgentProxyUrl(): string | null {
  try {
    const value = localStorage.getItem(STORAGE_KEY);
    return value ? parseAgentProxyUrl(value) : null;
  } catch {
    return null;
  }
}

export function saveAgentProxyUrl(value: string | null): boolean {
  const address = value === null ? null : parseAgentProxyUrl(value);
  if (value !== null && address === null) return false;
  try {
    if (address) localStorage.setItem(STORAGE_KEY, address);
    else localStorage.removeItem(STORAGE_KEY);
    return true;
  } catch {
    return false;
  }
}
