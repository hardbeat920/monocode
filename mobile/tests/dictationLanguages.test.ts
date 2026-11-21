import { describe, expect, it } from "vitest";

import {
  matchLanguage,
  planDictation,
  type EngineInfo,
} from "@/features/sessions/dictationLanguages";

const languages = (...ids: string[]) => ids.map((id) => ({ id, name: id }));

const apple: EngineInfo = {
  id: "apple",
  label: "Apple Speech",
  available: true,
  ready: true,
  automatic: false,
  languages: languages("en-GB", "en-US", "ru-RU", "uk-UA", "de-DE", "de-AT"),
};

const parakeet: EngineInfo = {
  id: "parakeet",
  label: "NVIDIA Parakeet",
  available: true,
  ready: true,
  automatic: true,
  languages: languages("en", "ru", "uk", "de"),
};

describe("matchLanguage", () => {
  it("prefers the exact language, ignoring case and separators", () => {
    expect(matchLanguage("ru-RU", ["en-US", "ru-RU"])).toBe("ru-RU");
    expect(matchLanguage("en_us", ["en-GB", "en-US"])).toBe("en-US");
  });

  it("takes a bare code for every region of its language", () => {
    expect(matchLanguage("ru-RU", ["en", "ru"])).toBe("ru");
    expect(matchLanguage("uk-UA", ["en", "ru", "uk"])).toBe("uk");
  });

  it("takes the language in its own country for a bare request", () => {
    expect(matchLanguage("de", ["de-AT", "de-DE"])).toBe("de-DE");
    expect(matchLanguage("ru", ["ru-RU"])).toBe("ru-RU");
  });

  it("falls back to the language in another country", () => {
    expect(matchLanguage("en-RU", ["en-GB", "en-US"])).toBe("en-GB");
  });

  it("keeps the requested script before another region", () => {
    expect(matchLanguage("sr-Latn-BA", ["sr-Cyrl-RS", "sr-Latn-RS"])).toBe("sr-Latn-RS");
    expect(matchLanguage("zh-Hant-HK", ["zh-Hans-CN", "zh-Hant-TW"])).toBe("zh-Hant-TW");
  });

  it("prefers the requested region among the same language", () => {
    expect(matchLanguage("zh-Hant-TW", ["zh-CN", "zh-HK", "zh-TW"])).toBe("zh-TW");
    expect(matchLanguage("pt-PT", ["pt-BR", "pt-PT"])).toBe("pt-PT");
  });

  it("doesn't stand one language in for another", () => {
    expect(matchLanguage("ru-RU", ["en-US", "uk-UA"])).toBeNull();
  });
});

describe("planDictation", () => {
  it("transcribes Russian on a phone set to English", () => {
    expect(planDictation({ engine: "apple", language: "ru-RU" }, [apple], "en-US")).toEqual({
      ok: true,
      engine: "apple",
      language: "ru-RU",
      fellBackFrom: undefined,
    });
  });

  it("keeps the phone's language for Automatic on an engine that can't detect", () => {
    const plan = planDictation({ engine: "apple", language: null }, [apple], "en-US");
    expect(plan).toMatchObject({ ok: true, engine: "apple", language: "en-US" });
  });

  it("matches the phone's language to one the engine takes", () => {
    // An English phone set to a Russian region reports "en-RU".
    const plan = planDictation({ engine: "apple", language: null }, [apple], "en-RU");
    expect(plan).toMatchObject({ ok: true, language: "en-GB" });
  });

  it("lets an engine with detection find the language itself", () => {
    const plan = planDictation({ engine: "parakeet", language: null }, [apple, parakeet], "en-US");
    expect(plan).toMatchObject({ ok: true, engine: "parakeet", language: null });
  });

  it("gives a detecting engine the picked language as a hint", () => {
    const plan = planDictation({ engine: "parakeet", language: "uk-UA" }, [apple, parakeet], "en-US");
    expect(plan).toMatchObject({ ok: true, engine: "parakeet", language: "uk" });
  });

  it("reports a language the engine doesn't take rather than using another", () => {
    const plan = planDictation({ engine: "apple", language: "pl-PL" }, [apple], "en-US");
    expect(plan.ok).toBe(false);
    if (plan.ok) return;
    expect(plan.problem.kind).toBe("unsupportedLanguage");
    expect(plan.problem.message).toContain("Polish");
    expect(plan.problem.message).toContain("Apple Speech");
  });

  it("leaves the language to the engine when it can't list them", () => {
    const unlisted = { ...apple, languages: [] };
    expect(planDictation({ engine: "apple", language: "ru-RU" }, [unlisted], "en-US"))
      .toMatchObject({ ok: true, language: "ru-RU" });
    expect(planDictation({ engine: "apple", language: null }, [unlisted], "en-US"))
      .toMatchObject({ ok: true, language: "en-US" });
  });

  it("falls back to an engine that can run when the picked one can't", () => {
    const missing = { ...parakeet, available: false, unavailableReason: "Not in this build." };
    const plan = planDictation({ engine: "parakeet", language: "ru-RU" }, [apple, missing], "en-US");
    expect(plan).toEqual({ ok: true, engine: "apple", language: "ru-RU", fellBackFrom: "parakeet" });
  });

  it("asks for an engine to be set up rather than swapping it", () => {
    const notDownloaded = {
      ...parakeet,
      ready: false,
      notReadyReason: "Download the model in Settings → Dictation.",
    };
    const plan = planDictation({ engine: "parakeet", language: null }, [apple, notDownloaded], "en-US");
    expect(plan).toEqual({
      ok: false,
      problem: {
        kind: "notReady",
        title: "NVIDIA Parakeet isn't ready",
        message: "Download the model in Settings → Dictation.",
      },
    });
  });

  it("explains when no engine can run", () => {
    const old = { ...apple, available: false, unavailableReason: "Apple Speech needs iOS 26 or later." };
    const plan = planDictation({ engine: "apple", language: "ru-RU" }, [old], "en-US");
    expect(plan).toEqual({
      ok: false,
      problem: {
        kind: "noEngine",
        title: "Dictation unavailable",
        message: "Apple Speech needs iOS 26 or later.",
      },
    });
  });
});
