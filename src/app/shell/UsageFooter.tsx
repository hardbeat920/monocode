import { RefreshCw, Terminal } from "../../shared/ui/icons";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { HarnessIcon } from "../../features/sessions/ui/HarnessIcon";
import { Popover, type PopoverDismissReason } from "../../shared/ui/Popover";
import { consumeCodexRateLimitResetCredit } from "../../features/providers/model/rateLimitsFetch";
import {
  errorRateLimits,
  unavailableRateLimits,
  type RateLimitProvider,
} from "../../features/providers/model/rateLimits";
import {
  getCachedRateLimits,
  loadRateLimits,
  setCachedRateLimits,
  useCachedRateLimits,
} from "../../features/providers/model/rateLimitsCache";
import {
  HARNESS_LABEL,
  HARNESS_TITLE,
  type HarnessId,
} from "../../features/sessions/model/session";
import {
  loginHarness,
  supportsHarnessLogin,
} from "../../integrations/harness/core/auth";
import {
  runningTerminalChipLabel,
  type RunningTerminal,
} from "../../features/terminal/model/terminalTab";
import { MOD } from "../../platform/tauri/platform";
import {
  USAGE_CHIP_CLASSES,
  USAGE_REFRESH_CLASSES,
  UsageProviderChip,
  type UsageChipVariant,
} from "./UsageProviderChip";
import { PiUsage } from "./PiUsage";
import {
  ProviderSignInPanel,
  type ProviderSignInState,
} from "../../features/sessions/ui/ProviderSignInPanel";
import {
  newProviderAccount,
  providerAccountExists,
  providerAccounts,
  saveProviderAccount,
  selectProviderAccount,
  selectedProviderAccountId,
  subscribeProviderAccounts,
  type ProviderAccountProvider,
} from "../../features/providers/model/providerAccounts";

const CLOCK_MS = 30_000;

export type UsageFooterSession = {
  id?: string;
  harness: HarnessId;
  model?: string;
  authRequired?: boolean;
  providerAccountId?: string;
};

export function UsageFooter({
  providers,
  session,
  project,
  terminals = [],
  terminalOpen = false,
  onToggleTerminal,
  onNewTerminal,
  onToggleProjectTerminal,
  projectTerminalActive = false,
  projectTerminalOpen = false,
  onSelectAccount,
  onManageAccounts,
  inset = false,
  pressureSlot,
}: {
  providers: RateLimitProvider[];
  session?: UsageFooterSession;
  project?: string;
  terminals?: RunningTerminal[];
  terminalOpen?: boolean;
  onToggleTerminal?: (fileId: string) => void;
  onNewTerminal?: () => void;
  onToggleProjectTerminal?: () => void;
  projectTerminalActive?: boolean;
  projectTerminalOpen?: boolean;
  onSelectAccount?: (
    provider: ProviderAccountProvider,
    accountId: string,
  ) => void;
  onManageAccounts?: (provider: ProviderAccountProvider) => void;
  /** Sits on the inset layout's outer glass, below the card. */
  inset?: boolean;
  /** Receives the slot beside the terminal button for the pressure chip. */
  pressureSlot?: (element: HTMLSpanElement | null) => void;
}) {
  const opencode = useCachedRateLimits("opencode");
  const showUsage =
    providers.some((provider) => provider !== "opencode") ||
    (providers.includes("opencode") && opencode.status !== "unavailable");
  const showTerminals = terminals.length > 0;
  const showTerminalButton = Boolean(
    onNewTerminal || onToggleProjectTerminal,
  );
  const terminalLabel = projectTerminalActive
    ? "Terminal"
    : `New Terminal (${MOD}\`)`;
  const onTerminalClick = projectTerminalActive
    ? (onToggleProjectTerminal ?? onNewTerminal)
    : (onNewTerminal ?? onToggleProjectTerminal);
  const ariaLabel = showUsage || session?.harness === "pi"
    ? "Provider usage"
    : showTerminals || showTerminalButton
      ? "Terminals"
      : session
        ? "Session"
        : undefined;

  return (
    <footer
      aria-label={ariaLabel}
      // Inset: the card's edge above reads as part of the gap, so a little
      // room below keeps the chips optically centred. The pills line up with
      // the card's edges, which carry their own padding.
      className={`flex shrink-0 items-center gap-1.5 overflow-x-auto text-[11px] text-content/55${
        inset
          ? " inset-footer mr-1.5 h-[30px] pb-0.5"
          : " h-7 border-t border-stroke px-3"
      }`}
    >
      <UsageChips
        providers={providers}
        session={session}
        project={project}
        onSelectAccount={onSelectAccount}
        onManageAccounts={onManageAccounts}
        variant={inset ? "inset" : "footer"}
        fallback={
          session ? (
            <SessionChip
              key={session.id ?? session.harness}
              session={session}
              inset={inset}
            />
          ) : null
        }
      />
      {showTerminals || showTerminalButton || pressureSlot ? (
        <div className="ml-auto flex shrink-0 items-center gap-2">
          {pressureSlot ? (
            <span
              ref={pressureSlot}
              className="flex items-center empty:hidden"
            />
          ) : null}
          {showTerminals ? (
            <RunningTerminalChip
              terminals={terminals}
              open={terminalOpen}
              onToggle={onToggleTerminal}
              inset={inset}
            />
          ) : showTerminalButton ? (
            <button
              type="button"
              className={`inline-flex h-5 shrink-0 items-center gap-1.5 whitespace-nowrap hover:bg-content/10 ${
                inset ? "rounded-full px-2" : "rounded-md px-1.5"
              } ${
                projectTerminalOpen
                  ? "text-accent"
                  : "text-content/40 hover:text-content"
              }`}
              aria-label={terminalLabel}
              aria-pressed={projectTerminalOpen}
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

/**
 * The provider usage chips and their refresh button, for the session footer
 * and anywhere else a conversation shows how much of its plan is left.
 * `accountPicker` off keeps the chips to usage alone, for a conversation
 * whose account is not switched from here. `variant` `composer` matches
 * the tool buttons under a Mono's composer instead of the footer.
 */
export function UsageChips({
  providers,
  session,
  project,
  onSelectAccount,
  onManageAccounts,
  accountPicker = true,
  fallback = null,
  variant = "footer",
}: {
  providers: RateLimitProvider[];
  session?: UsageFooterSession;
  project?: string;
  onSelectAccount?: (
    provider: ProviderAccountProvider,
    accountId: string,
  ) => void;
  onManageAccounts?: (provider: ProviderAccountProvider) => void;
  accountPicker?: boolean;
  /** Shown when there is no usage to show. */
  fallback?: ReactNode;
  variant?: UsageChipVariant;
}) {
  const wantClaude = providers.includes("claude");
  const wantCodex = providers.includes("codex");
  const wantOpencode = providers.includes("opencode");
  const wantDevin = providers.includes("devin");
  const [now, setNow] = useState(() => Date.now());
  const [refreshing, setRefreshing] = useState(false);
  const [, setAccountsVersion] = useState(0);
  const inflight = useRef<Promise<void> | null>(null);
  const claudeAccountId =
    session?.harness === "claude" && session.providerAccountId
      ? session.providerAccountId
      : selectedProviderAccountId("claude", project);
  const codexAccountId =
    session?.harness === "codex" && session.providerAccountId
      ? session.providerAccountId
      : selectedProviderAccountId("codex", project);
  const claudeAccounts = providerAccounts("claude");
  const codexAccounts = providerAccounts("codex");
  const claudeAccountAvailable = providerAccountExists(
    "claude",
    claudeAccountId,
  );
  const codexAccountAvailable = providerAccountExists("codex", codexAccountId);
  const cachedClaude = useCachedRateLimits("claude", claudeAccountId);
  const cachedCodex = useCachedRateLimits("codex", codexAccountId);
  const opencode = useCachedRateLimits("opencode");
  const devin = useCachedRateLimits("devin");
  const claude = claudeAccountAvailable
    ? cachedClaude
    : unavailableRateLimits(
        "claude",
        "This conversation uses a removed account",
      );
  const codex = codexAccountAvailable
    ? cachedCodex
    : unavailableRateLimits(
        "codex",
        "This conversation uses a removed account",
      );

  useEffect(
    () =>
      subscribeProviderAccounts(() => setAccountsVersion((value) => value + 1)),
    [],
  );

  // New accounts load once. Returning from Settings or focusing the window
  // reads the shared snapshot without starting another provider request.
  useEffect(() => {
    if (wantClaude && claudeAccountAvailable)
      void loadRateLimits("claude", claudeAccountId);
    if (wantCodex && codexAccountAvailable)
      void loadRateLimits("codex", codexAccountId);
    if (wantOpencode) void loadRateLimits("opencode");
    if (wantDevin) void loadRateLimits("devin");
  }, [
    claudeAccountAvailable,
    claudeAccountId,
    codexAccountAvailable,
    codexAccountId,
    wantClaude,
    wantCodex,
    wantOpencode,
    wantDevin,
  ]);

  const refresh = useCallback(() => {
    if (inflight.current) return inflight.current;
    setRefreshing(true);
    const jobs: Promise<unknown>[] = [];
    if (wantClaude && claudeAccountAvailable)
      jobs.push(loadRateLimits("claude", claudeAccountId, true));
    if (wantCodex && codexAccountAvailable)
      jobs.push(loadRateLimits("codex", codexAccountId, true));
    if (wantOpencode) jobs.push(loadRateLimits("opencode", "default", true));
    if (wantDevin) jobs.push(loadRateLimits("devin", "default", true));
    const run = Promise.allSettled(jobs)
      .then(() => undefined)
      .finally(() => {
        inflight.current = null;
        setRefreshing(false);
      });
    inflight.current = run;
    return run;
  }, [
    claudeAccountAvailable,
    claudeAccountId,
    codexAccountAvailable,
    codexAccountId,
    wantClaude,
    wantCodex,
    wantOpencode,
    wantDevin,
  ]);

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), CLOCK_MS);
    return () => window.clearInterval(timer);
  }, []);

  const consumeCodexReset = useCallback(
    async (creditId?: string) => {
      while (inflight.current) await inflight.current;
      setRefreshing(true);
      let outcome: Awaited<ReturnType<typeof consumeCodexRateLimitResetCredit>>;
      const operation = (async () => {
        try {
          outcome = await consumeCodexRateLimitResetCredit(
            creditId,
            codexAccountId,
          );
          await loadRateLimits("codex", codexAccountId, true);
        } catch (error) {
          const message =
            error instanceof Error
              ? error.message
              : "Could not use Codex reset";
          setCachedRateLimits(
            "codex",
            codexAccountId,
            errorRateLimits(
              "codex",
              message,
              getCachedRateLimits("codex", codexAccountId),
            ),
          );
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
    },
    [codexAccountId],
  );

  const reconnectProvider = useCallback(
    async (provider: RateLimitProvider, accountId: string) => {
      while (inflight.current) await inflight.current;
      setRefreshing(true);
      const operation = (async () => {
        try {
          await (accountId === "default"
            ? loginHarness(provider)
            : loginHarness(provider, accountId));
          const value = await loadRateLimits(provider, accountId, true);
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
          setCachedRateLimits(
            provider,
            accountId,
            errorRateLimits(
              provider,
              message,
              getCachedRateLimits(provider, accountId),
            ),
          );
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
    () => reconnectProvider("claude", claudeAccountId),
    [claudeAccountId, reconnectProvider],
  );

  const reconnectCodex = useCallback(
    () => reconnectProvider("codex", codexAccountId),
    [codexAccountId, reconnectProvider],
  );

  const reconnectDevin = useCallback(
    () => reconnectProvider("devin", "default"),
    [reconnectProvider],
  );

  const selectAccount = useCallback(
    (provider: ProviderAccountProvider, accountId: string) => {
      selectProviderAccount(provider, project, accountId);
      onSelectAccount?.(provider, accountId);
    },
    [onSelectAccount, project],
  );

  const addAccount = useCallback(
    async (provider: ProviderAccountProvider, label: string) => {
      const account = newProviderAccount(provider, label);
      await loginHarness(provider, account.id);
      saveProviderAccount(account);
      selectAccount(provider, account.id);
      return account;
    },
    [selectAccount],
  );

  const showOpencodeChip = wantOpencode && opencode.status !== "unavailable";
  const showUsage = wantClaude || wantCodex || showOpencodeChip || wantDevin;
  const claudeAccountProps = accountPicker
    ? {
        accounts: claudeAccounts,
        accountId: claudeAccountId,
        onSelectAccount: (accountId: string) =>
          selectAccount("claude", accountId),
        onAddAccount: (label: string) => addAccount("claude", label),
        onManageAccounts: onManageAccounts
          ? () => onManageAccounts("claude")
          : undefined,
      }
    : {};
  const codexAccountProps = accountPicker
    ? {
        accounts: codexAccounts,
        accountId: codexAccountId,
        onSelectAccount: (accountId: string) =>
          selectAccount("codex", accountId),
        onAddAccount: (label: string) => addAccount("codex", label),
        onManageAccounts: onManageAccounts
          ? () => onManageAccounts("codex")
          : undefined,
      }
    : {};

  if (session?.harness === "pi") {
    return (
      <PiUsage
        key={`${session.id}:${session.model}`}
        model={session.model}
        now={now}
        variant={variant}
      />
    );
  }
  if (!showUsage) return <>{fallback}</>;
  return (
    <>
      {wantClaude ? (
        <UsageProviderChip
          limits={claude}
          now={now}
          variant={variant}
          {...claudeAccountProps}
          onReconnect={reconnectClaude}
        />
      ) : null}
      {wantCodex ? (
        <UsageProviderChip
          limits={codex}
          now={now}
          project={project}
          variant={variant}
          {...codexAccountProps}
          onConsumeReset={consumeCodexReset}
          onReconnect={reconnectCodex}
        />
      ) : null}
      {showOpencodeChip ? (
        <UsageProviderChip
          limits={opencode}
          now={now}
          project={project}
          variant={variant}
        />
      ) : null}
      {wantDevin ? (
        // Devin has one login per machine, so there is no account picker.
        <UsageProviderChip
          limits={devin}
          now={now}
          project={project}
          variant={variant}
          onReconnect={reconnectDevin}
        />
      ) : null}
      <button
        type="button"
        className={USAGE_REFRESH_CLASSES[variant]}
        aria-label="Refresh usage"
        title="Refresh usage"
        disabled={refreshing}
        onClick={() => void refresh()}
      >
        <RefreshCw
          className={`size-2.5 ${refreshing ? "animate-spin" : ""}`}
          strokeWidth={1.75}
          aria-hidden
        />
      </button>
    </>
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

function SessionChip({
  session,
  inset,
}: {
  session: UsageFooterSession;
  inset: boolean;
}) {
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
        className={USAGE_CHIP_CLASSES[inset ? "inset" : "footer"]}
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
  inset,
}: {
  terminals: RunningTerminal[];
  open: boolean;
  onToggle?: (fileId: string) => void;
  inset: boolean;
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
        className={`inline-flex min-w-0 max-w-[16rem] items-center gap-1.5 whitespace-nowrap hover:bg-content/10 hover:text-content ${
          inset ? "h-5 rounded-full px-2" : "-mx-1 rounded-md px-1"
        }`}
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
