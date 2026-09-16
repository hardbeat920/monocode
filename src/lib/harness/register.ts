import { ensureClaudeRegistered } from "./claudeAdapter";
import { ensureCodexRegistered } from "./codexAdapter";
import { ensureCursorRegistered } from "./cursorAdapter";
import { ensureFxRegistered } from "./fxAdapter";
import { ensureGrokRegistered } from "./grokAdapter";
import { ensureOpenCodeRegistered } from "./opencodeAdapter";
import { ensureOmpRegistered } from "./ompAdapter";
import { ensurePiRegistered } from "./piAdapter";
import { ensureKimiRegistered } from "./kimiAdapter";
import { ensureAntigravityRegistered } from "./antigravityAdapter";

/** Register all known live harness adapters. Idempotent. */
export function registerBuiltinHarnesses(): void {
  ensureClaudeRegistered();
  ensureCursorRegistered();
  ensureCodexRegistered();
  ensureGrokRegistered();
  ensureOpenCodeRegistered();
  ensurePiRegistered();
  ensureOmpRegistered();
  ensureFxRegistered();
  ensureKimiRegistered();
  ensureAntigravityRegistered();
}
