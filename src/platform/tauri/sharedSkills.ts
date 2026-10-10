import { invoke } from "@tauri-apps/api/core";

export type SharedSkillExportState =
  "exported" | "pending" | "conflict" | "disabled" | "unsupported";

export type SharedSkillExportStatus = {
  targetKey: string;
  providers: string[];
  path: string;
  state: SharedSkillExportState;
  detail: string;
};

export type SharedSkillEntry = {
  id: string;
  name: string;
  description: string;
  digest: string;
  revision: number;
  shared: boolean;
  sourcePath: string;
  previewPath: string;
  origins: string[];
  statuses: SharedSkillExportStatus[];
  warnings: string[];
};

export type SharedSkillTarget = {
  key: string;
  root: string;
  providers: string[];
};

export type SharedSkillsSnapshot = {
  generation: number;
  entries: SharedSkillEntry[];
  targets: SharedSkillTarget[];
};

// The library belongs to this machine, even when the open project is remote.
export function sharedSkillsSnapshot(): Promise<SharedSkillsSnapshot> {
  return invoke("shared_skills_snapshot");
}

export function importSharedSkill(path: string): Promise<SharedSkillsSnapshot> {
  return invoke("shared_skills_import", { path });
}

export function applySharedSkill(id: string): Promise<SharedSkillsSnapshot> {
  return invoke("shared_skills_apply", { id });
}

export function setSharedSkillSharing(
  id: string,
  shared: boolean,
): Promise<SharedSkillsSnapshot> {
  return invoke("shared_skills_set_shared", { id, shared });
}

export function repairSharedSkills(): Promise<SharedSkillsSnapshot> {
  return invoke("shared_skills_repair");
}
