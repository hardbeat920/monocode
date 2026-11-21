import { requireOptionalNativeModule } from "expo";
import { File } from "expo-file-system";
import { Platform } from "react-native";

import { getDictationPreference } from "@/features/settings/dictation";

import {
  LANGUAGE_NAMES,
  languageName,
  matchLanguage,
  type DictationLanguage,
  type EngineId,
  type EngineInfo,
  PARAKEET_MODEL_IDS,
  type ParakeetModelId,
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
  /** `variant` is the one planned (EngineInfo.variant), if the engine has any. */
  prepare(language: string | null, variant?: string): Promise<Prepared>;
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

// --- NVIDIA Parakeet -----------------------------------------------------------

/** modules/monocode-parakeet: Parakeet TDT 0.6B and its post-trainings, by
 *  FluidAudio on Core ML. */
type ParakeetModule = {
  isSupported(): boolean;
  statuses(withSizes?: boolean): Promise<Record<ParakeetModelId, ParakeetStatus>>;
  download(model: ParakeetModelId): Promise<void>;
  cancelDownload(): Promise<void>;
  remove(model: ParakeetModelId): Promise<void>;
  transcribe(uri: string, language: string | null, model: ParakeetModelId): Promise<string>;
  unload(): Promise<void>;
  addListener(
    event: "onDownloadProgress",
    listener: (progress: ParakeetProgress) => void,
  ): { remove(): void };
};

export type ParakeetStatus = {
  /** This phone can run the model: enough memory, and iOS 18 for Redux. */
  supported: boolean;
  installed: boolean;
  downloading: boolean;
  loaded: boolean;
  sizeBytes: number;
};

export type ParakeetProgress = {
  model: ParakeetModelId;
  /** 0 to 1. */
  fraction: number;
  phase: "listing" | "downloading" | "compiling";
};

/** The 25 European languages Parakeet v3 and its post-trainings take. */
export const PARAKEET_LANGUAGES = [
  "bg", "cs", "da", "de", "el", "en", "es", "et", "fi", "fr", "hr", "hu", "it",
  "lt", "lv", "mt", "nl", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "uk",
] as const;

export type ParakeetModelInfo = {
  id: ParakeetModelId;
  name: string;
  /** What sets it apart, for its row in settings. */
  detail: string;
  /** What the download takes on the phone. */
  size: string;
  languages: readonly string[];
};

/** Each model offered, keyed by id, so adding one to PARAKEET_MODEL_IDS
 *  without describing it here doesn't type-check. */
const PARAKEET_CATALOG: Record<ParakeetModelId, Omit<ParakeetModelInfo, "id">> = {
  ultra: {
    name: "Parakeet Ultra",
    detail: "Most accurate, 25 languages. Its first use takes several minutes to prepare",
    size: "about 630 MB",
    languages: PARAKEET_LANGUAGES,
  },
  v3: {
    name: "Parakeet TDT v3",
    detail: "Recommended. NVIDIA's original, 25 languages",
    size: "about 470 MB",
    languages: PARAKEET_LANGUAGES,
  },
  redux: {
    name: "Parakeet Redux",
    detail: "Smallest, 25 languages, iOS 18. Its first use takes a few minutes to prepare",
    size: "about 220 MB",
    languages: PARAKEET_LANGUAGES,
  },
  v2: {
    name: "Parakeet TDT v2",
    detail: "English only, sharpest on English",
    size: "about 470 MB",
    languages: ["en"],
  },
};

/** The models offered, the default first. */
export const PARAKEET_MODELS: readonly ParakeetModelInfo[] = PARAKEET_MODEL_IDS.map((id) => ({
  id,
  ...PARAKEET_CATALOG[id],
}));

export const parakeetModelInfo = (id: ParakeetModelId) =>
  PARAKEET_MODELS.find((model) => model.id === id) ?? PARAKEET_MODELS[0]!;

/** A model's languages as settings lists them. */
export const parakeetLanguages = (model: ParakeetModelInfo): DictationLanguage[] =>
  model.languages.map((id) => ({
    id,
    name: LANGUAGE_NAMES[id]!.name,
    nativeName: LANGUAGE_NAMES[id]!.nativeName,
  }));

/** Whether a model recognizes the language itself. English-only v2 has
 *  nothing to detect: its Automatic is the phone's language. */
export const parakeetDetects = (model: ParakeetModelInfo) => model.languages.length > 1;

let parakeetModule: ParakeetModule | null | undefined;
function parakeetNative() {
  if (parakeetModule === undefined) {
    parakeetModule =
      Platform.OS === "ios"
        ? requireOptionalNativeModule<ParakeetModule>("MonocodeParakeet")
        : null;
  }
  return parakeetModule;
}

const NOT_INCLUDED = "NVIDIA Parakeet isn't included in this build of the app.";
const UNREADABLE = "Couldn't read which Parakeet models are on this phone. Try again.";
const isModelId = (id: string | undefined): id is ParakeetModelId =>
  PARAKEET_MODEL_IDS.includes(id as ParakeetModelId);
const notDownloaded = (model: ParakeetModelInfo) =>
  `Download ${model.name} (${model.size}) in Settings → Dictation.`;

export const parakeetEngine: DictationEngine = {
  id: "parakeet",
  label: "NVIDIA Parakeet",
  canRun() {
    try {
      return parakeetNative()?.isSupported() ?? false;
    } catch {
      return false;
    }
  },
  async info() {
    const native = parakeetNative();
    const available = parakeetEngine.canRun();
    const model = parakeetModelInfo(getDictationPreference().parakeetModel);
    const statuses =
      available && native ? await native.statuses(false).then((read) => read, () => null) : null;
    const unreadable = available && !!native && statuses === null;
    const status = statuses?.[model.id];
    const ready = !!status?.installed && status.supported;
    return {
      id: "parakeet",
      label: parakeetEngine.label,
      available,
      unavailableReason: available
        ? undefined
        : native
          ? "Needs an iPhone with at least 4 GB of memory."
          : "Not included in this build of the app.",
      ready,
      notReadyReason: ready
        ? undefined
        : unreadable
          ? UNREADABLE
          : status && !status.supported
          ? `${model.name} needs iOS 18 or later. Choose another model in Settings → Dictation.`
          : notDownloaded(model),
      automatic: parakeetDetects(model),
      languages: parakeetLanguages(model),
      variant: model.id,
    };
  },
  async prepare(language, variant) {
    const native = parakeetNative();
    if (!native) throw new Error(NOT_INCLUDED);
    // The model planned for, even if another was picked since.
    const model = parakeetModelInfo(
      isModelId(variant) ? variant : getDictationPreference().parakeetModel,
    );
    const statuses = await native.statuses(false).catch(() => {
      throw new Error(UNREADABLE);
    });
    const status = statuses[model.id];
    if (!status?.installed) throw new Error(`${model.name} isn't downloaded. ${notDownloaded(model)}`);
    // The English-only model refuses Russian rather than mishearing it.
    if (language !== null) {
      const taken = matchLanguage(language, model.languages);
      if (!taken) throw new Error(`${languageName(language)} isn't available with ${model.name}.`);
      return { language: taken, detail: model.id };
    }
    return { language, detail: model.id };
  },
  async transcribe(uri, prepared) {
    const native = parakeetNative();
    if (!native) throw new Error(NOT_INCLUDED);
    const model = isModelId(prepared.detail) ? prepared.detail : getDictationPreference().parakeetModel;
    return (await native.transcribe(uri, prepared.language, model)).trim();
  },
};

/** The Parakeet models on the phone: each downloaded on request, never bundled. */
export const parakeetModels = {
  /** Each model's state; rejects when it can't be read, so that isn't shown
   *  as nothing downloaded. */
  statuses: async (): Promise<Partial<Record<ParakeetModelId, ParakeetStatus>>> => {
    const native = parakeetNative();
    if (!native) return {};
    return native.statuses(true);
  },
  /** Downloads one, or follows its download already running, calling back
   *  with its progress until it is ready. */
  async download(model: ParakeetModelId, onProgress: (progress: ParakeetProgress) => void) {
    const native = parakeetNative();
    if (!native) throw new Error(NOT_INCLUDED);
    const subscription = native.addListener("onDownloadProgress", (progress) => {
      if (progress.model === model) onProgress(progress);
    });
    try {
      await native.download(model);
    } finally {
      subscription.remove();
    }
  },
  cancel: async () => parakeetNative()?.cancelDownload(),
  remove: async (model: ParakeetModelId) => parakeetNative()?.remove(model),
  /** Frees the loaded model, as when another one is picked. */
  unload: async () => parakeetNative()?.unload(),
};

/** Every engine, in the order settings lists them. */
export const ENGINES: readonly DictationEngine[] = [appleEngine, parakeetEngine];

export const engineFor = (id: EngineId) =>
  ENGINES.find((engine) => engine.id === id);

export const dictationEngineInfo = () =>
  Promise.all(ENGINES.map((engine) => engine.info()));
