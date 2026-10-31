import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { buildDependency, refreshHashes } from "./update-nix-hashes.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

test(
  "refreshes changed lockfiles even when old fixed outputs are cached",
  {
    skip: process.env.MONOCODE_NIX_INTEGRATION !== "1",
  },
  (t) => {
    const repository = fs.mkdtempSync(
      path.join(os.tmpdir(), "monocode-nix-cache-"),
    );
    t.after(() => fs.rmSync(repository, { recursive: true, force: true }));
    const nixpkgs = execFileSync(
      "nix",
      [
        "eval",
        "--raw",
        "--impure",
        "--expr",
        `(builtins.getFlake ${JSON.stringify(root)}).inputs.nixpkgs.outPath`,
      ],
      { encoding: "utf8" },
    ).trim();
    const system = execFileSync(
      "nix",
      ["eval", "--raw", "--impure", "--expr", "builtins.currentSystem"],
      { encoding: "utf8" },
    ).trim();
    fs.mkdirSync(path.join(repository, "nix"));
    const hashFile = path.join(repository, "nix/dependency-hashes.json");
    const fakeHash = `sha256-${"A".repeat(43)}=`;
    fs.writeFileSync(
      hashFile,
      JSON.stringify({ npmHash: fakeHash, cargoHash: fakeHash }),
    );
    fs.writeFileSync(
      path.join(repository, "flake.nix"),
      `
    {
      inputs.nixpkgs.url = "path:${nixpkgs}";
      outputs = { nixpkgs, ... }:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          hashes = builtins.fromJSON (builtins.readFile ./nix/dependency-hashes.json);
          dependency = name: hash: lockfile: pkgs.runCommand name {
            outputHash = hash;
            outputHashMode = "recursive";
            lockContents = builtins.readFile lockfile;
          } ''
            mkdir -p "$out"
            printf '%s' "$lockContents" > "$out/lock"
          '';
          vendorStaging = dependency "monocode-test-vendor-staging" hashes.cargoHash ./cargo.lock;
        in {
          packages.${system}.monocode = {
            npmDeps = dependency "monocode-test-npm" hashes.npmHash ./npm.lock;
            cargoDeps = pkgs.runCommand "monocode-test-vendor" { inherit vendorStaging; } ''
              cp -r "$vendorStaging" "$out"
            '';
          };
        };
    }
  `,
    );
    for (const name of ["npm.lock", "cargo.lock"]) {
      fs.writeFileSync(path.join(repository, name), "original lockfile");
    }
    const build = (attribute) => buildDependency(attribute, repository);
    const original = { ...refreshHashes(repository, build) };

    for (const name of ["npm.lock", "cargo.lock"]) {
      fs.writeFileSync(path.join(repository, name), "changed lockfile");
    }
    const refreshed = refreshHashes(repository, build);
    assert.notEqual(refreshed.npmHash, original.npmHash);
    assert.notEqual(refreshed.cargoHash, original.cargoHash);
    assert.deepEqual(JSON.parse(fs.readFileSync(hashFile, "utf8")), refreshed);
  },
);
