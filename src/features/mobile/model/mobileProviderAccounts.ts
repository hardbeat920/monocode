import {
  DEFAULT_PROVIDER_ACCOUNT_ID,
  providerAccountExists,
  providerAccounts,
  selectedProviderAccountId,
  supportsAccountProfiles,
} from "../../providers/model/providerAccounts";
import type { HarnessId, Session } from "../../sessions/model/session";

const PROVIDERS = ["claude", "codex"] as const;

/** Only profile names and IDs cross the mobile link; credentials stay on desktop. */
export function mobileProviderAccounts(project: string) {
  return {
    accounts: PROVIDERS.flatMap((provider) => providerAccounts(provider)),
    selectedAccounts: Object.fromEntries(
      PROVIDERS.map((provider) => [
        provider,
        selectedProviderAccountId(provider, project),
      ]),
    ),
  };
}

export function mobileSessionAccountId(session: Session): string | undefined {
  if (!supportsAccountProfiles(session.harness)) return undefined;
  return (
    session.providerAccountId ??
    (session.blocks.some((block) => block.role === "user")
      ? DEFAULT_PROVIDER_ACCOUNT_ID
      : selectedProviderAccountId(session.harness, session.cwd))
  );
}

/** Validate against the final provider, including when the phone changes models. */
export function mobileAccountChoice(
  harness: HarnessId,
  project: string,
  params: Record<string, unknown>,
): string | undefined {
  const explicit = params.providerAccountId !== undefined;
  if (!supportsAccountProfiles(harness)) {
    if (explicit)
      throw new Error("This provider does not support account switching");
    return undefined;
  }
  const accountId = explicit
    ? params.providerAccountId
    : selectedProviderAccountId(harness, project);
  if (
    typeof accountId !== "string" ||
    !providerAccountExists(harness, accountId)
  )
    throw new Error(
      "Account is no longer available. Choose another account in the model picker.",
    );
  return accountId;
}
