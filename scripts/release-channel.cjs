const fs = require("node:fs");

function releaseChannel(tag) {
  const match = /^v\d+\.\d+\.\d+(-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/.exec(tag || "");
  if (!match) throw new Error("Expected a release tag such as v0.9.1 or v0.9.1-beta.1");
  const prerelease = Boolean(match[1]);
  return { prerelease, feedKey: prerelease ? "beta/latest.json" : "latest.json" };
}

function updaterEndpoint(env) {
  const { prerelease, feedKey } = releaseChannel(env.GITHUB_REF_NAME);
  const base = (env.R2_PUBLIC_BASE_URL || "").trim().replace(/\/$/, "");
  const configured = (env.TAURI_UPDATER_ENDPOINT || "").trim();
  // Beta builds must never retain a configured production endpoint.
  const endpoint = !prerelease && configured ? configured : base ? `${base}/${feedKey}` : "";
  if (!endpoint) throw new Error("R2_PUBLIC_BASE_URL is required for beta updates; stable builds may use TAURI_UPDATER_ENDPOINT");
  if (new URL(endpoint).protocol !== "https:") throw new Error("Release updater endpoints must use HTTPS");
  return endpoint;
}

module.exports = { releaseChannel, updaterEndpoint };

if (require.main === module) {
  const { prerelease, feedKey } = releaseChannel(process.env.GITHUB_REF_NAME);
  if (!process.env.GITHUB_ENV) throw new Error("GITHUB_ENV is required");
  fs.appendFileSync(process.env.GITHUB_ENV, `RELEASE_PRERELEASE=${prerelease}\nUPDATER_FEED_KEY=${feedKey}\n`);
  console.log(`Release channel: ${prerelease ? "beta" : "stable"} (${feedKey})`);
}
