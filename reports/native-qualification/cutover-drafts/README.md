<p align="center">
  <img src="crates/view-transcript/assets/monocode.png" alt="MonoCode" width="88" />
</p>

<h1 align="center">MonoCode</h1>

MonoCode is a desktop UI for coding agents. It runs provider CLIs, displays their conversations in tabs and split panes, and lets you review approvals and file changes. The desktop uses GPUI. The desktop and remote host are Rust executables.

Install and sign in to at least one provider CLI before starting a session. MonoCode probes installed CLIs and lets you set custom binary paths in Settings. It does not sell tokens. Provider sign-in, subscription access, and API charges remain with the provider.

## Native release status

The native port is under qualification. Existing download links may still serve the earlier desktop. Build this checkout to try the native app with an isolated data directory. [Native qualification](reports/native-qualification.md) records tested behavior and open release gates.

The build targets macOS on Apple Silicon and Intel, Linux on x86_64, and Windows on x86_64. Compilation does not establish installer, graphics driver, audio, or updater behavior on every system. The macOS quick composer is macOS-only.

## Build from source

Use a current stable Rust toolchain. macOS needs the Xcode command-line tools. Intel macOS also needs NASM for the AVIF decoder. Windows needs the MSVC build tools and NASM. Linux prerequisites are listed in [packaging](packaging/README.md) and installed by this script:

```sh
bash scripts/install-native-linux-deps.sh
```

Build the desktop and host, then start the desktop with a test profile:

```sh
cargo build -p monocode-app -p monocode-host --bins --locked
cargo run -p monocode-app --locked -- --data-dir /tmp/monocode-native-dev
```

On Windows, use a path under `$env:TEMP` for `--data-dir`. Without that flag, the native app uses the existing MonoCode profile. Back up that profile before testing migration behavior.

Node, Tauri, WebKit, and WebView2 are not desktop or host build prerequisites. Provider CLIs have their own requirements.

For smaller development artifacts, set `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0` before running Cargo.

## Remote access

MonoCode Host runs agents and stores their sessions on another machine. In Settings, open Connections and use a pairing link or SSH setup. Native SSH setup downloads the exact host version and verifies its release checksum. It requires published matching host archives.

For an isolated local development host without a login service:

```sh
cargo run -p monocode-host --bin monocode-host --locked -- connect --no-service --json --data-dir /tmp/monocode-host-test
```

This starts a detached host and prints a pairing link. Stop the same test host when done:

```sh
cargo run -p monocode-host --bin monocode-host --locked -- stop --data-dir /tmp/monocode-host-test
```

See [remote access](docs/remote-access.md) for installation, service behavior, security, and current limits.

## Agent access to MonoCode

Start a composer message with `/operator` to grant that thread access to the MonoCode app CLI. MonoCode removes the command from the provider prompt and supplies the CLI path and instructions. The transcript keeps the request text. The CLI can act only during an active turn.

The CLI can list models, start and arrange sessions, send follow-ups, save drafts, move sessions into folders, and read notes. Run the supplied `app --help` command for the exact commands and JSON fields. Orchestration workers keep their scoped `control` workflow.

## Contributing and packages

Small, focused fixes are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for the source map and checks. [Packaging](packaging/README.md) describes Cargo bundle commands, platform dependencies, signing, and updater artifacts.

## License

[MIT](LICENSE). Provider names and logos are trademarks of their owners. See [NOTICE](NOTICE).
