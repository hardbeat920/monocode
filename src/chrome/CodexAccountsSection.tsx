import { useCallback, useEffect, useState } from "react";
import {
  captureCurrentExternalAccount,
  loadCodexAccounts,
  readExternalAuthInfo,
  removeCodexAccount,
  type CodexAccount,
  type ExternalAuthInfo,
} from "../lib/harness/codexAccounts";
import { formatResetDuration } from "../lib/rateLimits";
import { Check, Plus, RefreshCw, Trash2 } from "./icons";

/**
 * Codex multi-account management inside the usage popover. The pool feeds
 * failover: a session pins one account until a quota wall, then rotates to
 * another eligible identity. Whatever `codex login` holds in auth.json is the
 * external identity; "Save to pool" snapshots it so it stays usable after the
 * user signs in somewhere else.
 */
export function CodexAccountsSection({ now }: { now: number }) {
  const [external, setExternal] = useState<ExternalAuthInfo | null>(null);
  const [accounts, setAccounts] = useState<CodexAccount[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const reload = useCallback(async () => {
    const [list, auth] = await Promise.all([
      loadCodexAccounts().catch(() => [] as CodexAccount[]),
      readExternalAuthInfo(),
    ]);
    setAccounts(list);
    setExternal(auth);
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const run = async (task: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await task();
      await reload();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Action failed");
    } finally {
      setBusy(false);
    }
  };

  const capture = () =>
    run(async () => {
      await captureCurrentExternalAccount();
      setSaved(true);
    });

  const captured = new Set((accounts ?? []).map((a) => a.accountId));
  const rows = (accounts ?? []).filter((a) => a.id !== "external");

  return (
    <section className="mt-2 rounded-lg bg-content/[0.045] px-3 py-2.5 ring-1 ring-inset ring-content/[0.06]">
      <div className="flex items-baseline justify-between gap-3">
        <h3 className="text-[11px] font-medium text-content/65">Accounts</h3>
        <span className="shrink-0 text-[10px] tabular-nums text-content/40">
          {rows.length} pooled
        </span>
      </div>

      <div className="mt-2 flex flex-col gap-1.5">
        {external ? (
          <AccountRow
            email={external.email ?? "Unknown account"}
            plan={external.planType}
            badge="CLI login"
          />
        ) : null}
        {rows.map((account) => (
          <AccountRow
            key={account.id}
            email={account.email}
            plan={account.planType}
            badge={
              account.accountId === external?.accountId ? "CLI login" : null
            }
            status={accountStatus(account, now)}
            onRemove={() => {
              void run(() => removeCodexAccount(account.id));
            }}
            disabled={busy}
          />
        ))}
        {accounts == null ? (
          <p className="py-1 text-[10px] text-content/40">Loading…</p>
        ) : null}
      </div>

      <div className="mt-2 flex items-center justify-between gap-2 border-t border-content/[0.07] pt-2">
        <p className="min-w-0 text-[10px] leading-4 text-content/40">
          Sign in elsewhere via <code>codex login</code>, then save it here.
        </p>
        <button
          type="button"
          className="inline-flex h-6 shrink-0 items-center gap-1 rounded-md bg-content/[0.07] px-2.5 text-[10px] font-medium text-content/70 ring-1 ring-inset ring-content/[0.08] transition-[background-color,color,transform] duration-150 ease-out hover:bg-content/[0.11] hover:text-content active:scale-[0.97] disabled:pointer-events-none disabled:opacity-35"
          disabled={busy || !external || captured.has(external.accountId)}
          onClick={() => {
            setSaved(false);
            void capture();
          }}
        >
          {busy ? (
            <RefreshCw className="size-3 animate-spin" strokeWidth={1.75} />
          ) : saved ? (
            <Check className="size-3" strokeWidth={1.75} />
          ) : (
            <Plus className="size-3" strokeWidth={1.75} />
          )}
          {external && captured.has(external.accountId) ? "Saved" : "Save login"}
        </button>
      </div>
      {error ? (
        <p className="mt-1.5 text-[10px] leading-4 text-amber-700 dark:text-amber-300">
          {error}
        </p>
      ) : null}
    </section>
  );
}

function accountStatus(account: CodexAccount, now: number): string | null {
  if (account.disabledCause) return "disabled — sign in again";
  if (account.blockedUntilMs != null) {
    return account.blockedUntilMs <= now
      ? "quota reset"
      : `quota resets in ${formatResetDuration(account.blockedUntilMs - now)}`;
  }
  return null;
}

function AccountRow({
  email,
  plan,
  badge,
  status,
  onRemove,
  disabled,
}: {
  email: string;
  plan?: string | null;
  badge?: string | null;
  status?: string | null;
  onRemove?: () => void;
  disabled?: boolean;
}) {
  return (
    <div className="flex items-center gap-2">
      <div className="min-w-0 flex-1">
        <p className="truncate text-[11px] leading-4 text-content/80">
          {email}
        </p>
        {status ? (
          <p className="truncate text-[10px] leading-4 text-content/40">
            {status}
          </p>
        ) : null}
      </div>
      {plan ? (
        <span className="shrink-0 rounded bg-content/[0.06] px-1.5 py-0.5 text-[9px] font-medium uppercase tracking-wide text-content/50">
          {plan}
        </span>
      ) : null}
      {badge ? (
        <span className="shrink-0 rounded bg-emerald-500/10 px-1.5 py-0.5 text-[9px] font-medium text-emerald-700 dark:text-emerald-300">
          {badge}
        </span>
      ) : null}
      {onRemove ? (
        <button
          type="button"
          className="grid size-5 shrink-0 place-items-center rounded text-content/35 transition-colors hover:bg-content/10 hover:text-content disabled:pointer-events-none disabled:opacity-35"
          disabled={disabled}
          onClick={onRemove}
          aria-label={`Remove ${email}`}
          title="Remove from pool"
        >
          <Trash2 className="size-3" strokeWidth={1.75} />
        </button>
      ) : null}
    </div>
  );
}
