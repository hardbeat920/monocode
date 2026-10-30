import type { ConfigContext, ExpoConfig } from "expo/config";

/**
 * Builds under another Apple team: MONOCODE_IOS_BUNDLE_ID replaces app.json's
 * ID, which that team can't share (the widget follows as
 * "<id>.live-activity"), and MONOCODE_APPLE_TEAM_ID signs both targets.
 */
export default ({ config }: ConfigContext): ExpoConfig => {
  const bundleIdentifier = process.env.MONOCODE_IOS_BUNDLE_ID?.trim();
  const appleTeamId = process.env.MONOCODE_APPLE_TEAM_ID?.trim();
  return {
    ...config,
    ios: {
      ...config.ios,
      ...(bundleIdentifier ? { bundleIdentifier } : {}),
      ...(appleTeamId ? { appleTeamId } : {}),
    },
  } as ExpoConfig;
};
