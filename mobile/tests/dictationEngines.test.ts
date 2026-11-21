import { createRequire } from "node:module";
import { beforeEach, expect, it, vi } from "vitest";

const native = vi.hoisted(() => ({
  helper: null as null | Record<string, ReturnType<typeof vi.fn>>,
  library: {
    isAvailable: vi.fn(() => true),
    prepare: vi.fn(async () => {}),
    transcribe: vi.fn(async () => ({
      segments: [{ text: " Hello " }, { text: "" }, { text: "world." }],
      duration: 1,
    })),
  },
  preference: { engine: "apple", language: "ru-RU" as string | null },
}));

vi.mock("react-native", () => ({ Platform: { OS: "ios" } }));
vi.mock("expo", () => ({ requireOptionalNativeModule: () => native.helper }));
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
  getDictationPreference: () => native.preference,
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

beforeEach(() => {
  vi.resetModules();
  vi.clearAllMocks();
  native.helper = helper();
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
