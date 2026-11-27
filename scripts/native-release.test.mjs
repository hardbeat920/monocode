import assert from "node:assert/strict";
import { test } from "node:test";
import { nativeManifest, nativeTargets } from "./native-release.mjs";
import { previewVersion } from "./package-native.mjs";

const artifacts = nativeTargets.map((target) => ({
  target,
  filename: `${target} with spaces.bin`,
  signature: "signed-package",
}));
function manifest(changes = {}) {
  return nativeManifest({
    version: "0.16.0-preview.2",
    baseUrl: "https://dl.example.test/",
    artifacts,
    ...changes,
  });
}

test("all native architectures and installation types have isolated versioned URLs", () => {
  const result = manifest();
  assert.equal(result.app_id, "dev.usemono.gpui");
  assert.equal(result.channel, "native-preview");
  assert.deepEqual(
    Object.keys(result.platforms).sort(),
    [...nativeTargets].sort(),
  );
  for (const artifact of Object.values(result.platforms)) {
    assert.match(
      artifact.url,
      /^https:\/\/dl\.example\.test\/native-preview\/0\.16\.0-preview\.2\//,
    );
    assert.ok(artifact.url.includes("%20"));
  }
});
test("a missing Intel Mac or Linux package prevents publishing the feed", () => {
  for (const target of nativeTargets) {
    assert.throws(
      () =>
        manifest({ artifacts: artifacts.filter((a) => a.target !== target) }),
      /Missing native package/,
    );
  }
});
test("duplicate targets, unsigned packages and path traversal are refused", () => {
  assert.throws(
    () => manifest({ artifacts: [...artifacts, artifacts[0]] }),
    /Duplicate/,
  );
  assert.throws(
    () =>
      manifest({
        artifacts: artifacts.map((a, i) => (i ? a : { ...a, signature: "" })),
      }),
    /Missing signature/,
  );
  for (const filename of ["../latest.json", "directory\\latest.json"]) {
    assert.throws(
      () =>
        manifest({
          artifacts: artifacts.map((a, i) => (i ? a : { ...a, filename })),
        }),
      /plain filename/,
    );
  }
});
test("production tags and insecure download bases cannot create native releases", () => {
  for (const version of [
    "0.16.0",
    "v0.16.0",
    "native-v0.16.0-preview.1",
    "0.16.0-beta.1",
    "0.16.0-preview.1/../latest.json",
  ]) {
    assert.throws(() => manifest({ version }), /native preview version/);
  }
  for (const baseUrl of [
    "http://dl.example.test",
    "https://user:secret@dl.example.test",
    "https://dl.example.test?channel=stable",
  ]) {
    assert.throws(() => manifest({ baseUrl }), /HTTPS URL/);
  }
});
test("preview sequence fits native Windows version resources", () => {
  assert.equal(previewVersion("0.16.0-preview.12").windows, "0.16.0.12");
  assert.throws(() => previewVersion("0.16.0-preview.65536"));
});
