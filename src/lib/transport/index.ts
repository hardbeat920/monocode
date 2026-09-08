import { IS_IPAD } from "../platform";
import { LocalTransport } from "./local";
import {
  isCompanionAuthFailure,
  RemoteTransport,
  type RemoteOptions,
} from "./remote";
import {
  companionDialUrls,
  isLocalOnlyCommand,
  otherPairingHost,
  type PairingDetails,
} from "./protocol";
import type {
  Transport,
  TransportEventHandler,
  TransportMode,
  UnlistenFn,
} from "./types";

export type { Transport, TransportEventHandler, TransportMode, UnlistenFn };
export { LocalTransport } from "./local";
export { RemoteTransport, claimPairingCode, isCompanionAuthFailure } from "./remote";
export type { ClaimOptions } from "./remote";
export * from "./dialog";
export * from "./protocol";

/**
 * Process-wide backend binding. Desktop boots (and stays) on LocalTransport;
 * the companion switches to RemoteTransport after pairing. Call sites import
 * `invoke`/`listen` from here instead of `@tauri-apps/api/*` directly —
 * that one-line import swap per file is the entire rebase surface for
 * upstream updates.
 */

let active: Transport = new LocalTransport();
let companion: RemoteTransport | null = null;

export type CompanionStatus =
  | "local"
  | "connecting"
  | "connected"
  | "reconnecting"
  | "failed";

const statusHandlers = new Set<(status: CompanionStatus) => void>();
let companionStatus: CompanionStatus = "local";
let companionUnlisten: UnlistenFn | null = null;
let companionError: string | null = null;

export function getCompanionError(): string | null {
  return companionError;
}

function setCompanionStatus(next: CompanionStatus): void {
  if (companionStatus === next) return;
  companionStatus = next;
  for (const handler of [...statusHandlers]) {
    try {
      handler(next);
    } catch {
      // Status observers must never break the transport switch.
    }
  }
}

export function getTransport(): Transport {
  return active;
}

export function getTransportMode(): TransportMode {
  return active.mode;
}

/** True once a companion link is active, even while reconnecting. */
export function isRemote(): boolean {
  return companion != null;
}

export function getCompanionStatus(): CompanionStatus {
  return companionStatus;
}

export function onCompanionStatusChange(
  handler: (status: CompanionStatus) => void,
): UnlistenFn {
  statusHandlers.add(handler);
  return () => {
    statusHandlers.delete(handler);
  };
}

/** Drop-in for `invoke` from `@tauri-apps/api/core`. */
export function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (companion && isLocalOnlyCommand(command)) {
    // Window chrome belongs to the host; silently absorb on the companion.
    return Promise.resolve(undefined as T);
  }
  return active.invoke<T>(command, args);
}

/** Drop-in for `listen` from `@tauri-apps/api/event`. */
export function listen<T>(
  event: string,
  handler: TransportEventHandler<T>,
): Promise<UnlistenFn> {
  return active.listen<T>(event, handler);
}

/**
 * Switch the UI to a paired host. Old link (if any) is disposed first.
 * Reconnects automatically; observe via onCompanionStatusChange.
 */
export function connectCompanion(
  details: PairingDetails,
  options: RemoteOptions = {},
): RemoteTransport {
  disconnectCompanion();
  const transport = new RemoteTransport(companionDialUrls(details), options);
  companion = transport;
  active = transport;
  companionError = null;
  setCompanionStatus("connecting");
  companionUnlisten = transport.onStatusChange((status) => {
    if (companion !== transport) return;
    if (status === "open") {
      companionError = null;
      setCompanionStatus("connected");
      persistAdvertisedAltHost();
      return;
    }
    if (
      status === "closed" &&
      companionError &&
      isCompanionAuthFailure(companionError)
    ) {
      setCompanionStatus("failed");
      return;
    }
    setCompanionStatus("reconnecting");
  });
  void transport.connect().catch((error) => {
    if (companion !== transport) return;
    companionError = error instanceof Error ? error.message : String(error);
    setCompanionStatus(
      isCompanionAuthFailure(companionError) ? "failed" : "reconnecting",
    );
  });
  return transport;
}

/** Redial the saved host. Null when this install has no pairing. */
export function reconnectCompanion(
  options: RemoteOptions = {},
): RemoteTransport | null {
  const saved = loadPairing();
  if (!saved) return null;
  return connectCompanion(saved, options);
}

/** Swap LAN ↔ tailnet host when the pairing URL carried both. */
export function switchCompanionRoute(
  options: RemoteOptions = {},
): RemoteTransport | null {
  const saved = loadPairing();
  if (!saved?.altHost) return null;
  return connectAndRememberCompanion(
    {
      ...saved,
      host: saved.altHost,
      altHost: saved.host,
    },
    options,
  );
}

/** Drop the companion link and return to in-process desktop behavior. */
export function disconnectCompanion(): void {
  companionUnlisten?.();
  companionUnlisten = null;
  companionError = null;
  if (companion) {
    const live = companion;
    companion = null;
    live.dispose();
  }
  if (active.mode !== "local") active = new LocalTransport();
  setCompanionStatus("local");
}

const COMPANION_STORE_PREFIX = "monocode.companion.";

function storageGet(key: string): string | null {
  try {
    if (typeof localStorage === "undefined") return null;
    return localStorage.getItem(COMPANION_STORE_PREFIX + key);
  } catch {
    return null;
  }
}

function storageSet(key: string, value: string): void {
  try {
    if (typeof localStorage === "undefined") return;
    localStorage.setItem(COMPANION_STORE_PREFIX + key, value);
  } catch {
    // Private browsing etc: pairing just won't persist.
  }
}

function storageRemove(key: string): void {
  try {
    if (typeof localStorage === "undefined") return;
    localStorage.removeItem(COMPANION_STORE_PREFIX + key);
  } catch {
    // Private browsing etc.
  }
}

function storageClear(): void {
  for (const key of ["host", "port", "token", "secure", "alt"]) {
    storageRemove(key);
  }
}

export function savePairing(details: PairingDetails): void {
  storageSet("host", details.host);
  storageSet("port", String(details.port));
  storageSet("token", details.token);
  if (details.secure) storageSet("secure", "1");
  else storageRemove("secure");
  if (details.altHost && details.altHost !== details.host) {
    storageSet("alt", details.altHost);
  } else storageRemove("alt");
}

/**
 * Once the socket is up, remember the host's other live route so the
 * iPad can switch LAN ↔ Tailscale without opening Companion settings.
 * Retries: after a host restart the tailnet address can lag the LAN
 * listener by a few seconds.
 */
function persistAdvertisedAltHost(attempt = 0): void {
  const saved = loadPairing();
  if (!saved) return;
  void invoke<{ lanIp?: string | null; tailnetHost?: string | null }>(
    "remote_status",
  )
    .then((status) => {
      if (loadPairing()?.host !== saved.host) return;
      const altHost = otherPairingHost(saved.host, status);
      if (!altHost) {
        if (attempt < 8 && typeof window !== "undefined") {
          window.setTimeout(() => persistAdvertisedAltHost(attempt + 1), 1500);
        }
        return;
      }
      if (altHost === saved.altHost) return;
      savePairing({ ...saved, altHost });
    })
    .catch(() => {
      if (attempt < 8 && typeof window !== "undefined") {
        window.setTimeout(() => persistAdvertisedAltHost(attempt + 1), 1500);
      }
    });
}

export function loadPairing(): PairingDetails | null {
  const host = storageGet("host") ?? "";
  const port = Number(storageGet("port") ?? "");
  const token = storageGet("token") ?? "";
  if (!host || !Number.isInteger(port) || port <= 0 || port > 65535) {
    return null;
  }
  if (!token) return null;
  const secure = storageGet("secure") === "1";
  const altHost = (storageGet("alt") ?? "").trim();
  return {
    host,
    port,
    token,
    ...(secure ? { secure: true as const } : {}),
    ...(altHost && altHost !== host ? { altHost } : {}),
  };
}

export function clearPairing(): void {
  storageClear();
}

const COMPANION_MODE_KEY = "mode";

/**
 * Explicit "this install is a companion" flag. Set only by the pairing flow
 * on the iPad — never by the desktop — so a saved pairing alone can never
 * hijack a desktop into remote mode on a future update.
 */
export function setCompanionMode(companion: boolean): void {
  if (companion) storageSet(COMPANION_MODE_KEY, "1");
  else {
    try {
      if (typeof localStorage === "undefined") return;
      localStorage.removeItem(COMPANION_STORE_PREFIX + COMPANION_MODE_KEY);
    } catch {
      // Private browsing etc.
    }
  }
}

export function loadCompanionMode(): boolean {
  return storageGet(COMPANION_MODE_KEY) === "1";
}

/**
 * iPad app, or any install that paired as a thin client. Use this — not
 * `isRemote()` — for chrome that must stay hidden after disconnect.
 */
export function isCompanionClient(): boolean {
  return IS_IPAD || loadCompanionMode();
}

/** Pair, remember, and dial — the iPad pairing screen's one call. */
export function connectAndRememberCompanion(
  details: PairingDetails,
  options: RemoteOptions = {},
): RemoteTransport {
  savePairing(details);
  setCompanionMode(true);
  return connectCompanion(details, options);
}

/** Disconnect and forget everything: this install is a desktop again. */
export function forgetCompanion(): void {
  disconnectCompanion();
  clearPairing();
  setCompanionMode(false);
}

/**
 * Boot gate for companion installs. Resolves true when invokes will reach
 * the host (local mode, or the link opened in time). Resolves false on
 * timeout — boot proceeds into a disconnected shell that reloads itself
 * once the link opens (see main.tsx).
 */
export function waitForCompanionLink(timeoutMs = 8000): Promise<boolean> {
  if (!loadCompanionMode()) return Promise.resolve(true);
  const saved = loadPairing();
  if (!saved) return Promise.resolve(true);
  if (!companion) connectCompanion(saved);
  if (getCompanionStatus() === "connected") return Promise.resolve(true);
  return new Promise((resolve) => {
    let done = false;
    const timer = setTimeout(() => {
      if (done) return;
      done = true;
      unlisten();
      resolve(false);
    }, timeoutMs);
    const unlisten = onCompanionStatusChange((status) => {
      if (done) return;
      if (status === "connected" || status === "local") {
        done = true;
        clearTimeout(timer);
        unlisten();
        resolve(status === "connected");
      }
    });
  });
}
