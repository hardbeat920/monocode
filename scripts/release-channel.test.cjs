const assert = require("node:assert/strict");
const { test } = require("node:test");
const { execFileSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { releaseChannel, updaterEndpoint } = require("./release-channel.cjs");

test("stable tags, including hyphens in build metadata, use the stable feed", () => {
  for (const tag of ["v0.9.1", "v0.9.1+build-beta"]) {
    assert.deepEqual(releaseChannel(tag), { prerelease: false, feedKey: "latest.json" });
  }
});

test("prerelease tags use only the beta feed", () => {
  for (const tag of ["v0.9.1-beta", "v0.9.1-beta.1+build"]) {
    assert.deepEqual(releaseChannel(tag), { prerelease: true, feedKey: "beta/latest.json" });
  }
});

test("non-release refs are rejected", () => {
  for (const tag of ["main", "vnext", "", "v0.9.1/beta"]) {
    assert.throws(() => releaseChannel(tag), /Expected a release tag/);
  }
});

test("stable builds preserve the configured endpoint", () => {
  assert.equal(updaterEndpoint({
    GITHUB_REF_NAME: "v0.9.1",
    TAURI_UPDATER_ENDPOINT: "https://updates.example.com/stable.json",
    R2_PUBLIC_BASE_URL: "https://cdn.example.com",
  }), "https://updates.example.com/stable.json");
});

test("stable builds can use the R2 default", () => {
  assert.equal(updaterEndpoint({
    GITHUB_REF_NAME: "v0.9.1",
    R2_PUBLIC_BASE_URL: "https://cdn.example.com/",
  }), "https://cdn.example.com/latest.json");
});

test("beta builds override a configured production endpoint", () => {
  assert.equal(updaterEndpoint({
    GITHUB_REF_NAME: "v0.9.1-beta.1",
    TAURI_UPDATER_ENDPOINT: "https://updates.example.com/stable.json",
    R2_PUBLIC_BASE_URL: "https://cdn.example.com/",
  }), "https://cdn.example.com/beta/latest.json");
});

test("beta builds require an isolated R2 endpoint and HTTPS", () => {
  assert.throws(() => updaterEndpoint({
    GITHUB_REF_NAME: "v0.9.1-beta.1",
    TAURI_UPDATER_ENDPOINT: "https://updates.example.com/stable.json",
  }), /R2_PUBLIC_BASE_URL is required/);
  assert.throws(() => updaterEndpoint({
    GITHUB_REF_NAME: "v0.9.1-beta.1",
    R2_PUBLIC_BASE_URL: "http://cdn.example.com",
  }), /must use HTTPS/);
});

test("the CLI exports the same channel used for feed publication", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "monocode-release-channel-"));
  try {
    const envFile = path.join(dir, "github-env");
    execFileSync(process.execPath, [path.join(__dirname, "release-channel.cjs")], {
      env: { ...process.env, GITHUB_REF_NAME: "v0.9.1-beta.1", GITHUB_ENV: envFile },
    });
    assert.equal(fs.readFileSync(envFile, "utf8"), "RELEASE_PRERELEASE=true\nUPDATER_FEED_KEY=beta/latest.json\n");
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
