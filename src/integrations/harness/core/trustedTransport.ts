/** A resolver-owned executable and fixed argument vector for a child transport. */
export type TrustedTransportDescriptor = {
  provider: string;
  path: string;
  args: readonly string[];
};

export type TrustedTransportResolver = () => Promise<TrustedTransportDescriptor>;

/**
 * Validate a descriptor before it reaches a native spawn boundary.
 * The native command remains the final authority; this guard prevents accidental
 * frontend construction of a descriptor with mutable or shell-like arguments.
 */
export function validateTrustedTransport(
  descriptor: TrustedTransportDescriptor,
): TrustedTransportDescriptor {
  if (!descriptor.provider.trim()) throw new Error("Trusted transport provider is required");
  if (!descriptor.path.trim()) throw new Error("Trusted transport executable is required");
  if (descriptor.path.includes("\0")) throw new Error("Trusted transport executable is invalid");
  for (const arg of descriptor.args) {
    if (arg.includes("\0")) throw new Error("Trusted transport argument is invalid");
  }
  return {
    provider: descriptor.provider,
    path: descriptor.path,
    args: [...descriptor.args],
  };
}

/** Runtime child key; never expose this as a persisted HarnessId or UI ID. */
export function namespaceTransportChild(
  provider: string,
  sessionId: string,
  generation: number,
): string {
  const clean = (value: string, label: string) => {
    if (!value || value.includes("\0") || value.includes("#")) {
      throw new Error(`Invalid ${label} for transport child`);
    }
    return value;
  };
  if (!Number.isSafeInteger(generation) || generation < 0) {
    throw new Error("Invalid transport generation");
  }
  return `${clean(provider, "provider")}:${clean(sessionId, "session")}:${generation}`;
}
