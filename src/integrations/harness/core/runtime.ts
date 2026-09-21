import type { HarnessRuntimeSettings } from "../../../features/settings/model/settings";

/** Env vars from a runtime override, or undefined when there are none —
 * matches `spawnChild`'s optional `env` param so callers can pass this
 * straight through without an extra empty-object check. */
export function harnessRuntimeEnv(
  runtime: HarnessRuntimeSettings,
): Record<string, string> | undefined {
  const env: Record<string, string> = {};
  for (const { key, value } of runtime.env) {
    const trimmedKey = key.trim();
    if (trimmedKey) env[trimmedKey] = value;
  }
  return Object.keys(env).length > 0 ? env : undefined;
}

/** Extra CLI args from a runtime override, split on whitespace. No quoting
 * support, so a value with spaces inside it can't be passed as one arg. */
export function harnessRuntimeExtraArgs(
  runtime: HarnessRuntimeSettings,
): string[] {
  const trimmed = runtime.launchArgs.trim();
  return trimmed ? trimmed.split(/\s+/) : [];
}

/** The binary override, or empty string when the harness should resolve its
 * default from PATH. Kept as a tiny wrapper so every provider trims the same
 * way instead of repeating `.trim()` at each call site. */
export function harnessRuntimeBinaryPath(runtime: HarnessRuntimeSettings): string {
  return runtime.binaryPath.trim();
}
