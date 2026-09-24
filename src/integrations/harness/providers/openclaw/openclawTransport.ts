import { validateTrustedTransport, type TrustedTransportDescriptor } from "../../core/trustedTransport";

/** OpenClaw ACP is intentionally fixed to the documented stdio bridge command. */
export function openClawTransport(path: string): TrustedTransportDescriptor {
  return validateTrustedTransport({
    provider: "openclaw",
    path,
    args: ["acp"],
  });
}

/** Gateway credentials are native-runtime concerns, never frontend state. */
export type OpenClawGatewaySecretSource = "native-runtime";

export type OpenClawGatewayConfig = {
  url?: string;
  secretSource: OpenClawGatewaySecretSource;
};

export function openClawGatewayConfig(url?: string): OpenClawGatewayConfig {
  return { url: url?.trim() || undefined, secretSource: "native-runtime" };
}

export function openClawSessionKey(key: string | undefined): string {
  const trimmed = key?.trim();
  return trimmed ? `acp-bridge:${trimmed}` : `acp-bridge:${crypto.randomUUID()}`;
}
