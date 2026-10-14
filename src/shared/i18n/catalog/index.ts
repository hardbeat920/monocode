import * as app from "./app";
import * as automationsOrchestration from "./automations-orchestration";
import * as host from "./host";
import * as inbox from "./inbox";
import * as integrations from "./integrations";
import * as miscFiles from "./misc-files";
import * as miscProviders from "./misc-providers";
import * as monos from "./monos";
import * as scm from "./scm";
import * as sessionsModel from "./sessions-model";
import * as sessionsUi from "./sessions-ui";
import * as settings from "./settings";
import * as sharedPlatform from "./shared-platform";
import * as workspaceProjects from "./workspace-projects";
import type { Translations } from "./types";

export type { MessageValue, Translations } from "./types";

/**
 * Every key lives in exactly one domain file. Keys are namespaced by their
 * domain (`inbox.*`, `settings.*`), and a test fails if two files define the
 * same key, since the spreads below would otherwise silently override one.
 */
export const en = {
  ...app.en,
  ...automationsOrchestration.en,
  ...host.en,
  ...inbox.en,
  ...integrations.en,
  ...miscFiles.en,
  ...miscProviders.en,
  ...monos.en,
  ...scm.en,
  ...sessionsModel.en,
  ...sessionsUi.en,
  ...settings.en,
  ...sharedPlatform.en,
  ...workspaceProjects.en,
} as const;

export const tr: Translations<typeof en> = {
  ...app.tr,
  ...automationsOrchestration.tr,
  ...host.tr,
  ...inbox.tr,
  ...integrations.tr,
  ...miscFiles.tr,
  ...miscProviders.tr,
  ...monos.tr,
  ...scm.tr,
  ...sessionsModel.tr,
  ...sessionsUi.tr,
  ...settings.tr,
  ...sharedPlatform.tr,
  ...workspaceProjects.tr,
};

export type MessageKey = keyof typeof en;

export type PluralKey = {
  [K in MessageKey]: (typeof en)[K] extends string ? never : K;
}[MessageKey];

export type TextKey = Exclude<MessageKey, PluralKey>;
