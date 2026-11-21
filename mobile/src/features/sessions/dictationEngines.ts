import { requireOptionalNativeModule } from "expo";
import { File } from "expo-file-system";
import { Platform } from "react-native";

import type {
  DictationLanguage,
  EngineId,
  EngineInfo,
} from "./dictationLanguages";

// The speech-to-text engines dictation can run on, all on the phone: only the
// words go to the desktop. Each says what it can do here, readies itself for
// a language and transcribes a recording; the composer and settings work
// through this list, so another engine is one more entry.

/** What an engine readied: passed back when it transcribes. */
export type Prepared = { language: string | null; detail?: string };

export type DictationEngine = {
  id: EngineId;
  label: string;
  /** Quick and synchronous, for the composer's mic button. */
  canRun(): boolean;
  /** What it can do on this phone, languages included. */
  info(): Promise<EngineInfo>;
  /** Fetches what the language needs, refusing one it doesn't take. */
  prepare(language: string | null): Promise<Prepared>;
  /** The words in the recording at `uri`, or "" when there were none. */
  transcribe(uri: string, prepared: Prepared): Promise<string>;
};

// --- Apple Speech --------------------------------------------------------------

type AppleTranscription =
  typeof import("@react-native-ai/apple").AppleTranscription;

type AppleLocale = {
  id: string;
  name: string;
  nativeName: string;
  transcriber: "speech" | "dictation";
  installed: boolean;
};

/** This app's helper for Apple's transcribers (modules/monocode-speech). */
type SpeechModule = {
  appleAvailable(): boolean;
  appleLocales(): Promise<AppleLocale[]>;
  applePrepare(
    language: string,
  ): Promise<{ locale: string; transcriber: "speech" | "dictation" }>;
  appleDictate(uri: string, language: string): Promise<string>;
};

let library: AppleTranscription | null | undefined;
/** `@react-native-ai/apple`'s SpeechTranscriber, loaded on first use: the
 *  package's turbo modules are looked up as it loads, and throw where they
 *  aren't built in. */
function appleLibrary() {
  if (library === undefined) {
    try {
      library =
        Platform.OS === "ios"
          ? (
              // eslint-disable-next-line @typescript-eslint/no-require-imports
              require("@react-native-ai/apple") as typeof import("@react-native-ai/apple")
            ).AppleTranscription
          : null;
    } catch {
      library = null;
    }
  }
  return library;
}

let speechModule: SpeechModule | null | undefined;
function appleHelper() {
  if (speechModule === undefined) {
    speechModule =
      Platform.OS === "ios"
        ? requireOptionalNativeModule<SpeechModule>("MonocodeSpeech")
        : null;
  }
  return speechModule;
}

let appleLocales: Promise<DictationLanguage[]> | null = null;

const NEEDS_IOS_26 = "Apple Speech needs iOS 26 or later.";

export const appleEngine: DictationEngine = {
  id: "apple",
  label: "Apple Speech",
  canRun() {
    try {
      const helper = appleHelper();
      if (helper) return helper.appleAvailable();
      // Builds without the helper: SpeechTranscriber alone, as before.
      return appleLibrary()?.isAvailable("") ?? false;
    } catch {
      return false;
    }
  },
  async info() {
    const available = appleEngine.canRun();
    const helper = appleHelper();
    let languages: DictationLanguage[] = [];
    if (available && helper) {
      // Asked once a launch: the list only grows with an iOS update.
      appleLocales ??= helper
        .appleLocales()
        .then((locales) =>
          locales.map(({ id, name, nativeName }) => ({ id, name, nativeName })),
        )
        .catch(() => {
          appleLocales = null;
          return [];
        });
      languages = await appleLocales;
    }
    return {
      id: "apple",
      label: appleEngine.label,
      available,
      unavailableReason: available ? undefined : NEEDS_IOS_26,
      ready: true,
      automatic: false,
      languages,
    };
  },
  async prepare(language) {
    // Apple has no detection of its own: the plan gives it a language.
    if (!language) throw new Error("Choose a dictation language in Settings → Dictation.");
    const helper = appleHelper();
    if (helper) {
      const resolved = await helper.applePrepare(language);
      return { language: resolved.locale, detail: resolved.transcriber };
    }
    const native = appleLibrary();
    if (!native) throw new Error(NEEDS_IOS_26);
    await native.prepare(language);
    return { language, detail: "speech" };
  },
  async transcribe(uri, prepared) {
    const language = prepared.language!;
    const helper = appleHelper();
    // Languages only DictationTranscriber takes, as Russian and Ukrainian.
    if (prepared.detail === "dictation" && helper) {
      return (await helper.appleDictate(uri, language)).trim();
    }
    const native = appleLibrary();
    if (!native) throw new Error(NEEDS_IOS_26);
    const result = await native.transcribe(await new File(uri).arrayBuffer(), language);
    return result.segments
      .map((segment) => segment.text.trim())
      .filter(Boolean)
      .join(" ");
  },
};

/** Every engine, in the order settings lists them. */
export const ENGINES: readonly DictationEngine[] = [appleEngine];

export const engineFor = (id: EngineId) =>
  ENGINES.find((engine) => engine.id === id);

export const dictationEngineInfo = () =>
  Promise.all(ENGINES.map((engine) => engine.info()));
