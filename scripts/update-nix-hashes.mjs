import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dependencies = {
  npmHash: "npmDeps",
  cargoHash: "cargoDeps.vendorStaging",
};
const fakeHash = `sha256-${"A".repeat(43)}=`;

export function refreshHashes(repository, buildDependency) {
  const hashFile = path.join(repository, "nix/dependency-hashes.json");
  const original = fs.readFileSync(hashFile, "utf8");
  const hashes = JSON.parse(original);
  const writeHashes = () => {
    const temporary = `${hashFile}.tmp`;
    fs.writeFileSync(temporary, `${JSON.stringify(hashes, null, 2)}\n`);
    fs.renameSync(temporary, hashFile);
  };

  try {
    for (const [key, attribute] of Object.entries(dependencies)) {
      // A placeholder forces a fresh fixed-output build, even with cached hashes.
      hashes[key] = fakeHash;
      writeHashes();
      const result = buildDependency(attribute);
      if (result.status === 0) {
        throw new Error(`Expected a hash mismatch for ${attribute}`);
      }

      // Only recover a fixed-output hash mismatch, not network or build failures.
      const mismatch = result.output.match(
        /hash mismatch in fixed-output derivation[^\n]*:\s+specified:\s+sha256-[A-Za-z0-9+/=]+\s+got:\s+(sha256-[A-Za-z0-9+/=]+)/,
      );
      if (!mismatch) throw new Error(result.output);
      hashes[key] = mismatch[1];
      writeHashes();

      const verified = buildDependency(attribute);
      if (verified.status !== 0) throw new Error(verified.output);
    }
    return hashes;
  } catch (error) {
    fs.writeFileSync(hashFile, original);
    throw error;
  }
}

export function buildDependency(attribute, repository = root) {
  const result = spawnSync(
    "nix",
    [
      "build",
      `path:${repository}#monocode.${attribute}`,
      "--no-link",
      "--print-build-logs",
    ],
    { cwd: repository, encoding: "utf8", maxBuffer: 100 * 1024 * 1024 },
  );
  if (result.error) throw result.error;
  const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;
  process.stderr.write(output);
  return { status: result.status, output };
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  try {
    refreshHashes(root, buildDependency);
    console.log("Nix dependency hashes are up to date.");
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
