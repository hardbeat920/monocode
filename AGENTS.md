# Repository guidance

Use jj for local version control. Keep changes focused and follow CONTRIBUTING.md.

## Formatting

Installing the existing lockfile dependencies with `npm ci` is approved for this repository. Use the project-local Prettier for changed JavaScript, JSON, Markdown, and YAML files; use `nix fmt` for Nix files. Do not reformat unrelated source files.

## Nix packaging

The flake packages the desktop app from source. The version comes from `package.json`; dependency hashes are in `nix/dependency-hashes.json`.

After lockfile changes, run `node scripts/update-nix-hashes.mjs`. Verify the updater with `node --test scripts/update-nix-hashes.test.mjs` and the package with `nix flake check`. CI refreshes hashes on `main` and release publication, then builds Linux and macOS. Do not rewrite release tags to refresh hashes.
