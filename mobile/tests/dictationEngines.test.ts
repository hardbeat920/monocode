import { createRequire } from "node:module";
import { beforeEach, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({
  helper: null as null | Record<string, ReturnType<typeof vi.fn>>,
  parakeet: null as null | Record<string, ReturnType<typeof vi.fn>>,
  library: {
    isAvailable: vi.fn(() => true),
    prepare: vi.fn(async () => {}),
    transcribe: vi.fn(async () => ({
      segments: [{ text: " Hello " }, { text: "" }, { text: "world." }],
      duration: 1,
    })),
  },
  preference: { engine: "apple", language: "ru-RU" as string | null },
  model: "ultra" as string,
}));

vi.mock("react-native", () => ({ Platform: { OS: "ios" } }));
vi.mock("expo", () => ({
  requireOptionalNativeModule: (name: string) =>
    name === "MonocodeSpeech" ? native.helper : native.parakeet,
}));
vi.mock("expo-file-system", () => ({
  File: class {
    constructor(readonly uri: string) {}
    async arrayBuffer() {
      return new ArrayBuffer(4);
    }
  },
}));
// The engine requires the package lazily, which vi.mock doesn't reach.
const require = createRequire(import.meta.url);
require.cache[require.resolve("@react-native-ai/apple")] = {
  exports: { AppleTranscription: native.library },
} as unknown as NodeJS.Module;
vi.mock("@/features/settings/dictation", () => ({
  getDictationPreference: () => ({ parakeetModel: native.model, ...native.preference }),
}));

const helper = () => ({
  appleAvailable: vi.fn(() => true),
  appleLocales: vi.fn(async () => [
    { id: "en-US", name: "English (United States)", nativeName: "English (United States)", transcriber: "speech", installed: true },
    { id: "ru-RU", name: "Russian (Russia)", nativeName: "русский (Россия)", transcriber: "dictation", installed: false },
  ]),
  applePrepare: vi.fn(async (language: string) => ({
    locale: language,
    transcriber: language.startsWith("en") ? "speech" : "dictation",
  })),
  appleDictate: vi.fn(async () => " Привет, мир. "),
});

const status = (installed: boolean, supported = true) => ({
  supported, installed, downloading: false, loaded: false, sizeBytes: 0,
});
// Ultra and v2 downloaded unless told otherwise; Redux can't run (iOS 17).
const parakeet = (installed = true) => ({
  isSupported: vi.fn(() => true),
  statuses: vi.fn(async () => ({
    ultra: status(installed),
    v3: status(false),
    redux: status(false, false),
    v2: status(installed),
  })),
  download: vi.fn(async () => {}),
  cancelDownload: vi.fn(async () => {}),
  remove: vi.fn(async () => {}),
  transcribe: vi.fn(async () => " Привіт, світе. "),
  unload: vi.fn(async () => {}),
  addListener: vi.fn((_: string, listener: (progress: object) => void) => {
    listener({ model: "v3", fraction: 0.5, phase: "downloading" });
    listener({ model: "ultra", fraction: 0.9, phase: "downloading" });
    return { remove: vi.fn() };
  }),
});

beforeEach(() => {
  vi.resetModules();
  vi.clearAllMocks();
  native.helper = helper();
  native.parakeet = null;
  native.model = "ultra";
  native.preference = { engine: "apple", language: "ru-RU" };
  vi.stubGlobal("Intl", { ...Intl, DateTimeFormat: () => ({ resolvedOptions: () => ({ locale: "en-US" }) }) });
});

it("dictates Russian on an English phone through DictationTranscriber", async () => {
  const { prepareDictation, transcribe } = await import("@/features/sessions/dictation");
  const session = await prepareDictation();
  expect(native.helper!.applePrepare).toHaveBeenCalledWith("ru-RU");
  expect(session).toMatchObject({ engine: "apple", prepared: { language: "ru-RU", detail: "dictation" } });
  await expect(transcribe(session, "file:///rec.m4a")).resolves.toBe("Привет, мир.");
  expect(native.helper!.appleDictate).toHaveBeenCalledWith("file:///rec.m4a", "ru-RU");
  expect(native.library.transcribe).not.toHaveBeenCalled();
});

it("keeps SpeechTranscriber languages on @react-native-ai/apple", async () => {
  native.preference = { engine: "apple", language: null };
  const { prepareDictation, transcribe } = await import("@/features/sessions/dictation");
  const session = await prepareDictation();
  expect(native.helper!.applePrepare).toHaveBeenCalledWith("en-US");
  await expect(transcribe(session, "file:///rec.m4a")).resolves.toBe("Hello world.");
  expect(native.library.transcribe).toHaveBeenCalledWith(expect.any(ArrayBuffer), "en-US");
});

it("refuses a language Apple doesn't list, before recording", async () => {
  native.preference = { engine: "apple", language: "pl-PL" };
  const { prepareDictation, DictationError } = await import("@/features/sessions/dictation");
  const failure = prepareDictation();
  await expect(failure).rejects.toBeInstanceOf(DictationError);
  await expect(failure).rejects.toMatchObject({ problem: { kind: "unsupportedLanguage" } });
  expect(native.helper!.applePrepare).not.toHaveBeenCalled();
});

it("falls back to Apple Speech when the picked engine isn't in this build", async () => {
  native.preference = { engine: "parakeet", language: "ru-RU" };
  const { prepareDictation } = await import("@/features/sessions/dictation");
  await expect(prepareDictation()).resolves.toMatchObject({ engine: "apple", fellBackFrom: "parakeet" });
});

it("works as before on a build without the speech helper", async () => {
  native.helper = null;
  native.preference = { engine: "apple", language: null };
  const { canDictate, prepareDictation, transcribe } = await import("@/features/sessions/dictation");
  expect(canDictate()).toBe(true);
  const session = await prepareDictation();
  expect(native.library.prepare).toHaveBeenCalledWith("en-US");
  await expect(transcribe(session, "file:///rec.m4a")).resolves.toBe("Hello world.");
});

it("can't dictate before iOS 26", async () => {
  native.helper!.appleAvailable.mockReturnValue(false);
  const { canDictate, prepareDictation } = await import("@/features/sessions/dictation");
  expect(canDictate()).toBe(false);
  await expect(prepareDictation()).rejects.toMatchObject({
    problem: { kind: "noEngine", message: "Apple Speech needs iOS 26 or later." },
  });
});

it("lists Apple's languages for settings", async () => {
  const { dictationEngineInfo } = await import("@/features/sessions/dictationEngines");
  const [apple] = await dictationEngineInfo();
  expect(apple).toMatchObject({ id: "apple", available: true, automatic: false });
  expect(apple!.languages.map((language) => language.id)).toEqual(["en-US", "ru-RU"]);
});

it("lets Parakeet detect the language itself on Automatic", async () => {
  native.parakeet = parakeet();
  native.preference = { engine: "parakeet", language: null };
  const { prepareDictation, transcribe } = await import("@/features/sessions/dictation");
  const session = await prepareDictation();
  expect(session).toMatchObject({ engine: "parakeet", prepared: { language: null } });
  await expect(transcribe(session, "file:///rec.m4a")).resolves.toBe("Привіт, світе.");
  expect(native.parakeet!.transcribe).toHaveBeenCalledWith("file:///rec.m4a", null, "ultra");
});

it("hints Parakeet with the picked language", async () => {
  native.parakeet = parakeet();
  native.preference = { engine: "parakeet", language: "uk-UA" };
  const { prepareDictation, transcribe } = await import("@/features/sessions/dictation");
  const session = await prepareDictation();
  await transcribe(session, "file:///rec.m4a");
  expect(native.parakeet!.transcribe).toHaveBeenCalledWith("file:///rec.m4a", "uk", "ultra");
});

it("asks for the Parakeet model to be downloaded rather than swapping engines", async () => {
  native.parakeet = parakeet(false);
  native.preference = { engine: "parakeet", language: "ru-RU" };
  const { prepareDictation } = await import("@/features/sessions/dictation");
  await expect(prepareDictation()).rejects.toMatchObject({
    problem: { kind: "notReady", message: expect.stringContaining("Settings → Dictation") },
  });
  expect(native.parakeet!.transcribe).not.toHaveBeenCalled();
});

it("refuses a language Parakeet wasn't trained on", async () => {
  native.parakeet = parakeet();
  native.preference = { engine: "parakeet", language: "ja-JP" };
  const { prepareDictation } = await import("@/features/sessions/dictation");
  await expect(prepareDictation()).rejects.toMatchObject({ problem: { kind: "unsupportedLanguage" } });
});

it("reports why Parakeet can't run, and falls back to Apple", async () => {
  native.parakeet = parakeet();
  native.parakeet.isSupported!.mockReturnValue(false);
  native.preference = { engine: "parakeet", language: "ru-RU" };
  const { dictationEngineInfo } = await import("@/features/sessions/dictationEngines");
  const [, info] = await dictationEngineInfo();
  expect(info).toMatchObject({ available: false, unavailableReason: "Needs an iPhone with at least 4 GB of memory." });
  const { prepareDictation } = await import("@/features/sessions/dictation");
  await expect(prepareDictation()).resolves.toMatchObject({ engine: "apple", fellBackFrom: "parakeet" });
});

it("marks Parakeet missing from a build without its module", async () => {
  const { dictationEngineInfo } = await import("@/features/sessions/dictationEngines");
  const [, info] = await dictationEngineInfo();
  expect(info).toMatchObject({ id: "parakeet", available: false, unavailableReason: "Not included in this build of the app." });
});

it("lists Parakeet's 25 languages, Russian and Ukrainian among them", async () => {
  native.parakeet = parakeet();
  const { dictationEngineInfo } = await import("@/features/sessions/dictationEngines");
  const [, info] = await dictationEngineInfo();
  expect(info!.automatic).toBe(true);
  expect(info!.languages).toHaveLength(25);
  expect(info!.languages).toContainEqual({ id: "ru", name: "Russian", nativeName: "русский" });
  expect(info!.languages).toContainEqual({ id: "uk", name: "Ukrainian", nativeName: "українська" });
});

it("downloads a model with its own progress, and stops listening after", async () => {
  native.parakeet = parakeet(false);
  const { parakeetModels } = await import("@/features/sessions/dictationEngines");
  const progress = vi.fn();
  await parakeetModels.download("v3", progress);
  expect(native.parakeet!.download).toHaveBeenCalledWith("v3");
  // Another model's events aren't this one's progress.
  expect(progress).toHaveBeenCalledTimes(1);
  expect(progress).toHaveBeenCalledWith({ model: "v3", fraction: 0.5, phase: "downloading" });
  const subscription = native.parakeet!.addListener!.mock.results[0]!.value as { remove: ReturnType<typeof vi.fn> };
  expect(subscription.remove).toHaveBeenCalled();
});

it("transcribes with the picked model", async () => {
  native.parakeet = parakeet();
  native.model = "v2";
  native.preference = { engine: "parakeet", language: null };
  const { prepareDictation, transcribe } = await import("@/features/sessions/dictation");
  const session = await prepareDictation();
  // English-only v2 detects nothing: Automatic is the phone's English.
  expect(session).toMatchObject({ engine: "parakeet", prepared: { language: "en", detail: "v2" } });
  await transcribe(session, "file:///rec.m4a");
  expect(native.parakeet!.transcribe).toHaveBeenCalledWith("file:///rec.m4a", "en", "v2");
});

it("offers only English with the English-only model", async () => {
  native.parakeet = parakeet();
  native.model = "v2";
  native.preference = { engine: "parakeet", language: "ru-RU" };
  const { dictationEngineInfo } = await import("@/features/sessions/dictationEngines");
  const [, info] = await dictationEngineInfo();
  expect(info).toMatchObject({ automatic: false, languages: [{ id: "en", name: "English", nativeName: "English" }] });
  const { prepareDictation } = await import("@/features/sessions/dictation");
  await expect(prepareDictation()).rejects.toMatchObject({ problem: { kind: "unsupportedLanguage" } });
});

it("says a model this iOS can't run needs another pick", async () => {
  native.parakeet = parakeet();
  native.model = "redux";
  const { dictationEngineInfo } = await import("@/features/sessions/dictationEngines");
  const [, info] = await dictationEngineInfo();
  expect(info).toMatchObject({ ready: false, notReadyReason: expect.stringContaining("Parakeet Redux needs iOS 18") });
});

it("names the picked model when it isn't downloaded", async () => {
  native.parakeet = parakeet();
  native.model = "v3";
  const { dictationEngineInfo } = await import("@/features/sessions/dictationEngines");
  const [, info] = await dictationEngineInfo();
  expect(info).toMatchObject({ ready: false, notReadyReason: "Download Parakeet TDT v3 (about 470 MB) in Settings → Dictation." });
});

it("refuses a language the picked model doesn't take when it is checked", async () => {
  native.parakeet = parakeet();
  native.model = "v2";
  const { parakeetEngine } = await import("@/features/sessions/dictationEngines");
  await expect(parakeetEngine.prepare("ru-RU")).rejects.toThrow("Russian isn't available with Parakeet TDT v2.");
  await expect(parakeetEngine.prepare("en-GB")).resolves.toEqual({ language: "en", detail: "v2" });
});

it("readies the model it planned for, even if another is picked meanwhile", async () => {
  native.parakeet = parakeet();
  native.model = "ultra";
  const { parakeetEngine } = await import("@/features/sessions/dictationEngines");
  const info = await parakeetEngine.info();
  expect(info.variant).toBe("ultra");
  // English-only v2 picked after planning: Ultra's null (detect) still holds.
  native.model = "v2";
  await expect(parakeetEngine.prepare(null, info.variant)).resolves.toEqual({ language: null, detail: "ultra" });
});

it("doesn't call a failed status read a missing download", async () => {
  native.parakeet = parakeet();
  native.parakeet.statuses!.mockRejectedValue(new Error("native went away"));
  const { parakeetEngine } = await import("@/features/sessions/dictationEngines");
  const info = await parakeetEngine.info();
  expect(info).toMatchObject({ ready: false, notReadyReason: expect.stringContaining("Couldn't read which Parakeet models") });
});
