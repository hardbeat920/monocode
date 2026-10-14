import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import {
  clampUsedPercent,
  DAILY_WINDOW_MINUTES,
  formatExtraUsageBalance,
  formatRateLimitWindowChipLabel,
  formatResetCountdown,
  formatResetDuration,
  formatUsagePercent,
  formatWindowLabel,
  rateLimitWindowTooltip,
  type ProviderRateLimits,
  type RateLimitResetCredit,
  type RateLimitWindow,
} from "../../features/providers/model/rateLimits";
import type { CodexRateLimitResetOutcome } from "../../features/providers/model/rateLimitsFetch";
import {
  mascotPath,
  projectMascot,
} from "../../features/projects/model/projectMascots";
import { projectKey, projectName } from "../../shared/lib/paths";
import {
  HARNESS_TITLE,
  type HarnessId,
} from "../../features/sessions/model/session";
import {
  loadTabGroupColors,
  loadTabGroupCustomColors,
  loadTabGroupMascots,
  resolveTabGroupColor,
  resolveTabGroupMascot,
} from "../../features/workspace/model/tabGroups";
import { HarnessIcon } from "../../features/sessions/ui/HarnessIcon";
import {
  ArrowLeft,
  Check,
  ChevronRight,
  Plus,
  RefreshCw,
} from "../../shared/ui/icons";
import { Popover, type PopoverDismissReason } from "../../shared/ui/Popover";
import {
  ProviderSignInPanel,
  type ProviderSignInState,
} from "../../features/sessions/ui/ProviderSignInPanel";
import {
  supportsProviderAccounts,
  type ProviderAccount,
} from "../../features/providers/model/providerAccounts";
import {
  accountStatus,
  accountUsageKey,
  bestAlternativeAccount,
  useProviderAccountUsage,
} from "../../features/providers/model/accountUsage";
import {
  AccountStatusLabel,
  barClass,
  meterWindows,
  UsageMeter,
} from "../../features/providers/ui/ProviderAccountUsage";
import {
  identityKey,
  identityOrganizationTag,
  useProviderAccountIdentities,
  type ProviderAccountIdentity,
} from "../../features/providers/model/providerAccountIdentity";
import { ProviderAccountSubtitle } from "../../features/providers/ui/ProviderAccountSubtitle";
import { useShowRemainingUsage } from "../../features/settings/model/displayPrefs";
import { getLocaleTag, t as translate } from "../../shared/i18n/locale";
import { useT } from "../../shared/i18n/useT";

type UsageWindowEntry = {
  key: "session" | "weekly" | "monthly";
  window: RateLimitWindow;
};

export type UsageChipVariant = "footer" | "composer";

const USAGE_CHIP_CLASSES: Record<UsageChipVariant, string> = {
  footer:
    "-mx-1 inline-flex h-5 min-w-0 shrink-0 items-center gap-1.5 whitespace-nowrap rounded px-1 text-content/55 transition-[background-color,color,transform] duration-150 ease-out hover:bg-content/10 hover:text-content focus-visible:outline-2 focus-visible:outline-accent active:scale-[0.97]",
  composer:
    "inline-flex h-6.5 min-w-0 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-md px-1.5 text-content/55 transition-colors hover:bg-content/8 hover:text-content focus-visible:outline-2 focus-visible:outline-accent",
};

/** The refresh button beside the usage chips, per variant. */
export const USAGE_REFRESH_CLASSES: Record<UsageChipVariant, string> = {
  footer:
    "grid size-4.5 shrink-0 place-items-center rounded text-content/40 hover:bg-content/10 hover:text-content disabled:opacity-50",
  composer:
    "grid size-6.5 shrink-0 place-items-center rounded-md text-content/40 transition-colors hover:bg-content/8 hover:text-content disabled:opacity-50",
};

type ResetActionState =
  "idle" | "confirming" | "using" | CodexRateLimitResetOutcome | "error";

export function UsageProviderChip({
  limits,
  now,
  project,
  accounts = [],
  accountId,
  onSelectAccount,
  onAddAccount,
  onManageAccounts,
  onConsumeReset,
  onReconnect,
  presentation,
  variant = "footer",
}: {
  limits: ProviderRateLimits;
  now: number;
  presentation?: { harness: HarnessId; label: string; sourceLabel?: string };
  project?: string;
  accounts?: ProviderAccount[];
  accountId?: string;
  onSelectAccount?: (accountId: string) => void;
  onAddAccount?: (label: string) => Promise<ProviderAccount>;
  onManageAccounts?: () => void;
  onConsumeReset?: (creditId?: string) => Promise<CodexRateLimitResetOutcome>;
  onReconnect?: () => Promise<void>;
  /** `composer` matches the quiet tool buttons under a Mono's composer. */
  variant?: UsageChipVariant;
}) {
  const t = useT();
  const showRemaining = useShowRemainingUsage();
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const [accountView, setAccountView] = useState<"usage" | "accounts" | "add">(
    "usage",
  );
  const [resetAction, setResetAction] = useState<ResetActionState>("idle");
  const [activeResetKey, setActiveResetKey] = useState<string | null>(null);
  const [resetError, setResetError] = useState<string | null>(null);
  const [reconnectState, setReconnectState] =
    useState<ProviderSignInState>("idle");
  const [reconnectError, setReconnectError] = useState<string | null>(null);
  const loading =
    limits.status === "idle" ||
    (limits.status === "fetching" &&
      !limits.session &&
      !limits.weekly &&
      !limits.monthly);
  const disconnected = limits.status === "unavailable";
  const windows = usageWindows(limits);
  const loginView = Boolean(
    onReconnect &&
    windows.length === 0 &&
    (needsProviderLogin(limits) || reconnectState !== "idle"),
  );
  const tightest = windows.reduce<RateLimitWindow | null>((best, entry) => {
    if (!best || entry.window.usedPercent > best.usedPercent) {
      return entry.window;
    }
    return best;
  }, null);
  const tooltip = windows
    .map((entry) => rateLimitWindowTooltip(entry.window, now, showRemaining))
    .join(" · ");
  const providerLabel = presentation?.label ?? HARNESS_TITLE[limits.provider];
  const iconHarness = presentation?.harness ?? limits.provider;
  const activeAccount = accounts.find((account) => account.id === accountId);
  const canManageAccounts = Boolean(onSelectAccount && onAddAccount);
  const activeAccountLabel =
    activeAccount?.label ?? t("app.usageChip.removedAccount");
  const identities = useProviderAccountIdentities(
    accounts,
    `${open}:${limits.updatedAt}:${reconnectState}`,
  );
  const activeIdentity = activeAccount
    ? identities[identityKey(activeAccount)]
    : null;
  const accountProvider = supportsProviderAccounts(limits.provider)
    ? limits.provider
    : undefined;
  const otherAccounts = accounts.filter((account) => account.id !== accountId);
  const accountUsage = useProviderAccountUsage(
    accounts.map((account) => account.id).join("|"),
    {
      provider: accountProvider,
      enabled: open && Boolean(accountProvider) && otherAccounts.length > 0,
    },
  );
  // The footer's own snapshot is fresher for the active account.
  const usageFor = (account: ProviderAccount) =>
    account.id === accountId
      ? limits
      : accountUsage.usage[accountUsageKey(account)];
  const activeStatus = accountStatus(limits, now);
  const suggestion =
    activeStatus.tone === "exhausted" || activeStatus.tone === "low"
      ? bestAlternativeAccount(otherAccounts, usageFor, now)
      : null;
  const mascotProject = project ? projectName(project) : providerLabel;
  const appearanceKey = project ? projectKey(project) : mascotProject;
  const mascotName = resolveTabGroupMascot(
    appearanceKey,
    loadTabGroupMascots(),
  );
  const mascotColor = resolveTabGroupColor(
    appearanceKey,
    loadTabGroupColors(),
    loadTabGroupCustomColors(),
    mascotProject,
  );

  useEffect(() => {
    if (open) return;
    setResetAction("idle");
    setActiveResetKey(null);
    setResetError(null);
    setReconnectState("idle");
    setReconnectError(null);
    setAccountView("usage");
  }, [open]);

  const dismiss = (reason: PopoverDismissReason) => {
    setOpen(false);
    if (reason === "escape") {
      requestAnimationFrame(() => trigger.current?.focus());
    }
  };

  const useReset = async (
    credit: RateLimitResetCredit | undefined,
    rowKey: string,
  ) => {
    if (!onConsumeReset) return;
    setActiveResetKey(rowKey);
    setResetAction("using");
    setResetError(null);
    try {
      setResetAction(await onConsumeReset(credit?.id));
    } catch (error) {
      setResetError(
        error instanceof Error
          ? error.message
          : t("app.usageChip.useResetFailed"),
      );
      setResetAction("error");
    }
  };

  const reconnect = async () => {
    if (!onReconnect) return;
    setReconnectState("running");
    setReconnectError(null);
    try {
      await onReconnect();
      setReconnectState("complete");
    } catch (error) {
      setReconnectError(
        error instanceof Error
          ? error.message
          : t("app.usageFooter.signInFailed"),
      );
      setReconnectState("error");
    }
  };

  return (
    <>
      <button
        ref={trigger}
        type="button"
        className={USAGE_CHIP_CLASSES[variant]}
        aria-label={t("app.usageChip.usageDetailsFor", {
          provider: providerLabel,
        })}
        aria-expanded={open}
        aria-haspopup="dialog"
        title={
          tooltip ||
          limits.error ||
          (disconnected
            ? t("app.usageChip.notConnected")
            : loading
              ? t("app.usageChip.loadingUsage")
              : t("app.usageChip.usageDetails"))
        }
        onClick={() => setOpen((value) => !value)}
      >
        <HarnessIcon harness={iconHarness} className="size-3 shrink-0" />
        {loading ? (
          <span className="animate-pulse text-content/35">···</span>
        ) : disconnected ? (
          <span className="text-content/35">
            {t("app.usageChip.notConnectedShort")}
          </span>
        ) : windows.length === 0 ? (
          <span className="text-content/35">{emptyUsageLabel(limits)}</span>
        ) : (
          <>
            {accounts.length > 1 && activeAccount ? (
              <span className="max-w-24 truncate text-content/45">
                {activeAccount.label}
              </span>
            ) : null}
            {tightest ? <MiniBar usedPct={tightest.usedPercent} /> : null}
            <span className="flex min-w-0 items-center gap-1 tabular-nums">
              {windows.map((entry, index) => (
                <span
                  key={entry.key}
                  className="inline-flex items-center gap-1"
                >
                  {index > 0 ? (
                    <span className="text-content/25">·</span>
                  ) : null}
                  <span>
                    {formatUsagePercent(
                      showRemaining
                        ? 100 - clampUsedPercent(entry.window.usedPercent)
                        : entry.window.usedPercent,
                    )}{" "}
                    {formatRateLimitWindowChipLabel(entry.window, now)}
                  </span>
                </span>
              ))}
            </span>
          </>
        )}
      </button>
      {open ? (
        <Popover
          anchor={trigger}
          side="top"
          align="start"
          gap={7}
          width={accountView === "accounts" ? 340 : 300}
          maxHeight={460}
          autoFocus
          onDismiss={dismiss}
          role="dialog"
          aria-label={t("app.usageChip.usageDetailsFor", {
            provider: providerLabel,
          })}
          tabIndex={-1}
          className={`overflow-y-auto text-content ${accountView === "usage" && loginView ? "" : "p-2.5"}`}
        >
          {accountView === "accounts" ? (
            <ProviderAccountPicker
              providerLabel={providerLabel}
              accounts={accounts}
              identities={identities}
              accountId={accountId ?? ""}
              usageFor={usageFor}
              now={now}
              onBack={() => setAccountView("usage")}
              onAdd={() => setAccountView("add")}
              onManage={
                onManageAccounts
                  ? () => {
                      setOpen(false);
                      onManageAccounts();
                    }
                  : undefined
              }
              onSelect={(nextAccountId) => {
                onSelectAccount?.(nextAccountId);
                setOpen(false);
              }}
            />
          ) : accountView === "add" ? (
            <AddProviderAccount
              providerLabel={providerLabel}
              onBack={() => setAccountView("accounts")}
              onAdd={onAddAccount}
              onComplete={() => setOpen(false)}
            />
          ) : loginView ? (
            <>
              {canManageAccounts ? (
                <AccountSwitchRow
                  accountLabel={activeAccountLabel}
                  onClick={() => setAccountView("accounts")}
                />
              ) : null}
              <ProviderSignInPanel
                harness={limits.provider}
                state={reconnectState}
                error={reconnectError}
                onSignIn={() => void reconnect()}
              />
            </>
          ) : (
            <>
              <div className="flex items-start gap-2.5 px-1 pb-2.5 pt-0.5">
                <span className="grid size-7 shrink-0 place-items-center rounded-lg bg-content/[0.06] ring-1 ring-inset ring-content/[0.07]">
                  <HarnessIcon harness={iconHarness} className="size-4" />
                </span>
                <div className="min-w-0 flex-1">
                  <h2 className="text-[13px] font-medium leading-4">
                    {t("app.usageChip.providerUsage", {
                      provider: providerLabel,
                    })}
                  </h2>
                  <p className="mt-0.5 text-[10px] leading-4 text-content/40">
                    {updatedLabel(limits, now)}
                  </p>
                  {presentation?.sourceLabel ? (
                    <p className="mt-0.5 text-[10px] leading-4 text-content/55">
                      {presentation.sourceLabel}
                    </p>
                  ) : null}
                  {canManageAccounts ? (
                    <div className="pointer-events-none relative mt-1 -ml-1 inline-flex max-w-full items-center gap-1 rounded px-1 py-0.5 text-[10px] text-content/55">
                      {/* Keep account switching separate from email revelation. */}
                      <button
                        type="button"
                        className="pointer-events-auto absolute inset-0 rounded hover:bg-content/10 focus-visible:outline-2 focus-visible:outline-accent"
                        aria-label={t("app.usageChip.switchProviderAccount", {
                          provider: providerLabel,
                        })}
                        onClick={() => setAccountView("accounts")}
                      />
                      <span className="max-w-[60%] shrink-0 truncate">
                        {activeAccountLabel}
                      </span>
                      <ProviderAccountSubtitle
                        key={activeAccount && identityKey(activeAccount)}
                        identity={activeIdentity}
                        className="text-content/35"
                      />
                      <ChevronRight
                        className="size-2.5 shrink-0"
                        strokeWidth={1.75}
                        aria-hidden
                      />
                    </div>
                  ) : null}
                </div>
                {limits.status === "fetching" ? (
                  <span className="mt-0.5 inline-flex items-center gap-1 text-[10px] text-content/40">
                    <RefreshCw
                      className="size-2.5 animate-spin"
                      strokeWidth={1.75}
                      aria-hidden
                    />
                    {t("app.usageChip.updating")}
                  </span>
                ) : null}
              </div>

              {limits.status === "error" && windows.length > 0 ? (
                <p className="mb-2 rounded-lg bg-amber-400/10 px-2.5 py-2 text-[10px] leading-4 text-amber-700 dark:text-amber-300">
                  {t("app.usageChip.refreshFailed")}
                </p>
              ) : null}

              {windows.length > 0 ? (
                <div className="flex flex-col gap-1.5">
                  {windows.map((entry) => (
                    <UsageWindowCard
                      key={entry.key}
                      kind={entry.key}
                      window={entry.window}
                      now={now}
                    />
                  ))}
                </div>
              ) : (
                <EmptyUsageState limits={limits} loading={loading} />
              )}

              {limits.extraUsageBalance != null ? (
                <section className="mt-1.5 flex items-baseline justify-between gap-3 rounded-lg bg-content/[0.045] px-3 py-2.5 ring-1 ring-inset ring-content/[0.06]">
                  <h3 className="text-[11px] font-medium text-content/65">
                    {t("app.usageChip.extraUsageBalance")}
                  </h3>
                  <span className="shrink-0 text-[11px] font-medium tabular-nums">
                    {formatExtraUsageBalance(limits.extraUsageBalance)}
                  </span>
                </section>
              ) : null}

              {suggestion && onSelectAccount ? (
                <SwitchSuggestion
                  account={suggestion}
                  limits={usageFor(suggestion)}
                  exhausted={activeStatus.tone === "exhausted"}
                  now={now}
                  onSwitch={() => {
                    onSelectAccount(suggestion.id);
                    setOpen(false);
                  }}
                />
              ) : null}

              {limits.provider === "codex" ? (
                <BankedResets
                  limits={limits}
                  now={now}
                  action={resetAction}
                  activeResetKey={activeResetKey}
                  error={resetError}
                  mascotProject={mascotProject}
                  mascotName={mascotName}
                  mascotColor={mascotColor}
                  onConfirm={(creditId) => {
                    setActiveResetKey(creditId);
                    setResetAction("confirming");
                  }}
                  onCancel={() => {
                    setActiveResetKey(null);
                    setResetAction("idle");
                  }}
                  onUse={useReset}
                  canUse={Boolean(onConsumeReset)}
                />
              ) : null}
            </>
          )}
        </Popover>
      ) : null}
    </>
  );
}

function AccountSwitchRow({
  accountLabel,
  onClick,
}: {
  accountLabel: string;
  onClick: () => void;
}) {
  const t = useT();
  return (
    <div className="px-2.5 pt-2.5">
      <button
        type="button"
        className="flex h-8 w-full items-center gap-2 rounded-lg bg-content/[0.045] px-2.5 text-left text-[11px] ring-1 ring-inset ring-content/[0.06] hover:bg-content/[0.08]"
        aria-label={t("app.usageChip.switchAccountFrom", {
          account: accountLabel,
        })}
        onClick={onClick}
      >
        <span className="min-w-0 flex-1 truncate">{accountLabel}</span>
        <span className="text-[10px] text-content/40">
          {t("app.usageChip.switch")}
        </span>
        <ChevronRight
          className="size-3 shrink-0 text-content/35"
          strokeWidth={1.75}
          aria-hidden
        />
      </button>
    </div>
  );
}

function ProviderAccountPicker({
  providerLabel,
  accounts,
  identities,
  accountId,
  usageFor,
  now,
  onBack,
  onAdd,
  onManage,
  onSelect,
}: {
  providerLabel: string;
  accounts: ProviderAccount[];
  identities: Record<string, ProviderAccountIdentity | null>;
  accountId: string;
  usageFor: (account: ProviderAccount) => ProviderRateLimits | undefined;
  now: number;
  onBack: () => void;
  onAdd: () => void;
  onManage?: () => void;
  onSelect: (accountId: string) => void;
}) {
  const t = useT();
  const statusId = useId();
  return (
    <div>
      <div className="flex h-7 items-center gap-1">
        <button
          type="button"
          className="grid size-6 place-items-center rounded-md text-content/45 hover:bg-content/10 hover:text-content"
          aria-label={t("app.usageChip.backToUsage")}
          onClick={onBack}
        >
          <ArrowLeft className="size-3.5" strokeWidth={1.75} aria-hidden />
        </button>
        <h2 className="text-[13px] font-medium">
          {t("app.usageChip.providerAccounts", { provider: providerLabel })}
        </h2>
      </div>
      <p className="mt-1 px-1 text-[10px] leading-4 text-content/40">
        {t("app.usageChip.accountsHint")}
      </p>
      <div
        className="mt-2 flex flex-col gap-1"
        role="group"
        aria-label={t("app.usageChip.providerAccounts", {
          provider: providerLabel,
        })}
      >
        {accounts.map((account) => {
          const selected = account.id === accountId;
          const identity = identities[identityKey(account)];
          const orgTag = identityOrganizationTag(identity);
          const usage = usageFor(account);
          const meters = meterWindows(usage);
          return (
            <div
              key={account.id}
              className={`pointer-events-none relative flex w-full items-center gap-3 rounded-lg px-2.5 py-2 text-left text-[11px] ring-1 ring-inset transition-colors ${
                selected
                  ? "bg-accent/10 text-content ring-accent/20"
                  : "bg-content/[0.035] text-content/70 ring-content/[0.06]"
              }`}
            >
              {/* A sibling target keeps the email button out of the selection button. */}
              <button
                type="button"
                aria-pressed={selected}
                aria-label={account.label}
                aria-describedby={`${statusId}-${account.id}`}
                className="pointer-events-auto absolute inset-0 rounded-lg hover:bg-content/[0.04] focus-visible:outline-2 focus-visible:outline-accent"
                onClick={() => onSelect(account.id)}
              />
              <span className="min-w-0 flex-1 py-0.5">
                <span className="flex min-w-0 items-baseline gap-1.5">
                  <span className="shrink-0 truncate">{account.label}</span>
                  <ProviderAccountSubtitle
                    identity={identity}
                    className="text-[10px] text-content/35"
                  />
                  {orgTag ? (
                    <span className="max-w-[6rem] shrink-0 truncate rounded bg-content/[0.07] px-1 text-[9px] leading-4 text-content/50">
                      {orgTag}
                    </span>
                  ) : null}
                </span>
                <span className="mt-1 flex min-w-0 items-center gap-3 text-[10px]">
                  <span
                    id={`${statusId}-${account.id}`}
                    // Without meters, a long "unknown" reason truncates.
                    className={meters.length > 0 ? "shrink-0" : "min-w-0"}
                  >
                    <AccountStatusLabel
                      status={accountStatus(usage, now)}
                      className="min-w-0"
                    />
                  </span>
                  {meters.length > 0 ? (
                    <span className="flex min-w-0 flex-1 gap-2.5">
                      {meters.map((entry) => (
                        // Short "5h" / "wk" titles, as on the footer chip.
                        <UsageMeter
                          key={entry.title}
                          title={formatWindowLabel(entry.window.windowMinutes)}
                          window={entry.window}
                          now={now}
                          className="min-w-0 flex-1"
                        />
                      ))}
                    </span>
                  ) : null}
                </span>
              </span>
              {selected ? (
                <Check
                  className="size-3.5 shrink-0 text-accent"
                  strokeWidth={1.9}
                  aria-hidden
                />
              ) : null}
            </div>
          );
        })}
      </div>
      <button
        type="button"
        className="mt-2 flex h-8 w-full items-center gap-2 rounded-lg px-2.5 text-left text-[11px] text-content/55 hover:bg-content/[0.07] hover:text-content"
        onClick={onAdd}
      >
        <Plus className="size-3.5" strokeWidth={1.75} aria-hidden />
        {t("app.usageChip.addAccount")}
      </button>
      {onManage ? (
        <button
          type="button"
          className="mt-0.5 flex h-8 w-full items-center gap-2 rounded-lg px-2.5 text-left text-[11px] text-content/45 hover:bg-content/[0.07] hover:text-content"
          onClick={() => {
            onManage();
          }}
        >
          {t("app.usageChip.manageAccounts")}
        </button>
      ) : null}
    </div>
  );
}

function SwitchSuggestion({
  account,
  limits,
  exhausted,
  now,
  onSwitch,
}: {
  account: ProviderAccount;
  limits: ProviderRateLimits | undefined;
  exhausted: boolean;
  now: number;
  onSwitch: () => void;
}) {
  const t = useT();
  return (
    <section className="mt-2 flex items-center gap-2.5 rounded-lg bg-content/[0.045] px-3 py-2.5 ring-1 ring-inset ring-content/[0.06]">
      <div className="min-w-0 flex-1">
        <p className="text-[10px] leading-4 text-content/45">
          {exhausted
            ? t("app.usageChip.outOfUsage")
            : t("app.usageChip.runningLow")}{" "}
          · {t("app.usageChip.switchTo")}
        </p>
        <p className="mt-0.5 flex min-w-0 items-center gap-2 text-[11px]">
          <span className="min-w-0 truncate font-medium">{account.label}</span>
          <AccountStatusLabel
            status={accountStatus(limits, now)}
            className="text-[10px]"
          />
        </p>
      </div>
      <button
        type="button"
        className="h-7 shrink-0 rounded-md bg-content px-2.5 text-[11px] font-medium text-background-base transition-transform duration-150 hover:bg-content/85 active:scale-[0.97]"
        onClick={onSwitch}
      >
        {t("app.usageChip.switch")}
      </button>
    </section>
  );
}

function AddProviderAccount({
  providerLabel,
  onBack,
  onAdd,
  onComplete,
}: {
  providerLabel: string;
  onBack: () => void;
  onAdd?: (label: string) => Promise<ProviderAccount>;
  onComplete: () => void;
}) {
  const t = useT();
  const [label, setLabel] = useState("");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!onAdd || !label.trim() || running) return;
    setRunning(true);
    setError(null);
    try {
      await onAdd(label);
      onComplete();
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : t("app.usageChip.addAccountFailed"),
      );
      setRunning(false);
    }
  };

  return (
    <form onSubmit={(event) => void submit(event)}>
      <div className="flex h-7 items-center gap-1">
        <button
          type="button"
          className="grid size-6 place-items-center rounded-md text-content/45 hover:bg-content/10 hover:text-content disabled:opacity-40"
          aria-label={t("app.usageChip.backToAccounts")}
          disabled={running}
          onClick={onBack}
        >
          <ArrowLeft className="size-3.5" strokeWidth={1.75} aria-hidden />
        </button>
        <h2 className="text-[13px] font-medium">
          {t("app.usageChip.addProviderAccount", { provider: providerLabel })}
        </h2>
      </div>
      <p className="mt-1 px-1 text-[10px] leading-4 text-content/40">
        {t("app.usageChip.addAccountHint")}
      </p>
      <label className="mt-3 block text-[10px] font-medium text-content/55">
        {t("app.usageChip.accountName")}
        <input
          autoFocus
          type="text"
          maxLength={48}
          value={label}
          disabled={running}
          placeholder={t("app.usageChip.accountNamePlaceholder")}
          className="mt-1.5 h-8 w-full rounded-lg border border-content/10 bg-content/[0.04] px-2.5 text-[11px] text-content outline-none placeholder:text-content/25 focus:border-accent/45 disabled:opacity-55"
          onChange={(event) => setLabel(event.target.value)}
        />
      </label>
      <button
        type="submit"
        className="mt-3 inline-flex h-8 w-full items-center justify-center gap-1.5 rounded-lg bg-content px-3 text-[11px] font-medium text-background-base hover:bg-content/85 disabled:cursor-default disabled:opacity-45"
        disabled={running || !label.trim()}
      >
        {running ? (
          <RefreshCw className="size-3.5 animate-spin" aria-hidden />
        ) : null}
        {running
          ? t("app.usageChip.waitingForBrowser")
          : t("app.usageChip.signInAndAdd")}
      </button>
      {error ? (
        <p className="mt-2 text-[10px] leading-4 text-red-500" role="status">
          {error}
        </p>
      ) : null}
    </form>
  );
}

function usageWindows(limits: ProviderRateLimits): UsageWindowEntry[] {
  return [
    limits.session
      ? ({ key: "session", window: limits.session } as const)
      : null,
    limits.weekly ? ({ key: "weekly", window: limits.weekly } as const) : null,
    limits.monthly
      ? ({ key: "monthly", window: limits.monthly } as const)
      : null,
  ].filter((entry): entry is UsageWindowEntry => entry != null);
}

function UsageWindowCard({
  kind,
  window,
  now,
}: {
  kind: UsageWindowEntry["key"];
  window: RateLimitWindow;
  now: number;
}) {
  const t = useT();
  const showRemaining = useShowRemainingUsage();
  const pct = clampUsedPercent(window.usedPercent);
  const remaining = 100 - pct;
  const shown = showRemaining ? remaining : pct;
  const title =
    kind === "session" && window.windowMinutes === DAILY_WINDOW_MINUTES
      ? t("app.usageChip.dailyLimit")
      : kind === "session"
        ? t("app.usageChip.fiveHourLimit")
        : kind === "weekly"
          ? t("app.usageChip.weeklyLimit")
          : kind === "monthly"
            ? t("app.usageChip.monthlyLimit")
            : t("app.usageChip.windowLimit", {
                window: formatWindowLabel(window.windowMinutes),
              });
  return (
    <section className="rounded-lg bg-content/[0.045] px-3 py-2.5 ring-1 ring-inset ring-content/[0.06]">
      <div className="flex items-baseline justify-between gap-3">
        <h3 className="text-[11px] font-medium text-content/65">{title}</h3>
        <span className="shrink-0 text-[11px] font-medium tabular-nums">
          {formatUsagePercent(shown)}{" "}
          {showRemaining
            ? t("app.usageChip.remaining")
            : t("app.usageChip.used")}
        </span>
      </div>
      <div
        className="mt-2 h-1.5 overflow-hidden rounded-full bg-content/10"
        role="progressbar"
        aria-label={`${title} ${showRemaining ? t("app.usageChip.remaining") : t("app.usageChip.used")}`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(shown)}
      >
        <span
          className={`block h-full rounded-full ${barClass(pct)}`}
          style={{ width: `${shown}%` }}
        />
      </div>
      <div className="mt-1.5 flex items-center justify-between gap-3 text-[10px] leading-4 text-content/40">
        <span className="tabular-nums">
          {formatUsagePercent(showRemaining ? pct : remaining)}{" "}
          {showRemaining
            ? t("app.usageChip.used")
            : t("app.usageChip.remaining")}
        </span>
        <span
          className="truncate text-right tabular-nums"
          title={
            window.resetsAt == null
              ? undefined
              : new Date(window.resetsAt).toLocaleString(getLocaleTag())
          }
        >
          {window.resetsAt == null
            ? t("app.usageChip.window", {
                window: formatWindowLabel(window.windowMinutes),
              })
            : formatResetCountdown(window.resetsAt - now)}
        </span>
      </div>
    </section>
  );
}

function BankedResets({
  limits,
  now,
  action,
  activeResetKey,
  error,
  mascotProject,
  mascotName,
  mascotColor,
  onConfirm,
  onCancel,
  onUse,
  canUse,
}: {
  limits: ProviderRateLimits;
  now: number;
  action: ResetActionState;
  activeResetKey: string | null;
  error: string | null;
  mascotProject: string;
  mascotName: string | null;
  mascotColor: string;
  onConfirm: (rowKey: string) => void;
  onCancel: () => void;
  onUse: (credit: RateLimitResetCredit | undefined, rowKey: string) => void;
  canUse: boolean;
}) {
  const t = useT();
  const count = limits.resetCredits?.availableCount ?? 0;
  if (count <= 0) return null;

  const detailedCredits = (limits.resetCredits?.credits ?? []).filter(
    (credit) => credit.status === "available" || credit.status === "unknown",
  );
  const unlistedCount = Math.max(0, count - detailedCredits.length);
  const rows: Array<RateLimitResetCredit | null> = [
    ...detailedCredits,
    ...Array.from({ length: unlistedCount }, () => null),
  ];
  return (
    <section className="mt-2.5 border-t border-content/[0.08] pt-2.5">
      <div className="relative min-h-[78px] overflow-hidden rounded-lg bg-content/[0.04] px-3 py-3 pr-[84px] ring-1 ring-inset ring-content/[0.06]">
        <div className="relative z-10 min-w-0">
          <div className="flex items-center gap-1.5">
            <h3 className="text-[11px] font-medium">
              {t("app.usageChip.bankedResets")}
            </h3>
            <span className="rounded-full bg-content/[0.07] px-1.5 py-px text-[9px] font-medium tabular-nums text-content/65 ring-1 ring-inset ring-content/[0.07]">
              {count}
            </span>
          </div>
          <p className="mt-0.5 text-[10px] leading-4 text-content/40">
            {t("app.usageChip.resetsAvailable", { count })}
          </p>
        </div>
        <BankedResetMascot
          project={mascotProject}
          name={mascotName}
          color={mascotColor}
        />
      </div>

      <div
        className="mt-2 max-h-56 overflow-y-auto overscroll-contain"
        aria-label={t("app.usageChip.availableBankedResets")}
      >
        <div className="flex flex-col gap-1.5">
          {rows.map((credit, index) => {
            const rowKey = credit?.id ?? `unlisted-${index}`;
            const selected = activeResetKey === rowKey;
            return (
              <BankedResetRow
                key={rowKey}
                credit={credit}
                index={index}
                now={now}
                action={selected ? action : "idle"}
                error={selected ? error : null}
                disabled={action === "using" && !selected}
                canUse={canUse}
                onConfirm={() => onConfirm(rowKey)}
                onCancel={onCancel}
                onUse={() => onUse(credit ?? undefined, rowKey)}
              />
            );
          })}
        </div>
      </div>
    </section>
  );
}

function BankedResetMascot({
  project,
  name,
  color,
}: {
  project: string;
  name: string | null;
  color: string;
}) {
  const mascot = projectMascot(project, name);
  const spritePath = `${mascot.restPath}${mascotFacePlatePath(mascot.rest)}`;
  const maskId = `banked-reset-mascot-${useId().replace(/:/g, "")}`;
  return (
    <div
      className="reset-mascot-scene"
      data-reset-mascot-mood="happy"
      data-mascot-name={mascot.name}
      style={{ color }}
      aria-hidden
    >
      <span className="reset-mascot-glow" />
      <span className="reset-mascot-spark reset-mascot-spark-a" />
      <span className="reset-mascot-spark reset-mascot-spark-b" />
      <svg
        className="reset-mascot-sprite"
        viewBox="0 0 8 8"
        shapeRendering="crispEdges"
      >
        <defs>
          <mask
            id={maskId}
            maskUnits="userSpaceOnUse"
            x="0"
            y="0"
            width="8"
            height="8"
          >
            <rect width="8" height="8" fill="black" />
            <path d={spritePath} fill="white" />
            <g fill="black">
              <rect x="2" y="3" width="1" height="1" />
              <rect x="5" y="3" width="1" height="1" />
              <rect x="2" y="4" width="1" height="1" />
              <rect x="5" y="4" width="1" height="1" />
              <rect x="3" y="5" width="2" height="1" />
            </g>
          </mask>
        </defs>
        <path d={spritePath} fill="currentColor" mask={`url(#${maskId})`} />
      </svg>
    </div>
  );
}

/** Fill only the middle of each face row before cutting the mood back out. */
function mascotFacePlatePath(rows: readonly string[]): string {
  return mascotPath(
    rows.map((row, y) => {
      if (y < 2 || y > 5) return ".".repeat(row.length);
      const first = row.indexOf("#");
      const last = row.lastIndexOf("#");
      if (first < 0) return ".".repeat(row.length);
      return `${".".repeat(first)}${"#".repeat(last - first + 1)}${".".repeat(row.length - last - 1)}`;
    }),
  );
}

function BankedResetRow({
  credit,
  index,
  now,
  action,
  error,
  disabled,
  canUse,
  onConfirm,
  onCancel,
  onUse,
}: {
  credit: RateLimitResetCredit | null;
  index: number;
  now: number;
  action: ResetActionState;
  error: string | null;
  disabled: boolean;
  canUse: boolean;
  onConfirm: () => void;
  onCancel: () => void;
  onUse: () => void;
}) {
  const t = useT();
  return (
    <article className="rounded-lg bg-content/[0.04] px-2.5 py-2 ring-1 ring-inset ring-content/[0.06]">
      <h4 className="text-[10px] font-medium leading-4 text-content/70">
        {credit?.title ?? t("app.usageChip.bankedReset", { index: index + 1 })}
      </h4>
      {credit?.description ? (
        <p className="mt-0.5 text-[10px] leading-4 text-content/45">
          {credit.description}
        </p>
      ) : null}
      <div className="mt-1.5 flex min-h-6 items-center justify-between gap-2">
        <p
          className="min-w-0 truncate text-[10px] tabular-nums text-content/40"
          title={
            credit?.expiresAt == null
              ? undefined
              : new Date(credit.expiresAt).toLocaleString(getLocaleTag())
          }
        >
          {credit?.expiresAt == null
            ? t("app.usageChip.expiryNotProvided")
            : credit.expiresAt <= now
              ? t("app.usageChip.expiresNow")
              : t("app.usageChip.expiresIn", {
                  duration: formatResetDuration(credit.expiresAt - now),
                })}
        </p>
        {action === "using" ? (
          <span className="inline-flex shrink-0 items-center gap-1.5 text-[10px] text-content/45">
            <RefreshCw
              className="size-3 animate-spin"
              strokeWidth={1.75}
              aria-hidden
            />
            {t("app.usageChip.applying")}
          </span>
        ) : isResetOutcome(action) || action === "error" ? (
          <span
            className={`shrink-0 text-[10px] ${
              action === "reset"
                ? "text-emerald-700 dark:text-emerald-300"
                : "text-content/50"
            }`}
            role="status"
          >
            {action === "error" ? error : resetOutcomeLabel(action)}
          </span>
        ) : canUse && action !== "confirming" ? (
          <button
            type="button"
            className="h-6 shrink-0 rounded-md bg-content/[0.07] px-2.5 text-[10px] font-medium text-content/70 ring-1 ring-inset ring-content/[0.08] transition-[background-color,color,transform] duration-150 ease-out hover:bg-content/[0.11] hover:text-content active:scale-[0.97] disabled:pointer-events-none disabled:opacity-35"
            disabled={disabled}
            onClick={onConfirm}
          >
            {t("app.usageChip.useReset")}
          </button>
        ) : null}
      </div>
      {action === "confirming" ? (
        <div className="mt-2 flex items-center justify-between gap-2 border-t border-content/[0.07] pt-2">
          <p className="text-[10px] leading-4 text-content/50">
            {t("app.usageChip.spendResetConfirm")}
          </p>
          <div className="flex shrink-0 gap-1">
            <button
              type="button"
              className="h-6 rounded-md px-2 text-[10px] text-content/50 hover:bg-content/10 hover:text-content"
              onClick={onCancel}
            >
              {t("app.usageChip.cancel")}
            </button>
            <button
              type="button"
              className="h-6 rounded-md bg-content px-2.5 text-[10px] font-medium text-background-base transition-transform duration-150 ease-out active:scale-[0.97]"
              onClick={onUse}
            >
              {t("app.usageChip.confirm")}
            </button>
          </div>
        </div>
      ) : null}
    </article>
  );
}

function EmptyUsageState({
  limits,
  loading,
}: {
  limits: ProviderRateLimits;
  loading: boolean;
}) {
  const t = useT();
  return (
    <div className="rounded-lg bg-content/[0.04] px-3 py-4 text-center ring-1 ring-inset ring-content/[0.06]">
      <p className="text-[11px] font-medium text-content/65">
        {loading
          ? t("app.usageChip.loadingUsage")
          : limits.status === "unavailable"
            ? t("app.usageChip.notConnected")
            : t("app.piUsage.unavailable")}
      </p>
      {limits.error ? (
        <p className="mx-auto mt-1 max-w-[15rem] text-[10px] leading-4 text-content/40">
          {limits.error}
        </p>
      ) : null}
    </div>
  );
}

export function needsProviderLogin(limits: ProviderRateLimits): boolean {
  if (limits.status === "unavailable") return true;
  if (limits.status !== "error") return false;
  const text = limits.error?.toLowerCase() ?? "";
  return (
    text.includes("expired") ||
    text.includes("sign-in") ||
    text.includes("not signed in") ||
    text.includes("not connected") ||
    text.includes("authentication")
  );
}

function updatedLabel(limits: ProviderRateLimits, now: number): string {
  if (limits.updatedAt <= 0) return translate("app.usageChip.rateLimitDetails");
  const elapsedMinutes = Math.max(
    0,
    Math.floor((now - limits.updatedAt) / 60_000),
  );
  if (elapsedMinutes === 0) return translate("app.usageChip.updatedJustNow");
  if (elapsedMinutes < 60)
    return translate("app.usageChip.updatedMinutesAgo", {
      minutes: elapsedMinutes,
    });
  return translate("app.usageChip.updatedHoursAgo", {
    hours: Math.floor(elapsedMinutes / 60),
  });
}

function resetOutcomeLabel(outcome: CodexRateLimitResetOutcome): string {
  if (outcome === "reset") return translate("app.usageChip.outcomeReset");
  if (outcome === "nothingToReset")
    return translate("app.usageChip.outcomeNothingToReset");
  if (outcome === "noCredit") return translate("app.usageChip.outcomeNoCredit");
  return translate("app.usageChip.outcomeAlreadyUsed");
}

function isResetOutcome(
  value: ResetActionState,
): value is CodexRateLimitResetOutcome {
  return (
    value === "reset" ||
    value === "nothingToReset" ||
    value === "noCredit" ||
    value === "alreadyRedeemed"
  );
}

function emptyUsageLabel(limits: ProviderRateLimits): string {
  if (limits.status !== "error") return "—";
  const text = limits.error?.toLowerCase() ?? "";
  if (text.includes("expired") || text.includes("sign-in"))
    return translate("app.usageChip.expired");
  return "—";
}

function MiniBar({ usedPct }: { usedPct: number }) {
  const showRemaining = useShowRemainingUsage();
  const pct = clampUsedPercent(usedPct);
  return (
    <span
      className="h-1 w-8 shrink-0 overflow-hidden rounded-full bg-content/10"
      aria-hidden
    >
      <span
        className={`block h-full rounded-full ${barClass(pct)}`}
        style={{ width: `${showRemaining ? 100 - pct : pct}%` }}
      />
    </span>
  );
}
