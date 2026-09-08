import { useCallback, useEffect, useState } from "react";
import QRCode from "react-qr-code";
import {
  Heading,
  Row,
  SecondaryButton,
  Toggle,
} from "../chrome/settingsControls";
import {
  EmbedLoginSection,
  GoogleLoginBlock,
  logoutLocalEmbed,
  TailnetStatusCard,
  useLocalEmbed,
} from "./TailnetStatusCard";
import {
  buildPairUrl,
  COMPANION_PORT_DEFAULT,
  connectAndRememberCompanion,
  disconnectCompanion,
  forgetCompanion,
  loadCompanionMode,
  loadPairing,
  otherPairingHost,
  parsePairUrl,
  reconnectCompanion,
  savePairing,
  switchCompanionRoute,
  type PairingDetails,
} from "../lib/transport";
import {
  getCompanionStatus,
  invoke,
  isRemote,
  onCompanionStatusChange,
  type CompanionStatus,
} from "../lib/transport";

function lanOn(status: RemoteStatus | null): boolean {
  if (!status?.enabled) return false;
  if (status.lan === true) return true;
  if (status.lan === false) return false;
  return status.mode === "lan";
}

function tailscaleOn(status: RemoteStatus | null): boolean {
  if (!status?.enabled) return false;
  if (status.tailscale === true) return true;
  if (status.tailscale === false) return false;
  return status.mode === "tailscale";
}

function withAlt(
  details: PairingDetails,
  alt: string | null | undefined,
): PairingDetails {
  const host = alt?.trim();
  if (!host || host === details.host) return details;
  return { ...details, altHost: host };
}

type RemotePairing = {
  port: number;
  token: string;
  lanIp?: string | null;
  tailnetHost?: string | null;
  version: number;
};

type RemoteStatus = {
  enabled: boolean;
  lan?: boolean;
  tailscale?: boolean;
  mode: "tailscale" | "lan";
  port: number;
  version: number;
  lanIp?: string | null;
  tailnetHost?: string | null;
  systemTailscale?: boolean;
};

type TailnetStatus = {
  installed: boolean;
  running: boolean;
  loginName?: string | null;
  displayName?: string | null;
  tailnetName?: string | null;
  dnsName?: string | null;
  magicDns: boolean;
};

type PeerCount = {
  connected: number;
};

type PairCode = {
  code: string;
  expiresIn: number;
};

type EmbedStatus = {
  running: boolean;
  authorized: boolean;
  tailnetIp?: string | null;
  loginUrl?: string | null;
  error?: string | null;
  loginName?: string | null;
  displayName?: string | null;
  tailnetName?: string | null;
  hostname?: string | null;
};

function useCompanionStatus(): CompanionStatus {
  const [status, setStatus] = useState<CompanionStatus>(() =>
    getCompanionStatus(),
  );
  useEffect(() => onCompanionStatusChange(setStatus), []);
  return status;
}

async function copyText(value: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(value);
    return true;
  } catch {
    // Clipboard unavailable (permissions, insecure context): select manually.
    return false;
  }
}

function companionStatusLabel(status: CompanionStatus): string {
  switch (status) {
    case "connected":
      return "Connected";
    case "connecting":
    case "reconnecting":
      return "Reconnecting…";
    case "failed":
    case "local":
      return "Disconnected";
    default: {
      const _exhaustive: never = status;
      return _exhaustive;
    }
  }
}

/**
 * iPad / parked-companion status. Must not fall through to the Mac host
 * pairing UI after disconnect (`isRemote()` is then false).
 */
function CompanionClientCard({
  linkStatus,
  onDisconnect,
  onReconnect,
  onUnpair,
}: {
  linkStatus: CompanionStatus;
  onDisconnect: () => void;
  onReconnect: () => void;
  onUnpair: () => void;
}) {
  const [saved, setSaved] = useState(loadPairing);
  useEffect(() => {
    setSaved(loadPairing());
  }, [linkStatus]);
  useEffect(() => {
    if (linkStatus !== "connected") return;
    let cancelled = false;
    void invoke<RemoteStatus>("remote_status")
      .then((status) => {
        if (cancelled) return;
        const current = loadPairing();
        if (!current) return;
        const altHost = otherPairingHost(current.host, {
          lanIp: status.lanIp,
          tailnetHost: status.tailnetHost,
        });
        if (!altHost || altHost === current.altHost) return;
        savePairing({ ...current, altHost });
        setSaved(loadPairing());
      })
      .catch(() => {
        // Older hosts omit route fields; the saved pairing still works.
      });
    return () => {
      cancelled = true;
    };
  }, [linkStatus]);
  const { embed, logout } = useLocalEmbed(true);
  const [copied, setCopied] = useState<string | null>(null);
  const [loggingOut, setLoggingOut] = useState(false);
  const onCopy = useCallback(async (key: string, value: string) => {
    if (await copyText(value)) {
      setCopied(key);
      window.setTimeout(() => {
        setCopied((current) => (current === key ? null : current));
      }, 1500);
    }
  }, []);
  const hostLine = saved ? `${saved.host}:${saved.port}` : "No saved host";
  const connected = linkStatus === "connected";
  return (
    <>
      <Row
        label="Status"
        description="Sessions, files, and agents run on the paired Mac."
      >
        <span className="text-[13px] text-content">
          {companionStatusLabel(linkStatus)}
        </span>
      </Row>
      <Row
        label="Host"
        description={
          saved?.altHost
            ? `Also ${saved.altHost}:${saved.port}`
            : undefined
        }
      >
        <span className="max-w-xs break-all text-right font-mono text-[12px] text-content/70">
          {hostLine}
        </span>
      </Row>
      {saved?.altHost ? (
        <Row
          label="Route"
          description="Switch between this Mac's LAN address and Tailscale address without pairing again."
        >
          <SecondaryButton onClick={() => switchCompanionRoute()}>
            Use other route
          </SecondaryButton>
        </Row>
      ) : null}
      <Row
        label="Connection"
        description={
          connected
            ? "Drop the live link. Pairing is kept so you can reconnect."
            : "Redial the saved host."
        }
      >
        {connected ? (
          <SecondaryButton onClick={onDisconnect}>Disconnect</SecondaryButton>
        ) : (
          <SecondaryButton onClick={onReconnect} disabled={!saved}>
            Reconnect
          </SecondaryButton>
        )}
      </Row>
      <Row
        label="Pairing"
        description="Forget this Mac. The next launch asks you to pair again."
      >
        <SecondaryButton danger onClick={onUnpair}>
          Unpair
        </SecondaryButton>
      </Row>
      <Heading title="Tailscale" />
      <p className="pb-2 text-[12px] leading-relaxed text-content/45">
        Sign in with Google so this device can reach the Mac from anywhere.
        Nothing to install.
      </p>
      <EmbedLoginSection
        embed={embed}
        copied={copied === "login-url" || copied === "tailnet-ip"}
        onCopy={(key, value) => void onCopy(key, value)}
        loggingOut={loggingOut}
        onLogout={() => {
          setLoggingOut(true);
          void logout().finally(() => setLoggingOut(false));
        }}
      />
    </>
  );
}

/**
 * Host-side pairing screen (desktop Settings > Companion). Shows how to
 * reach this machine from the iPad over LAN or Tailscale, plus the token.
 * Rendered as read-only pairing details on a connected companion instead —
 * a companion never serves its own link.
 */
export function CompanionPage() {
  const linkStatus = useCompanionStatus();
  const [remote, setRemote] = useState(isRemote());
  const [status, setStatus] = useState<RemoteStatus | null>(null);
  const [pairing, setPairing] = useState<RemotePairing | null>(null);
  const [tailnet, setTailnet] = useState<TailnetStatus | null>(null);
  const [embed, setEmbed] = useState<EmbedStatus | null>(null);
  const [peers, setPeers] = useState<PeerCount | null>(null);
  const [pairCode, setPairCode] = useState<PairCode | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);
  const [loggingOut, setLoggingOut] = useState(false);

  useEffect(
    () =>
      onCompanionStatusChange(() => {
        setRemote(isRemote());
      }),
    [],
  );

  const refresh = useCallback(async () => {
    setError(null);
    try {
      const [nextStatus, nextPairing, nextTailnet] = await Promise.all([
        invoke<RemoteStatus>("remote_status"),
        invoke<RemotePairing>("remote_pairing"),
        invoke<TailnetStatus>("remote_tailnet"),
      ]);
      setStatus(nextStatus);
      setPairing(nextPairing);
      setTailnet(nextTailnet);
      try {
        setEmbed(await invoke<EmbedStatus>("remote_embed_status"));
      } catch {
        // Mobile shells have no embedded node; the card renders offline.
        setEmbed(null);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    if (!remote) void refresh();
  }, [refresh, remote]);

  // Live status: the node moves through starting → login → online on its
  // own, and companions come and go — poll lightly while the link is up.
  useEffect(() => {
    if (remote || !status?.enabled) return;
    let stopped = false;
    const poll = async () => {
      try {
        const [nextEmbed, nextPeers] = await Promise.all([
          invoke<EmbedStatus>("remote_embed_status"),
          invoke<PeerCount>("remote_peers"),
        ]);
        if (!stopped) {
          setEmbed(nextEmbed);
          setPeers(nextPeers);
        }
      } catch {
        // Link went away mid-poll; the next refresh recovers.
      }
    };
    const timer = window.setInterval(() => void poll(), 4000);
    void poll();
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [remote, status?.enabled]);

  const onToggle = useCallback(
    async (route: "tailscale" | "lan") => {
      setBusy(true);
      setError(null);
      try {
        const on = route === "lan" ? lanOn(status) : tailscaleOn(status);
        await invoke<RemotePairing>("remote_set_route", {
          route,
          enabled: !on,
        });
        await refresh();
      } catch (err) {
        setError(err instanceof Error ? err.message : String(err));
      } finally {
        setBusy(false);
      }
    },
    [refresh, status],
  );

  const onSystemToggle = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke<RemoteStatus>("remote_set_system_tailscale", {
        enabled: status?.systemTailscale !== true,
      });
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, [refresh, status]);

  const refreshPairCode = useCallback(async () => {
    try {
      const next = await invoke<{ code: string; expiresIn: number }>(
        "remote_pairing_code",
      );
      setPairCode(next);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  useEffect(() => {
    if (!remote && status?.enabled) void refreshPairCode();
  }, [refreshPairCode, remote, status?.enabled]);

  const onCopy = useCallback(async (key: string, value: string) => {
    if (await copyText(value)) {
      setCopied(key);
      window.setTimeout(() => {
        setCopied((current) => (current === key ? null : current));
      }, 1500);
    }
  }, []);

  if (loadCompanionMode() || remote) {
    return (
      <CompanionClientCard
        linkStatus={linkStatus}
        onDisconnect={() => disconnectCompanion()}
        onReconnect={() => {
          reconnectCompanion();
        }}
        onUnpair={() => {
          forgetCompanion();
          window.location.reload();
        }}
      />
    );
  }

  const port = pairing?.port ?? status?.port ?? COMPANION_PORT_DEFAULT;
  const lanHost = pairing?.lanIp?.trim() || status?.lanIp?.trim() || null;
  // Prefer live tailnet identity over the pairing snapshot's host.
  const tailnetHost =
    tailnet?.dnsName?.trim() || pairing?.tailnetHost?.trim() || null;
  const lanEnabled = lanOn(status);
  const tailscaleEnabled = tailscaleOn(status);
  const lanBase: PairingDetails | null =
    lanEnabled && lanHost && pairing
      ? { host: lanHost, port, token: pairing.token }
      : null;
  // Tailscale route: the embedded node first, the Mac client's served
  // address as fallback when the node is still joining.
  const cliDetails: PairingDetails | null =
    tailscaleEnabled && tailnetHost && pairing && tailnet?.running
      ? { host: tailnetHost, port, token: pairing.token }
      : null;
  // Embedded node: pair with the node's own tailnet IP (stable per node).
  const embedDetails: PairingDetails | null =
    tailscaleEnabled && embed?.tailnetIp && pairing
      ? { host: embed.tailnetIp, port, token: pairing.token }
      : null;
  const tailscaleBase = embedDetails ?? cliDetails;
  const lanDetails = lanBase
    ? withAlt(lanBase, tailscaleBase?.host)
    : null;
  const tailscaleRoute = tailscaleBase ? withAlt(tailscaleBase, lanHost) : null;

  const routes = [
    lanEnabled ? "Local network" : null,
    tailscaleEnabled ? "Tailscale" : null,
  ]
    .filter(Boolean)
    .join(" + ");
  const statusDetail = status?.enabled
    ? `${routes || "Companion"} · port ${status.port}${
        tailscaleEnabled
          ? embed?.running
            ? embed.authorized
              ? " · tailnet node joined"
              : " · tailnet node joining…"
            : " · tailnet node off"
          : ""
      }`
    : "Turn on a route to serve this Mac.";

  return (
    <>
      {error ? <p className="pb-2 text-[12px] text-red-400">{error}</p> : null}

      <Row label="Status" description={statusDetail}>
        <span className="text-[13px] text-content">
          {status?.enabled
            ? (peers?.connected ?? 0) > 0
              ? `Connected — ${peers?.connected} ${
                  peers?.connected === 1 ? "device" : "devices"
                }`
              : "Waiting for devices…"
            : "Off"}
        </span>
      </Row>

      {pairing ? (
        <>
          {status?.enabled ? (
            <Row
              label="Pairing code"
              description="On the iPad choose Enter code and type these digits."
            >
              {pairCode ? (
                <span
                  className="font-mono text-[22px] font-semibold tracking-[0.18em] text-content"
                  aria-label={`Pairing code ${pairCode.code}`}
                >
                  {pairCode.code}
                </span>
              ) : null}
              <SecondaryButton
                onClick={() => void refreshPairCode()}
                disabled={busy}
              >
                {pairCode ? "New code" : "Show code"}
              </SecondaryButton>
            </Row>
          ) : null}

          <Heading title="Routes" />
          <Row
            label="Tailscale"
            description={
              embed?.loginName || tailnet?.loginName
                ? `Works from anywhere. Signed in as ${
                    embed?.loginName || tailnet?.loginName
                  }.`
                : "Works from anywhere. Sign in with Google below — the iPad does the same inside MonoCode."
            }
          >
            <Toggle
              label="Tailscale"
              on={tailscaleEnabled}
              disabled={busy}
              onChange={() => void onToggle("tailscale")}
            />
          </Row>
          {tailscaleEnabled ? (
            <div className="border-b border-content/5 py-4">
              <TailnetStatusCard
                embed={embed}
                info={{
                  loginName: embed?.loginName || tailnet?.loginName,
                  displayName: embed?.displayName || tailnet?.displayName,
                  tailnetName: embed?.tailnetName || tailnet?.tailnetName,
                  hostname: embed?.hostname,
                }}
                peersConnected={peers?.connected ?? null}
                pairUrl={null}
                onCopy={(key, value) => void onCopy(key, value)}
                copied={copied === "tailnet-ip"}
              />
              {!embed?.authorized ? (
                <GoogleLoginBlock
                  loginUrl={embed?.loginUrl ?? null}
                  copied={copied === "login-url"}
                  onCopy={() => void onCopy("login-url", embed?.loginUrl ?? "")}
                />
              ) : null}
              <div className="flex items-center justify-between gap-3 pt-1">
                <p className="text-[12px] leading-relaxed text-content/45">
                  {embed?.authorized
                    ? "Sign out to use a different Google account on this Mac."
                    : "If Google shows an error about another tailnet or an existing node, reset and try again."}
                </p>
                <button
                  type="button"
                  disabled={loggingOut || busy}
                  onClick={() => {
                    setLoggingOut(true);
                    void logoutLocalEmbed()
                      .then((next) => setEmbed(next))
                      .finally(() => setLoggingOut(false));
                  }}
                  className="shrink-0 rounded-md border border-red-400/30 px-3 py-1.5 text-[12px] text-red-400 hover:bg-red-400/10 disabled:opacity-40"
                >
                  {loggingOut
                    ? "Resetting…"
                    : embed?.authorized
                      ? "Sign out of Tailscale"
                      : "Use a different Google account"}
                </button>
              </div>
              {tailscaleRoute ? (
                <RouteRow
                  details={tailscaleRoute}
                  copied={copied === "route-tailscale"}
                  onCopy={() =>
                    void onCopy("route-tailscale", buildPairUrl(tailscaleRoute))
                  }
                />
              ) : (
                <p className="pt-3 text-[12px] text-content/45">
                  Node still joining — the route appears here once it has an
                  address.
                </p>
              )}
              <div className="flex items-start gap-6 pt-4">
                <div className="min-w-0 flex-1">
                  <div className="text-[13px] font-medium text-content">
                    System Tailscale
                  </div>
                  <p className="mt-1 text-[12px] leading-relaxed text-content/45">
                    Also forward this port through the Tailscale app already
                    on this Mac, if you signed in there with Google.
                  </p>
                </div>
                <Toggle
                  label="System Tailscale"
                  on={status?.systemTailscale === true}
                  disabled={busy}
                  onChange={() => void onSystemToggle()}
                />
              </div>
            </div>
          ) : null}

          <Row
            label="Local network"
            description="Same Wi-Fi only. Nothing leaves the house — no tailnet, no login."
          >
            <Toggle
              label="Local network"
              on={lanEnabled}
              disabled={busy}
              onChange={() => void onToggle("lan")}
            />
          </Row>
          {lanEnabled ? (
            <div className="border-b border-content/5 py-4">
              {lanDetails ? (
                <RouteRow
                  details={lanDetails}
                  copied={copied === "route-lan"}
                  onCopy={() =>
                    void onCopy("route-lan", buildPairUrl(lanDetails))
                  }
                />
              ) : (
                <p className="text-[12px] text-content/45">
                  No LAN address detected — enter this Mac&apos;s IP on the
                  iPad manually.
                </p>
              )}
            </div>
          ) : null}

          <Heading title="Advanced" />
          <details className="group border-b border-content/5 py-4">
            <summary className="cursor-pointer list-none text-[13px] font-medium text-content hover:text-content">
              Token
            </summary>
            <p className="mt-1 text-[12px] leading-relaxed text-content/45">
              Rarely needed — the 6-digit code covers pairing.
            </p>
            <div className="pt-1">
              <CopyRow
                label="Token"
                value={pairing.token}
                copied={copied === "token"}
                onCopy={() => void onCopy("token", pairing.token)}
              />
            </div>
          </details>
          <details className="group border-b border-content/5 py-4">
            <summary className="cursor-pointer list-none text-[13px] font-medium text-content hover:text-content">
              Pair this device with a different host
            </summary>
            <div className="pt-1">
              <PairThisDevice />
            </div>
          </details>
        </>
      ) : (
        <p className="py-4 text-[12px] leading-relaxed text-content/45">
          Loading pairing details…
        </p>
      )}
    </>
  );
}


/**
 * First-run pairing: turns THIS device into a companion of another host.
 * This is the only entry point on a fresh install (which otherwise boots
 * into desktop mode with a local backend). On connect the app reloads into
 * remote mode; the desktop host path never uses this.
 */
function PairThisDevice() {
  const [url, setUrl] = useState("");
  const [host, setHost] = useState("");
  const [port, setPort] = useState("17233");
  const [token, setToken] = useState("");
  const [secure, setSecure] = useState(false);
  const [altHost, setAltHost] = useState("");
  const [error, setError] = useState<string | null>(null);

  const fillFromUrl = () => {
    const details = parsePairUrl(url.trim());
    if (!details) {
      setError("That is not a MonoCode pairing link.");
      return;
    }
    setError(null);
    setHost(details.host);
    setPort(String(details.port));
    setToken(details.token);
    setSecure(details.secure ?? false);
    setAltHost(details.altHost ?? "");
  };

  const connect = () => {
    const portNumber = Number(port);
    if (!host.trim()) {
      setError("Enter the host.");
      return;
    }
    if (!Number.isInteger(portNumber) || portNumber <= 0 || portNumber > 65535) {
      setError("Port must be 1–65535.");
      return;
    }
    if (!token.trim()) {
      setError("Enter the pairing token.");
      return;
    }
    try {
      connectAndRememberCompanion({
        host: host.trim(),
        port: portNumber,
        token: token.trim(),
        ...(secure ? { secure: true as const } : {}),
        ...(altHost.trim() && altHost.trim() !== host.trim()
          ? { altHost: altHost.trim() }
          : {}),
      });
      window.location.reload();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <div className="pt-1">
      <p className="text-[12px] leading-relaxed text-content/45">
        Paste the pairing URL from the host, or enter the details manually.
        Connecting reloads this device as a companion.
      </p>
      <div className="mt-2 flex gap-2">
        <input
          value={url}
          onChange={(event) => setUrl(event.target.value)}
          placeholder="monocode://pair?…"
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
          className="min-w-0 flex-1 rounded-md border border-content/15 bg-transparent px-3 py-2 font-mono text-[12px] text-content placeholder:text-content/30"
        />
        <button
          type="button"
          onClick={fillFromUrl}
          className="shrink-0 rounded-md border border-content/10 px-3 py-2 text-[12px] text-content hover:bg-content/10"
        >
          Fill
        </button>
      </div>
      <div className="mt-2 grid grid-cols-[1fr_96px] gap-2">
        <input
          value={host}
          onChange={(event) => setHost(event.target.value)}
          placeholder="Host (IP or tailnet name)"
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
          aria-label="Host"
          className="rounded-md border border-content/15 bg-transparent px-3 py-2 font-mono text-[12px] text-content placeholder:text-content/30"
        />
        <input
          value={port}
          onChange={(event) => setPort(event.target.value)}
          inputMode="numeric"
          aria-label="Port"
          className="rounded-md border border-content/15 bg-transparent px-3 py-2 font-mono text-[12px] text-content"
        />
      </div>
      <input
        value={token}
        onChange={(event) => setToken(event.target.value)}
        placeholder="Token"
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
        aria-label="Token"
        className="mt-2 w-full rounded-md border border-content/15 bg-transparent px-3 py-2 font-mono text-[12px] text-content placeholder:text-content/30"
      />
      <label className="mt-2 flex min-h-9 items-center gap-2 text-[13px] text-content/80">
        <input
          type="checkbox"
          checked={secure}
          onChange={(event) => setSecure(event.target.checked)}
          className="size-4"
        />
        Secure (wss)
      </label>
      {error ? <p className="mt-2 text-[12px] text-red-400">{error}</p> : null}
      <div className="mt-2 flex gap-2">
        <button
          type="button"
          onClick={connect}
          className="min-h-10 rounded-md bg-content px-4 py-2 text-[13px] font-medium text-background-base"
        >
          Connect
        </button>
      </div>
    </div>
  );
}

/**
 * One pairing route: QR for scanning plus host/port strings for typing.
 * The 6-digit code from the hero covers auth on every route.
 */
function RouteRow({
  details,
  copied,
  onCopy,
}: {
  details: PairingDetails;
  copied: boolean;
  onCopy: () => void;
}) {
  return (
    <div className="flex gap-4 py-3">
      <div className="shrink-0 rounded-lg bg-white p-2">
        <QRCode
          value={buildPairUrl(details)}
          size={128}
          aria-label="Pairing code"
        />
      </div>
      <div className="min-w-0 flex-1">
        <p className="break-all font-mono text-[12px] leading-relaxed text-content/75">
          {details.host}:{details.port}
        </p>
        <div className="mt-2">
          <SecondaryButton onClick={onCopy}>
            {copied ? "Copied link" : "Copy link"}
          </SecondaryButton>
        </div>
        <p className="mt-1.5 text-[11px] leading-relaxed text-content/40">
          Scan, or type the host and the 6-digit code above.
        </p>
      </div>
    </div>
  );
}

function CopyRow({
  label,
  value,
  hint,
  copied,
  onCopy,
}: {
  label: string;
  value: string;
  hint?: string;
  copied: boolean;
  onCopy: () => void;
}) {
  return (
    <div className="flex items-start gap-6 border-b border-content/5 py-4 last:border-b-0">
      <div className="min-w-0 flex-1">
        <div className="text-[13px] font-medium text-content">{label}</div>
        <p className="mt-1 break-all font-mono text-[12px] leading-relaxed text-content/70">
          {value}
        </p>
        {hint ? (
          <p className="mt-1 break-all text-[12px] text-content/35">{hint}</p>
        ) : null}
      </div>
      <SecondaryButton onClick={onCopy}>
        {copied ? "Copied" : "Copy"}
      </SecondaryButton>
    </div>
  );
}
