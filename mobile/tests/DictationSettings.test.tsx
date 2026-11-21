import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import DictationScreen from "@/app/settings/dictation";
import type { EngineInfo } from "@/features/sessions/dictationLanguages";

const state = vi.hoisted(() => ({
  preference: {
    engine: "apple" as "apple" | "parakeet",
    language: null as string | null,
    parakeetModel: "ultra",
  },
  listeners: new Set<() => void>(),
  prepare: vi.fn(async (_language: string | null) => ({ language: "ru-RU" })),
  alert: vi.fn(),
  saveFails: false,
  statusesFail: false,
  written: [] as object[],
  statuses: {} as Record<string, object>,
  download: vi.fn(async (_model: string, _onProgress: (progress: object) => void) => {}),
  remove: vi.fn(async () => {}),
  unload: vi.fn(async () => {}),
  engines: [] as EngineInfo[],
}));

vi.mock("expo-haptics", () => ({
  selectionAsync: vi.fn(),
  notificationAsync: vi.fn(),
  NotificationFeedbackType: { Success: "success" },
}));
vi.mock("react-native", () => ({
  StyleSheet: { create: (styles: object) => styles, hairlineWidth: 1 },
  View: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  Text: ({ children, onPress }: { children: ReactNode; onPress?: () => void }) => (
    <span onClick={onPress}>{children}</span>
  ),
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
  radii: { card: 20, row: 12 },
}));
vi.mock("@/features/settings/dictation", async () => {
  const { useSyncExternalStore } = await import("react");
  const set = (next: object) => {
    state.preference = { ...state.preference, ...next };
    state.listeners.forEach((listener) => listener());
    if (state.saveFails) return Promise.reject(new Error("disk full"));
    state.written.push({ ...state.preference });
    return Promise.resolve();
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
const dictationLocaleMock = vi.hoisted(() => vi.fn(() => "en-US"));
vi.mock("@/features/sessions/dictation", () => ({ dictationLocale: dictationLocaleMock }));
vi.mock("@/features/sessions/dictationEngines", () => ({
  dictationEngineInfo: async () => state.engines,
  engineFor: () => ({ prepare: state.prepare }),
  PARAKEET_MODELS: [
    { id: "ultra", name: "Parakeet Ultra", detail: "Most accurate, 25 languages", size: "about 630 MB", languages: [] },
    { id: "v3", name: "Parakeet TDT v3", detail: "NVIDIA's original, 25 languages", size: "about 470 MB", languages: [] },
    { id: "redux", name: "Parakeet Redux", detail: "Smallest", size: "about 220 MB", languages: [] },
    { id: "v2", name: "Parakeet TDT v2", detail: "English only", size: "about 470 MB", languages: ["en"] },
  ],
  parakeetModels: {
    statuses: async () => {
      if (state.statusesFail) throw new Error("native went away");
      return state.statuses;
    },
    download: state.download,
    cancel: vi.fn(),
    remove: state.remove,
    unload: state.unload,
  },
  parakeetDetects: (model: { languages: string[] }) => model.languages.length > 1,
  parakeetLanguages: (model: { languages: string[] }) =>
    model.languages.map((id) => ({ id, name: id === "ru" ? "Russian" : "English", nativeName: id === "ru" ? "русский" : undefined })),
  parakeetModelInfo: (id: string) => ({
    id,
    name: id === "v2" ? "Parakeet TDT v2" : "Parakeet Ultra",
    languages: id === "v2" ? ["en"] : ["ru", "en"],
  }),
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
  state.preference = { engine: "apple", language: null, parakeetModel: "ultra" };
  state.engines = [apple];
  state.prepare.mockClear();
  state.alert.mockClear();
  state.saveFails = false;
  state.statusesFail = false;
  state.written = [];
  dictationLocaleMock.mockReturnValue("en-US");
  state.statuses = {};
  state.download.mockReset();
  state.download.mockImplementation(async () => {});
});
afterEach(() => act(() => root.unmount()));

async function render() {
  container = document.createElement("div");
  root = createRoot(container);
  await act(async () => root.render(<DictationScreen />));
}
// Rows read their hint after the label: "Russian (Russia), русский (Россия)".
const button = (label: string) =>
  container.querySelector<HTMLButtonElement>(
    `button[aria-label="${label}"], button[aria-label^="${label}, "]`,
  )!;

it("lists the engine's languages with Automatic as the phone's language", async () => {
  await render();
  expect(button("Automatic").getAttribute("aria-pressed")).toBe("true");
  expect(container.textContent).toContain("Phone language, English (United States)");
  // VoiceOver hears which language Automatic uses.
  expect(button("Automatic").getAttribute("aria-label")).toBe(
    "Automatic, Phone language, English (United States)",
  );
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

it("keeps the previous language when the engine refuses one", async () => {
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
  await render();
  await act(async () => button("Russian (Russia)").click());
  expect(state.preference.language).toBe("ru-RU");
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
  // The retry is what reaches storage, with the pick shown.
  expect(state.written).toEqual([expect.objectContaining({ language: "ru-RU" })]);
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

it("saves a language only once the engine has taken it", async () => {
  let accept: () => void = () => {};
  state.prepare.mockImplementationOnce(
    () => new Promise((resolve) => { accept = () => resolve({ language: "ru-RU" }); }),
  );
  await render();
  await act(async () => button("Russian (Russia)").click());
  // Shown as picked while its speech assets download, but not saved yet.
  expect(button("Russian (Russia)").getAttribute("aria-pressed")).toBe("true");
  expect(state.preference.language).toBeNull();
  await act(async () => accept());
  expect(state.preference.language).toBe("ru-RU");
});

it("checks Automatic with the locale recording will use", async () => {
  state.engines = [{ ...apple, languages: [{ id: "en-GB", name: "English (UK)" }, { id: "ru-RU", name: "Russian (Russia)" }] }];
  state.preference.language = "ru-RU";
  await render();
  await act(async () => button("Automatic").click());
  // The phone's en-US isn't listed: recording takes en-GB, so the check does too.
  expect(state.prepare).toHaveBeenCalledWith("en-GB");
  expect(state.preference.language).toBeNull();
});

const parakeet: EngineInfo = {
  ...apple,
  id: "parakeet",
  label: "NVIDIA Parakeet",
  automatic: true,
  languages: [{ id: "ru", name: "Russian", nativeName: "русский" }],
};
const modelStatus = (installed: boolean, extra: object = {}) => ({
  supported: true, installed, downloading: false, loaded: false, sizeBytes: installed ? 640e6 : 0, ...extra,
});

describe("Parakeet models", () => {
  beforeEach(() => {
    state.preference.engine = "parakeet";
    state.engines = [apple, parakeet];
    state.statuses = {
      ultra: modelStatus(true),
      v3: modelStatus(false),
      redux: modelStatus(false, { supported: false }),
    };
  });

  it("lists the models with what each one is and where it stands", async () => {
    await render();
    expect(button("Parakeet Ultra").getAttribute("aria-pressed")).toBe("true");
    expect(container.textContent).toContain("Downloaded, 640 MB");
    expect(container.textContent).toContain("Not downloaded, about 470 MB");
    // Redux needs iOS 18: shown, but can't be picked or downloaded.
    expect(container.textContent).toContain("Needs iOS 18 or later");
    expect(button("Parakeet Redux").disabled).toBe(true);
    expect(button("Download Parakeet Redux")).toBeNull();
  });

  it("isn't shown for Apple Speech", async () => {
    state.preference.engine = "apple";
    await render();
    expect(container.textContent).not.toContain("Parakeet Ultra");
  });

  it("picks a model", async () => {
    await render();
    await act(async () => button("Parakeet TDT v3").click());
    expect(state.preference.parakeetModel).toBe("v3");
    expect(button("Parakeet TDT v3").getAttribute("aria-pressed")).toBe("true");
  });

  it("downloads a model with its progress", async () => {
    let finish: () => void = () => {};
    state.download.mockImplementation((_model, onProgress) =>
      new Promise<void>((resolve) => {
        onProgress({ model: "v3", fraction: 0.42, phase: "downloading" });
        finish = () => {
          state.statuses = { ...state.statuses, v3: modelStatus(true) };
          resolve();
        };
      }),
    );
    await render();
    await act(async () => button("Download Parakeet TDT v3").click());
    expect(state.download).toHaveBeenCalledWith("v3", expect.any(Function));
    expect(container.textContent).toContain("Downloading… 42%");
    // One download at a time.
    expect(button("Cancel downloading Parakeet TDT v3")).toBeTruthy();
    await act(async () => finish());
    expect(button("Parakeet TDT v3").getAttribute("aria-label")).toContain("Downloaded, 640 MB");
    expect(button("Delete Parakeet TDT v3")).toBeTruthy();
  });

  it("follows a download started before settings was reopened", async () => {
    state.statuses = { ...state.statuses, v3: modelStatus(false, { downloading: true }) };
    state.download.mockImplementation(async () => {
      state.statuses = { ...state.statuses, v3: modelStatus(true) };
    });
    await render();
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    // The native side joins the running download rather than starting another.
    expect(state.download).toHaveBeenCalledTimes(1);
    expect(state.download).toHaveBeenCalledWith("v3", expect.any(Function));
    expect(button("Delete Parakeet TDT v3")).toBeTruthy();
  });

  it("deletes a model after asking", async () => {
    await render();
    await act(async () => button("Delete Parakeet Ultra").click());
    const buttons = state.alert.mock.calls[0]![2] as { text: string; onPress?: () => void }[];
    await act(async () => buttons.find((entry) => entry.text === "Delete")!.onPress!());
    expect(state.remove).toHaveBeenCalledWith("ultra");
  });
});

describe("Parakeet before its model is downloaded", () => {
  beforeEach(() => {
    state.preference.engine = "parakeet";
    state.engines = [apple, { ...parakeet, ready: false, notReadyReason: "Download Parakeet Ultra" }];
    state.statuses = { ultra: modelStatus(false) };
  });

  it("still takes a language the model knows", async () => {
    await render();
    await act(async () => button("Russian").click());
    expect(state.prepare).not.toHaveBeenCalled();
    expect(state.preference.language).toBe("ru");
  });

  it("shows no error for a cancelled download", async () => {
    state.download.mockRejectedValueOnce(Object.assign(new Error("The download was cancelled."), { code: "ERR_PARAKEET_CANCELLED" }));
    await render();
    await act(async () => button("Download Parakeet Ultra").click());
    expect(state.alert).not.toHaveBeenCalled();
    expect(button("Download Parakeet Ultra")).toBeTruthy();
  });

  it("reports a download that failed", async () => {
    state.download.mockRejectedValueOnce(new Error("Check your connection and try again."));
    await render();
    await act(async () => button("Download Parakeet Ultra").click());
    expect(state.alert).toHaveBeenCalledWith("Download failed", "Check your connection and try again.");
  });
});

it("keeps the language across models, and marks it where it isn't taken", async () => {
  state.preference = { engine: "parakeet", language: "ru", parakeetModel: "ultra" };
  state.engines = [apple, parakeet];
  state.statuses = { ultra: modelStatus(true), v2: modelStatus(true) };
  await render();
  await act(async () => button("Parakeet TDT v2").click());
  // Not erased: still there for Apple Speech or a multilingual model.
  expect(state.preference).toMatchObject({ parakeetModel: "v2", language: "ru" });
  expect(container.textContent).toContain(
    "Russian isn't available for Parakeet TDT v2. Choose another language, model or engine.",
  );
  // The list is the new model's at once, before engine info is read again.
  expect(button("English")).toBeTruthy();
  expect(button("Russian")).toBeNull();
});

it("drops a check under way when the engine changes, and takes the next pick", async () => {
  let accept: () => void = () => {};
  state.prepare.mockImplementationOnce(
    () => new Promise((resolve) => { accept = () => resolve({ language: "ru-RU" }); }),
  );
  state.engines = [apple, { ...parakeet, available: true }];
  state.statuses = { ultra: modelStatus(true) };
  await render();
  await act(async () => button("Russian (Russia)").click());
  await act(async () => button("NVIDIA Parakeet").click());
  await act(async () => accept());
  // Apple's late answer doesn't save Russian under Parakeet.
  expect(state.preference).toMatchObject({ engine: "parakeet", language: null });
  expect(container.querySelector("[data-busy]")).toBeNull();
  // Nothing is left blocking: a pick goes straight through.
  await act(async () => button("Russian").click());
  expect(state.preference.language).toBe("ru");
});

it("marks Automatic when the phone's language isn't one the model takes", async () => {
  state.preference = { engine: "parakeet", language: null, parakeetModel: "v2" };
  state.engines = [apple, parakeet];
  state.statuses = { v2: modelStatus(true) };
  vi.mocked(dictationLocaleMock).mockReturnValue("ru-RU");
  await render();
  expect(container.textContent).toContain(
    "Automatic uses the phone's language, Russian, which Parakeet TDT v2 doesn't take.",
  );
});

it("says when it can't read the models, and reads them again on request", async () => {
  state.preference = { engine: "parakeet", language: null, parakeetModel: "ultra" };
  state.engines = [apple, parakeet];
  state.statuses = { ultra: modelStatus(true) };
  state.statusesFail = true;
  await render();
  expect(container.textContent).toContain("Couldn't read which models are on this phone.");
  // Not claimed as missing: no Get offered on a read that failed.
  expect(button("Download Parakeet Ultra")).toBeNull();
  state.statusesFail = false;
  const retry = [...container.querySelectorAll("span")].find((span) => span.textContent === "Try Again")!;
  await act(async () => retry.click());
  expect(container.textContent).not.toContain("Couldn't read which models");
  expect(button("Delete Parakeet Ultra")).toBeTruthy();
});
