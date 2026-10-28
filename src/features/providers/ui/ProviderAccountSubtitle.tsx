import type { ProviderAccountIdentity } from "../model/providerAccountIdentity";
import { PrivateEmail } from "../../../shared/ui/PrivateEmail";

export function ProviderAccountSubtitle({
  identity,
  fallback,
  className = "",
}: {
  identity: ProviderAccountIdentity | null | undefined;
  fallback?: string;
  className?: string;
}) {
  if (!identity?.plan && !identity?.email) {
    return fallback ? (
      <span className={`min-w-0 ${className}`}>{fallback}</span>
    ) : null;
  }

  return (
    <span className={`inline-flex min-w-0 items-baseline gap-1 ${className}`}>
      {identity.plan ? <span className="shrink-0">{identity.plan}</span> : null}
      {identity.plan && identity.email ? <span aria-hidden>·</span> : null}
      {identity.email ? (
        <PrivateEmail key={identity.email} email={identity.email} />
      ) : null}
    </span>
  );
}
