{
  lib,
  stdenv,
  rustPlatform,
  fetchNpmDeps,
  npmHooks,
  nodejs_24,
  cargo-tauri,
  pkg-config,
  wrapGAppsHook3,
  makeWrapper,
  gtk3,
  glib-networking,
  webkitgtk_4_1,
  openssl,
}:
let
  manifest = lib.importJSON ../package.json;
  hashes = lib.importJSON ./dependency-hashes.json;
  src = lib.cleanSourceWith {
    src = ../.;
    filter =
      path: type:
      lib.cleanSourceFilter path type
      && !builtins.elem (builtins.baseNameOf path) [
        "node_modules"
        "target"
        "dist"
        "result"
      ];
  };
in
rustPlatform.buildRustPackage {
  pname = "monocode";
  inherit (manifest) version;
  inherit src;

  cargoHash = hashes.cargoHash;
  npmDeps = fetchNpmDeps {
    inherit src;
    name = "monocode-${manifest.version}-npm-deps";
    hash = hashes.npmHash;
  };

  nativeBuildInputs = [
    nodejs_24
    npmHooks.npmConfigHook
    cargo-tauri.hook
    pkg-config
    makeWrapper
  ]
  ++ lib.optionals stdenv.hostPlatform.isLinux [ wrapGAppsHook3 ];

  buildInputs = lib.optionals stdenv.hostPlatform.isLinux [
    gtk3
    glib-networking
    webkitgtk_4_1
    openssl
  ];

  # Nix owns updates and macOS signing; no updater artifacts or signing keys.
  postPatch = ''
    node --input-type=module <<'EOF'
    import fs from 'node:fs';
    const path = 'src-tauri/tauri.conf.json';
    const config = JSON.parse(fs.readFileSync(path, 'utf8'));
    config.bundle.createUpdaterArtifacts = false;
    config.bundle.macOS.signingIdentity = null;
    fs.writeFileSync(path, JSON.stringify(config, null, 2));
    EOF
  '';

  postInstall = lib.optionalString stdenv.hostPlatform.isDarwin ''
    mkdir -p "$out/bin"
    makeWrapper "$out/Applications/MonoCode.app/Contents/MacOS/monocode" "$out/bin/monocode"
  '';

  # GUI/integration tests need a desktop session; upstream CI runs them separately.
  doCheck = false;

  meta = {
    description = "A GUI for your coding agents";
    homepage = "https://github.com/hardbeat920/monocode";
    license = lib.licenses.mit;
    mainProgram = "monocode";
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
      "aarch64-darwin"
    ];
  };
}
