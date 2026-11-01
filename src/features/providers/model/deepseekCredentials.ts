import { invoke } from "@tauri-apps/api/core";

export type DeepseekKeyStatus = {
  configured: boolean;
  hint?: string | null;
};

export type DeepseekBalanceFetch = {
  status: "ok" | "error" | "unavailable" | string;
  httpStatus?: number | null;
  body?: string | null;
  error?: string | null;
};

export type DeepseekBalanceInfo = {
  currency: string;
  totalBalance: string;
  grantedBalance?: string;
  toppedUpBalance?: string;
};

export type DeepseekBalanceView = {
  available: boolean | null;
  infos: DeepseekBalanceInfo[];
  label: string | null;
  error: string | null;
  status: "idle" | "loading" | "ok" | "error" | "unavailable";
};

export type DeepseekAccountStatus = {
  signedIn: boolean;
  phase: string;
  error?: string | null;
  attemptId?: string | null;
};

export type DeepseekOfficialLoginResult = {
  status: "ok" | "cancelled" | "failed" | "expired" | string;
  signedIn: boolean;
  error?: string | null;
};

export type DeepseekAccountQuotaFetch = {
  status: string;
  httpStatus?: number | null;
  body?: string | null;
  error?: string | null;
};

export type DeepseekWallet = {
  currency: string;
  balance: string;
};

export type DeepseekQuotaView = {
  status: "idle" | "loading" | "ok" | "error" | "unauthenticated" | "unauthorized";
  label: string | null;
  wallets: DeepseekWallet[];
  bonusWallets: DeepseekWallet[];
  error: string | null;
};

export function timezoneOffsetSeconds(): number {
  return -new Date().getTimezoneOffset() * 60;
}

export async function loadDeepseekKeyStatus(): Promise<DeepseekKeyStatus> {
  return invoke<DeepseekKeyStatus>("deepseek_key_status");
}

export async function saveDeepseekApiKey(key: string): Promise<DeepseekKeyStatus> {
  return invoke<DeepseekKeyStatus>("deepseek_save_api_key", { key });
}

export async function clearDeepseekApiKey(): Promise<DeepseekKeyStatus> {
  return invoke<DeepseekKeyStatus>("deepseek_clear_api_key");
}

export async function fetchDeepseekBalanceRaw(): Promise<DeepseekBalanceFetch> {
  return invoke<DeepseekBalanceFetch>("fetch_deepseek_balance");
}

export async function loadDeepseekAccountStatus(): Promise<DeepseekAccountStatus> {
  return invoke<DeepseekAccountStatus>("deepseek_account_status");
}

export async function startDeepseekOfficialLogin(
  offsetSeconds: number = timezoneOffsetSeconds(),
): Promise<DeepseekOfficialLoginResult> {
  return invoke<DeepseekOfficialLoginResult>("deepseek_start_official_login", {
    timezoneOffsetSeconds: offsetSeconds,
  });
}

export async function cancelDeepseekOfficialLogin(): Promise<DeepseekAccountStatus> {
  return invoke<DeepseekAccountStatus>("deepseek_cancel_official_login");
}

export async function signOutDeepseekOfficial(
  offsetSeconds: number = timezoneOffsetSeconds(),
): Promise<DeepseekAccountStatus> {
  return invoke<DeepseekAccountStatus>("deepseek_sign_out_official", {
    timezoneOffsetSeconds: offsetSeconds,
  });
}

export async function fetchDeepseekAccountQuotaRaw(
  offsetSeconds: number = timezoneOffsetSeconds(),
): Promise<DeepseekAccountQuotaFetch> {
  return invoke<DeepseekAccountQuotaFetch>("fetch_deepseek_account_quota", {
    timezoneOffsetSeconds: offsetSeconds,
  });
}

/** Parse the official /user/balance JSON into a display-friendly view. */
export function parseDeepseekBalance(
  result: DeepseekBalanceFetch,
): DeepseekBalanceView {
  if (result.status === "unavailable") {
    return {
      available: null,
      infos: [],
      label: null,
      error: result.error?.trim() || "未配置 DeepSeek API Key",
      status: "unavailable",
    };
  }
  if (result.status !== "ok" || !result.body) {
    return {
      available: null,
      infos: [],
      label: null,
      error: result.error?.trim() || "余额不可用",
      status: "error",
    };
  }
  try {
    const json = JSON.parse(result.body) as {
      is_available?: boolean;
      balance_infos?: Array<{
        currency?: string;
        total_balance?: string;
        granted_balance?: string;
        topped_up_balance?: string;
      }>;
    };
    const infos: DeepseekBalanceInfo[] = (json.balance_infos ?? [])
      .map((row) => ({
        currency: (row.currency ?? "").trim() || "—",
        totalBalance: String(row.total_balance ?? "").trim(),
        grantedBalance: row.granted_balance?.trim(),
        toppedUpBalance: row.topped_up_balance?.trim(),
      }))
      .filter((row) => row.totalBalance.length > 0);
    const label =
      infos.length === 0
        ? null
        : infos
            .map((info) => formatBalanceAmount(info.totalBalance, info.currency))
            .join(" · ");
    return {
      available: json.is_available ?? null,
      infos,
      label,
      error: null,
      status: "ok",
    };
  } catch {
    return {
      available: null,
      infos: [],
      label: null,
      error: "余额响应不是有效 JSON",
      status: "error",
    };
  }
}

export function formatBalanceAmount(amount: string, currency: string): string {
  const cur = currency.toUpperCase();
  if (cur === "CNY" || cur === "RMB") return `¥${amount}`;
  if (cur === "USD") return `$${amount}`;
  return `${amount} ${currency}`.trim();
}

export async function loadDeepseekBalance(): Promise<DeepseekBalanceView> {
  try {
    const raw = await fetchDeepseekBalanceRaw();
    return parseDeepseekBalance(raw);
  } catch (error) {
    return {
      available: null,
      infos: [],
      label: null,
      error:
        error instanceof Error ? error.message : "余额请求失败",
      status: "error",
    };
  }
}

function formatQuotaAmount(amount: string, currency: string): string {
  const n = Number(amount);
  const cur = currency.toUpperCase();
  const prefix = cur === "CNY" || cur === "RMB" ? "¥" : cur === "USD" ? "$" : "";
  if (!Number.isFinite(n)) {
    return prefix ? `${prefix}${amount}` : `${amount} ${currency}`.trim();
  }
  const abs = Math.abs(n);
  const shown =
    abs > 0 && abs < 0.01
      ? n < 0
        ? "-0.01"
        : "<0.01"
      : n.toFixed(2);
  if (prefix) return `${prefix}${shown}`;
  return `${shown} ${currency}`.trim();
}

export function parseDeepseekQuota(
  result: DeepseekAccountQuotaFetch,
): DeepseekQuotaView {
  if (result.status === "unauthenticated") {
    return {
      status: "unauthenticated",
      label: null,
      wallets: [],
      bonusWallets: [],
      error: result.error?.trim() || "未登录 DeepSeek 官方账号",
    };
  }
  if (result.status === "unauthorized") {
    return {
      status: "unauthorized",
      label: null,
      wallets: [],
      bonusWallets: [],
      error: result.error?.trim() || "账号授权无效或已过期",
    };
  }
  if (result.status !== "ok" || !result.body) {
    return {
      status: "error",
      label: null,
      wallets: [],
      bonusWallets: [],
      error: result.error?.trim() || "额度不可用",
    };
  }
  try {
    const json = JSON.parse(result.body) as {
      normal_wallets?: Array<{ currency?: string; balance?: string }>;
      bonus_wallets?: Array<{ currency?: string; balance?: string }>;
    };
    const wallets = (json.normal_wallets ?? [])
      .map((row) => ({
        currency: (row.currency ?? "").trim() || "—",
        balance: String(row.balance ?? "").trim(),
      }))
      .filter((row) => row.balance.length > 0);
    const bonusWallets = (json.bonus_wallets ?? [])
      .map((row) => ({
        currency: (row.currency ?? "").trim() || "—",
        balance: String(row.balance ?? "").trim(),
      }))
      .filter((row) => row.balance.length > 0);
    const parts = [
      ...wallets.map((w) => formatQuotaAmount(w.balance, w.currency)),
      ...bonusWallets.map(
        (w) => `赠金 ${formatQuotaAmount(w.balance, w.currency)}`,
      ),
    ];
    return {
      status: "ok",
      label: parts.length > 0 ? parts.join(" · ") : "—",
      wallets,
      bonusWallets,
      error: null,
    };
  } catch {
    return {
      status: "error",
      label: null,
      wallets: [],
      bonusWallets: [],
      error: "额度响应不是有效 JSON",
    };
  }
}

export async function loadDeepseekAccountQuota(): Promise<DeepseekQuotaView> {
  try {
    const raw = await fetchDeepseekAccountQuotaRaw();
    return parseDeepseekQuota(raw);
  } catch (error) {
    return {
      status: "error",
      label: null,
      wallets: [],
      bonusWallets: [],
      error: error instanceof Error ? error.message : "额度请求失败",
    };
  }
}
