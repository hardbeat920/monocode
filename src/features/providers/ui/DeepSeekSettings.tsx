import { useCallback, useEffect, useState, type ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useLanguage } from "../../../shared/i18n/useTranslation";
import { translate } from "../../../shared/i18n/locale";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { RefreshCw } from "../../../shared/ui/icons";
import { refreshOpenCodeCatalog } from "../../../integrations/harness/providers/opencode/opencodeCatalog";
import {
  getDeepseekCatalogSync,
  refreshDeepseekCatalog,
  subscribeDeepseekCatalogSync,
  type DeepseekCatalogSync,
} from "../../../integrations/harness/providers/deepseek/deepseekCatalog";
import { UpstreamProviderIcon } from "../../sessions/ui/UpstreamProviderIcon";
import {
  cancelDeepseekOfficialLogin,
  clearDeepseekApiKey,
  loadDeepseekAccountQuota,
  loadDeepseekAccountStatus,
  loadDeepseekBalance,
  loadDeepseekKeyStatus,
  saveDeepseekApiKey,
  signOutDeepseekOfficial,
  startDeepseekOfficialLogin,
  type DeepseekAccountStatus,
  type DeepseekBalanceView,
  type DeepseekKeyStatus,
  type DeepseekQuotaView,
} from "../model/deepseekCredentials";

const PLATFORM_URL = "https://platform.deepseek.com/api_keys";

type GroupProps = {
  id?: string;
  title: ReactNode;
  description?: string;
  action?: ReactNode;
  children: ReactNode;
};

type RowProps = {
  id?: string;
  label: ReactNode;
  description?: string;
  children?: ReactNode;
};

/**
 * DeepSeek settings: official account OAuth (PKCE) is primary; API key remains a secondary path.
 */
export function DeepSeekSettings({
  Row,
}: {
  Group?: (props: GroupProps) => ReactNode;
  Row: (props: RowProps) => ReactNode;
  embedded?: boolean;
}) {
  useLanguage();
  const [account, setAccount] = useState<DeepseekAccountStatus | null>(null);
  const [status, setStatus] = useState<DeepseekKeyStatus | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [loggingIn, setLoggingIn] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [quota, setQuota] = useState<DeepseekQuotaView>({
    status: "idle",
    label: null,
    wallets: [],
    bonusWallets: [],
    error: null,
  });
  const [balance, setBalance] = useState<DeepseekBalanceView>({
    available: null,
    infos: [],
    label: null,
    error: null,
    status: "idle",
  });
  const [catalogSync, setCatalogSync] = useState<DeepseekCatalogSync>(
    getDeepseekCatalogSync,
  );
  const [showApiKey, setShowApiKey] = useState(false);

  const refreshAccount = useCallback(async () => {
    try {
      const next = await loadDeepseekAccountStatus();
      setAccount(next);
      return next;
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : translate("Could not read DeepSeek account status"),
      );
      return null;
    }
  }, []);

  const refreshStatus = useCallback(async () => {
    try {
      const next = await loadDeepseekKeyStatus();
      setStatus(next);
      return next;
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : translate("Could not read DeepSeek key status"),
      );
      return null;
    }
  }, []);

  const refreshQuota = useCallback(async (signedIn?: boolean) => {
    const isSignedIn = signedIn ?? account?.signedIn;
    if (!isSignedIn) {
      setQuota({
        status: "unauthenticated",
        label: null,
        wallets: [],
        bonusWallets: [],
        error: null,
      });
      return;
    }
    setQuota((prev) => ({ ...prev, status: "loading", error: null }));
    const next = await loadDeepseekAccountQuota();
    setQuota(next);
  }, [account?.signedIn]);

  const refreshBalance = useCallback(
    async (configured?: boolean) => {
      const isConfigured = configured ?? status?.configured;
      if (!isConfigured) {
        setBalance({
          available: null,
          infos: [],
          label: null,
          error: null,
          status: "unavailable",
        });
        return;
      }
      setBalance((prev) => ({ ...prev, status: "loading", error: null }));
      const next = await loadDeepseekBalance();
      setBalance(next);
    },
    [status?.configured],
  );

  const refreshCatalog = useCallback(async () => {
    await refreshDeepseekCatalog();
    setCatalogSync(getDeepseekCatalogSync());
  }, []);

  useEffect(() => {
    return subscribeDeepseekCatalogSync(() => {
      setCatalogSync(getDeepseekCatalogSync());
    });
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const nextAccount = await refreshAccount();
      if (cancelled) return;
      const nextKey = await refreshStatus();
      if (cancelled) return;
      if (nextAccount?.signedIn) await refreshQuota(true);
      else {
        setQuota({
          status: "unauthenticated",
          label: null,
          wallets: [],
          bonusWallets: [],
          error: null,
        });
      }
      if (nextKey?.configured) await refreshBalance(true);
      else {
        setBalance({
          available: null,
          infos: [],
          label: null,
          error: null,
          status: "unavailable",
        });
      }
      if (!cancelled) await refreshCatalog();
    })();
    return () => {
      cancelled = true;
    };
  }, [refreshAccount, refreshStatus, refreshQuota, refreshBalance, refreshCatalog]);

  const onOfficialLogin = async () => {
    if (busy || loggingIn) return;
    setLoggingIn(true);
    setBusy(true);
    setError(null);
    try {
      const result = await startDeepseekOfficialLogin();
      const next = await refreshAccount();
      if (result.status === "ok" || result.signedIn) {
        void refreshCatalog();
        await refreshQuota(true);
      } else if (result.status === "cancelled") {
        setError(null);
      } else {
        setError(
          result.error?.trim() ||
            translate("Official DeepSeek login failed"),
        );
      }
      if (next?.signedIn) {
        void refreshCatalog();
        await refreshQuota(true);
      }
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : translate("Official DeepSeek login failed"),
      );
    } finally {
      setLoggingIn(false);
      setBusy(false);
      void refreshAccount();
    }
  };

  const onCancelLogin = async () => {
    try {
      await cancelDeepseekOfficialLogin();
    } catch {
      // ignore
    } finally {
      setLoggingIn(false);
      setBusy(false);
      void refreshAccount();
    }
  };

  const onOfficialLogout = async () => {
    if (busy || loggingIn) return;
    setBusy(true);
    setError(null);
    try {
      await signOutDeepseekOfficial();
      setAccount(await loadDeepseekAccountStatus());
      setQuota({
        status: "unauthenticated",
        label: null,
        wallets: [],
        bonusWallets: [],
        error: null,
      });
      void refreshCatalog();
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : translate("Could not sign out DeepSeek account"),
      );
    } finally {
      setBusy(false);
    }
  };

  const onSave = async () => {
    if (busy || !draft.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const next = await saveDeepseekApiKey(draft);
      setStatus(next);
      setDraft("");
      void refreshOpenCodeCatalog();
      void refreshCatalog();
      await refreshBalance(true);
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : translate("Could not save DeepSeek API key"),
      );
    } finally {
      setBusy(false);
    }
  };

  const onClear = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const next = await clearDeepseekApiKey();
      setStatus(next);
      setBalance({
        available: null,
        infos: [],
        label: null,
        error: null,
        status: "unavailable",
      });
      void refreshOpenCodeCatalog();
      void refreshCatalog();
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : translate("Could not clear DeepSeek API key"),
      );
    } finally {
      setBusy(false);
    }
  };

  const signedIn = Boolean(account?.signedIn);
  const configured = Boolean(status?.configured);
  const catalogLabel = catalogSyncLabel(catalogSync);
  const catalogTone =
    catalogSync.status === "ok" && catalogSync.source === "live"
      ? "ok"
      : catalogSync.status === "loading"
        ? "muted"
        : catalogSync.source === "fallback" ||
            catalogSync.status === "unauthenticated" ||
            catalogSync.status === "unauthorized" ||
            catalogSync.status === "error"
          ? "warn"
          : "muted";

  const quotaLabel =
    quota.status === "loading"
      ? translate("Checking quota…")
      : quota.status === "ok" && quota.label
        ? quota.label
        : quota.status === "unauthenticated"
          ? translate("Not signed in")
          : quota.error || translate("Quota unavailable");

  const balanceLabel =
    balance.status === "loading"
      ? translate("Checking balance…")
      : balance.status === "ok" && balance.label
        ? balance.label
        : balance.status === "unavailable"
          ? translate("Not configured")
          : balance.error || translate("Balance unavailable");

  return (
    <div
      id="setting-deepseek-provider"
      data-setting-id="deepseek-provider"
      className="border-b border-content/5 last:border-b-0"
    >
      <div className="flex items-center gap-4 px-4 py-3.5">
        <div className="flex min-w-0 flex-1 items-center gap-2.5">
          <span className="grid size-7 shrink-0 place-items-center rounded-lg bg-content/[0.05] ring-1 ring-inset ring-content/[0.06]">
            <UpstreamProviderIcon providerId="deepseek" className="size-4" />
          </span>
          <div className="min-w-0">
            <div className="text-[13px] font-medium text-content">
              {translate("DeepSeek")}
            </div>
            <div className="mt-0.5 text-[11px] text-content/40">
              {signedIn
                ? translate("Logged in")
                : translate("Official OAuth in Accounts")}
            </div>
          </div>
        </div>
        <button
          type="button"
          disabled={
            catalogSync.status === "loading" ||
            quota.status === "loading" ||
            balance.status === "loading" ||
            loggingIn
          }
          onClick={() => {
            void refreshCatalog();
            if (signedIn) void refreshQuota(true);
            if (configured) void refreshBalance(true);
            void refreshAccount();
          }}
          aria-label={translate("Refresh")}
          title={translate("Refresh")}
          className="grid size-7 place-items-center rounded-md text-content/40 transition-transform duration-150 hover:bg-content/10 hover:text-content active:scale-[0.96] disabled:opacity-40"
        >
          <RefreshCw
            className={`size-3.5 ${
              catalogSync.status === "loading" ||
              quota.status === "loading" ||
              balance.status === "loading"
                ? "animate-spin"
                : ""
            }`}
            strokeWidth={1.75}
          />
        </button>
      </div>
      <div className="border-t border-content/5 bg-content/[0.015]">
      <Row
        id="deepseek-official-login"
        label={translate("Official account login")}
        description={translate(
          "Opens the system browser for DeepSeek’s official OAuth. MonoCode completes login itself; it does not run dsh login.",
        )}
      >
        <div className="flex max-w-full flex-wrap items-center gap-2">
          {signedIn ? (
            <>
              <span className="text-[12px] text-content/55">
                {translate("Logged in")}
              </span>
              <SecondaryButton
                onClick={() => void onOfficialLogout()}
                disabled={busy || loggingIn}
              >
                {translate("Log out")}
              </SecondaryButton>
            </>
          ) : loggingIn ? (
            <>
              <button
                type="button"
                disabled
                className="inline-flex h-8 items-center rounded-lg bg-content/70 px-3.5 text-[12px] font-medium text-background-base"
              >
                {translate("Signing in…")}
              </button>
              <SecondaryButton onClick={() => void onCancelLogin()}>
                {translate("Cancel")}
              </SecondaryButton>
            </>
          ) : (
            <button
              type="button"
              disabled={busy}
              onClick={() => void onOfficialLogin()}
              className="inline-flex h-8 items-center rounded-lg bg-content px-3.5 text-[12px] font-medium text-background-base transition-transform duration-150 hover:bg-content/85 active:scale-[0.98] disabled:cursor-default disabled:opacity-40"
            >
              {translate("Log in DeepSeek")}
            </button>
          )}
        </div>
      </Row>

      <Row
        id="deepseek-quota"
        label={translate("Account quota")}
        description={translate(
          "Official account wallets from get_user_summary. Separate from API key balance.",
        )}
      >
        <span
          className={`text-[12px] ${
            quota.status === "error" || quota.status === "unauthorized"
              ? "text-red-400/90"
              : quota.status === "ok"
                ? "text-content/70"
                : "text-content/40"
          }`}
          role="status"
        >
          {quotaLabel}
        </span>
      </Row>

      <Row
        id="deepseek-models"
        label={translate("Harness models")}
        description={translate(
          "Loads live models from the DeepSeek platform with the official account. Offline ids are only a fallback.",
        )}
      >
        <span
          className={`text-[12px] ${
            catalogTone === "ok"
              ? "text-content/70"
              : catalogTone === "warn"
                ? "text-amber-400/90"
                : "text-content/40"
          }`}
          role="status"
        >
          {catalogLabel}
        </span>
      </Row>

      <Row
        id="deepseek-api-key"
        label={translate("API key (optional)")}
        description={translate(
          "Saved locally into OpenCode credentials (auth.json). Disconnect removes only the DeepSeek entry. Independent from official account login.",
        )}
      >
        <div className="flex max-w-full flex-col gap-2">
          <div className="flex flex-wrap items-center gap-2">
            <SecondaryButton onClick={() => setShowApiKey((v) => !v)}>
              {showApiKey
                ? translate("Hide API key")
                : translate("API key")}
            </SecondaryButton>
            <SecondaryButton
              onClick={() => void openUrl(PLATFORM_URL).catch(() => {})}
            >
              {translate("Open API keys page")}
            </SecondaryButton>
          </div>
          {showApiKey ? (
            configured ? (
              <div className="flex max-w-full flex-wrap items-center gap-2">
                <span className="text-[12px] text-content/55">
                  {translate("API key saved")}
                  {status?.hint ? (
                    <span className="ml-1.5 font-mono text-content/40">
                      {status.hint}
                    </span>
                  ) : null}
                </span>
                <SecondaryButton onClick={() => void onClear()} disabled={busy}>
                  {translate("Remove API key")}
                </SecondaryButton>
                <span
                  className={`text-[12px] ${
                    balance.status === "error"
                      ? "text-red-400/90"
                      : balance.status === "ok"
                        ? "text-content/70"
                        : "text-content/40"
                  }`}
                  role="status"
                >
                  {balanceLabel}
                </span>
              </div>
            ) : (
              <div className="flex max-w-full flex-wrap items-center gap-2">
                <label className="flex h-8 w-56 max-w-full shrink-0 items-center rounded-md border border-content/10 px-2 focus-within:border-content/20">
                  <input
                    type="password"
                    value={draft}
                    onChange={(event) => setDraft(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") void onSave();
                    }}
                    placeholder="sk-…"
                    aria-label={translate("DeepSeek API key")}
                    autoComplete="off"
                    spellCheck={false}
                    className="min-w-0 flex-1 bg-transparent text-[12px] text-content outline-none placeholder:text-content/35"
                  />
                </label>
                <button
                  type="button"
                  disabled={busy || !draft.trim()}
                  onClick={() => void onSave()}
                  className="inline-flex h-8 items-center rounded-lg bg-content px-3.5 text-[12px] font-medium text-background-base transition-transform duration-150 hover:bg-content/85 active:scale-[0.98] disabled:cursor-default disabled:opacity-40"
                >
                  {busy ? translate("Saving") : translate("Save API key")}
                </button>
              </div>
            )
          ) : null}
        </div>
      </Row>

      {error ? (
        <p className="border-b border-content/5 px-4 pb-3 text-[12px] text-red-400/90 last:border-b-0">
          {error}
        </p>
      ) : null}
      </div>
    </div>
  );
}

function catalogSyncLabel(sync: DeepseekCatalogSync): string {
  if (sync.status === "loading") {
    return translate("Syncing models…");
  }
  if (sync.source === "live" && sync.status === "ok") {
    return translate("Synced {count} models", { count: sync.count });
  }
  if (sync.source === "acp" && sync.count > 0) {
    const base = translate("Synced {count} models", { count: sync.count });
    return sync.error ? `${base} · ${sync.error}` : base;
  }
  if (sync.status === "unauthenticated") {
    return (
      sync.error?.trim() ||
      translate("Not signed in · offline list")
    );
  }
  if (sync.status === "unauthorized") {
    return sync.error?.trim() || translate("Authorization expired (401)");
  }
  if (sync.source === "fallback") {
    return (
      sync.error?.trim() ||
      translate("Not signed in · offline list")
    );
  }
  if (sync.status === "error") {
    return sync.error?.trim() || translate("Model sync failed");
  }
  return translate("Not synced yet");
}
