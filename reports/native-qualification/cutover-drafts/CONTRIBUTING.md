# Contributing

MonoCode is early. Keep a pull request focused on one problem and describe the resulting behavior. Open an issue before changing product direction or reorganizing large parts of the app.

New providers are paused while the current adapters converge on session lifecycle, catalog discovery, usage, approvals, commands, and skills. Fixes and tests for existing providers are welcome.

## Run the native app

Use a current stable Rust toolchain and the platform build dependencies in [packaging](packaging/README.md). On Linux, run `bash scripts/install-native-linux-deps.sh`. Windows needs MSVC and NASM. Intel macOS needs NASM.

Install and sign in to one provider CLI. A missing provider does not prevent you from using another one. Custom executable paths belong in Settings.

```sh
cargo build -p monocode-app -p monocode-host --bins --locked
cargo run -p monocode-app --locked -- --data-dir /tmp/monocode-native-dev
```

On Windows, choose a path under `$env:TEMP`. Use an isolated profile for development. Test migrations on a backup, never on the only copy of a working profile.

## Where things live

- `apps/monocode-app/src/` has boot, native windows, app adapters, pages, and the shell.
- `crates/core/` has persisted types, provider-independent events, and reducers.
- `crates/harness/` has provider catalogs, protocols, and adapter lifecycles.
- `crates/process/` owns child processes. `crates/terminal/` owns PTYs.
- `crates/engine/` has sessions, submit, workspace, projects, Inbox, automations, attention, and remote state.
- `crates/store/`, `crates/settings/`, and `crates/layout/` preserve database, preference, and workspace contracts.
- `crates/view-*/` and `crates/ui/` have GPUI views and shared controls. Views receive app behavior through host traits.
- `crates/host/` runs the headless Rust host. `crates/remote/` has its protocol, TLS, pairing, SSH, and service code.
- `crates/package/`, `crates/updater/`, and `packaging/` create native bundles and preserve distribution assets.

Keep protocol changes separate from provider UI changes when possible. Pure protocol fixtures should run without a provider installation or paid turn. A live provider test must name the provider version, use an isolated project and profile, and retain its actual result.

## Checks before a pull request

Install Node for the fake provider transport tests. They run inline fixture scripts and do not contact paid models. Windows compiles a native fixture launcher with the existing Rust toolchain. The desktop and host build and runtime do not require Node.

Run the checks that apply to the change. Native CI runs these on macOS, Linux, and Windows:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --lib --bins --all-features --locked
cargo test -p monocode-host -p monocode-remote -p monocode-package --tests --locked
cargo test -p monocode-app --test ssh_askpass --all-features --locked
cargo test -p monocode-platform --test inline_video --locked
cargo build -p monocode-app -p monocode-host --bins --locked
```

The inline video fixture needs native media support. Windows Session 0 cannot create the required swap chain. A CI skip there requires a separate run in an interactive desktop before release.

Use focused tests while editing. Report any required check you could not run and why. A successful compile does not replace a GUI, provider, installer, or updater test.

For limited disk space, set `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0`. Keep unrelated checkouts and accepted screenshots intact.

## UI and compatibility changes

Test the actual interaction when feasible. Before-and-after screenshots help reviewers assess layout changes. GPUI test windows do not provide native platform handles, so platform integration fixtures need real windows. Preserve keyboard focus, child event handling, and hidden-view behavior when changing the shell.

Existing session JSON, SQLite tables, connection credentials, host services, and updater signatures are compatibility contracts. Use owned copies and disposable services for migration checks. Never include private session content or credentials in a PR or test log.

## Pull requests

State the problem, the new behavior, and relevant validation in the [PR template](.github/pull_request_template.md). Do not mix unrelated source changes. Describe unverified platform behavior separately from a passing local test.

Follow [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Send security reports through [SECURITY.md](SECURITY.md).
