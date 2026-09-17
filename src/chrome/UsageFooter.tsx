import { RefreshCw, Terminal } from "./icons";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { HarnessIcon } from "./HarnessIcon";
import { Popover, type PopoverDismissReason } from "./Popover";
import {
  consumeCodexRateLimitResetCredit,
  fetchClaudeRateLimits,
  fetchCodexRateLimits,
} from "../lib/rateLimitsFetch";
import {
  errorRateLimits,
  fetchingRateLimits,
  idleRateLimits,
  RATE_LIMIT_POLL_MS,
  shouldFetchProvider,
  type ProviderRateLimits,
  type RateLimitProvider,
} from "../lib/rateLimits";
import { HARNESS_LABEL, HARNESS_TITLE, type HarnessId } from "../lib/session";
import { loginHarness, supportsHarnessLogin } from "../lib/harness/auth";
import {
  runningTerminalChipLabel,
  type RunningTerminal,
} from "../lib/terminalTab";
import { homeDir } from "../lib/fs";
import { resolveClaudeProfileEnv } from "../lib/harness/claudeProfiles";
import { MOD } from "../lib/platform";
import { UsageProviderChip } from "./UsageProviderChip";
import {
  ProviderSignInPanel,
  type ProviderSignInState,
} from "./ProviderSignInPanel";

const CLOCK_MS = 30_000;

/** Resolves the active session's account to a CLAUDE_CONFIG_DIR, so the
 * usage fetch reads the same account the session actually runs under
 * (undefined for the default/no-override account). */
async function resolveClaudeConfigDir(
  session: UsageFooterSession | undefined,
): Promise<string | undefined> {
  if (!session?.cwd) return undefined;
  const home = await homeDir();
  const { env } = resolveClaudeProfileEnv(session.profile, session.cwd, home);
  return env.CLAUDE_CONFIG_DIR;
}

export type UsageFooterSession = {
  id?: string;
  harness: HarnessId;
  /** Which account this session's Claude usage should be read for. */
  cwd?: string;
  profile?: string;
  authRequired?: boolean;
};

export function UsageFooter({
  providers,
  session,
  project,
  terminals = [],
  terminalOpen = false,
  onToggleTerminal,
  onNewTerminal,
  onShowTerminal,
  projectTerminalActive = false,
}: {
  providers: RateLimitProvider[];
  session?: UsageFooterSession;
  project?: string;
  terminals?: RunningTerminal[];
  terminalOpen?: boolean;
  onToggleTerminal?: (fileId: string) => void;
  onNewTerminal?: () => void;
  onShowTerminal?: () => void;
  projectTerminalActive?: boolean;
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

  const refresh = useCallback(
    (force = false) => {
      if (inflight.current) return inflight.current;
      const visible = document.visibilityState === "visible";
      const fetchClaude =
        wantClaude &&
        shouldFetchProvider(claudeRef.current, { force, visible });
      const fetchCodex =
        wantCodex && shouldFetchProvider(codexRef.current, { force, visible });
      if (!fetchClaude && !fetchCodex) return;
      if (force) setRefreshing(true);
      const jobs: Promise<void>[] = [];
      if (fetchClaude) {
        setClaude((current) => fetchingRateLimits("claude", current));
        jobs.push(
          resolveClaudeConfigDir(session)
            .then((configDir) => fetchClaudeRateLimits(configDir))
            .then((value) => {
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
    },
    [wantClaude, wantCodex, session],
  );

  useEffect(() => {
    // A different session's account is now in view — the in-flight/cached
    // claude reading belongs to whichever account was previously active, so
    // drop it and force a fresh fetch for the new one rather than showing a
    // stale (possibly wrong-account) percentage while it catches up.
    setClaude(idleRateLimits("claude"));
    void refresh(true);
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

  const consumeCodexReset = useCallback(async (creditId?: string) => {
    while (inflight.current) await inflight.current;
    setRefreshing(true);
    setCodex((current) => fetchingRateLimits("codex", current));
    let outcome: Awaited<ReturnType<typeof consumeCodexRateLimitResetCredit>>;
    const operation = (async () => {
      try {
        outcome = await consumeCodexRateLimitResetCredit(creditId);
        setCodex(await fetchCodexRateLimits());
      } catch (error) {
        const message =
          error instanceof Error ? error.message : "Could not use Codex reset";
        setCodex((current) => errorRateLimits("codex", message, current));
        throw error;
      }
    })();
    const tracked = operation.finally(() => {
      inflight.current = null;
      setRefreshing(false);
    });
    inflight.current = tracked.catch(() => undefined);
    await tracked;
    return outcome!;
  }, []);

  const reconnectProvider = useCallback(
    async (
      provider: RateLimitProvider,
      fetchLimits: () => Promise<ProviderRateLimits>,
      setLimits: Dispatch<SetStateAction<ProviderRateLimits>>,
    ) => {
      while (inflight.current) await inflight.current;
      setRefreshing(true);
      setLimits((current) => fetchingRateLimits(provider, current));
      const operation = (async () => {
        try {
          await loginHarness(provider);
          const value = await fetchLimits();
          setLimits(value);
          if (value.status !== "ok") {
            throw new Error(
              value.error ||
                `${HARNESS_TITLE[provider]} sign-in could not be verified`,
            );
          }
        } catch (error) {
          const message =
            error instanceof Error
              ? error.message
              : "Could not complete sign-in";
          setLimits((current) => errorRateLimits(provider, message, current));
          throw error;
        }
      })();
      const tracked = operation.finally(() => {
        inflight.current = null;
        setRefreshing(false);
      });
      inflight.current = tracked.catch(() => undefined);
      await tracked;
    },
    [],
  );

  const reconnectClaude = useCallback(
    () =>
      reconnectProvider(
        "claude",
        () => resolveClaudeConfigDir(session).then(fetchClaudeRateLimits),
        setClaude,
      ),
    [reconnectProvider, session],
  );

  const reconnectCodex = useCallback(
    () => reconnectProvider("codex", fetchCodexRateLimits, setCodex),
    [reconnectProvider],
  );

  const showUsage = wantClaude || wantCodex;
  const showTerminals = terminals.length > 0;
  const showTerminalButton = Boolean(onNewTerminal || onShowTerminal);
  const terminalLabel = projectTerminalActive
    ? "Terminal"
    : `New Terminal (${MOD}\`)`;
  const onTerminalClick = projectTerminalActive
    ? (onShowTerminal ?? onNewTerminal)
    : (onNewTerminal ?? onShowTerminal);
  const ariaLabel = showUsage
    ? "Provider usage"
    : showTerminals || showTerminalButton
      ? "Terminals"
      : session
        ? "Session"
        : undefined;

  return (
    <footer
      aria-label={ariaLabel}
      className="flex h-7 shrink-0 items-center gap-1.5 overflow-x-auto border-t border-stroke px-3 text-[11px] text-content/55"
    >
      {showUsage ? (
        <>
          {wantClaude ? (
            <UsageProviderChip
              limits={claude}
              now={now}
              onReconnect={reconnectClaude}
            />
          ) : null}
          {wantCodex ? (
            <UsageProviderChip
              limits={codex}
              now={now}
              project={project}
              onConsumeReset={consumeCodexReset}
              onReconnect={reconnectCodex}
            />
          ) : null}
          <button
            type="button"
            className="grid size-4.5 shrink-0 place-items-center rounded text-content/40 hover:bg-content/10 hover:text-content disabled:opacity-50"
            aria-label="Refresh usage"
            title="Refresh usage"
            disabled={refreshing}
            onClick={() => void refresh(true)}
          >
            <RefreshCw
              className={`size-2.5 ${refreshing ? "animate-spin" : ""}`}
              strokeWidth={1.75}
              aria-hidden
            />
          </button>
        </>
      ) : session ? (
        <SessionChip key={session.id ?? session.harness} session={session} />
      ) : null}
      {showTerminals || showTerminalButton ? (
        <div className="ml-auto flex shrink-0 items-center gap-2">
          {showTerminals ? (
            <RunningTerminalChip
              terminals={terminals}
              open={terminalOpen}
              onToggle={onToggleTerminal}
            />
          ) : showTerminalButton ? (
            <button
              type="button"
              className={`inline-flex h-5 shrink-0 items-center gap-1.5 whitespace-nowrap rounded px-1.5 hover:bg-content/10 ${
                projectTerminalActive
                  ? "text-accent"
                  : "text-content/40 hover:text-content"
              }`}
              aria-label={terminalLabel}
              aria-pressed={projectTerminalActive}
              title={terminalLabel}
              onClick={onTerminalClick}
            >
              <Terminal className="size-3.5" strokeWidth={1.75} aria-hidden />
              <span>Terminal</span>
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
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [loginState, setLoginState] = useState<ProviderSignInState>("idle");
  const [loginError, setLoginError] = useState<string | null>(null);
  const authRequired = Boolean(
    session.authRequired && loginState !== "complete",
  );
  const canLogin = authRequired && supportsHarnessLogin(session.harness);

  useEffect(() => {
    if (!session.authRequired && loginState === "complete") {
      setLoginState("idle");
    }
  }, [loginState, session.authRequired]);

  const dismiss = (reason: PopoverDismissReason) => {
    setOpen(false);
    if (reason === "escape") {
      requestAnimationFrame(() => trigger.current?.focus());
    }
  };

  const signIn = async () => {
    setLoginState("running");
    setLoginError(null);
    try {
      await loginHarness(session.harness);
      setOpen(false);
      setLoginState("complete");
    } catch (error) {
      setLoginError(
        error instanceof Error ? error.message : "Could not complete sign-in",
      );
      setLoginState("error");
    }
  };

  if (!canLogin) {
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

  return (
    <>
      <button
        ref={trigger}
        type="button"
        className="-mx-1 inline-flex h-5 min-w-0 shrink-0 items-center gap-1.5 whitespace-nowrap rounded px-1 text-content/55 transition-[background-color,color,transform] duration-150 ease-out hover:bg-content/10 hover:text-content focus-visible:outline-2 focus-visible:outline-accent active:scale-[0.97]"
        aria-label={`${HARNESS_TITLE[session.harness]} sign-in required`}
        aria-expanded={open}
        aria-haspopup="dialog"
        title={`${HARNESS_TITLE[session.harness]} sign-in required`}
        onClick={() => setOpen((value) => !value)}
      >
        <HarnessIcon harness={session.harness} className="size-3 shrink-0" />
        <span>{HARNESS_LABEL[session.harness]}</span>
        {authRequired ? (
          <span className="text-[10px] text-amber-600 dark:text-amber-300">
            sign in
          </span>
        ) : null}
      </button>
      {open ? (
        <Popover
          anchor={trigger}
          side="top"
          align="start"
          gap={7}
          width={300}
          autoFocus
          onDismiss={dismiss}
          role="dialog"
          aria-label={`${HARNESS_TITLE[session.harness]} sign-in`}
          tabIndex={-1}
          className="text-content"
        >
          <ProviderSignInPanel
            harness={session.harness}
            state={loginState}
            error={loginError}
            onSignIn={() => void signIn()}
          />
        </Popover>
      ) : null}
    </>
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
              <span className="min-w-0 flex-1 truncate">
                {terminal.process}
              </span>
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
