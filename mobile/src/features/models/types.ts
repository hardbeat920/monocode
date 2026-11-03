import type { Harness, RuntimeMode } from "@/features/workspace/types";

export type { RuntimeMode };

// The desktop's model shapes (src/features/sessions/model/models.ts), as its
// `models.list` sends them for the model sheet.

export type ModelSettingChoice = { value: string; label: string };

/** A model's own setting: effort, fast mode, thinking and the like. */
export type ModelSetting = {
  id: string;
  label: string;
  kind: "select" | "toggle";
  /** The default. */
  value: string;
  options: ModelSettingChoice[];
  description?: string;
};

export type CatalogModel = {
  id: string;
  harness: Harness;
  name: string;
  /** Who serves it, for providers that front several. */
  provider?: string;
  settings: ModelSetting[];
};

/** What a session runs with: the model sheet's part and the access sheet's. */
export type ModelChoice = {
  harness: Harness;
  model: string;
  modelSettings: Record<string, string>;
  runtimeMode: RuntimeMode;
  providerAccountId?: string;
};

/** The model sheet's part of a choice. */
export type ModelPick = Omit<ModelChoice, "runtimeMode" | "providerAccountId">;

export type ModelAccount = {
  id: string;
  provider: "claude" | "codex";
  label: string;
  isDefault?: boolean;
};

export type ModelCatalog = {
  harnesses: { id: Harness; label: string }[];
  models: CatalogModel[];
  runtimeModes: { id: RuntimeMode; label: string; hint: string }[];
  /** Missing on desktops that predate mobile account switching. */
  accounts?: ModelAccount[];
  selectedAccounts?: Partial<Record<Harness, string>>;
  /** What the conversation, or a new session in the project, runs with now. */
  current: ModelChoice;
};
