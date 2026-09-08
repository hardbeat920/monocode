import { useEffect, useState } from "react";
import QRCode from "react-qr-code";
import { openUrl } from "@tauri-apps/plugin-opener";
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { SecondaryButton } from "../chrome/settingsControls";
import { googleSignInHref } from "../lib/tailscaleLogin";

export type EmbedStatusView = {
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

export type TailnetInfoView = {
  loginName?: string | null;
  displayName?: string | null;
  tailnetName?: string | null;
  hostname?: string | null;
};

type Phase = "online" | "waiting" | "offline" | "error";

function phaseOf(embed: EmbedStatusView | null): Phase {
  if (!embed || !embed.running) return "offline";
  if (embed.error) return "error";
  if (embed.authorized) return "online";
  return "waiting";
}

const PHASE_META: Record<Phase, { label: string; dot: string; glow: string }> = {
  online: {
    label: "Online",
    dot: "bg-emerald-400",
    glow: "shadow-[0_0_12px_2px_rgba(52,211,153,0.55)]",
  },
  waiting: {
    label: "Waiting for Google",
    dot: "bg-amber-400",
    glow: "shadow-[0_0_12px_2px_rgba(251,191,36,0.55)]",
  },
  offline: {
    label: "Offline",
    dot: "bg-content/30",
    glow: "",
  },
  error: {
    label: "Error",
    dot: "bg-red-400",
    glow: "shadow-[0_0_12px_2px_rgba(248,113,113,0.55)]",
  },
};

/** Official four-color Google G. */
function GoogleMark({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className={className}
      aria-hidden
      focusable="false"
    >
      <path
        fill="#4285F4"
        d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92a5.06 5.06 0 0 1-2.2 3.32v2.77h3.57c2.08-1.92 3.27-4.74 3.27-8.1Z"
      />
      <path
        fill="#34A853"
        d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84A11 11 0 0 0 12 23Z"
      />
      <path
        fill="#FBBC05"
        d="M5.84 14.09A6.6 6.6 0 0 1 5.5 12c0-.72.13-1.43.34-2.09V7.07H2.18A11 11 0 0 0 1 12c0 1.77.42 3.45 1.18 4.93l3.66-2.84Z"
      />
      <path
        fill="#EA4335"
        d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53Z"
      />
    </svg>
  );
}

/**
 * Live tailnet status card: phase, hostname, Google identity.
 */
export function TailnetStatusCard({
  embed,
  info,
  peersConnected,
  pairUrl,
  onCopy,
  copied,
}: {
  embed: EmbedStatusView | null;
  info: TailnetInfoView | null;
  peersConnected: number | null;
  pairUrl: string | null;
  onCopy: (key: string, value: string) => void;
  copied: boolean;
}) {
  const phase = phaseOf(embed);
  const meta = PHASE_META[phase];
  const hostname =
    embed?.hostname?.trim() ||
    info?.hostname?.trim() ||
    (phase === "online" ? "monocode" : null);
  const displayName =
    embed?.displayName || info?.displayName || undefined;
  const loginName = embed?.loginName || info?.loginName || undefined;
  const identity = displayName || loginName || hostname;
  const tailnetName =
    embed?.tailnetName || info?.tailnetName || undefined;

  return (
    <div className="overflow-hidden rounded-xl border border-content/10 bg-gradient-to-b from-content/[0.07] to-transparent">
      <div className="flex items-center gap-3 px-4 py-3">
        <span className="relative flex size-2.5">
          {phase === "online" ? (
            <span
              className={`absolute inline-flex h-full w-full animate-ping rounded-full opacity-60 ${meta.dot}`}
            />
          ) : null}
          <span
            className={`relative inline-flex size-2.5 rounded-full ${meta.dot} ${meta.glow}`}
          />
        </span>
        <span className="text-[13px] font-semibold text-content">
          {meta.label}
        </span>
        {embed?.tailnetIp ? (
          <button
            type="button"
            title="Copy tailnet IP"
            onClick={() => void onCopy("tailnet-ip", embed.tailnetIp ?? "")}
            className="ml-auto font-mono text-[13px] text-content/85 hover:text-content"
          >
            {copied ? "copied" : embed.tailnetIp}
          </button>
        ) : hostname ? (
          <span className="ml-auto font-mono text-[12px] text-content/45">
            {hostname}
          </span>
        ) : null}
      </div>

      <dl className="grid grid-cols-2 gap-px border-t border-content/10 bg-content/10">
        <Stat
          label="Signed in"
          value={identity ?? "—"}
          sub={
            loginName && identity !== loginName ? loginName : undefined
          }
        />
        <Stat
          label="Hostname"
          value={hostname ?? "—"}
          sub={phase === "online" ? "this device on the tailnet" : undefined}
        />
        <Stat
          label="Tailnet"
          value={tailnetName ?? (phase === "online" ? "connected" : "—")}
        />
        <Stat
          label="Devices connected"
          value={peersConnected != null ? String(peersConnected) : "—"}
        />
      </dl>

      {phase === "online" && pairUrl ? (
        <div className="flex items-center gap-4 border-t border-content/10 px-4 py-3">
          <div className="shrink-0 rounded-lg bg-white p-2">
            <QRCode value={pairUrl} size={112} aria-label="Pairing code" />
          </div>
          <div className="min-w-0">
            <div className="text-[13px] font-medium text-content">
              Scan to pair the iPad
            </div>
            <p className="mt-0.5 break-all font-mono text-[11px] leading-relaxed text-content/45">
              {pairUrl}
            </p>
          </div>
        </div>
      ) : null}

      {phase === "error" && embed?.error ? (
        <p className="border-t border-content/10 px-4 py-3 text-[12px] text-red-400">
          {embed.error}
        </p>
      ) : null}
    </div>
  );
}

/**
 * Official-looking Google SSO control. Always visible until this device is
 * authorized, including a preparing state so the button is never delayed.
 */
export function GoogleLoginBlock({
  loginUrl,
  copied,
  onCopy,
}: {
  loginUrl: string | null;
  copied: boolean;
  onCopy: () => void;
}) {
  const ready = Boolean(loginUrl);
  const href = loginUrl ? googleSignInHref(loginUrl) : null;
  return (
    <div className="flex items-start gap-4 py-4">
      <div className="min-w-0 flex-1">
        <button
          type="button"
          disabled={!ready}
          onClick={() => {
            if (href) void openUrl(href);
          }}
          className="inline-flex min-h-11 items-center justify-center gap-3 rounded-lg bg-white px-4 text-[14px] font-medium text-[#1f1f1f] shadow-[0_1px_2px_rgba(60,64,67,0.3),0_1px_3px_1px_rgba(60,64,67,0.15)] transition-[transform,opacity] duration-150 ease-out active:scale-96 disabled:opacity-60"
        >
          <GoogleMark className="size-5" />
          {ready ? "Sign in with Google" : "Preparing Google sign-in…"}
        </button>
        <p className="mt-1.5 text-[12px] leading-relaxed text-content/45">
          Opens <span className="text-content/70">Connect this device</span>.
          After Google, tap <span className="text-content/70">Connect</span> —
          do not stop on the Tailscale machines list. Same Google account as
          the Mac.
        </p>
        {href ? (
          <button
            type="button"
            onClick={onCopy}
            className="mt-1 break-all text-left font-mono text-[11px] leading-relaxed text-content/35 hover:text-content/60"
          >
            {copied ? "copied ✓" : href}
          </button>
        ) : null}
      </div>
      {href ? (
        <div className="shrink-0 rounded-lg bg-white p-2">
          <QRCode value={href} size={112} aria-label="Google sign-in code" />
        </div>
      ) : null}
    </div>
  );
}

export async function logoutLocalEmbed(): Promise<EmbedStatusView> {
  return tauriInvoke<EmbedStatusView>("remote_embed_logout");
}

/** Start/poll the local embedded node (Mac client or iPad). */
export function useLocalEmbed(active: boolean): {
  embed: EmbedStatusView | null;
  logout: () => Promise<void>;
} {
  const [embed, setEmbed] = useState<EmbedStatusView | null>(null);
  useEffect(() => {
    if (!active) return;
    let stopped = false;
    const tick = async () => {
      try {
        let next = await tauriInvoke<EmbedStatusView>("remote_embed_status");
        if (!next.running) {
          next = await tauriInvoke<EmbedStatusView>("remote_embed_start", {
            input: {},
          });
        }
        if (!stopped) setEmbed(next);
      } catch {
        if (!stopped) setEmbed(null);
      }
    };
    const timer = window.setInterval(() => void tick(), 4000);
    const onVis = () => {
      if (document.visibilityState === "visible") void tick();
    };
    document.addEventListener("visibilitychange", onVis);
    window.addEventListener("pageshow", onVis);
    window.addEventListener("focus", onVis);
    void tick();
    return () => {
      stopped = true;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVis);
      window.removeEventListener("pageshow", onVis);
      window.removeEventListener("focus", onVis);
    };
  }, [active]);
  const logout = async () => {
    const next = await logoutLocalEmbed();
    setEmbed(next);
  };
  return { embed, logout };
}

export function EmbedLoginSection({
  embed,
  copied,
  onCopy,
  onLogout,
  loggingOut = false,
}: {
  embed: EmbedStatusView | null;
  copied: boolean;
  onCopy: (key: string, value: string) => void;
  onLogout?: () => void;
  loggingOut?: boolean;
}) {
  const authorized = Boolean(embed?.authorized);
  return (
    <>
      <TailnetStatusCard
        embed={embed}
        info={{
          loginName: embed?.loginName,
          displayName: embed?.displayName,
          tailnetName: embed?.tailnetName,
          hostname: embed?.hostname,
        }}
        peersConnected={null}
        pairUrl={null}
        onCopy={onCopy}
        copied={copied}
      />
      {!authorized ? (
        <GoogleLoginBlock
          loginUrl={embed?.loginUrl ?? null}
          copied={copied}
          onCopy={() => onCopy("login-url", embed?.loginUrl ?? "")}
        />
      ) : null}
      {onLogout ? (
        <div className="flex items-center justify-between gap-3 pt-1 pb-2">
          <p className="text-[12px] leading-relaxed text-content/45">
            {authorized
              ? "Sign out to use a different Google account on this device."
              : "If Google shows an error about another tailnet or an existing node, reset and try again."}
          </p>
          <SecondaryButton danger disabled={loggingOut} onClick={onLogout}>
            {loggingOut
              ? "Resetting…"
              : authorized
                ? "Sign out of Tailscale"
                : "Use a different Google account"}
          </SecondaryButton>
        </div>
      ) : null}
    </>
  );
}

function Stat({
  label,
  value,
  sub,
}: {
  label: string;
  value: string;
  sub?: string;
}) {
  return (
    <div className="bg-background-base px-4 py-2.5">
      <dt className="text-[11px] uppercase tracking-wide text-content/35">
        {label}
      </dt>
      <dd className="mt-0.5 truncate text-[13px] text-content" title={value}>
        {value}
      </dd>
      {sub ? (
        <dd className="truncate text-[11px] text-content/40" title={sub}>
          {sub}
        </dd>
      ) : null}
    </div>
  );
}
