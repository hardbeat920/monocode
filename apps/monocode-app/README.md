# Native app

MonoCode draws its desktop UI with Rust and GPUI. It boots the shared engine, restores workspace tabs, and connects the native views to sessions, files, terminals, projects, inbox, settings, and remote hosts.

## Run

Use a separate data directory for development while another MonoCode instance is running:

```sh
cargo run -p monocode-app -- --data-dir /tmp/monocode-native-dev
```

To inspect existing sessions, back up SQLite into the development directory first:

```sh
mkdir -p /tmp/monocode-native-dev
sqlite3 "$HOME/Library/Application Support/com.monocode.desktop/monocode.db" ".backup /tmp/monocode-native-dev/monocode.db"
cargo run -p monocode-app -- --data-dir /tmp/monocode-native-dev
```

The app chooses `--data-dir`, then `MONOCODE_DATA_DIR`, then the existing MonoCode data directory. It uses the existing database migrations and JSON settings keys. First launch can import WebKit settings into the selected directory by reading the previous app's local storage.

On a machine with limited free space, set `CARGO_PROFILE_DEV_DEBUG=0` for the build. GPUI dependencies still use the workspace's optimized development profile.

The executable also runs `app`, `control`, and `host` subcommands. The first two are the agent control clients. `host` runs the native remote host.

## Validate

```sh
cargo check -p monocode-app --all-targets --features screenshot --locked
cargo test --workspace --lib --bins --features monocode-app/screenshot --locked
cargo test -p monocode-platform --test inline_video --locked
```

The ignored live test runs a supervised provider turn, approves a file write, reloads the session in another process, and resumes the conversation. It requires an isolated data directory and a logged-in provider CLI. It defaults to Claude. Set `MONOCODE_LIVE_HARNESS=codex` to test Codex.

```sh
MONOCODE_DATA_DIR=/tmp/monocode-native-live cargo test -p monocode-app --test live_engine -- --ignored
```

## Capture native views

The screenshot feature captures GPUI's rendered window and exits. Engine views use the supplied data directory.

```sh
cargo build -p monocode-app --features screenshot --locked
./target/debug/monocode-app --data-dir /tmp/monocode-native-dev --screenshot /tmp/monocode-shell.png
./target/debug/monocode-app --view widgets --theme light --screenshot /tmp/monocode-widgets.png
./target/debug/monocode-app --list-views
```

`--view` can open the shell, individual pages, sidebar tabs, or widget galleries. `--size WxH` sets the content size. `--settle-ms` controls image and store loading time before capture. `--open-session` opens a stored session after workspace restoration.

Captures use the display's pixel density. They composite the transparent window over `--backdrop`, which defaults to `#5f5560`. `--backdrop none` preserves alpha. The capture draws stand-ins for macOS traffic lights because AppKit draws the actual controls outside GPUI's scene. Inspect the running app separately to verify desktop blur, native controls, focus, and interaction.

## Build packages

`monocode-package` creates desktop bundles and native host archives. For an Apple Silicon macOS bundle:

```sh
cargo build -p monocode-app -p monocode-host --bins --release --target aarch64-apple-darwin --locked
cargo run -p monocode-package --locked -- bundle --target aarch64-apple-darwin
```

The retained Tauri sources and release path remain available during validation of this branch. The native release workflow stages packages without switching the production update feed.

See the [native packaging instructions](../../packaging/README.md) for platform dependencies and Windows static-runtime flags. The [qualification report](../../reports/native-qualification.md) records tested behavior and the remaining runtime and installation checks.
