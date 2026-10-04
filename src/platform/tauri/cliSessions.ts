import { invoke } from "@tauri-apps/api/core";
import type { HarnessId } from "../../features/sessions/model/session";

export type CliHarness = Extract<HarnessId, "claude" | "codex" | "grok">;

/** A conversation a provider CLI recorded on disk, outside MonoCode. */
export type CliSession = {
  harness: CliHarness;
  providerSessionId: string;
  cwd: string;
  title: string;
  model?: string;
  /** Epoch ms. */
  createdAt: number;
  updatedAt: number;
  path: string;
};

export type CliToolEntry = {
  kind: "tool";
  id: string;
  /** The provider's tool name, for example `Bash` or `exec_command`. */
  name: string;
  title?: string;
  /** Activity kind such as `execute`, `edit` or `search`. */
  toolKind?: string;
  input?: unknown;
  command?: string;
  paths?: string[];
  output?: string;
  failed: boolean;
  at?: number;
};

/** One step of a CLI transcript, in order. Mirrors Rust `cli_sessions::Entry`. */
export type CliEntry =
  | { kind: "user" | "assistant" | "reasoning"; text: string; at?: number }
  | CliToolEntry;

/** CLI sessions recorded for `cwd` that MonoCode has no row for yet. */
export function listCliSessions(cwd: string): Promise<CliSession[]> {
  return invoke<CliSession[]>("cli_sessions_list", { cwd });
}

export function readCliSession(
  session: Pick<CliSession, "harness" | "path">,
): Promise<CliEntry[]> {
  return invoke<CliEntry[]>("cli_session_read", {
    harness: session.harness,
    path: session.path,
  });
}
