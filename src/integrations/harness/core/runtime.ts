import type { HarnessId } from "../../../features/sessions/model/session";
import {
  loadHarnessRuntime,
  type HarnessRuntimeSettings,
} from "../../../features/settings/model/settings";

/** POSIX env var names: a leading letter/underscore, then letters, digits, or
 * underscores. A key that doesn't match this can't be a real env var — most
 * likely the user typed `KEY=value` into the key field by mistake — so it's
 * dropped rather than sent to the child process as a broken assignment. */
const VALID_ENV_KEY = /^[A-Za-z_][A-Za-z0-9_]*$/;

/** Env vars from a runtime override, or undefined when there are none —
 * matches `spawnChild`'s optional `env` param so callers can pass this
 * straight through without an extra empty-object check. */
export function harnessRuntimeEnv(
  runtime: HarnessRuntimeSettings,
): Record<string, string> | undefined {
  const env: Record<string, string> = {};
  for (const { key, value } of runtime.env) {
    const trimmedKey = key.trim();
    if (VALID_ENV_KEY.test(trimmedKey)) env[trimmedKey] = value;
  }
  return Object.keys(env).length > 0 ? env : undefined;
}

/** Splits a launch-args string the way a shell would for simple cases:
 * whitespace-separated, with single or double quotes grouping a value that
 * contains spaces (`--config "my file.json"` -> ["--config", "my file.json"]).
 * No escaping, nesting, or unmatched-quote recovery — good enough for the
 * flags CLIs actually take, not a full shell parser. */
export function harnessRuntimeExtraArgs(
  runtime: HarnessRuntimeSettings,
): string[] {
  const trimmed = runtime.launchArgs.trim();
  if (!trimmed) return [];
  const args: string[] = [];
  const pattern = /"([^"]*)"|'([^']*)'|(\S+)/g;
  let match: RegExpExecArray | null;
  while ((match = pattern.exec(trimmed))) {
    args.push(match[1] ?? match[2] ?? match[3] ?? "");
  }
  return args;
}

/** The binary override, or empty string when the harness should resolve its
 * default from PATH. Kept as a tiny wrapper so every provider trims the same
 * way instead of repeating `.trim()` at each call site. */
export function harnessRuntimeBinaryPath(runtime: HarnessRuntimeSettings): string {
  return runtime.binaryPath.trim();
}

/** Resolves a harness's binary, honoring a runtime binary-path override.
 * The override always wins on `path`, but this still calls `resolveDefault`
 * first so fields the default resolver supplies beyond the path — like
 * Antigravity's required launch args — aren't lost just because the user
 * pointed at a different binary. Only when the default can't resolve at all
 * (the usual reason to set an override) does this fall back to the override
 * alone, without those extra fields.
 *
 * Every provider's interactive session *and* its isolated helpers (catalog
 * probes, title/commit-message generation) should resolve through this, not
 * through `resolveXBinary()` directly — that was the gap that let the
 * override half-apply in earlier versions of this feature. */
export async function resolveHarnessBinary<T extends { path: string }>(
  harness: HarnessId,
  resolveDefault: () => Promise<T>,
): Promise<{ path: string } & Partial<T>> {
  const override = harnessRuntimeBinaryPath(loadHarnessRuntime(harness));
  if (!override) return resolveDefault();
  try {
    const resolved = await resolveDefault();
    return { ...resolved, path: override };
  } catch {
    return { path: override } as { path: string } & Partial<T>;
  }
}
