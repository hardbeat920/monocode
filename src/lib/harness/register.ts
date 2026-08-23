import { ensureClaudeRegistered } from "./claudeAdapter";
import { ensureAmpRegistered } from "./ampAdapter";
import { ensureCodexRegistered } from "./codexAdapter";
import { ensureCursorRegistered } from "./cursorAdapter";
import { ensureFxRegistered } from "./fxAdapter";
import { ensureOpenCodeRegistered } from "./opencodeAdapter";
import { ensurePiRegistered } from "./piAdapter";

/** Register all known live harness adapters. Idempotent. */
export function registerBuiltinHarnesses(): void {
  ensureClaudeRegistered();
  ensureCursorRegistered();
  ensureCodexRegistered();
  ensureOpenCodeRegistered();
  ensurePiRegistered();
  ensureFxRegistered();
  ensureAmpRegistered();
}
