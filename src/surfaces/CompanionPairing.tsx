import { useCallback, useEffect, useState } from "react";
import {
  claimPairingCode,
  connectAndRememberCompanion,
  forgetCompanion,
  getCompanionError,
  getCompanionStatus,
  loadPairing,
  onCompanionStatusChange,
  otherPairingHost,
  parsePairUrl,
  type PairingDetails,
} from "../lib/transport";
import { QrScanner } from "./QrScanner";
import { EmbedLoginSection, useLocalEmbed } from "./TailnetStatusCard";

/**
 * First-run / disconnected screen for companion installs. Scan-first: the
 * camera reads the full pairing URL (host, port, token, and connection
 * type ride along, nothing to choose). Otherwise a 6-digit code plus host —
 * the connection type (LAN vs Tailscale) is probed automatically, ws then
 * wss. Full-URL paste stays as the last resort.
 * Rendered *instead of* App by main.tsx — App itself stays untouched.
 */
export function CompanionPairing({ onConnected }: { onConnected: () => void }) {
  const saved = loadPairing();
  const [mode, setMode] = useState<"scan" | "code">("scan");
  const [host, setHost] = useState(saved?.host ?? "");
  const [port, setPort] = useState(saved ? String(saved.port) : "17233");
  const [code, setCode] = useState("");
  const [url, setUrl] = useState("");
  const [status, setStatus] = useState(() => getCompanionStatus());
  const [error, setError] = useState<string | null>(null);
  const [connecting, setConnecting] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const [loggingOut, setLoggingOut] = useState(false);
  const { embed, logout } = useLocalEmbed(true);

  useEffect(() => onCompanionStatusChange(setStatus), []);

  useEffect(() => {
    if (status === "connected") {
      setConnecting(false);
      onConnected();
    }
    if (status === "failed") {
      setConnecting(false);
      setError(
        getCompanionError() ??
          "That pairing code is no longer valid — scan the code on the Mac again.",
      );
    }
  }, [status, onConnected]);

  const connectWithToken = useCallback((details: PairingDetails) => {
    try {
      connectAndRememberCompanion(details);
    } catch (err) {
      setConnecting(false);
      setError(err instanceof Error ? err.message : String(err));
    }
  }, []);

  const onScan = useCallback(
    (text: string) => {
      const trimmed = text.trim();
      // Ignore the Google-login QR from the Mac Companion page. Opening it
      // here sends the iPad into Safari and the pairing camera never comes
      // back until the app is relaunched.
      if (/login\.tailscale\.com/i.test(trimmed)) {
        return;
      }
      const details = parsePairUrl(trimmed);
      if (!details) {
        setError("That code is not a MonoCode pairing link — try manual entry.");
        setMode("code");
        return;
      }
      const tailnetHost =
        details.host.endsWith(".ts.net") ||
        /^100\.(6[4-9]|[7-9]\d|1[01]\d|12[0-7])\./.test(details.host);
      if (tailnetHost && embed && !embed.authorized) {
        setError(
          "Sign in with Google above first, then scan the Mac pairing code.",
        );
        return;
      }
      setError(null);
      setConnecting(true);
      connectWithToken(details);
      // Unpause the camera if neither host answers. Reconnect keeps
      // trying in the background; a later open still calls onConnected.
      window.setTimeout(() => {
        if (getCompanionStatus() === "connected") {
          setConnecting(false);
          return;
        }
        setConnecting(false);
        setError(
          getCompanionError() ??
            `Could not reach ${details.host}:${details.port}. Scan the Local network code if you are on the same Wi-Fi, allow Local Network for MonoCode, and keep this Mac's companion link enabled.`,
        );
      }, 16000);
    },
    [connectWithToken, embed],
  );

  const onClaim = useCallback(async () => {
    const portNumber = Number(port);
    if (!host.trim()) {
      setError("Enter the host shown under the code on the Mac.");
      return;
    }
    if (!Number.isInteger(portNumber) || portNumber <= 0 || portNumber > 65535) {
      setError("Port must be 1–65535 (usually 17233).");
      return;
    }
    setError(null);
    setConnecting(true);
    try {
      const claim = await claimPairingCode(
        { host: host.trim(), port: portNumber },
        code,
      );
      const connectedHost = host.trim();
      const altHost = otherPairingHost(connectedHost, claim);
      connectWithToken({
        host: connectedHost,
        port: portNumber,
        token: claim.token,
        ...(altHost ? { altHost } : {}),
      });
    } catch (err) {
      setConnecting(false);
      setError(err instanceof Error ? err.message : String(err));
    }
  }, [code, connectWithToken, host, port]);

  const onUseAsDesktop = useCallback(() => {
    forgetCompanion();
    window.location.reload();
  }, []);

  return (
    <div className="flex min-h-dvh items-center justify-center bg-background-base p-6 text-content">
      <div className="w-full max-w-md rounded-xl border border-content/10 bg-content/5 p-6">
        <h1 className="text-[17px] font-semibold">Pair with MonoCode</h1>
        <p className="mt-1 text-[13px] leading-relaxed text-content/55">
          {status === "local"
            ? "Scan the code on your Mac, or enter the 6-digit pairing code."
            : "Reconnecting to the paired host…"}
        </p>

        <div className="mt-4">
          <div className="text-[13px] font-medium text-content">Tailscale</div>
          <p className="mt-1 pb-2 text-[12px] leading-relaxed text-content/45">
            Sign in with Google if you are pairing over Tailscale. After
            Google, tap Connect on &quot;Connect this device&quot;. Same
            account as the Mac.
          </p>
          <EmbedLoginSection
            embed={embed}
            copied={copied === "login-url" || copied === "tailnet-ip"}
            loggingOut={loggingOut}
            onLogout={() => {
              setLoggingOut(true);
              void logout().finally(() => setLoggingOut(false));
            }}
            onCopy={(key, value) => {
              void navigator.clipboard.writeText(value).then(
                () => {
                  setCopied(key);
                  window.setTimeout(() => {
                    setCopied((current) => (current === key ? null : current));
                  }, 1500);
                },
                () => {
                  /* clipboard may be blocked; the URL is still visible */
                },
              );
            }}
          />
        </div>

        <div className="mt-4 grid grid-cols-2 gap-1 rounded-md border border-content/10 p-0.5 text-[13px]">
          {(
            [
              ["scan", "Scan code"],
              ["code", "Enter code"],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              onClick={() => {
                setMode(id);
                setError(null);
              }}
              aria-pressed={mode === id}
              className={`min-h-10 rounded-[5px] px-2 ${
                mode === id
                  ? "bg-content/10 text-content"
                  : "text-content/50 hover:text-content"
              }`}
            >
              {label}
            </button>
          ))}
        </div>

        {mode === "scan" ? (
          <div className="mt-3">
            <QrScanner
              onScan={onScan}
              paused={connecting || status !== "local"}
            />
          </div>
        ) : (
          <div className="mt-3">
            <div className="grid grid-cols-[1fr_96px] gap-2">
              <div>
                <label className="block text-[12px] font-medium text-content/70">
                  Host
                </label>
                <input
                  value={host}
                  onChange={(event) => setHost(event.target.value)}
                  placeholder="192.168.1.20 or tailnet name"
                  autoCapitalize="off"
                  autoCorrect="off"
                  spellCheck={false}
                  className="mt-1 w-full rounded-md border border-content/15 bg-transparent px-3 py-2 font-mono text-[12px] text-content placeholder:text-content/30"
                />
              </div>
              <div>
                <label className="block text-[12px] font-medium text-content/70">
                  Port
                </label>
                <input
                  value={port}
                  onChange={(event) => setPort(event.target.value)}
                  inputMode="numeric"
                  className="mt-1 w-full rounded-md border border-content/15 bg-transparent px-3 py-2 font-mono text-[12px] text-content"
                />
              </div>
            </div>
            <label className="mt-3 block text-[12px] font-medium text-content/70">
              6-digit code
            </label>
            <input
              value={code}
              onChange={(event) => setCode(event.target.value)}
              placeholder="123 456"
              inputMode="numeric"
              autoComplete="one-time-code"
              className="mt-1 w-full rounded-md border border-content/15 bg-transparent px-3 py-2 text-center font-mono text-[20px] tracking-[0.3em] text-content placeholder:text-content/30"
            />
            <p className="mt-1 text-[12px] text-content/40">
              Shown big on the Mac. Connection type is detected automatically.
            </p>
          </div>
        )}

        {error ? (
          <p className="mt-3 text-[12px] text-red-400">{error}</p>
        ) : null}

        {mode === "code" ? (
          <button
            type="button"
            disabled={connecting}
            onClick={() => void onClaim()}
            className="mt-4 min-h-11 w-full rounded-md bg-content px-3 py-2 text-[14px] font-medium text-background-base disabled:opacity-50"
          >
            {connecting ? "Connecting…" : "Connect"}
          </button>
        ) : null}
        {connecting && mode === "scan" ? (
          <p className="mt-3 text-[12px] text-content/50">Connecting…</p>
        ) : null}

        <details className="mt-4">
          <summary className="cursor-pointer text-[12px] text-content/45 hover:text-content">
            Paste a full pairing URL instead
          </summary>
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
              onClick={() => {
                const details = parsePairUrl(url.trim());
                if (!details) {
                  setError("That URL is not a MonoCode pairing link.");
                  return;
                }
                setError(null);
                setConnecting(true);
                connectWithToken(details);
              }}
              className="shrink-0 rounded-md border border-content/15 px-3 py-2 text-[13px] text-content hover:bg-content/10"
            >
              Fill &amp; connect
            </button>
          </div>
        </details>

        <button
          type="button"
          onClick={onUseAsDesktop}
          className="mt-2 min-h-11 w-full rounded-md px-3 py-2 text-[12px] text-content/45 hover:text-content"
        >
          Use this device as a desktop instead
        </button>
      </div>
    </div>
  );
}
