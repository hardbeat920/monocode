import { act, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import { BackgroundSection } from "@/features/background/BackgroundSettings";
import { normalizeBackground } from "@/features/background/settings";

const device = vi.hoisted(() => ({ os: "ios" }));
vi.mock("react-native", () => ({
  Platform: { get OS() { return device.os; } },
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Image: () => <img alt="Background preview" />,
  ActivityIndicator: () => null,
  StyleSheet: { absoluteFill: {}, hairlineWidth: 1 },
  Pressable: ({ children, onPress, accessibilityState }: { children: ReactNode; onPress: () => void; accessibilityState?: { selected: boolean } }) => <button aria-pressed={accessibilityState?.selected} onClick={onPress}>{children}</button>,
}));
vi.mock("@expo/ui", () => ({
  Host: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  Slider: ({ value, onValueChange, modifiers }: { value: number; onValueChange: (v: number) => void; modifiers: unknown[] }) => <input type="range" aria-label={`visibility-${value}`} min={.05} max={.65} step={.01} value={value} data-modifiers={modifiers.length} onChange={e => onValueChange(Number(e.target.value))} />,
}));
vi.mock("@expo/ui/jetpack-compose/modifiers", () => ({ semantics: () => ({}) }));
vi.mock("@expo/ui/swift-ui/modifiers", () => ({ accessibilityLabel: () => ({}), tint: () => ({}) }));
vi.mock("@/ui/theme", () => ({ radii: { card: 16 }, useColors: () => ({}), themedStyles: (fn: (colors: object) => object) => () => fn({}) }));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/features/settings/SettingsUI", () => ({ SettingsRow: ({ label, onPress }: { label: string; onPress: () => void }) => <button onClick={onPress}>{label}</button> }));
vi.mock("@/features/background/useEmptyBackground", async () => {
  const { OPACITY_RANGE } = await import("@/features/background/settings");
  return { OPACITY_RANGE, useEmptyBackground: () => {
    const [settings, set] = useState(normalizeBackground({ image: "photo.jpg", effect: "none", scope: "all" }));
    return { settings, background: { source: {}, opacity: settings.opacity }, busy: false,
      update: (patch: object) => set(current => ({ ...current, ...patch })),
      setOpacity: (opacity: number) => set(current => ({ ...current, opacity })),
      pickImage: vi.fn(), removeImage: vi.fn() };
  } };
});
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
describe("background settings on both platforms", () => {
  it.each(["ios", "android"])("exposes six working effects, scope and separate visibility controls on %s", async os => {
    device.os = os;
    const container = document.createElement("div");
    const root = createRoot(container);
    await act(async () => root.render(<BackgroundSection projectId="project" />));
    const button = (text: string) => Array.from(container.querySelectorAll("button")).find(b => b.textContent === text)!;
    for (const effect of ["None", "Dither", "ASCII", "Halftone", "Scanlines", "Haze"]) {
      await act(async () => button(effect).click());
      expect(button(effect).getAttribute("aria-pressed")).toBe("true");
    }
    await act(async () => button("Empty sessions").click());
    expect(button("Empty sessions").getAttribute("aria-pressed")).toBe("true");
    await act(async () => button("All sessions").click());
    expect(button("All sessions").getAttribute("aria-pressed")).toBe("true");
    expect(container.textContent).toContain("Empty chat visibility");
    expect(container.textContent).toContain("Session visibility");
    expect(container.querySelectorAll("input").length).toBe(2);
    expect(container.querySelector("input")?.getAttribute("data-modifiers")).toBe(os === "ios" ? "2" : "1");
    await act(async () => root.unmount());
  });
});
