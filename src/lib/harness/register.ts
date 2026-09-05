import { ensureClaudeRegistered } from "./claudeAdapter";
import { ensureCodexRegistered } from "./codexAdapter";
import { ensureCursorRegistered } from "./cursorAdapter";
import { ensureFxRegistered } from "./fxAdapter";
import { ensureGrokRegistered } from "./grokAdapter";
import {
  ensureOpenCodeProviderRegistered,
  ensureOpenCodeRegistered,
} from "./opencodeAdapter";
import { ensureOmpRegistered } from "./ompAdapter";
import { ensurePiRegistered } from "./piAdapter";

/** Register all known live harness adapters. Idempotent. */
export function registerBuiltinHarnesses(): void {
  ensureClaudeRegistered();
  ensureCursorRegistered();
  ensureCodexRegistered();
  ensureGrokRegistered();
  ensureOpenCodeRegistered();
  ensureOpenCodeProviderRegistered("zai");
  ensureOpenCodeProviderRegistered("mimo");
  ensureOpenCodeProviderRegistered("openrouter");
  ensureOpenCodeProviderRegistered("nvidia");
  ensurePiRegistered();
  ensureOmpRegistered();
  ensureFxRegistered();
}
