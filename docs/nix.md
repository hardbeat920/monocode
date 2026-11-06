# Install with Nix

MonoCode provides a source-built flake package for x86_64 and aarch64 Linux and Apple Silicon macOS (`aarch64-darwin`). The Nix package does not support Intel macOS. Enable Nix's `nix-command` and `flakes` experimental features before using these commands.

The flake pins nixpkgs to the current NixOS stable release, 26.05.

Install and sign in to at least one [supported agent CLI](providers.md) separately. MonoCode uses the agents available in your environment; the package does not bundle them.

## Run or install

Run without installing:

```bash
nix run github:hardbeat920/monocode
```

Install in your user profile:

```bash
nix profile add github:hardbeat920/monocode
monocode
```

The first build compiles the frontend and Rust application and may take a while. On Linux, the package includes its GTK/WebKit dependencies, desktop entry, and icons. On macOS, it includes `Applications/MonoCode.app` and a `monocode` launcher.

## NixOS and nix-darwin

Add the input to your system flake:

```nix
inputs.monocode.url = "github:hardbeat920/monocode";
```

Pass your inputs into the existing `nixosSystem` or `darwinSystem` call:

```nix
specialArgs = { inherit inputs; };
```

Then add the package in a system module:

```nix
{ inputs, pkgs, ... }:
{
  environment.systemPackages = [
    inputs.monocode.packages.${pkgs.stdenv.hostPlatform.system}.monocode
  ];
}
```

Rebuild with your usual `nixos-rebuild switch --flake ...` or `darwin-rebuild switch --flake ...` command. NixOS desktop sessions discover the installed desktop entry. nix-darwin links the app under `/Applications/Nix Apps`.

On Linux, run MonoCode inside a graphical session with a session D-Bus. Credential storage also needs a Secret Service provider, such as GNOME Keyring or KeePassXC with Secret Service enabled.

## Home Manager

Pass `inputs` through your Home Manager configuration's `extraSpecialArgs`, then use:

```nix
{ inputs, pkgs, ... }:
{
  home.packages = [
    inputs.monocode.packages.${pkgs.stdenv.hostPlatform.system}.monocode
  ];
}
```

## Overlay

If you prefer `pkgs.monocode`, add `inputs.monocode.overlays.default` to `nixpkgs.overlays`. The overlay builds against your configuration's nixpkgs instead of this repository's pinned version, so toolchain compatibility depends on that version.

## Updates and local builds

Update MonoCode through Nix, not the in-app updater:

```bash
nix profile upgrade monocode
# Or update the input in your system flake, then rebuild:
nix flake update monocode
```

To build a checkout:

```bash
nix build .#monocode
nix flake check
nix fmt
```

`nix flake check` builds the package for the current system; it does not run desktop integration tests. macOS builds are not Developer ID signed or notarized.

The package reads its version from `package.json`. Dependency hashes live in `nix/dependency-hashes.json`. After changing `package-lock.json` or `Cargo.lock`, refresh and verify them with:

```bash
node scripts/update-nix-hashes.mjs
```

The updater temporarily substitutes placeholder hashes to force fresh npm and Cargo staging builds, even when old outputs are cached. It verifies the calculated hashes and restores the original file if either dependency fails. Run its cache regression test with `MONOCODE_NIX_INTEGRATION=1 node --test scripts/update-nix-hashes.integration.test.mjs` (requires Nix).

The [Nix workflow](../.github/workflows/nix.yml) runs this automatically on pushes to `main`, published releases, and manual dispatch. It commits changed hashes to `main`, then builds that exact commit on Linux and macOS. Release events refresh `main`; they do not rewrite release tags. Pull requests build their submitted hashes without write access.

The workflow requires Actions to have permission to push to `main`. If branch protection blocks bot commits, run the refresh command locally and include the hashes in your pull request. A concurrent push causes the bot's push to fail rather than overwrite newer changes; rerun the workflow to refresh the latest `main`. The workflow does not publish releases or update nixpkgs automatically.
