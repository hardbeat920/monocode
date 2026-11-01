import type { ReactNode } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const { box, seen } = vi.hoisted(() => ({
  box: ({ children }: { children?: unknown }) => children as never,
  seen: {
    field: [] as boolean[],
    rain: [] as boolean[],
    press: {} as Record<string, () => void>,
    dismiss: () => {},
  },
}));
vi.mock("expo-camera", () => ({
  CameraView: () => null,
  useCameraPermissions: () => [{ granted: true }, async () => ({ granted: true })],
}));
vi.mock("react-native", () => ({
  ActivityIndicator: () => null,
  Image: () => null,
  Modal: box,
  Pressable: box,
  StyleSheet: { create: <T,>(s: T) => s, absoluteFill: {}, hairlineWidth: 1 },
  Text: box,
  View: box,
}));
vi.mock("react-native-reanimated", () => {
  const chain: Record<string, unknown> = {};
  for (const name of ["duration", "delay", "easing"]) chain[name] = () => chain;
  return {
    default: { View: box },
    Easing: { out: () => 0, cubic: 0 },
    FadeIn: chain,
    FadeInDown: chain,
    useSharedValue: (value: unknown) => ({ get: () => value, set: () => {} }),
  };
});
vi.mock("react-native-safe-area-context", () => ({ useSafeAreaInsets: () => ({ top: 0, bottom: 0 }) }));
vi.mock("@/ui/brand", () => ({ MONOCODE_MARK: 0 }));
vi.mock("@/ui/LiquidView", () => ({
  LiquidView: ({ children, accessibilityLabel, onPress }: { children?: ReactNode; accessibilityLabel: string; onPress: () => void }) => {
    seen.press[accessibilityLabel] = onPress;
    return <div>{children}</div>;
  },
}));
vi.mock("@/ui/theme", () => ({
  themedStyles: () => () => new Proxy({}, { get: () => ({}) }),
  useColors: () => ({}),
  withAlpha: () => "",
}));
vi.mock("@/features/background/AsciiField", () => ({
  AsciiField: ({ paused }: { paused?: boolean }) => {
    seen.field.push(!!paused);
    return null;
  },
}));
vi.mock("@/features/mascots/MascotRain", () => ({
  MascotRain: ({ paused }: { paused?: boolean }) => {
    seen.rain.push(!!paused);
    return null;
  },
}));
vi.mock("@/features/connection/PairingLinkSheet", () => ({
  PairingLinkSheet: ({ onDismiss }: { onDismiss: () => void }) => {
    seen.dismiss = onDismiss;
    return null;
  },
}));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({ working: false, error: null, pair: async () => {} }),
}));

import { ConnectScreen } from "@/features/connection/ConnectScreen";

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  seen.field.length = 0;
  seen.rain.length = 0;
  root = createRoot(document.createElement("div"));
});
afterEach(() => act(() => root.unmount()));

it("rests the ASCII field and the rain while the paste sheet or the scanner is open", async () => {
  await act(async () => root.render(<ConnectScreen />));
  expect(seen.field.at(-1)).toBe(false);
  expect(seen.rain.at(-1)).toBe(false);

  await act(async () => seen.press["Paste a pairing link"]());
  expect(seen.field.at(-1)).toBe(true);
  expect(seen.rain.at(-1)).toBe(true);

  // Dismissing the sheet runs them again.
  await act(async () => seen.dismiss());
  expect(seen.field.at(-1)).toBe(false);
  expect(seen.rain.at(-1)).toBe(false);

  // Opening the scanner rests them too.
  await act(async () => seen.press["Connect to desktop"]());
  expect(seen.field.at(-1)).toBe(true);
  expect(seen.rain.at(-1)).toBe(true);
});
