# Native packages

The native build uses Cargo. The desktop runs GPUI, and `monocode-host` runs the Rust remote engine without opening a window. Neither build needs Node or WebKit. Existing Tauri and Node sources remain in the checkout until the application passes the migration checks.

## Build and bundle

Build both executables for the packaging target, then run the Rust package command.

```sh
rustup target add aarch64-apple-darwin
cargo build -p monocode-app -p monocode-host --bins --release --target aarch64-apple-darwin -j 2
cargo run -p monocode-package -- bundle --target aarch64-apple-darwin
```

Use `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, or `x86_64-pc-windows-msvc` on their matching runners. Portable host jobs also build `aarch64-unknown-linux-gnu` and `aarch64-pc-windows-msvc`, which match the ARM64 host installers. These ARM64 jobs produce host archives only. The package command uses prebuilt executables under `target/<target>/release`. It checks each executable's binary format and CPU against the requested Rust target before creating an archive. `--binaries PATH` selects another directory. `--formats host` produces only the portable host archive. Linux accepts `--formats deb,rpm,appimage`; macOS accepts `--formats archive` to omit the DMG. The default output directory is `build/native`.

The macOS bundle preserves `com.monocode.desktop`, the usage descriptions, icon assets, and the `monocode` URL scheme. Its executable is `monocode-app`. The bundle also contains `monocode-host`. Local bundles use an ad hoc signature. A release passes `--signing-identity 'Developer ID Application: ...' --notary-profile monocode-native`. The profile must already exist in the signing keychain. The command signs the host and bundle, submits the bundle to Apple, staples the ticket, verifies the signature, and creates the archive and DMG.

Linux needs the packages in `scripts/install-native-linux-deps.sh`. Debian and RPM packages keep the package name `mono-code`, `/usr/bin/monocode`, and `MonoCode.desktop` so they upgrade the existing installation. They link the system libraries and require GStreamer's base, good, and libav plugin packages for inline video. AppImage uses `linuxdeploy`, includes both executables, and copies their dynamic dependencies. It also bundles GStreamer's dynamically loaded libraries, installed plugins, and plugin scanner. Its wrapper selects those bundled libraries and plugins. Install the native Vulkan driver for your GPU. The package command sets `LDAI_OUTPUT` as described in the [AppImage plugin documentation](https://github.com/linuxdeploy/linuxdeploy-plugin-appimage/blob/master/README.md).

Date labels use Foundation on macOS, Windows.Globalization on Windows, and ICU on Linux. Linux builds need `libicu-dev` or `libicu-devel`. The Ubuntu release runner links the supplied static ICU libraries and locale data into the executables, so portable host archives do not require an ICU installation. On distributions that supply only shared ICU libraries, local builds use them. The package command rejects a portable host linked to shared ICU. Build release archives on Ubuntu or point `PKG_CONFIG_PATH` to a static ICU build.

Windows needs NSIS, NASM for the AVIF decoder, and the MSVC Rust toolchain. Set `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS='-C target-feature=+crt-static'` before building with `--target x86_64-pc-windows-msvc`. The ARM64 host uses `CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_RUSTFLAGS='-C target-feature=+crt-static'` and `--target aarch64-pc-windows-msvc`. The native workflows set these flags so the desktop and single-executable host archives do not need the Visual C++ redistributable. Rust documents the flag in its [C runtime linkage reference](https://doc.rust-lang.org/reference/linkage.html#static-and-dynamic-c-runtimes). Inline video uses the operating system's Media Foundation decoder. The installer keeps the existing per-user directory and registry keys. It registers the `monocode` pairing URL scheme for the current user and passes the complete URI as a quoted argument, following [Microsoft's URI handler registration](https://learn.microsoft.com/en-us/previous-versions/windows/internet-explorer/ie-developer/platform-apis/aa767914(v=vs.85)). It accepts `/P /R /UPDATE /ARGS`, waits for the previous desktop executable to close, and can restart the new desktop with its arguments. Uninstall removes the program and shortcuts and preserves user data. It removes the URL handler only if the command still points to this installation.

The platform `inline_video` test decodes the bundled H.264 fixture and checks playback, seeking, pause, and temporary file cleanup. On Linux it also checks the BGRA frame buffer. On macOS and Windows it creates a hidden native window. Run it on each target before release. Linux AppImage playback and codecs beyond the fixture need separate checks with the packaged desktop.

Windows CI checks the runner's session before this native video test. It reports a skipped fixture in Session 0 because [DXGI cannot create a swap chain there](https://learn.microsoft.com/en-us/windows/win32/api/dxgi/nf-dxgi-idxgifactory-createswapchain). Other sessions run the fixture and fail on decoder errors. A Session 0 skip requires an interactive desktop fixture run before release.

## Host installation

Host archives have one executable and use this naming format.

```text
monocode-host_<version>_<Rust-target>.tar.gz
```

The desktop's SSH setup downloads the archive matching its own version. It verifies the SHA-256 from `SHA256SUMS`, rejects archives with unexpected entries, and checks the executable's reported version before running `connect --json`. The existing connection code still checks host identity and asks before interrupting active turns for an update. `MONOCODE_HOST_RELEASE_BASE_URL` selects an HTTPS release directory for development builds.

To install from a checkout on the host, run the matching script.

```sh
sh scripts/install-native-host.sh 0.6.0
```

```powershell
powershell -File scripts/install-native-host.ps1 -Version 0.6.0
```

These scripts install and start the host through `connect`, so run them only on the host you intend to configure. To run a local development host without registering a login service, use this command.

```sh
cargo run -p monocode-host --bin monocode-host -- connect --no-service --json --data-dir /tmp/monocode-host-test
```

## Updates and releases

Native updates retain the existing Tauri-compatible minisign key and signature format. Set `TAURI_UPDATER_ENDPOINT` and `TAURI_UPDATER_PUBKEY` before compiling the desktop. The public key and endpoint are build-time constants. A build without them reports that updates are not configured.

```sh
cargo run -p monocode-updater --features sign --bin monocode-updater-sign -- sign build/native/MonoCode_0.6.0_aarch64-apple-darwin.app.tar.gz
cargo run -p monocode-updater --features sign --bin monocode-updater-sign -- verify build/native/MonoCode_0.6.0_aarch64-apple-darwin.app.tar.gz
cargo run -p monocode-package -- checksums
cargo run -p monocode-package -- manifest --base-url https://github.com/hardbeat920/monocode/releases/download/v0.6.0
```

The signer reads `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. The package command refuses to generate feed entries without signature files. It keeps disk images and portable host archives out of the updater manifest. Feed keys are `darwin-aarch64`, `darwin-x86_64`, `windows-x86_64`, and installer-specific Linux keys such as `linux-x86_64-deb`.

`native-ci.yml` checks Rust on macOS, Linux, and Windows. Provider protocol test fixtures require Node 24, which CI installs only in its check job. Product executables and portable host jobs do not require Node. `native-release.yml` is manual. By default it builds unsigned rehearsal packages and uploads workflow artifacts. Its `signed` input adds updater signatures and macOS notarization. Its `draft` input uploads a draft release after all platforms build and verify their artifacts. It never publishes a GitHub release or changes the production updater feed.

Before a native release can replace production, complete the application parity checks, replace the legacy tag workflow, test the packages on their target systems, publish every matching host archive and `SHA256SUMS`, then publish the validated updater feed. Keep the production feed unchanged during rehearsal. Actual Developer ID signing, notarization, Linux installation and graphics drivers, and Windows installation and restart need their platform checks.
