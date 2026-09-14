import type { ModelSettingChoice } from "../models";

/**
 * Per-account Claude Code identities, mirroring the `claude`/`claudo`/`claudz`
 * shell functions in ~/.zshrc (each sets CLAUDE_CONFIG_DIR to a different
 * account's config dir — plain `claude` uses whatever is already the default,
 * no override). MonoCode has no notion of this on its own — it always
 * resolves and spawns a single `claude` binary — so this is threaded through
 * explicitly from the model-settings picker down to `harness_spawn`.
 */
export type ClaudeProfileId = "personal" | "nishanth" | "benitto";

export type ClaudeProfile = {
  id: ClaudeProfileId;
  label: string;
  /** Relative to $HOME, matching the zshrc CLAUDE_CONFIG_DIR values.
   * `undefined` means "no override" — the plain `claude` default account. */
  configDirName?: string;
};

export const CLAUDE_PROFILES: ClaudeProfile[] = [
  { id: "personal", label: "Personal" },
  { id: "nishanth", label: "Nishanth", configDirName: ".claude-personal" },
  { id: "benitto", label: "Benitto", configDirName: ".claude-office2" },
];

export const CLAUDE_PROFILE_DEFAULT: ClaudeProfileId = "personal";

export const CLAUDE_PROFILE_OPTIONS: ModelSettingChoice[] = CLAUDE_PROFILES.map(
  (profile) => ({ value: profile.id, label: profile.label }),
);

/**
 * Folder-based guardrail: which profiles a project's cwd is allowed to
 * launch under, independent of what the picker has selected. All three
 * profiles are currently unrestricted everywhere — add a rule here if a
 * folder should ever be limited to a subset again.
 */
function allowedProfileIds(_cwd: string): ClaudeProfileId[] {
  return ["personal", "nishanth", "benitto"];
}

export function claudeProfileOptionsFor(cwd: string): ModelSettingChoice[] {
  const allowed = new Set(allowedProfileIds(cwd));
  return CLAUDE_PROFILE_OPTIONS.filter((option) => allowed.has(option.value as ClaudeProfileId));
}

/**
 * Resolves the requested profile against the folder allow-list for `cwd`
 * and returns the env to launch `claude` with. A disallowed request is
 * silently corrected to the folder's first allowed profile rather than
 * launching under the wrong account — callers should surface `warning` to
 * the user when present. A profile with no `configDirName` (Personal) means
 * no CLAUDE_CONFIG_DIR override at all — the plain default account.
 */
export function resolveClaudeProfileEnv(
  requestedId: string | undefined,
  cwd: string,
  home: string,
): { env: Record<string, string>; profileId: ClaudeProfileId; warning?: string } {
  const allowed = allowedProfileIds(cwd);
  const requested = (requestedId ?? CLAUDE_PROFILE_DEFAULT) as ClaudeProfileId;
  const profileId = allowed.includes(requested) ? requested : allowed[0];
  const profile =
    CLAUDE_PROFILES.find((candidate) => candidate.id === profileId) ?? CLAUDE_PROFILES[0];
  const env: Record<string, string> = profile.configDirName
    ? { CLAUDE_CONFIG_DIR: `${home.replace(/\/+$/, "")}/${profile.configDirName}` }
    : {};
  if (requestedId && profileId !== requestedId) {
    return {
      env,
      profileId,
      warning: `"${requestedId}" isn't allowed for this project — using "${profileId}" instead.`,
    };
  }
  return { env, profileId };
}
