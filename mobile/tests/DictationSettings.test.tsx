import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import DictationScreen from "@/app/settings/dictation";
import type { EngineInfo } from "@/features/sessions/dictationLanguages";

const state = vi.hoisted(() => ({
  preference: { engine: "apple" as "apple" | "parakeet", language: null as string | null },
  listeners: new Set<() => void>(),
  prepare: vi.fn(async (_language: string | null) => ({ language: "ru-RU" })),
  alert: vi.fn(),
  saveFails: false,
  engines: [] as EngineInfo[],
}));

vi.mock("expo-haptics", () => ({ selectionAsync: vi.fn() }));
vi.mock("react-native", () => ({
  StyleSheet: { create: (styles: object) => styles, hairlineWidth: 1 },
  View: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children: ReactNode }) => <span>{children}</span>,
  TextInput: ({ value, onChangeText }: { value: string; onChangeText: (text: string) => void }) => (
    <input value={value} onChange={(event) => onChangeText(event.target.value)} />
  ),
  Pressable: ({ children, onPress, accessibilityLabel, accessibilityState, disabled }: {
    children: ReactNode; onPress: () => void; accessibilityLabel: string;
    accessibilityState?: { selected?: boolean }; disabled?: boolean;
  }) => (
    <button aria-label={accessibilityLabel} aria-pressed={accessibilityState?.selected} disabled={disabled} onClick={onPress}>
      {children}
    </button>
  ),
  ActivityIndicator: () => <i data-busy />,
  Alert: { alert: state.alert },
}));
vi.mock("@/features/settings/SettingsUI", () => ({
  SettingsPage: ({ children }: { children: ReactNode }) => <main>{children}</main>,
}));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/theme", () => ({
  themedStyles: (create: (colors: object) => object) => () => create({}),
  useColors: () => ({}),
}));
vi.mock("@/features/settings/dictation", async () => {
  const { useSyncExternalStore } = await import("react");
  const set = (next: object) => {
    state.preference = { ...state.preference, ...next };
    state.listeners.forEach((listener) => listener());
    return state.saveFails ? Promise.reject(new Error("disk full")) : Promise.resolve();
  };
  return {
    getDictationPreference: () => state.preference,
    useDictationPreference: () => [
      useSyncExternalStore(
        (listener) => {
          state.listeners.add(listener);
          return () => state.listeners.delete(listener);
        },
        () => state.preference,
      ),
      set,
    ],
  };
});
vi.mock("@/features/sessions/dictation", () => ({ dictationLocale: () => "en-US" }));
vi.mock("@/features/sessions/dictationEngines", () => ({
  dictationEngineInfo: async () => state.engines,
  engineFor: () => ({ prepare: state.prepare }),
}));

const apple: EngineInfo = {
  id: "apple",
  label: "Apple Speech",
  available: true,
  ready: true,
  automatic: false,
  languages: [
    { id: "en-US", name: "English (United States)" },
    { id: "ru-RU", name: "Russian (Russia)", nativeName: "русский (Россия)" },
    { id: "uk-UA", name: "Ukrainian (Ukraine)", nativeName: "українська (Україна)" },
  ],
};

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let root: ReturnType<typeof createRoot>;
let container: HTMLDivElement;
beforeEach(() => {
  state.preference = { engine: "apple", language: null };
  state.engines = [apple];
  state.prepare.mockClear();
  state.alert.mockClear();
  state.saveFails = false;
});
afterEach(() => act(() => root.unmount()));

async function render() {
  container = document.createElement("div");
  root = createRoot(container);
  await act(async () => root.render(<DictationScreen />));
}
const button = (label: string) =>
  container.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!;

it("lists the engine's languages with Automatic as the phone's language", async () => {
  await render();
  expect(button("Automatic").getAttribute("aria-pressed")).toBe("true");
  expect(container.textContent).toContain("The phone's language, English (United States).");
  expect(container.textContent).toContain("русский (Россия)");
  expect(button("Ukrainian (Ukraine)")).toBeTruthy();
});

it("picks Russian and checks it with the engine", async () => {
  await render();
  await act(async () => button("Russian (Russia)").click());
  expect(state.preference.language).toBe("ru-RU");
  expect(state.prepare).toHaveBeenCalledWith("ru-RU");
  expect(button("Russian (Russia)").getAttribute("aria-pressed")).toBe("true");
});

it("goes back to the previous language when the engine refuses one", async () => {
  state.prepare.mockRejectedValueOnce(new Error("Ukrainian isn't available for Apple Speech on this device."));
  state.preference.language = "ru-RU";
  await render();
  await act(async () => button("Ukrainian (Ukraine)").click());
  expect(state.preference.language).toBe("ru-RU");
  expect(state.alert).toHaveBeenCalledWith(
    "Language unavailable",
    "Ukrainian isn't available for Apple Speech on this device.",
  );
});

it("flags a saved language the engine doesn't take", async () => {
  state.preference.language = "pl-PL";
  await render();
  expect(container.textContent).toContain("Polish isn't available for Apple Speech.");
});

it("shows an engine that can't run, and says which stands in", async () => {
  state.preference.engine = "parakeet";
  state.engines = [
    apple,
    { ...apple, id: "parakeet", label: "NVIDIA Parakeet", available: false, unavailableReason: "Needs iOS 17 or later.", automatic: true },
  ];
  await render();
  expect(button("NVIDIA Parakeet").disabled).toBe(true);
  expect(container.textContent).toContain("Needs iOS 17 or later.");
  expect(container.textContent).toContain("NVIDIA Parakeet isn't available here, so Apple Speech is used.");
});

it("doesn't let a late refusal undo a newer pick", async () => {
  let refuse: (reason: Error) => void = () => {};
  state.prepare.mockImplementationOnce(
    () => new Promise((_, reject) => { refuse = reject; }),
  );
  await render();
  await act(async () => button("Ukrainian (Ukraine)").click());
  // Settings closes while Ukrainian is checked, and Russian is picked after.
  act(() => root.unmount());
  state.preference = { ...state.preference, language: "ru-RU" };
  await render();
  await act(async () => refuse(new Error("Ukrainian isn't available.")));
  expect(state.preference.language).toBe("ru-RU");
  expect(state.alert).not.toHaveBeenCalled();
});

it("says when a pick couldn't be saved", async () => {
  state.saveFails = true;
  await render();
  await act(async () => button("Russian (Russia)").click());
  expect(state.preference.language).toBe("ru-RU");
  expect(state.alert).toHaveBeenCalledWith(
    "Couldn't save",
    expect.stringContaining("until Monocode closes"),
    expect.any(Array),
  );
  // Try Again writes the shown pick, now that storage works.
  state.saveFails = false;
  const buttons = state.alert.mock.calls[0]![2] as { text: string; onPress?: () => void }[];
  const saves = vi.fn();
  state.listeners.add(saves);
  await act(async () => buttons.find((entry) => entry.text === "Try Again")!.onPress!());
  expect(saves).toHaveBeenCalled();
  expect(state.preference.language).toBe("ru-RU");
  expect(state.alert).toHaveBeenCalledTimes(1);
});

it("clears the language filter when the engine changes", async () => {
  const many = Array.from({ length: 14 }, (_, index) => ({ id: `x${index}`, name: `Language ${index}` }));
  state.engines = [
    { ...apple, languages: [...apple.languages, ...many] },
    { ...apple, id: "parakeet", label: "NVIDIA Parakeet", automatic: true, languages: [{ id: "ru", name: "Russian" }] },
  ];
  await render();
  const input = container.querySelector("input")!;
  const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  act(() => {
    setValue.call(input, "zzz");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(button("Automatic")).toBeNull();
  await act(async () => button("NVIDIA Parakeet").click());
  expect(button("Automatic")).toBeTruthy();
  expect(button("Russian")).toBeTruthy();
});
