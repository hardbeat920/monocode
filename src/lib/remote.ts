import { invoke } from "@tauri-apps/api/core";

export type RemoteStatus = {
  enabled: boolean;
  port: number;
  token: string;
  urls: string[];
};

const TOKEN_KEY = "monocode.remoteAccessToken";
const ENABLED_KEY = "monocode.remoteAccessEnabled";

function createRemoteAccessToken(): string {
  return crypto.randomUUID();
}

export function loadRemoteAccessEnabled(): boolean {
  try {
    return localStorage.getItem(ENABLED_KEY) === "1";
  } catch {
    return false;
  }
}

export function saveRemoteAccessEnabled(enabled: boolean) {
  try {
    if (enabled) {
      localStorage.setItem(ENABLED_KEY, "1");
    } else {
      localStorage.removeItem(ENABLED_KEY);
    }
  } catch {
    // private mode / quota
  }
}

export async function initRemoteAccess(): Promise<void> {
  if (!loadRemoteAccessEnabled()) return;
  try {
    const status = await remoteStatus();
    if (status.enabled) return;
    await remoteStart();
  } catch (error) {
    console.warn("Failed to restore remote access:", error);
  }
}

export function loadRemoteAccessToken(): string {
  try {
    const existing = localStorage.getItem(TOKEN_KEY)?.trim();
    if (existing) return existing;
    const created = createRemoteAccessToken();
    localStorage.setItem(TOKEN_KEY, created);
    return created;
  } catch {
    return createRemoteAccessToken();
  }
}

export function saveRemoteAccessToken(token: string) {
  try {
    const trimmed = token.trim();
    if (trimmed) {
      localStorage.setItem(TOKEN_KEY, trimmed);
    } else {
      localStorage.removeItem(TOKEN_KEY);
    }
  } catch {
    // private mode / quota
  }
}

export function remoteStatus(): Promise<RemoteStatus> {
  return invoke<RemoteStatus>("remote_status");
}

export function remoteStart(port?: number, token?: string): Promise<RemoteStatus> {
  const args: { port?: number; token?: string } = {};
  if (port != null) args.port = port;
  const preferred = token?.trim() || loadRemoteAccessToken();
  if (preferred) args.token = preferred;
  return invoke<RemoteStatus>("remote_start", args).then((status) => {
    saveRemoteAccessEnabled(true);
    return status;
  });
}

export function remoteStop(): Promise<RemoteStatus> {
  return invoke<RemoteStatus>("remote_stop").then((status) => {
    saveRemoteAccessEnabled(false);
    return status;
  });
}

export function remoteRegenerateToken(): Promise<RemoteStatus> {
  return invoke<RemoteStatus>("remote_regenerate_token");
}

export function remoteSetToken(token: string): Promise<RemoteStatus> {
  return invoke<RemoteStatus>("remote_set_token", { token: token.trim() });
}

export function remoteUrlWithToken(url: string, token: string): string {
  const parsed = new URL(url);
  parsed.searchParams.set("token", token);
  return parsed.toString();
}
