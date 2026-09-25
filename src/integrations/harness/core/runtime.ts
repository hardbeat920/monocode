import type { HarnessId } from "../../../features/sessions/model/session";
import { resolveBinaryOverride } from "./child";
import {
  loadHarnessRuntime,
  type HarnessRuntimeSettings,
} from "../../../features/settings/model/settings";

/** Drops a key like `KEY=value` typed into the key field instead of sending
 * the child a broken assignment. */
const VALID_ENV_KEY = /^[A-Za-z_][A-Za-z0-9_]*$/;

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
 * contains spaces (`--config "my file.json"` -> ["--config", "my file.json"],
 * `--config="my file.json"` -> ["--config=my file.json"]).
 * No escaping, nesting, or unmatched-quote recovery — good enough for the
 * flags CLIs actually take, not a full shell parser. */
export function harnessRuntimeExtraArgs(
  runtime: HarnessRuntimeSettings,
): string[] {
  const trimmed = runtime.launchArgs.trim();
  if (!trimmed) return [];
  const tokens = trimmed.match(/(?:"[^"]*"|'[^']*'|\S)+/g) ?? [];
  return tokens.map((token) =>
    token.replace(
      /"([^"]*)"|'([^']*)'/g,
      (_, double: string | undefined, single: string | undefined) =>
        double ?? single ?? "",
    ),
  );
}

export function harnessRuntimeBinaryPath(runtime: HarnessRuntimeSettings): string {
  return runtime.binaryPath.trim();
}

/** Still calls `resolveDefault` under an override so fields it supplies
 * beyond the path, like Antigravity's launch args, survive. A failing default
 * is expected here, since a missing default install is the usual reason to
 * set an override. */
export async function resolveHarnessBinary<T extends { path: string }>(
  harness: HarnessId,
  resolveDefault: () => Promise<T>,
): Promise<{ path: string } & Partial<T>> {
  const override = harnessRuntimeBinaryPath(loadHarnessRuntime(harness));
  if (!override) return resolveDefault();
  const path = await resolveBinaryOverride(override);
  try {
    const resolved = await resolveDefault();
    return { ...resolved, path };
  } catch {
    return { path } as { path: string } & Partial<T>;
  }
}
