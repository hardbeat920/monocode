# Native cutover inventory

This is a source inventory for M6 on `gpui-native`, recorded on October 3, 2026 at base commit `b585761aafb39c265618191897c5579fe509ccd2`. It does not authorize deletion, publication, host replacement, or an updater feed change. Localization repairs follow the frozen source checkpoint. [Native qualification](../native-qualification.md) records runtime results and their limits.

## Removal manifest after qualification

The following directory prefixes cover every tracked legacy implementation file. Counts come from `git ls-files`, not generated files or dependencies. No pending tracked or untracked changes appeared in these legacy paths during this inventory.

| Path | Tracked files | Coverage and replacement |
| --- | ---: | --- |
| `src/` | 929 | 736 TypeScript files, 177 TSX files, 4 CSS files, 11 provider SVGs, and the Inbox instructions. Native replacements are `apps/monocode-app`, `crates/core`, `crates/harness`, `crates/engine`, the view crates, `crates/ui`, and `crates/settings`. Preserve the independent assets listed below. |
| `host/` | 55 | 50 TypeScript files, 3 Node scripts, `tsconfig.json`, and `windows-acl.ps1`. Native replacements are `crates/host` and `crates/remote/src/host`. This includes all Node host tests, `build.mjs`, `npm.mjs`, and `provider-guard.mjs`. |
| `src-tauri/` | 72 | The separate Cargo workspace and lockfile, 42 Rust files including `build.rs`, capabilities, all four Tauri configuration files, plists, and icon assets. Preserve source artwork before removing this prefix. Rust implementations now live in the root workspace. |
| `public/` | 1 | `monocode.png`. The root README must point to a retained native asset before this directory goes away. |

Remove these exact root files with the legacy directories:

```text
index.html
quick-composer.html
vite.config.ts
vitest.config.ts
tsconfig.json
tsconfig.node.json
package.json
package-lock.json
```

Those two config files are the only tracked TypeScript outside `src/` and `host/`. These prefixes and files therefore cover all 965 tracked `.ts` and `.tsx` files. File names and example text inside Rust tests, such as `app.ts`, are data and must remain.

Replace or remove these scripts before deleting their inputs:

| Path | Required action |
| --- | --- |
| `scripts/bump-version.mjs` | Replace with a Rust version command or a documented Cargo version procedure. The current script edits both npm files, `src-tauri/tauri.conf.json`, and a legacy `monocode` lockfile entry. Native packages inherit `[workspace.package].version`; the root Cargo lockfile and `CHANGELOG.md` must agree with the release tag. |
| `scripts/install-dev-host.mjs` | Remove. It packages and transfers the npm host and expects obsolete `@@PACKAGE@@` placeholders. Use `install-native-host.sh` or `install-native-host.ps1` on the intended host. |
| `scripts/install-linux-deps-debian.sh` | Remove after all docs and CI use `scripts/install-native-linux-deps.sh`. The old script installs Tauri GTK and WebKit dependencies. |
| `scripts/install-linux-deps-fedora.sh` | Remove with the old Fedora and Enterprise Linux Tauri jobs. Native distribution support and package dependencies need their own qualified install instructions. |

Generated legacy outputs are optional cleanup candidates, not source removal targets:

```text
node_modules/
dist/
dist-ssr/
build/host/
build/host-npm/
build/host-packages/
build/monocode-host-*.tgz
```

Do not remove `build/` as a whole. `build/native/` holds native packages and checksums. Do not remove `target/` as part of cutover. It contains native executables and retained screenshot evidence. Existing screenshots, reports, private database copies, source hashes, and qualification logs need separate retention decisions.

## Assets and source material to preserve

The native build has no compile-time include from `src/`, `host/`, `public/`, or `src-tauri/`. Its icon source is `crates/ui/assets`, its distribution assets are `packaging`, and its Inbox instructions are `crates/engine/src/inbox/instructions.md`.

The following independent copies compare byte for byte equal:

| Legacy path | Retained native path |
| --- | --- |
| `public/monocode.png` | `crates/view-transcript/assets/monocode.png` |
| `src/instructions/inbox.md` | `crates/engine/src/inbox/instructions.md` |
| `src-tauri/icons/icon.icns` | `packaging/assets/icon.icns` |
| `src-tauri/icons/icon.ico` | `packaging/assets/icon.ico` |
| `src-tauri/icons/icon.png` | `packaging/assets/icon.png` |
| `src-tauri/macos/Assets.car` | `packaging/macos/Assets.car` |
| `src-tauri/Entitlements.plist` | `packaging/macos/Entitlements.plist` |
| `host/windows-acl.ps1` | `crates/remote/src/host/windows-acl.ps1` |
| `src-tauri/macos/AppIcon.icon/Assets/icon.png` | `packaging/macos/AppIcon.icon/Assets/icon.png` |
| `src-tauri/macos/AppIcon.icon/icon.json` | `packaging/macos/AppIcon.icon/icon.json` |
| `src-tauri/icons/installer-sidebar.bmp` | `packaging/assets/installer-sidebar.bmp` |

All 11 provider SVGs exist independently under `crates/ui/assets/providers`. Seven match byte for byte. Antigravity, Claude, Codex, and omp have native SVG adjustments, including explicit dimensions. Keep those native copies and their current behavior.

The editable macOS icon and installer sidebar bitmap now have independent copies under `packaging`. The originals remain unchanged. Their copied bytes have these SHA-256 hashes:

| Copied artwork | Bytes | SHA-256 |
| --- | ---: | --- |
| `packaging/macos/AppIcon.icon/Assets/icon.png` | 7,273 | `33900edd600c6dd40e2dec8799ef0bf4cb2218c702b57714c6bcf9138580cab1` |
| `packaging/macos/AppIcon.icon/icon.json` | 725 | `3639062dc6934e26f61b9062b42e83db0f6c3e4177c899a5fb9ed487e3371d67` |
| `packaging/assets/installer-sidebar.bmp` | 154,542 | `a61a5c9478f678a931fce71d575efcd7e660d44c399094773da5631869af65a5` |

Keep the editable icon alongside its compiled `Assets.car`. The native installer currently does not reference the sidebar bitmap.

Keep `apps/`, `crates/`, `vendor/portable-pty/`, `packaging/`, the root Cargo files, `.cargo/`, `LICENSE`, `NOTICE`, `CHANGELOG.md`, and the native qualification evidence. Keep all current user and agent source edits. `AGENTS.md`, `CLAUDE.md`, personal instructions, provider credentials, SSH configuration, real desktop data, and installed host services are outside this removal manifest.

## Rust code that still mentions or uses Node

`crates/remote/src/host/node_compat_tests.rs` is an ignored migration test. It launches `node` with `build/host/monocode-host.mjs` and needs `npm run host:build`. Its 20-result takeover run passed, but deleting the Node source would make that explicit test skip. Before cutover, preserve its evidence and either replace its expected responses with frozen Rust protocol fixtures or retire this migration-only test deliberately.

`crates/host/src/testing.rs` runs inline Node scripts for the normal Rust provider transport tests. These bodies remain independent of legacy source. Windows builds a native test launcher once with Rust and runs the same scripts beside it. Native CI must provide Node for these fixtures even though building and running the desktop and host do not require it. If M6 requires removing Node from tests as well as the product, replace these protocol fixtures with Rust before removing that test dependency.

`RuntimeBundle::node()` in `crates/remote/src/host/runtime.rs` remains Rust compatibility logic. Native startup uses a native executable bundle. Keep the service and data compatibility behavior unless a separate change proves it unnecessary.

`scripts/test-remote-ssh.py` now defaults to native host and desktop executables and must remain. Its optional `.mjs` argument adds Node only for an explicitly requested legacy comparison. Remove that optional branch if the final tooling contract forbids any Node-dependent mode.

Rust comments that name their original TypeScript source are provenance, not dependencies. Provider installation hints that use npm are external provider instructions. They do not make the desktop or host a Node application.

## CI and release replacement

| File | Required cutover change |
| --- | --- |
| `.github/workflows/ci.yml` | Replace or remove all legacy host, web, Tauri, RPM, and clean Tauri RPM jobs. None may reference npm, Vite, WebKit, or `src-tauri` after removal. Preserve native package clean-install checks with the new runtime dependencies. |
| `.github/workflows/native-ci.yml` | Promote to the canonical CI workflow. It already runs Rust formatting, strict all-target/all-feature Clippy, tests, builds, and portable host packages on macOS, Linux, and Windows. Change its branch trigger from `gpui-native` to the final protected branch and reconcile required status checks. Include `scripts/test-remote-ssh.py` and workflow inputs in path filters if those jobs should run on their changes. |
| `.github/workflows/release.yml` | Retire the legacy `v*` publication path. It builds Tauri bundles, publishes the npm host, uploads R2 objects, and switches `latest.json`. It cannot remain active beside a native tag publisher. |
| `.github/workflows/native-release.yml` | Keep manual rehearsal until release gates pass. Before making it the tag workflow, add tag versus Cargo version and changelog checks, make signed release settings mandatory, and carry forward the validated publication sequence. Its current optional draft upload does not publish a release or switch the production feed. |

Preserve `TAURI_UPDATER_PUBKEY`, `TAURI_UPDATER_ENDPOINT`, `TAURI_SIGNING_PRIVATE_KEY`, and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. These names retain the existing updater key and signature format; their prefix is not a Tauri runtime dependency. Preserve Apple signing and notarization secrets. R2 credentials and stable download URLs remain publication inputs if the existing download domain stays in use. An npm publishing token is no longer needed by native releases.

The final publication sequence must upload every architecture's desktop packages, updater signatures, portable host archive, and `SHA256SUMS` before making the release visible or switching the feed. Native SSH bootstrap downloads the exact desktop version from release assets. A visible release without its matching host archive or checksum would break new SSH setup. Keep `MonoCode.dmg` and `MonoCode_x64.dmg` aliases valid if the root Install links retain them. Feed replacement requires a separate, reviewed publication step after installer and update checks.

## Native commands for CI and README

These commands describe the existing native workflows. They were not run as part of this inventory.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --lib --bins --all-features --locked
cargo test -p monocode-host -p monocode-remote -p monocode-package --tests --locked
cargo test -p monocode-app --test ssh_askpass --all-features --locked
cargo test -p monocode-platform --test inline_video --locked
cargo build -p monocode-app -p monocode-host --bins --locked
```

The normal development command uses an isolated profile:

```sh
cargo run -p monocode-app --locked -- --data-dir /tmp/monocode-native-dev
```

For low-space development, set `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0`. Use `scripts/install-native-linux-deps.sh` for Linux build prerequisites. Windows requires the MSVC toolchain and NASM; packages require NSIS. Intel macOS builds require NASM for the AVIF decoder.

Build packages on the runner matching the target. This example is Apple Silicon:

```sh
rustup target add aarch64-apple-darwin
cargo build -p monocode-app -p monocode-host --bins --release --target aarch64-apple-darwin --locked
cargo run -p monocode-package --locked -- bundle --target aarch64-apple-darwin
```

The other supported release targets are `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, and `x86_64-pc-windows-msvc`. On Windows set `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS=-C target-feature=+crt-static` before the target build. Linux AppImage requires `linuxdeploy`; a portable Linux host must use static ICU, as on the Ubuntu release runner.

A signed desktop build needs the existing updater endpoint and public key set before compilation. On macOS pass `--signing-identity` and `--notary-profile` to `bundle`. The native workflow prepares the signing keychain and notarization profile. Once all platform outputs are collected:

```sh
cargo run -p monocode-updater --features sign --bin monocode-updater-sign --locked -- sign PACKAGE
cargo run -p monocode-updater --features sign --bin monocode-updater-sign --locked -- verify PACKAGE
cargo run -p monocode-package --locked -- checksums
cargo run -p monocode-package --locked -- manifest --base-url https://github.com/hardbeat920/monocode/releases/download/v0.6.0 --require-targets darwin-aarch64,darwin-x86_64,windows-x86_64,linux-x86_64-deb,linux-x86_64-rpm,linux-x86_64-appimage
```

Replace `PACKAGE` with each generated updater package and use the actual release version in the manifest URL. DMGs and portable host archives are not updater entries. Do not publish the generated manifest during rehearsal.

For a development host without a login service:

```sh
cargo run -p monocode-host --bin monocode-host --locked -- connect --no-service --json --data-dir /tmp/monocode-host-test
```

Host installation commands belong on the machine being configured:

```sh
sh scripts/install-native-host.sh 0.6.0
```

```powershell
powershell -File scripts/install-native-host.ps1 -Version 0.6.0
```

## Documentation and configuration updates

- `README.md` must replace the `public/monocode.png` link, Node/Tauri build prerequisites, npm commands, WebView2 setup claim, WebKit package explanations, Tauri output paths, and the `npx monocode-host` example. Keep the product, provider, and agent CLI instructions that still apply.
- `CONTRIBUTING.md` must replace the setup and check commands and map the source tree to the Rust app, engine, harness, and view crates.
- `.github/pull_request_template.md` must replace its `npm run check` checkbox with the native checks.
- `docs/remote-access.md` must replace the npx and Node requirements, npm development and publishing instructions, legacy Windows `.cmd` launcher, and legacy verification commands. Document the exact-version archive and checksum bootstrap, native `.exe` launcher, and the preserved service, pairing, and active-turn confirmation behavior.
- `Cargo.toml` must remove `src-tauri` from `workspace.exclude` after that directory is gone. Keep `vendor/portable-pty` excluded from direct membership.
- `.cargo/config.toml` must drop its two-workspace comment. Keep the shared target path and current incremental setting unless a separate build change is intended.
- `.gitignore` can drop web and npm-only cache patterns after removal. Keep native artifacts, secrets, working-note ignores, and editor rules.
- `apps/monocode-app/README.md` and `packaging/README.md` must replace their temporary retained-source statements only when cutover actually occurs.
- `docs/gpui-rebuild.md` and `docs/gpui-port-conventions.md` must record final milestone evidence and remove obsolete transition instructions. Preserve the port's behavior and licensing decisions.
- `apps/monocode-app/PROGRESS-*.md`, `NEEDS-shell.md`, and crate progress notes are stale implementation notes. Reconcile them with the authoritative qualification report before archiving or removing them. This inventory does not remove them.

## Gates that remain open

The literal M0 session JSON gate passed. `crates/core/tests/golden_json_bytes.rs` compared all 126 transcript and model-settings columns and three linked-item columns byte for byte against the untouched private copy. There were no populated Inbox-context columns. The backup hash remained unchanged. See [the strict result](data-compat/json-bytes.log).

RemotePane already mounts the shared SessionToolbar, which routes selected worktree, workspace mode, base, and branch changes directly to the remote engine. Its workspace shortcut initially reached only local panes. The actual rendered regression reproduced that failure, then passed after WorkspaceArea routed the remote variant through the toolbar. The test verifies selected checkout and base, host worktree creation before session creation and send, Supervised mode, no local Git calls, and a hidden workspace trigger after binding. Actual separate-computer window behavior remains unverified.

| Gate | Concrete state |
| --- | --- |
| Final source qualification | The frozen 2,629-file source checkpoint passed 5,439 Mac tests and 5,373 Windows tests, with strict Clippy, integrations, and fresh binaries. Linux stopped at a listener test race. The corrected fixture and complete remote suite passed afterward. Localization repairs now require a new source freeze and combined qualification. See the coordinator's [qualification report](../native-qualification.md). |
| M1 literal Claude lifecycle | Live Claude access is externally blocked. The organization disabled subscription access for Claude Code after the first attempt hit quota. Native Codex approved-write, persistence, and fresh-process resume passed, but that result does not satisfy the plan's literal Claude exit check. Do not change credentials or permission modes to manufacture a pass. |
| M2 installed-provider smoke | Cursor, Droid, and the real Codex engine passed. The new OpenCode 2 adapter passed protocol fixtures and one real turn through an isolated free model. Grok hit an exhausted balance with HTTP 402. Claude subscription access is disabled. Missing Pi, omp, fx, Hermes, and Antigravity executables were not tested. See [OpenCode 2 qualification](opencode-v2/qualification.md). |
| M5 rollout compatibility | The Node-to-Rust saved pairing, data takeover, and 20 RPC comparison passed. An actual retained desktop GUI connection and a live provider resume across Node/Rust host replacement remain unverified. Native separate-computer remote GPUI and Connect interaction remain unverified. |
| Quick composer | Five panel policy checks passed. The real AppKit fixture reached presentation, but the locked Mac console prevented key focus. Native app automation also reports a pipe startup failure. Focus, capture permission, and ordinary-window fallback interaction remain unverified. See [panel qualification](macos-panel-qualification.md). |
| Release packages | The frozen Mac binaries passed bundle, archive, DMG, ad hoc signing, and owned widget rendering checks. Earlier Windows portable-host and Linux DEB, RPM, AppImage, owned graphics, and bundled H.264 checks passed their documented checkpoints. Regenerate packages after localization qualification. Developer ID notarization, Windows installation, upgrade, restart and tray interaction, and Linux clean installation remain unverified. See [Mac package qualification](final-macos-package/qualification.md). |
| Updater | A disposable signed loopback feed exercised the real Mac updater. Tampered content was rejected, and the trusted download replaced only the owned test bundle and passed content, hash, CLI, and signing checks. Actual GUI relaunch and production feed migration remain unverified. See [the isolated install evidence](final-macos-package/qualification.md). |

The native source cutover is technically feasible once these gates pass. Unique artwork has been retained. Until then, retain the legacy implementation and its release path. No production profile, service, package installation, release, or updater feed changed in this inventory.
