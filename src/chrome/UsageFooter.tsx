import { RefreshCw } from "./icons";
import { Fragment, useEffect, useRef, useState, useSyncExternalStore } from "react";
import { HarnessIcon } from "./HarnessIcon";
import { Popover } from "./Popover";
import {
  getRateLimitsSnapshot,
  refreshRateLimits,
  setRateLimitProviders,
  subscribeRateLimits,
} from "../lib/rateLimitsStore";
import {
  clampUsedPercent,
  formatRateLimitWindowChipLabel,
  formatUsagePercent,
  sharedWindowResetLabel,
  isRateLimitProvider,
  rateLimitWindowTooltip,
  type ProviderRateLimits,
  type RateLimitProvider,
  type RateLimitWindow,
} from "../lib/rateLimits";
import { HARNESS_LABEL, HARNESS_TITLE, type HarnessId } from "../lib/session";
import {
  runningTerminalChipLabel,
  type RunningTerminal,
} from "../lib/terminalTab";

const CLOCK_MS = 30_000;

export type UsageFooterSession = {
  harness: HarnessId;
};

export function UsageFooter({
  providers,
  session,
  terminals = [],
  terminalOpen = false,
  onToggleTerminal,
}: {
  providers: RateLimitProvider[];
  session?: UsageFooterSession;
  terminals?: RunningTerminal[];
  terminalOpen?: boolean;
  onToggleTerminal?: (fileId: string) => void;
}) {
  const snapshot = useSyncExternalStore(
    subscribeRateLimits,
    getRateLimitsSnapshot,
    getRateLimitsSnapshot,
  );
  const claude = snapshot.claude;
  const codex = snapshot.codex;
  const cursor = snapshot.cursor;
  const grok = snapshot.grok;
  const refreshing = snapshot.refreshing;
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    setRateLimitProviders(providers);
  }, [providers]);

  const refresh = (force = false) => refreshRateLimits(force);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), CLOCK_MS);
    return () => window.clearInterval(timer);
  }, []);

  const showUsage = providers.length > 0;
  const usageChips = providers
    .map((provider) =>
      provider === "claude"
        ? claude
        : provider === "codex"
          ? codex
          : provider === "cursor"
            ? cursor
            : provider === "grok"
              ? grok
              : null,
    )
    .filter((limits): limits is ProviderRateLimits => limits != null);
  // The session chip is the fallback when no usage chip covers the active
  // provider. With the roster pinned it sits alongside the usage chips, so an
  // OpenCode session still says "opencode" instead of vanishing behind them.
  const showSession =
    session != null &&
    !(
      isRateLimitProvider(session.harness) &&
      providers.includes(session.harness)
    );
  const showTerminals = terminals.length > 0;
  const showRight = showUsage || showTerminals;
  const ariaLabel = showUsage
    ? "Provider usage"
    : showTerminals
      ? "Terminals"
      : session
        ? "Session"
        : undefined;

  return (
    <footer
      aria-label={ariaLabel}
      className="flex h-7 shrink-0 items-center gap-3 overflow-x-auto border-t border-content/10 px-3 text-[11px] text-content/55"
    >
      {showSession && session ? <SessionChip session={session} /> : null}
      {usageChips.length > 0 ? (
        <span className="inline-flex min-w-0 items-center gap-2.5">
          {usageChips.map((limits, index) => (
            <Fragment key={limits.provider}>
              {index > 0 ? <ProviderDivider /> : null}
              <ProviderChip limits={limits} now={now} />
            </Fragment>
          ))}
        </span>
      ) : null}
      {showRight ? (
        <div className="ml-auto flex shrink-0 items-center gap-2">
          {showTerminals ? (
            <RunningTerminalChip
              terminals={terminals}
              open={terminalOpen}
              onToggle={onToggleTerminal}
            />
          ) : null}
          {showUsage ? (
            <button
              type="button"
              className="grid size-5 shrink-0 place-items-center rounded text-content/40 hover:bg-content/10 hover:text-content disabled:opacity-50"
              aria-label="Refresh usage"
              title="Refresh usage"
              disabled={refreshing}
              onClick={() => void refresh(true)}
            >
              <RefreshCw
                className={`size-3 ${refreshing ? "animate-spin" : ""}`}
                strokeWidth={1.75}
                aria-hidden
              />
            </button>
          ) : null}
        </div>
      ) : null}
    </footer>
  );
}

function TerminalLiveMark() {
  return (
    <span className="terminal-live shrink-0" aria-hidden>
      <span className="terminal-live-bar" />
      <span className="terminal-live-bar" />
      <span className="terminal-live-bar" />
    </span>
  );
}

function ProviderDivider() {
  return <span aria-hidden className="h-3.5 w-px shrink-0 bg-content/20" />;
}

function SessionChip({ session }: { session: UsageFooterSession }) {
  return (
    <span
      className="inline-flex min-w-0 items-center gap-1.5 whitespace-nowrap"
      title={HARNESS_TITLE[session.harness]}
    >
      <HarnessIcon harness={session.harness} className="size-3 shrink-0" />
      <span>{HARNESS_LABEL[session.harness]}</span>
    </span>
  );
}

function RunningTerminalChip({
  terminals,
  open: panelOpen,
  onToggle,
}: {
  terminals: RunningTerminal[];
  open: boolean;
  onToggle?: (fileId: string) => void;
}) {
  const root = useRef<HTMLButtonElement>(null);
  const [menuOpen, setMenuOpen] = useState(false);
  const label = runningTerminalChipLabel(terminals);
  const many = terminals.length > 1;
  const title = terminals
    .map((terminal) => `"${terminal.process}" in ${terminal.label}`)
    .join("\n");
  const ariaLabel =
    terminals.length === 1
      ? panelOpen
        ? `Hide ${terminals[0]?.process}`
        : `Show ${terminals[0]?.process}`
      : panelOpen
        ? "Hide running terminals"
        : `${terminals.length} terminals are running processes`;

  const toggle = (fileId: string) => {
    setMenuOpen(false);
    onToggle?.(fileId);
  };

  return (
    <>
      <button
        ref={root}
        type="button"
        className="inline-flex min-w-0 max-w-[16rem] items-center gap-1.5 whitespace-nowrap rounded px-1 -mx-1 hover:bg-content/10 hover:text-content"
        aria-label={ariaLabel}
        aria-pressed={panelOpen}
        aria-expanded={many && !panelOpen ? menuOpen : undefined}
        aria-haspopup={many && !panelOpen ? "menu" : undefined}
        title={title}
        onClick={() => {
          if (panelOpen || !many) {
            const target = terminals[0];
            if (target) toggle(target.id);
            return;
          }
          setMenuOpen((value) => !value);
        }}
      >
        <TerminalLiveMark />
        <span className="truncate font-mono text-[10px] tabular-nums">
          {label}
        </span>
      </button>
      {menuOpen && many && !panelOpen ? (
        <Popover
          anchor={root}
          side="top"
          align="end"
          autoFocus
          onDismiss={() => setMenuOpen(false)}
          role="menu"
          aria-label="Running terminals"
          className="min-w-[12rem] p-1"
        >
          {terminals.map((terminal) => (
            <button
              key={terminal.id}
              type="button"
              role="menuitem"
              className="flex h-7 w-full items-center gap-2 rounded-lg px-2 text-left text-[12px] leading-none text-content hover:bg-content/10"
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => toggle(terminal.id)}
            >
              <span className="min-w-0 flex-1 truncate">{terminal.process}</span>
              <span className="max-w-[7rem] shrink-0 truncate text-[11px] text-content/40">
                {terminal.label}
              </span>
            </button>
          ))}
        </Popover>
      ) : null}
    </>
  );
}

function ProviderChip({
  limits,
  now,
}: {
  limits: ProviderRateLimits;
  now: number;
}) {
  const loading =
    limits.status === "idle" ||
    (limits.status === "fetching" && !limits.session && !limits.weekly);
  const disconnected = limits.status === "unavailable";
  const windows = [
    limits.session ? { key: "session", window: limits.session } : null,
    limits.weekly ? { key: "weekly", window: limits.weekly } : null,
  ].filter((entry): entry is { key: string; window: RateLimitWindow } => {
    return entry != null;
  });
  const tightest = windows.reduce<RateLimitWindow | null>((best, entry) => {
    if (!best || entry.window.usedPercent > best.usedPercent) {
      return entry.window;
    }
    return best;
  }, null);
  const tooltip = windows
    .map((entry) => rateLimitWindowTooltip(entry.window, now))
    .join(" · ");
  const sharedReset = sharedWindowResetLabel(
    windows.map((entry) => entry.window),
    now,
  );

  return (
    <span
      className="inline-flex min-w-0 items-center gap-1.5 whitespace-nowrap"
      title={
        tooltip ||
        limits.error ||
        (disconnected
          ? "Not connected"
          : loading
            ? "Loading usage…"
            : undefined)
      }
    >
      <HarnessIcon harness={limits.provider} className="size-3 shrink-0" />
      {loading ? (
        <span className="animate-pulse text-content/35">···</span>
      ) : disconnected ? (
        <span className="text-content/35">not connected</span>
      ) : windows.length === 0 ? (
        <span className="text-content/35">{emptyUsageLabel(limits)}</span>
      ) : (
        <>
          {tightest ? <MiniBar usedPct={tightest.usedPercent} /> : null}
          <span className="flex min-w-0 items-center gap-1 tabular-nums">
            {windows.map((entry, index) => (
              <span key={entry.key} className="inline-flex items-center gap-1">
                {index > 0 ? <span className="text-content/25">·</span> : null}
                <span>
                  {formatUsagePercent(entry.window.usedPercent)}{" "}
                  <span
                    className={
                      entry.window.chipLabel ? undefined : "text-content/40"
                    }
                  >
                    {formatRateLimitWindowChipLabel(entry.window, now)}
                  </span>
                </span>
              </span>
            ))}
            {sharedReset ? (
              <span className="inline-flex items-center gap-1">
                <span className="text-content/25">·</span>
                <span className="text-content/40">{sharedReset}</span>
              </span>
            ) : null}
          </span>
        </>
      )}
    </span>
  );
}

function emptyUsageLabel(limits: ProviderRateLimits): string {
  if (limits.status !== "error") return "—";
  const text = limits.error?.toLowerCase() ?? "";
  if (text.includes("expired") || text.includes("sign-in")) return "expired";
  return "—";
}

function MiniBar({ usedPct }: { usedPct: number }) {
  const pct = clampUsedPercent(usedPct);
  return (
    <span
      className="h-1 w-8 shrink-0 overflow-hidden rounded-full bg-content/10"
      aria-hidden
    >
      <span
        className={`block h-full rounded-full ${barClass(pct)}`}
        style={{ width: `${pct}%` }}
      />
    </span>
  );
}

function barClass(pct: number): string {
  if (pct >= 90) return "bg-red-400";
  if (pct >= 80) return "bg-amber-400";
  return "bg-content/45";
}
