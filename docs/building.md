# Build from source

MonoCode supports macOS, Linux, and Windows. You need Node.js 20+ and a current stable Rust toolchain. Install and sign in to at least one agent CLI using [provider setup](providers.md).

## Platform prerequisites

On Linux, install the standard Tauri prerequisites, including `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libsoup-3.0-dev`, and `libjavascriptcoregtk-4.1-dev` on Debian/Ubuntu. The setup helpers below install the native dependencies and packaging tools for their distributions.

On Windows, install the following prerequisites before running or building from source:

- [Microsoft C++ Build Tools](https://v2.tauri.app/start/prerequisites/#microsoft-c-build-tools), with the **Desktop development with C++** workload selected.
- A stable **MSVC Rust toolchain**. For x86_64 Windows, use `stable-x86_64-pc-windows-msvc`, which is also the toolchain used by the Windows packaging script. Run `rustup show active-toolchain` to check your selected toolchain. See [Tauri's Rust setup](https://v2.tauri.app/start/prerequisites/#rust) for installation instructions.
- The [WebView2 runtime](https://developer.microsoft.com/microsoft-edge/webview2/), required to run the app with `npm run tauri dev`. Windows 10 and 11 normally include it; if it is missing, install the **Evergreen Bootstrapper** from the WebView2 download page. The packaged installer bootstraps WebView2 when it is missing, but running from source requires the runtime to be installed already.

## Run locally

From the repository root:

```bash
npm install
npm run tauri dev
```

## Ubuntu / Debian packages

On an Ubuntu/Debian workstation, the repository can install the native Tauri prerequisites and build distributable Linux packages directly:

```bash
npm run setup:linux:deb
npm ci
npm run build:linux
```

The Linux build emits `.deb` and AppImage bundles under `target/release/bundle/`, in the `deb/` and `appimage/` directories. `build:linux` repacks the AppImage so it uses the host WebKitGTK 4.1 stack instead of bundled Ubuntu libraries. See [Linux installation](install.md) for runtime dependencies and package instructions.

## Fedora / Enterprise Linux packages

On Fedora or Enterprise Linux 10 (registered RHEL, Rocky, Alma, CentOS Stream, Oracle):

```bash
npm run setup:linux:fedora
npm ci
npm run build:fedora
```

On EL 10, the setup helper enables EPEL 10 and CRB automatically, since the development packages need CRB. On Oracle Linux it enables `ol10_codeready_builder` and `ol10_developer_EPEL`. Fedora does not need these extra repositories.

The build emits a `.rpm` under `target/release/bundle/rpm/`, installable with:

```bash
sudo dnf install ./target/release/bundle/rpm/MonoCode-*.rpm
```

EL 9 and older are unsupported (`webkit2gtk4.1-devel` only exists in EPEL 10). The published release `.rpm` is built on Enterprise Linux 10 so it loads on Fedora and EL 10. See [Linux installation](install.md#fedora--enterprise-linux-packages) for release package installation and runtime repository setup.

## Windows packages

```bash
npm ci
npm run build:windows
```

The Windows build emits an NSIS installer under `target/release/bundle/nsis/`.

## Tauri configuration

- [src-tauri/tauri.conf.json](../src-tauri/tauri.conf.json) contains the shared application and bundle configuration.
- [src-tauri/tauri.linux.conf.json](../src-tauri/tauri.linux.conf.json) loads automatically for Linux development and builds.
- [src-tauri/tauri.windows.conf.json](../src-tauri/tauri.windows.conf.json) loads automatically for Windows development and builds.

## Beta releases and updates

Prerelease tags such as `v0.9.1-beta.1` publish to `beta/latest.json`, and beta builds use that feed even when a stable updater endpoint is configured. Beta releases leave the stable feed and macOS download links unchanged.

To trial AppImage updates, install a beta AppImage in a writable directory, publish a newer beta, and verify the update downloads, installs, and relaunches successfully before publishing a stable version.

The [release workflow](../.github/workflows/release.yml) packages the platform builds and publishes the updater feed. [scripts/release-channel.cjs](../scripts/release-channel.cjs) selects the stable or beta feed.

Read [CONTRIBUTING.md](../CONTRIBUTING.md) for contribution guidelines and checks. The experimental remote host has its own [build and packaging instructions](remote-access.md#release-packaging).
