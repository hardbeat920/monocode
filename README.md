<p align="center">
  <img src="public/monocode.png" alt="MonoCode" width="88" />
</p>

<h1 align="center">MonoCode</h1>

<p align="center">
  <strong>A GUI for your coding agents.</strong>
</p>

<p align="center">
  <img width="1680" height="1050" alt="MonoCode with agent sessions open side by side" src="https://github.com/user-attachments/assets/2cd4a6ec-eb1e-4b45-8627-a76442ea3874" />
</p>

Works with your subscriptions on Claude Code, Codex, Cursor, Grok Build, OpenCode, Antigravity, Pi, omp, fx, Hermes Agent, and Devin. If they’re installed and logged in, MonoCode can run them. Tabs are sessions. The composer is the input. MonoCode does not sell tokens.

## Install

Install and sign in to at least one supported agent first. See [provider setup](docs/providers.md) for instructions.

| Platform | Download |
| --- | --- |
| macOS, Apple Silicon | [MonoCode.dmg](https://dl.usemono.dev/MonoCode.dmg) |
| macOS, Intel | [MonoCode_x64.dmg](https://dl.usemono.dev/MonoCode_x64.dmg) |
| Linux, x86_64 | [.deb, .rpm, or AppImage](https://github.com/hardbeat920/monocode/releases/latest) |
| Windows, x86_64 | [Installer](https://github.com/hardbeat920/monocode/releases/latest) |

On macOS, open the DMG and drag MonoCode to Applications. On Windows, run the installer. See [Linux setup](docs/install.md) for dependencies and package instructions.

## Get started

Open a project folder, choose an agent, and send a message. Each tab is a session. Split panes to work with sessions side by side.

- Keep conversations organized by project.
- Use separate Git worktrees for parallel tasks.
- Edit files and review changes in the app.
- Start a message with `/operator` to let an agent open sessions and manage worktrees and notes.

Monos are experimental agents that work across your projects. They keep a memory and can run scheduled tasks you set up with them.

MonoCode is still early. If something breaks, [report a bug](https://github.com/hardbeat920/monocode/issues).

## Guides

- [Provider setup](docs/providers.md)
- [Linux installation](docs/install.md)
- [Agent access with /operator](docs/agent-access.md)
- [Remote sessions, experimental](docs/remote-access.md)

## Build from source

You’ll need Node.js 20+ and a stable Rust toolchain. See [build setup](docs/building.md) for platform dependencies and packaging.

```bash
npm install
npm run tauri dev
```

Small, focused contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

### macOS packages

Build on macOS with the Node.js and Rust prerequisites above and [Xcode Command Line Tools](https://v2.tauri.app/start/prerequisites/#macos) installed (`xcode-select --install`):

```bash
npm ci
npm run tauri -- build --bundles app,dmg
```

This builds for your Mac's architecture and emits `MonoCode.app` under `target/release/bundle/macos/` and a `.dmg` under `target/release/bundle/dmg/`.

The repository enables updater artifacts by default. That build also generates an updater archive and signature, requiring `TAURI_SIGNING_PRIVATE_KEY` and, if the key is password-protected, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. See [Tauri's updater signing instructions](https://v2.tauri.app/plugin/updater/#signing-updates).

For a local package without updater artifacts or an updater signing key, use this build command instead:

```bash
npm run tauri -- build --bundles app,dmg \
  --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

This override still produces the `.app` and `.dmg`; it skips updater archive and signature generation for this build. The app's updater UI and runtime configuration are separate.

Local builds use the configured ad-hoc macOS signing identity (`-`). Updater signing is separate from Apple code signing and notarization; the [release workflow](.github/workflows/release.yml) supplies those credentials and the updater configuration for published packages.

## Contributors

Thanks to everyone who contributes to MonoCode!

[![MonoCode contributors](https://contrib.rocks/image?repo=hardbeat920/monocode)](https://github.com/hardbeat920/monocode/graphs/contributors)

Contributor image by [contrib.rocks](https://contrib.rocks).

## License

[MIT](LICENSE). Provider names and logos are trademarks of their owners. See [NOTICE](NOTICE).
