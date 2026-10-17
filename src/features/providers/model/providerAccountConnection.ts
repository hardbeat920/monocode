import type { ProviderRateLimits } from "./rateLimits";
import type { ProviderAccountIdentity } from "./providerAccountIdentity";

export type ProviderAccountConnection =
  "connected" | "not-installed" | "not-signed-in" | "checking" | "unverified";

/** A default CLI profile exists even before installation or sign-in. */
export function providerAccountConnection(
  installed: boolean | null,
  identity: ProviderAccountIdentity | null | undefined,
  limits: ProviderRateLimits | undefined,
): ProviderAccountConnection {
  if (installed === null) return "checking";
  if (!installed) return "not-installed";
  if (
    limits?.status === "unavailable" &&
    /not (?:signed in|authenticated|logged in)|authentication required/i.test(
      limits.error ?? "",
    )
  ) {
    return "not-signed-in";
  }
  if (
    limits?.status === "ok" ||
    [
      identity?.email,
      identity?.name,
      identity?.plan,
      identity?.organization,
    ].some((value) => typeof value === "string" && value.trim().length > 0)
  ) {
    return "connected";
  }
  if (
    identity === undefined ||
    !limits ||
    limits.status === "idle" ||
    limits.status === "fetching"
  ) {
    return "checking";
  }
  return "unverified";
}

export function providerAccountConnectionLabel(
  connection: ProviderAccountConnection,
): string {
  switch (connection) {
    case "connected":
      return "Connected";
    case "not-installed":
      return "CLI not installed";
    case "not-signed-in":
      return "Not signed in";
    case "checking":
      return "Checking connection…";
    case "unverified":
      return "Connection unverified";
  }
}
