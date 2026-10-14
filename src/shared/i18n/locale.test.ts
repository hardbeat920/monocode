import { describe, expect, it, vi } from "vitest";
import { en, tr } from "./catalog";
import * as app from "./catalog/app";
import * as automationsOrchestration from "./catalog/automations-orchestration";
import * as host from "./catalog/host";
import * as inbox from "./catalog/inbox";
import * as integrations from "./catalog/integrations";
import * as miscFiles from "./catalog/misc-files";
import * as miscProviders from "./catalog/misc-providers";
import * as monos from "./catalog/monos";
import * as scm from "./catalog/scm";
import * as sessionsModel from "./catalog/sessions-model";
import * as sessionsUi from "./catalog/sessions-ui";
import * as settings from "./catalog/settings";
import * as sharedPlatform from "./catalog/shared-platform";
import * as workspaceProjects from "./catalog/workspace-projects";
import {
  LOCALE_PREFERENCE_KEY,
  formatMessage,
  resolveSystemLocale,
  setLocalePreference,
  subscribeLocale,
} from "./locale";

const domains = [
  app,
  automationsOrchestration,
  host,
  inbox,
  integrations,
  miscFiles,
  miscProviders,
  monos,
  scm,
  sessionsModel,
  sessionsUi,
  settings,
  sharedPlatform,
  workspaceProjects,
];

type Entry = string | { one: string; other: string };

function placeholdersOf(entry: Entry): string[] {
  const text = typeof entry === "string" ? entry : `${entry.one} ${entry.other}`;
  return [...new Set([...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1]!))].sort();
}

describe("i18n catalogs", () => {
  it("defines every key in exactly one domain file", () => {
    const domainKeyCount = domains.reduce(
      (total, domain) => total + Object.keys(domain.en).length,
      0,
    );
    expect(Object.keys(en)).toHaveLength(domainKeyCount);
  });

  it("gives Turkish the same keys as English", () => {
    expect(Object.keys(tr).sort()).toEqual(Object.keys(en).sort());
  });

  it("uses the same placeholders in both locales", () => {
    for (const key of Object.keys(en) as (keyof typeof en)[]) {
      expect(placeholdersOf(tr[key] as Entry), key).toEqual(placeholdersOf(en[key] as Entry));
    }
  });

  it("has no empty messages", () => {
    for (const locale of [en, tr] as Record<string, Entry>[]) {
      for (const [key, entry] of Object.entries(locale)) {
        const values = typeof entry === "string" ? [entry] : [entry.one, entry.other];
        for (const value of values) expect(value.trim(), key).not.toBe("");
      }
    }
  });

  it("keeps plural entries as one/other pairs", () => {
    for (const [key, entry] of Object.entries(en)) {
      if (typeof entry === "string") continue;
      expect(Object.keys(entry).sort(), key).toEqual(["one", "other"]);
    }
  });
});

describe("resolveSystemLocale", () => {
  it("uses the first supported language in the OS preference order", () => {
    expect(resolveSystemLocale(["tr-TR", "en-US"])).toBe("tr");
    expect(resolveSystemLocale(["fr-FR", "en-GB"])).toBe("en");
    expect(resolveSystemLocale(["de-DE", "tr"])).toBe("tr");
  });

  it("falls back to English for unsupported or missing languages", () => {
    expect(resolveSystemLocale(["de-DE"])).toBe("en");
    expect(resolveSystemLocale([])).toBe("en");
  });
});

describe("formatMessage", () => {
  it("fills named placeholders and leaves unknown ones visible", () => {
    expect(formatMessage("Hello {name}, {missing}", "en", { name: "Ayşe" })).toBe(
      "Hello Ayşe, {missing}",
    );
  });

  it("picks the plural form from count", () => {
    const entry = { one: "{count} file", other: "{count} files" };
    expect(formatMessage(entry, "en", { count: 1 })).toBe("1 file");
    expect(formatMessage(entry, "en", { count: 3 })).toBe("3 files");
  });

  it("uses the Turkish plural rule, which keeps the noun singular after a number", () => {
    const entry = { one: "{count} dosya", other: "{count} dosya" };
    expect(formatMessage(entry, "tr", { count: 3 })).toBe("3 dosya");
  });
});

describe("setLocalePreference", () => {
  it("persists the choice and notifies subscribers", () => {
    const store = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => store.set(key, value),
    });
    const listener = vi.fn();
    const unsubscribe = subscribeLocale(listener);

    setLocalePreference("tr");
    expect(store.get(LOCALE_PREFERENCE_KEY)).toBe("tr");
    expect(listener).toHaveBeenCalledTimes(1);

    unsubscribe();
    setLocalePreference("en");
    expect(listener).toHaveBeenCalledTimes(1);
    vi.unstubAllGlobals();
  });
});
