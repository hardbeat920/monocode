import { useEffect, useMemo, useState } from "react";
import {
  UsageChips,
  type UsageFooterSession,
} from "../../../app/shell/UsageFooter";
import {
  DEFAULT_PROVIDER_ACCOUNT_ID,
  providerAccountExists,
  providerAccountLabel,
  selectedProviderAccountId,
  subscribeProviderAccounts,
  type ProviderAccountProvider,
} from "../../providers/model/providerAccounts";
import type { RateLimitProvider } from "../../providers/model/rateLimits";
import {
  piBillingProvider,
  piUsageProvider,
} from "../../providers/model/piUsage";
import { useCachedRateLimits } from "../../providers/model/rateLimitsCache";
import { getSession } from "../../sessions/data/sessionStore";
import {
  HARNESS_TITLE,
  type HarnessId,
  type Session,
} from "../../sessions/model/session";
import { monoSpawnedSessions } from "../model/monoSpawnedSessions";

const RATE_LIMIT_PROVIDERS = ["claude", "codex", "opencode", "devin"] as const;
type UsageEntry = {
  key: string;
  harness: HarnessId;
  provider?: RateLimitProvider;
  session?: UsageFooterSession;
  accountLabel?: string;
  project?: string;
  unavailable?: string;
};

function isRateLimitProvider(harness: HarnessId): harness is RateLimitProvider {
  return RATE_LIMIT_PROVIDERS.includes(harness as RateLimitProvider);
}

/** Shows limits only for this Mono and the child sessions in its launch receipts. */
export function MonoUsageFooter({
  session,
  sessions,
}: {
  session: Session;
  sessions?: readonly Session[];
}) {
  const opencodeLimits = useCachedRateLimits("opencode");
  const [, setAccountsVersion] = useState(0);
  const launches = useMemo(
    () => monoSpawnedSessions(session.blocks),
    [session.blocks],
  );
  const launchIds = launches.map((launch) => launch.sessionId);
  const [loadedChildren, setLoadedChildren] = useState<Session[]>([]);
  const knownChildren = sessions?.filter((child) =>
    launchIds.includes(child.id),
  );
  const knownIds = new Set(knownChildren?.map((child) => child.id) ?? []);
  const missingIds = sessions
    ? launchIds.filter((id) => !knownIds.has(id))
    : launchIds;
  const pollChildren = sessions === undefined;

  useEffect(
    () =>
      subscribeProviderAccounts(() =>
        setAccountsVersion((version) => version + 1),
      ),
    [],
  );

  useEffect(() => {
    let current = true;
    setLoadedChildren([]);
    if (!missingIds.length) return;
    let loading = false;
    const loadChildren = () => {
      if (!current || loading) return;
      loading = true;
      void Promise.all(missingIds.map((id) => getSession(id)))
        .then((children) => {
          if (current)
            setLoadedChildren(
              children.filter((child): child is Session => !!child),
            );
        })
        .catch(() => {
          if (current) setLoadedChildren([]);
        })
        .finally(() => {
          loading = false;
        });
    };
    loadChildren();
    const refresh = pollChildren
      ? window.setInterval(loadChildren, 30_000)
      : undefined;
    return () => {
      current = false;
      if (refresh !== undefined) window.clearInterval(refresh);
    };
  }, [missingIds.join("\0"), pollChildren]);

  const children = new Map<string, Session>();
  for (const child of [...(knownChildren ?? []), ...loadedChildren])
    if (launchIds.includes(child.id)) children.set(child.id, child);

  const entries = new Map<string, UsageEntry>();
  const add = (
    harness: HarnessId,
    source?: Session,
    project = session.cwd,
    child = false,
    model?: string,
  ) => {
    const resolvedModel = source?.model ?? model;
    if (harness === "pi") {
      const piProvider = piUsageProvider(resolvedModel);
      if (!piProvider) {
        entries.set("unavailable:pi", {
          key: "unavailable:pi",
          harness,
        });
        return;
      }
      const key = `pi:${piProvider}`;
      if (!entries.has(key))
        entries.set(key, {
          key,
          harness,
          provider: piBillingProvider(piProvider),
          session: {
            id: source?.id,
            harness: "pi",
            model: resolvedModel,
          },
        });
      return;
    }
    if (!isRateLimitProvider(harness)) {
      entries.set(`unavailable:${harness}`, {
        key: `unavailable:${harness}`,
        harness,
      });
      return;
    }
    if (!source && child && (harness === "claude" || harness === "codex")) {
      const key = `account-unavailable:${harness}`;
      entries.set(key, {
        key,
        harness,
        unavailable: "account details unavailable",
      });
      return;
    }
    const accountProvider =
      harness === "claude" || harness === "codex" || harness === "devin";
    const legacyUsedSession = Boolean(
      source?.blocks.some((block) => block.role === "user"),
    );
    const accountId = accountProvider
      ? (source?.providerAccountId ??
        (legacyUsedSession
          ? DEFAULT_PROVIDER_ACCOUNT_ID
          : selectedProviderAccountId(
              harness as ProviderAccountProvider,
              project,
            )))
      : undefined;
    const key = `${harness}:${accountId ?? "default"}`;
    if (entries.has(key)) return;
    const accountLabel = accountProvider
      ? providerAccountExists(harness as ProviderAccountProvider, accountId)
        ? providerAccountLabel(harness as ProviderAccountProvider, accountId)
        : "Removed account"
      : undefined;
    entries.set(key, {
      key,
      harness,
      provider: harness,
      accountLabel,
      project,
      session:
        source && (source.providerAccountId || legacyUsedSession)
          ? {
              id: source.id,
              harness: source.harness,
              model: source.model,
              providerAccountId: accountId,
            }
          : undefined,
    });
  };

  add(session.harness, session, session.cwd, false, session.model);
  for (const launch of launches) {
    const child = children.get(launch.sessionId);
    add(
      child?.harness ?? launch.harness,
      child,
      child?.cwd ?? launch.cwd,
      true,
      launch.model,
    );
  }

  return (
    <footer
      aria-label="Mono provider usage"
      className="flex h-7 shrink-0 items-center gap-2 overflow-x-auto border-t border-stroke px-3 text-[11px] text-content/55"
    >
      {[...entries.values()].map((entry) =>
        entry.provider ? (
          <span
            key={entry.key}
            className="inline-flex shrink-0 items-center gap-1.5"
          >
            {entry.provider === "opencode" &&
            opencodeLimits.status === "unavailable" ? (
              <span>OpenCode · usage unavailable</span>
            ) : (
              <UsageChips
                providers={[entry.provider]}
                session={entry.session}
                project={entry.project}
                accountPicker={!entry.session}
              />
            )}
            {entry.accountLabel ? (
              <span title={`${HARNESS_TITLE[entry.harness]} account`}>
                {entry.accountLabel}
              </span>
            ) : null}
          </span>
        ) : entry.unavailable ? (
          <span
            key={entry.key}
            className="shrink-0 whitespace-nowrap"
            title={`${HARNESS_TITLE[entry.harness]} usage is unavailable`}
          >
            {HARNESS_TITLE[entry.harness]} · {entry.unavailable}
          </span>
        ) : (
          <span
            key={entry.key}
            className="shrink-0 whitespace-nowrap"
            title={`${HARNESS_TITLE[entry.harness]} usage is not supported`}
          >
            {HARNESS_TITLE[entry.harness]} · usage unavailable
          </span>
        ),
      )}
    </footer>
  );
}
