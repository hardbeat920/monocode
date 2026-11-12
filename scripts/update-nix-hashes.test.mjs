import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { refreshHashes } from "./update-nix-hashes.mjs";

const fakeHash = `sha256-${"A".repeat(43)}=`;
const oldHash = `sha256-${"B".repeat(43)}=`;
const newHash = `sha256-${"C".repeat(43)}=`;
const mismatch = {
  status: 1,
  output: `error: hash mismatch in fixed-output derivation '/nix/store/deps.drv':
         specified: ${fakeHash}
            got:    ${newHash}
`,
};
const success = { status: 0, output: "" };

function fixture(t) {
  const repository = fs.mkdtempSync(
    path.join(os.tmpdir(), "monocode-nix-hashes-"),
  );
  t.after(() => fs.rmSync(repository, { recursive: true, force: true }));
  fs.mkdirSync(path.join(repository, "nix"));
  const hashFile = path.join(repository, "nix/dependency-hashes.json");
  const original = `${JSON.stringify({ cargoHash: oldHash, npmHash: oldHash }, null, 2)}\n`;
  fs.writeFileSync(hashFile, original);
  return { repository, hashFile, original };
}

test("leaves matching hashes unchanged", (t) => {
  const { repository, hashFile, original } = fixture(t);
  let calls = 0;
  refreshHashes(repository, () => {
    calls += 1;
    return calls % 2 === 1
      ? { ...mismatch, output: mismatch.output.replace(newHash, oldHash) }
      : success;
  });
  assert.equal(fs.readFileSync(hashFile, "utf8"), original);
});

test("refreshes and verifies both dependency hashes", (t) => {
  const { repository, hashFile } = fixture(t);
  const calls = [];
  refreshHashes(repository, (attribute) => {
    calls.push(attribute);
    const key = attribute === "npmDeps" ? "npmHash" : "cargoHash";
    const hashes = JSON.parse(fs.readFileSync(hashFile, "utf8"));
    assert.equal(hashes[key], calls.length % 2 === 1 ? fakeHash : newHash);
    return calls.length % 2 === 1 ? mismatch : success;
  });
  assert.deepEqual(calls, [
    "npmDeps",
    "npmDeps",
    "cargoDeps.vendorStaging",
    "cargoDeps.vendorStaging",
  ]);
  assert.deepEqual(JSON.parse(fs.readFileSync(hashFile, "utf8")), {
    cargoHash: newHash,
    npmHash: newHash,
  });
});

test("restores all hashes if a later dependency fails", (t) => {
  const { repository, hashFile, original } = fixture(t);
  const results = [mismatch, success, { status: 1, output: "network failure" }];
  assert.throws(
    () => refreshHashes(repository, () => results.shift()),
    /network failure/,
  );
  assert.equal(fs.readFileSync(hashFile, "utf8"), original);
});

test("does not accept a hash until the dependency rebuild succeeds", (t) => {
  const { repository, hashFile, original } = fixture(t);
  assert.throws(
    () => refreshHashes(repository, () => mismatch),
    /hash mismatch/,
  );
  assert.equal(fs.readFileSync(hashFile, "utf8"), original);
});

test("does not persist a placeholder if Nix unexpectedly succeeds", (t) => {
  const { repository, hashFile, original } = fixture(t);
  assert.throws(
    () => refreshHashes(repository, () => success),
    /Expected a hash mismatch/,
  );
  assert.equal(fs.readFileSync(hashFile, "utf8"), original);
});
