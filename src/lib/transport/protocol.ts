/** Companion wire protocol (host desktop <-> thin iPad client).
 *
 * MERGE NOTE (upstream-friendly): this file is additive-only. It introduces
 * no changes to existing desktop behavior — the desktop keeps using
 * LocalTransport, and every name below is a plain string so future upstream
 * commands keep working without edits here.
 *
 * Transport: WebSocket (works over LAN and Tailscale — both are just IP).
 *   ws://<host>:<port>/v1/connect?token=<pairing-token>
 *
 * Frames (JSON text):
 *   client -> host  { id, type: "invoke", command, args? }
 *   host   -> client { id, type: "result", ok: true, payload } |
 *                      { id, type: "result", ok: false, error }
 *   host   -> client { type: "event", event, payload }
 *
 * Sync model: the host stays the single source of truth (sessions, SQLite,
 * filesystem, agent CLIs). The companion is a thin client: same React UI,
 * but every `invoke` is forwarded and every backend event (`harness-stdout`,
 * `pty-data`, ...) is re-broadcast. No separate sync protocol is needed for
 * v1 — realtime streaming IS the sync.
 */

export const COMPANION_PROTO_VERSION = 1;

/** Default host port. IANA-unassigned in the dynamic range; no conflict. */
export const COMPANION_PORT_DEFAULT = 17233;

export const COMPANION_WS_PATH = "/v1/connect";

/** Pairing deep-link / QR payload scheme. */
export const PAIR_URL_SCHEME = "monocode://pair";

export type RpcRequest = {
  id: number;
  type: "invoke";
  command: string;
  args?: Record<string, unknown>;
};

export type RpcResult =
  | { id: number; type: "result"; ok: true; payload: unknown }
  | { id: number; type: "result"; ok: false; error: string };

export type RpcEvent = {
  type: "event";
  event: string;
  payload: unknown;
};

export type RpcIncoming = RpcResult | RpcEvent;

export function isRpcResult(message: RpcIncoming): message is RpcResult {
  return message.type === "result";
}

/**
 * Binary envelope for the two byte-returning commands (`read_binary_file`,
 * `fetch_inbox_media`). Locally Tauri delivers a real ArrayBuffer; over the
 * JSON socket the host wraps bytes as `{ __bytes: "<base64>" }` and
 * RemoteTransport decodes back to ArrayBuffer, so call sites stay identical.
 */
export const BYTES_ENVELOPE_KEY = "__bytes";

export function isBytesEnvelope(value: unknown): value is {
  [BYTES_ENVELOPE_KEY]: string;
} {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const keys = Object.keys(value);
  return (
    keys.length === 1 &&
    keys[0] === BYTES_ENVELOPE_KEY &&
    typeof (value as Record<string, unknown>)[BYTES_ENVELOPE_KEY] ===
      "string"
  );
}

export function decodeBytesEnvelope(envelope: {
  [BYTES_ENVELOPE_KEY]: string;
}): ArrayBuffer {
  const binary = atob(envelope[BYTES_ENVELOPE_KEY]);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes.buffer;
}

/**
 * Window-chrome commands that only make sense on the host that owns the
 * window. In remote mode these resolve locally as no-ops instead of being
 * forwarded. Everything else is forwarded verbatim, so new upstream commands
 * automatically work over the companion link without edits here.
 */
export const LOCAL_ONLY_COMMANDS: ReadonlySet<string> = new Set([
  "set_window_background_blur",
  "set_traffic_lights_visible",
  "set_dock_badge",
  "confirm_quit",
  "hide_window",
  "destroy_window",
  "enable_window_glass",
  "open_new_window",
]);

export function isLocalOnlyCommand(command: string): boolean {
  return LOCAL_ONLY_COMMANDS.has(command);
}

export type PairingDetails = {
  host: string;
  port: number;
  token: string;
  /**
   * True when the host is reached through `tailscale serve` (or any other
   * TLS-terminating proxy): dial with wss:// instead of ws://.
   *
   * Tailscale path (recommended over plain LAN when leaving home):
   *   tailscale serve --bg --https=443 http://localhost:17233
   * then pair with host=<machine>.<tailnet>.ts.net, port=443, secure=true.
   * The daemon terminates outer TLS (auto-provisioned cert) and proxies the
   * WebSocket upgrade through to this server. Raw-TCP alternative:
   *   tailscale serve --bg --tcp=17233 tcp://localhost:17233
   * (WireGuard already encrypts; keep secure=false and use the MagicDNS name
   * or 100.x address as host.)
   */
  secure?: boolean;
  /** Other live route (LAN if `host` is tailnet, or the reverse). */
  altHost?: string;
};

const TOKEN_PATTERN = /^[A-Za-z0-9_-]{16,256}$/;

export function isPairingToken(value: string): boolean {
  return TOKEN_PATTERN.test(value);
}

/** Build the QR / deep-link payload, e.g. monocode://pair?host=..&port=..&token=..&v=1 */
export function buildPairUrl(details: PairingDetails): string {
  const params = new URLSearchParams({
    host: details.host,
    port: String(details.port),
    token: details.token,
    v: String(COMPANION_PROTO_VERSION),
  });
  if (details.secure) params.set("secure", "1");
  if (details.altHost && details.altHost !== details.host) {
    params.set("alt", details.altHost);
  }
  return `${PAIR_URL_SCHEME}?${params.toString()}`;
}

/** Parse a pairing URL back. Returns null for foreign schemes / bad tokens. */
export function parsePairUrl(url: string): PairingDetails | null {
  if (!url.startsWith(`${PAIR_URL_SCHEME}?`)) return null;
  let params: URLSearchParams;
  try {
    // Swap only the scheme so the rest parses as a normal hierarchical URL.
    params = new URL(url.replace(`${PAIR_URL_SCHEME}://`, "https://"))
      .searchParams;
  } catch {
    return null;
  }
  const host = (params.get("host") ?? "").trim();
  const port = Number(params.get("port") ?? "");
  const token = params.get("token") ?? "";
  if (!host || !Number.isInteger(port) || port <= 0 || port > 65535) {
    return null;
  }
  if (!isPairingToken(token)) return null;
  const secure = params.get("secure");
  const altHost = (params.get("alt") ?? "").trim();
  return {
    host,
    port,
    token,
    ...(secure === "1" || secure === "true" ? { secure: true as const } : {}),
    ...(altHost && altHost !== host ? { altHost } : {}),
  };
}

/**
 * Dial URL for the companion. ws:// for direct LAN, wss:// through
 * `tailscale serve --https` (or any TLS-terminating proxy).
 */
export function buildCompanionWsUrl(details: PairingDetails): string {
  const scheme = details.secure ? "wss" : "ws";
  const params = new URLSearchParams({
    token: details.token,
    v: String(COMPANION_PROTO_VERSION),
  });
  return `${scheme}://${details.host}:${details.port}${COMPANION_WS_PATH}?${params.toString()}`;
}

/** Tailnet CGNAT (100.64/10), Tailscale IPv6 (fd7a:115c:a1e0::/48), or MagicDNS. */
export function isTailnetHost(host: string): boolean {
  const trimmed = host.trim().replace(/^\[|\]$/g, "");
  if (trimmed.endsWith(".ts.net") || trimmed.toLowerCase() === "ts.net") {
    return true;
  }
  const v4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(trimmed);
  if (v4) {
    const first = Number(v4[1]);
    const second = Number(v4[2]);
    return first === 100 && second >= 64 && second <= 127;
  }
  const lower = trimmed.toLowerCase();
  return lower.startsWith("fd7a:115c:a1e0:");
}

/**
 * Hosts to try, LAN first. Scanning the Tailscale QR used to dial only
 * 100.x, and the iPad tsnet path can hang — the LAN alt in the same code
 * already reaches the Mac listener.
 */
export function pairingDialOrder(details: PairingDetails): PairingDetails[] {
  const hosts: string[] = [];
  for (const host of [details.host, details.altHost]) {
    const trimmed = host?.trim() ?? "";
    if (trimmed && !hosts.includes(trimmed)) hosts.push(trimmed);
  }
  hosts.sort(
    (left, right) => Number(isTailnetHost(left)) - Number(isTailnetHost(right)),
  );
  return hosts.map((host) => {
    const altHost = hosts.find((other) => other !== host);
    return {
      host,
      port: details.port,
      token: details.token,
      ...(details.secure ? { secure: true as const } : {}),
      ...(altHost ? { altHost } : {}),
    };
  });
}

export function companionDialUrls(details: PairingDetails): string[] {
  return pairingDialOrder(details).map((item) => buildCompanionWsUrl(item));
}

/**
 * Pair-mode dial URL: no token yet. The socket may only invoke `pair_claim`
 * with the host's 6-digit code; success returns the real token and upgrades
 * the socket to fully paired.
 */
export function buildPairWsUrl(details: {
  host: string;
  port: number;
  secure?: boolean;
}): string {
  const scheme = details.secure ? "wss" : "ws";
  return `${scheme}://${details.host}:${details.port}${COMPANION_WS_PATH}?pair=1`;
}

export type PairClaimPayload = {
  token: string;
  /** LAN IP advertised by the host when that route is on. */
  lanIp?: string | null;
  /** Tailnet name or 100.x address advertised when Tailscale is on. */
  tailnetHost?: string | null;
};

/** The other live route, if the host advertised one that isn't `connectedHost`. */
export function otherPairingHost(
  connectedHost: string,
  extras: { lanIp?: string | null; tailnetHost?: string | null },
): string | undefined {
  const connected = connectedHost.trim();
  for (const candidate of [extras.lanIp, extras.tailnetHost]) {
    const host = candidate?.trim();
    if (host && host !== connected) return host;
  }
  return undefined;
}

export function normalizePairCode(raw: string): string {
  return raw.replace(/\D/g, "");
}

export function isPairCode(value: string): boolean {
  return normalizePairCode(value).length === 6;
}
