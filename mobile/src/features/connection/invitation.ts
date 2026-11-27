import type { Desktop, Identity } from "./nativeLink";
export type Invitation = Omit<Desktop, "deviceId"> & {
  version: number;
  expires: number;
  identity: Identity;
};
export function parseInvitation(text: string): Invitation {
  if (text.length > 12_000) throw new Error("This pairing code is too large");
  const url = new URL(text.trim());
  if (url.protocol !== "monocode:" || url.hostname !== "pair")
    throw new Error(
      "Scan a Monocode pairing code from desktop Settings → Connections",
    );
  const code = url.searchParams.get("i");
  if (!code || !/^[A-Za-z0-9_-]+$/.test(code))
    throw new Error("Invalid pairing code");
  const base64 = code.replaceAll("-", "+").replaceAll("_", "/");
  const bytes = atob(base64.padEnd(Math.ceil(base64.length / 4) * 4, "="));
  const invitation = JSON.parse(
    decodeURIComponent(
      Array.from(
        bytes,
        (byte) => `%${byte.charCodeAt(0).toString(16).padStart(2, "0")}`,
      ).join(""),
    ),
  ) as Invitation;
  if (
    invitation.version !== 1 ||
    typeof invitation.fingerprint !== "string" ||
    !/^[a-f0-9]{64}$/.test(invitation.fingerprint) ||
    !Array.isArray(invitation.addresses) ||
    invitation.addresses.length < 1 ||
    invitation.addresses.length > 16 ||
    !invitation.addresses.every(
      (address) =>
        typeof address.host === "string" &&
        address.host.length > 0 &&
        address.host.length <= 253 &&
        Number.isInteger(address.port) &&
        address.port > 0 &&
        address.port <= 65535,
    ) ||
    typeof invitation.identity?.certificate !== "string" ||
    typeof invitation.identity?.privateKey !== "string" ||
    typeof invitation.expires !== "number"
  )
    throw new Error("Invalid pairing code");
  if (invitation.expires <= Date.now() / 1000)
    throw new Error(
      "This code expired. Choose Pair phone on desktop to make a new one.",
    );
  return invitation;
}
