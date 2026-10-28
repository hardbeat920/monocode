# Monocode Native preview

Monocode Native is the experimental Rust + GPUI app. The production app still
uses React/Tauri. Native builds have their own Cargo workspace and lockfile.

## Requirements

- Node.js 22+ and a current stable Rust toolchain.
- On macOS, Xcode with macOS components.
- Linux core checks need D-Bus headers and pkg-config (`libdbus-1-dev` and
  `pkg-config` on Debian/Ubuntu).
- At least one installed, signed-in agent CLI. See [provider setup](../providers.md).

The native commands do not require `npm install` or a frontend build.
macOS is the primary preview target. A Linux ARM64 source build has been
smoke-tested in a VM; Linux x64 packages and Windows GUI interaction still need
runtime testing. See [preview limitations](parity.md).

Fenced `mermaid` blocks render as native diagrams in Markdown previews and
conversations. See [Mermaid diagrams](mermaid.md) for examples and limitations.

## Develop

From the repository root:

```sh
npm run native:dev
```

Development uses a separate database. To open a particular project:

```sh
npm run native:dev -- --cwd /path/to/project
```

Type `r` then Enter in the terminal to rebuild and restart. The current app stays
open until the new build succeeds. Ctrl+C stops the app and runner. There is no
hot reload or automatic rebuild on save.

On macOS, the command packages and launches `Monocode Native.app`. Other
platforms launch the executable directly.

## Build

```sh
npm run native:build
```

This creates an optimized build without launching it. **The release app uses the
installed MonoCode database, and changes save there.** No extra flags are needed.

On macOS, open:

```sh
open "crates/monocode-gpui/target/release/Monocode Native.app"
```

The app has a separate name and application ID (`dev.usemono.gpui`) so it can be
installed alongside MonoCode. Local source builds are signed locally, without
Apple notarization or configured automatic updates. Linux and Windows builds
produce a standalone executable
under `crates/monocode-gpui/target/release/`. `CARGO_TARGET_DIR` and
`CARGO_BUILD_TARGET` can change these output paths.

The **Monocode Native Preview** GitHub Actions workflow runs checks and build
verification.

## Data and configuration

Development stores `monocode.db` under:

| Platform | Development directory |
| --- | --- |
| macOS | `~/Library/Application Support/Monocode Development/` |
| Linux | `$XDG_DATA_HOME/Monocode Development/`, or `~/.local/share/Monocode Development/` |
| Windows | `%LOCALAPPDATA%\Monocode Development\` |

`MONOCODE_NATIVE_DATA_DIR` overrides this development directory.

Release builds open `com.monocode.desktop/monocode.db` under macOS Application
Support, Linux's XDG data directory, or Windows' `%APPDATA%`. The app uses the
existing desktop schema and session IDs. GPUI preferences are saved beside the
database in `monocode.db.gpui.sqlite3`.

Supported desktop preferences and workspace state are read from the installed
app's webview localStorage on macOS and Linux. The Windows WebView2 reader is
implemented and covered by synthetic fixtures on Linux; reading a real Windows
profile remains unverified. Native preference edits are stored separately.
Some desktop layout concepts still need native support; see
[preview limitations](parity.md).

The Windows reader always opens a temporary copy of localStorage. Copying while
Tauri is writing is best-effort: an inconsistent copy can leave desktop
preferences incomplete or unavailable for that launch. Close Tauri before
starting GPUI for the first time. See [verification](verification.md) for
recorded checks.

To inspect the installed database without opening a window or changing it, use
the executable from a release build:

```sh
crates/monocode-gpui/target/release/monocode-gpui --check-data
```

Provider CLI paths can be changed in Settings → Providers. CLI authentication
continues to use each provider's existing sign-in.

## Checks and fixtures

```sh
npm run native:check
```

This runs the script tests, Rust formatting, strict Clippy and native tests.
See [verification](verification.md) for individual commands, optional provider
checks and the latest recorded results.

A synthetic long-history fixture uses development data and makes no provider
request:

```sh
MONOCODE_NATIVE_DATA_DIR=/tmp/monocode-native-lab npm run native:dev -- --fixture
```

Fixtures are refused with production data. Use a fresh temporary directory when
you want an empty fixture store.

## Contributing

See [architecture](migration.md) for the crate boundaries and renderer patches,
and [preview limitations](parity.md) for remaining work. The
[session visuals guide](visuals.md) documents the agent-facing visual API and
examples.
