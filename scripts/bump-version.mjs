#!/usr/bin/env node
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
  console.error("usage: npm run set-version -- 0.1.1");
  process.exit(1);
}

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function replaceFirst(path, pattern, replacement) {
  const text = readFileSync(path, "utf8");
  const next = text.replace(pattern, replacement);
  if (next === text) {
    console.error(`failed to update ${path}`);
    process.exit(1);
  }
  writeFileSync(path, next);
}

replaceFirst(
  join(root, "package.json"),
  /("version": ")[^"]+(")/,
  `$1${version}$2`,
);
// The lockfile carries the version twice: once at the top and once on the
// root package. Missing them left npm's lockfile claiming 0.1.0 sixteen
// releases later. The second pattern is anchored on `packages` because the
// top-level object repeats the same name/version pair.
replaceFirst(
  join(root, "package-lock.json"),
  /^(\{\n\s*"name": "monocode-desktop",\n\s*"version": ")[^"]+(")/,
  `$1${version}$2`,
);
replaceFirst(
  join(root, "package-lock.json"),
  /("packages": \{\n\s*"": \{\n\s*"name": "monocode-desktop",\n\s*"version": ")[^"]+(")/,
  `$1${version}$2`,
);
replaceFirst(
  join(root, "Cargo.toml"),
  /^(version = ")[^"]+(")/m,
  `$1${version}$2`,
);
replaceFirst(
  join(root, "src-tauri/tauri.conf.json"),
  /("version": ")[^"]+(")/,
  `$1${version}$2`,
);
replaceFirst(
  join(root, "Cargo.lock"),
  /(name = "monocode"\nversion = ")[^"]+(")/,
  `$1${version}$2`,
);

console.log(`version ${version}`);
