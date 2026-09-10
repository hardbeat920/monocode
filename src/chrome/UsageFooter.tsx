import { RefreshCw } from "./icons";
import { useCallback, useEffect, useRef, useState } from "react";
import { HarnessIcon } from "./HarnessIcon";
import { Popover } from "./Popover";
import {
  fetchClaudeRateLimits,
  fetchCodexRateLimits,
} from "../lib/rateLimitsFetch";
import {
  clampUsedPercent,
  fetchingRateLimits,
  formatRateLimitWindowChipLabel,
  formatUsagePercent,
  idleRateLimits,
  RATE_LIMIT_POLL_MS,
  rateLimitWindowTooltip,
  shouldFetchProvider,
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
  const wantClaude = providers.includes("claude");
  const wantCodex = providers.includes("codex");
  const [claude, setClaude] = useState<ProviderRateLimits>(() =>
    idleRateLimits("claude"),
  );
  const [codex, setCodex] = useState<ProviderRateLimits>(() =>
    idleRateLimits("codex"),
  );
  const [now, setNow] = useState(() => Date.now());
  const [refreshing, setRefreshing] = useState(false);
  const inflight = useRef<Promise<void> | null>(null);
  const claudeRef = useRef(claude);
  const codexRef = useRef(codex);
  claudeRef.current = claude;
  codexRef.current = codex;

  const refresh = useCallback((force = false) => {
    if (inflight.current) return inflight.current;
    const visible = document.visibilityState === "visible";
    const fetchClaude =
      wantClaude &&
      shouldFetchProvider(claudeRef.current, { force, visible });
    const fetchCodex =
      wantCodex &&
      shouldFetchProvider(codexRef.current, { force, visible });
    if (!fetchClaude && !fetchCodex) return;
    if (force) setRefreshing(true);
    const jobs: Promise<void>[] = [];
    if (fetchClaude) {
      setClaude((current) => fetchingRateLimits("claude", current));
      jobs.push(
        fetchClaudeRateLimits().then((value) => {
          setClaude(value);
        }),
      );
    }
    if (fetchCodex) {
      setCodex((current) => fetchingRateLimits("codex", current));
      jobs.push(
        fetchCodexRateLimits().then((value) => {
          setCodex(value);
        }),
      );
    }
    const run = Promise.allSettled(jobs)
      .then(() => undefined)
      .finally(() => {
        inflight.current = null;
        setRefreshing(false);
      });
    inflight.current = run;
    return run;
  }, [wantClaude, wantCodex]);

  useEffect(() => {
    void refresh();
    const poll = window.setInterval(() => void refresh(), RATE_LIMIT_POLL_MS);
    const onVisible = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.clearInterval(poll);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [refresh]);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), CLOCK_MS);
    return () => window.clearInterval(timer);
  }, []);

  const showUsage = wantClaude || wantCodex;
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
      {showUsage ? (
        <>
          {wantClaude ? <ProviderChip limits={claude} now={now} /> : null}
          {wantCodex ? <ProviderChip limits={codex} now={now} /> : null}
        </>
      ) : session ? (
        <SessionChip session={session} />
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
                  {formatRateLimitWindowChipLabel(entry.window, now)}
                </span>
              </span>
            ))}
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
