// What dictation runs on and in which language, decided apart from the native
// engines so it can be tested: the engine picked in settings or, when this
// phone can't run it, one that it can; and the language picked, matched to
// one the engine takes, or the engine's own detection, or the phone's.

export type EngineId = "apple" | "parakeet";

export type DictationLanguage = {
  /** BCP-47, as "ru-RU" or, for engines that take no region, "ru". */
  id: string;
  /** In the phone's language: "Russian (Russia)". */
  name: string;
  /** In its own: "русский (Россия)". */
  nativeName?: string;
};

export type EngineInfo = {
  id: EngineId;
  label: string;
  /** Built into this app and able to run on this phone. */
  available: boolean;
  /** Why not, when it isn't. */
  unavailableReason?: string;
  /** Ready to transcribe: false while a model it needs isn't on the phone. */
  ready: boolean;
  /** Why not, when it isn't. */
  notReadyReason?: string;
  /** Recognizes the spoken language itself, so "Automatic" can be offered. */
  automatic: boolean;
  /** The languages it takes. Empty when they can't be listed here, in which
   *  case the engine is left to accept or refuse one when it prepares. */
  languages: DictationLanguage[];
  /** What this info describes, as Parakeet's model id: passed back to the
   *  engine when it prepares, so it readies what was planned. */
  variant?: string;
};

/** The Parakeet models offered, the default first (their native ids). */
export const PARAKEET_MODEL_IDS = ["v3", "ultra", "redux", "v2"] as const;
export type ParakeetModelId = (typeof PARAKEET_MODEL_IDS)[number];

/** Settings' pick. A null language is "Automatic": the engine's own
 *  detection, or the phone's language for an engine without it. */
export type DictationPreference = {
  engine: EngineId;
  language: string | null;
  /** The Parakeet model used when Parakeet is the engine. */
  parakeetModel: ParakeetModelId;
};

/** Whether a model download ended because it was cancelled, not because it
 *  failed (modules/monocode-parakeet's cancel code). */
export const isCancelledDownload = (reason: unknown) =>
  (reason as { code?: string } | null)?.code === "ERR_PARAKEET_CANCELLED";

export const DEFAULT_PREFERENCE: DictationPreference = {
  engine: "apple",
  language: null,
  // NVIDIA's multilingual original: quick to prepare on every supported
  // phone. Ultra is more accurate but takes much longer to prepare the first
  // time, so it's left to choose.
  parakeetModel: "v3",
};

export type DictationProblem = {
  kind: "noEngine" | "notReady" | "unsupportedLanguage";
  title: string;
  message: string;
};

export type DictationPlan =
  | {
      ok: true;
      engine: EngineId;
      /** What the engine is asked for; null lets it detect the language. */
      language: string | null;
      /** The engine picked in settings, when it couldn't run and this one
       *  stands in. */
      fellBackFrom?: EngineId;
      /** The engine's variant the plan was made for (see EngineInfo). */
      variant?: string;
    }
  | { ok: false; problem: DictationProblem };

const base = (id: string) => id.split(/[-_]/)[0]!.toLowerCase();
const normalized = (id: string) => id.replace(/_/g, "-").toLowerCase();
/** The BCP-47 region subtag ("tw" in "zh-Hant-TW"), if any. */
const region = (id: string) =>
  normalized(id)
    .split("-")
    .slice(1)
    .find((part) => /^([a-z]{2}|\d{3})$/.test(part));
/** The BCP-47 script subtag ("latn" in "sr-Latn-RS"), if any. */
const script = (id: string) =>
  normalized(id)
    .split("-")
    .slice(1)
    .find((part) => /^[a-z]{4}$/.test(part));

/**
 * The language among `supported` that best stands for `requested`: the same
 * one, else the same language in its own country ("ru" → "ru-RU"), else the
 * same language anywhere. Null when the engine doesn't take the language.
 */
export function matchLanguage(
  requested: string,
  supported: readonly string[],
): string | null {
  const wanted = normalized(requested);
  const exact = supported.find((id) => normalized(id) === wanted);
  if (exact) return exact;
  const language = base(requested);
  let sameLanguage = supported.filter((id) => base(id) === language);
  if (sameLanguage.length === 0) return null;
  // A script ("sr-Latn") outranks a region: Latin Serbian isn't Cyrillic.
  const wantedScript = script(requested);
  if (wantedScript) {
    const sameScript = sameLanguage.filter((id) => script(id) === wantedScript);
    if (sameScript.length > 0) sameLanguage = sameScript;
  }
  // A bare code is the whole language; an engine taking one takes them all.
  const bare = sameLanguage.find((id) => !/[-_]/.test(id));
  if (bare) return bare;
  // The requested region ("zh-Hant-TW" → "zh-TW"), then the language's own
  // country ("ru" → "ru-RU"), then any.
  const wantedRegion = region(requested);
  const sameRegion = wantedRegion
    ? sameLanguage.find((id) => region(id) === wantedRegion)
    : undefined;
  if (sameRegion) return sameRegion;
  const home = sameLanguage.find((id) => region(id) === language);
  return home ?? sameLanguage[0]!;
}

/** A language's name, from the engine's list when it is on it. */
export function languageName(
  id: string,
  languages: readonly DictationLanguage[] = [],
) {
  const listed = languages.find((language) => language.id === id);
  if (listed) return listed.name;
  const known = LANGUAGE_NAMES[base(id)];
  return known ? known.name : id;
}

/**
 * The engine and language a dictation runs with.
 *
 * - An engine that can't run on this phone (missing from this build, or an
 *   older iOS) gives way to the first that can.
 * - An engine that can run but isn't ready, as a model not yet downloaded,
 *   is reported rather than swapped: the user picked it on purpose.
 * - A language the engine doesn't take is reported rather than transcribed
 *   in another, which would only give wrong words.
 */
export function planDictation(
  preference: Pick<DictationPreference, "engine" | "language">,
  engines: readonly EngineInfo[],
  deviceLocale: string,
): DictationPlan {
  const picked = engines.find((engine) => engine.id === preference.engine);
  const engine = picked?.available
    ? picked
    : engines.find((candidate) => candidate.available);
  if (!engine) {
    return {
      ok: false,
      problem: {
        kind: "noEngine",
        title: "Dictation unavailable",
        message:
          picked?.unavailableReason ??
          engines.find((candidate) => candidate.unavailableReason)
            ?.unavailableReason ??
          "Dictation isn't available on this device.",
      },
    };
  }
  const fellBackFrom = engine.id === preference.engine ? undefined : preference.engine;
  const ok = (language: string | null): DictationPlan => ({
    ok: true,
    engine: engine.id,
    language,
    fellBackFrom,
    variant: engine.variant,
  });
  if (!engine.ready) {
    return {
      ok: false,
      problem: {
        kind: "notReady",
        title: `${engine.label} isn't ready`,
        message:
          engine.notReadyReason ?? `Set up ${engine.label} in Settings → Dictation.`,
      },
    };
  }

  const ids = engine.languages.map((language) => language.id);
  if (preference.language === null) {
    if (engine.automatic) return ok(null);
    // The phone's language, as dictation has always used.
    if (ids.length === 0) {
      return ok(deviceLocale);
    }
    const language = matchLanguage(deviceLocale, ids);
    if (language) return ok(language);
    return unsupported(engine, deviceLocale);
  }

  if (ids.length === 0) {
    return ok(preference.language);
  }
  const language = matchLanguage(preference.language, ids);
  if (language) return ok(language);
  return unsupported(engine, preference.language);
}

function unsupported(engine: EngineInfo, language: string): DictationPlan {
  return {
    ok: false,
    problem: {
      kind: "unsupportedLanguage",
      title: "Language unavailable",
      message: `${languageName(language)} isn't available for ${engine.label} on this device. Choose another language or engine in Settings → Dictation.`,
    },
  };
}

/** Names for languages an engine lists only by code. */
export const LANGUAGE_NAMES: Record<string, { name: string; nativeName: string }> = {
  bg: { name: "Bulgarian", nativeName: "български" },
  cs: { name: "Czech", nativeName: "čeština" },
  da: { name: "Danish", nativeName: "dansk" },
  de: { name: "German", nativeName: "Deutsch" },
  el: { name: "Greek", nativeName: "Ελληνικά" },
  en: { name: "English", nativeName: "English" },
  es: { name: "Spanish", nativeName: "español" },
  et: { name: "Estonian", nativeName: "eesti" },
  fi: { name: "Finnish", nativeName: "suomi" },
  fr: { name: "French", nativeName: "français" },
  hr: { name: "Croatian", nativeName: "hrvatski" },
  hu: { name: "Hungarian", nativeName: "magyar" },
  it: { name: "Italian", nativeName: "italiano" },
  lt: { name: "Lithuanian", nativeName: "lietuvių" },
  lv: { name: "Latvian", nativeName: "latviešu" },
  mt: { name: "Maltese", nativeName: "Malti" },
  nl: { name: "Dutch", nativeName: "Nederlands" },
  pl: { name: "Polish", nativeName: "polski" },
  pt: { name: "Portuguese", nativeName: "português" },
  ro: { name: "Romanian", nativeName: "română" },
  ru: { name: "Russian", nativeName: "русский" },
  sk: { name: "Slovak", nativeName: "slovenčina" },
  sl: { name: "Slovenian", nativeName: "slovenščina" },
  sv: { name: "Swedish", nativeName: "svenska" },
  uk: { name: "Ukrainian", nativeName: "українська" },
};
