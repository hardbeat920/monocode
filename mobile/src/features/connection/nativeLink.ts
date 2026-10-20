import { requireOptionalNativeModule } from "expo";
import { Platform } from "react-native";

type NativeLink = {
  execute(input: string): Promise<string>;
  nativeTranscriptVersion?: number;
};
const module = requireOptionalNativeModule<NativeLink>("MonocodeLink");
// Older development builds and Android retain the existing transcript.
export const hasNativeTranscript =
  Platform.OS === "ios" && module?.nativeTranscriptVersion === 1;

export type Identity = { certificate: string; privateKey: string };
export type Address = { host: string; port: number };
export type Desktop = {
  name: string;
  fingerprint: string;
  addresses: Address[];
  deviceId: string;
};
export type Pairing = { desktop: Desktop; identity: Identity };

export async function nativeCall<T>(input: object): Promise<T> {
  if (!module)
    throw new Error("Rebuild the mobile app to enable desktop connections.");
  const response = JSON.parse(await module.execute(JSON.stringify(input)));
  if (!response.ok)
    throw new Error(response.error ?? "Desktop connection failed");
  return response.result as T;
}
export function request<T>(
  pairing: Pairing,
  method: string,
  params: object = {},
  id = makeCommandId(),
): Promise<T> {
  return nativeCall({
    operation: "exchange",
    identity: pairing.identity,
    pin: pairing.desktop.fingerprint,
    addresses: pairing.desktop.addresses,
    request: { version: 1, id, method, params },
  });
}
/**
 * Opens a watch: the desktop keeps the request's stream open and writes each
 * change to it. Pull them with `nextUpdates`; `closeWatch` stops it.
 */
export async function openWatch(
  pairing: Pairing,
  method: string,
  params: object,
): Promise<number> {
  const { watchId } = await nativeCall<{ watchId: number }>({
    operation: "watch",
    identity: pairing.identity,
    pin: pairing.desktop.fingerprint,
    addresses: pairing.desktop.addresses,
    request: { version: 1, id: makeCommandId(), method, params },
  });
  return watchId;
}
/** The updates that arrived, waiting up to `timeoutMs` for the first; throws once the watch ended. */
export async function nextUpdates<T>(
  watchId: number,
  timeoutMs: number,
): Promise<T[]> {
  const { updates } = await nativeCall<{ updates: T[] }>({
    operation: "next",
    watchId,
    timeoutMs,
  });
  return updates;
}
export function closeWatch(watchId: number) {
  return nativeCall({ operation: "unwatch", watchId }).catch(() => {});
}
export function makeCommandId() {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}-${Math.random().toString(36).slice(2)}`;
}
export { parseInvitation } from "./invitation";
