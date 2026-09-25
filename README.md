<p align="center">
  <img src="public/monocode.png" alt="MonoCode" width="88" />
</p>

<h1 align="center">MonoCode</h1>

<p align="center">
  <strong>A desktop UI for your coding agents.</strong>
</p>

<p align="center">
  <img width="1680" height="1050" alt="Screenshot 2026-09-04 at 06 34 00" src="https://github.com/user-attachments/assets/2cd4a6ec-eb1e-4b45-8627-a76442ea3874" />
</p>

Works with your subscriptions on Claude Code, Codex, Cursor, Grok Build, OpenCode, Antigravity, Pi, omp, fx, and Hermes Agent. If they’re installed and logged in, MonoCode can run them. Tabs are sessions. The composer is the input. MonoCode does not sell tokens.

## Install

> Install and log in to at least one provider first:
>
> - [Claude Code](https://claude.com/product/claude-code) - `claude auth login`
> - [Codex](https://developers.openai.com/codex/cli) - `codex login`
> - [Cursor CLI](https://cursor.com/cli) - `agent login`
> - [Grok Build](https://docs.x.ai/build/overview) - `curl -fsSL https://x.ai/cli/install.sh | bash` then `grok login`
> - [OpenCode](https://opencode.ai) - `opencode auth login`
> - [Antigravity](https://antigravity.google/docs/cli-install) (macOS/Linux) - `curl -fsSL https://antigravity.google/cli/install.sh | bash`, then run `agy` once to sign in
> - [Antigravity](https://antigravity.google/docs/cli-install) (Windows) - install the CLI with `irm https://antigravity.google/cli/install.ps1 | iex`, then run `agy` once to sign in. Download the [official ACP server](https://github.com/agentclientprotocol/registry/blob/main/antigravity-acp/agent.json) for your Windows architecture and extract both executables to `%LOCALAPPDATA%\agy-acp`. The ACP server also requires an authentication method: create `%USERPROFILE%\.gemini\antigravity-acp\settings.json` with `{"auth":{"type":"oauth-personal"}}` for a personal Google account. CLI sign-in alone does not select the ACP method.
> - [Pi](https://pi.dev/) - `npm install -g @earendil-works/pi-coding-agent`
> - [omp](https://omp.sh) - `curl -fsSL https://omp.sh/install | sh`
> - [fx](https://fx.sh) - `curl -fsSL https://fx.sh/setup.sh | bash` then `fx login`
> - [Hermes Agent](https://github.com/NousResearch/hermes-agent) - macOS/Linux: `curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash`; Windows PowerShell: `iex (irm https://hermes-agent.nousresearch.com/install.ps1)`; then run `hermes model`

macOS (Apple Silicon): download [MonoCode.dmg](https://dl.usemono.dev/MonoCode.dmg), open it, drag MonoCode to Applications.

macOS (Intel): download [MonoCode_x64.dmg](https://dl.usemono.dev/MonoCode_x64.dmg), open it, drag MonoCode to Applications.

Linux (x86_64): download the `.deb` or AppImage from [GitHub Releases](https://github.com/hardbeat920/monocode/releases/latest). Install the `.deb` with `sudo apt install ./MonoCode_*.deb`, or make the AppImage executable with `chmod +x MonoCode_*.AppImage` and run it directly.

Windows (x86_64): download the NSIS installer from [GitHub Releases](https://github.com/hardbeat920/monocode/releases/latest) and run it.

## Some notes

This is very early and you should expect bugs.

### Agent access to MonoCode

Type `/mono` at the start of a composer message to enable MonoCode access in that thread. For example, `/mono start two Codex sessions: one to inspect the API and one to review the UI`, or `/mono list my notes`. The slash picker also offers this command. The transcript shows only the request text in a translucent amber bubble; MonoCode removes the command from the request sent to the agent and supplies the local `app` CLI path and instructions on that turn. Later turns in the same thread can use the CLI without repeating `/mono`; other threads receive no CLI instructions or app access. The CLI can act only during an active agent turn. The agent can run the shown `app --help` command for the exact JSON input fields.

- `models.list` shows available providers, models, settings, and permission modes.
- `sessions.start` opens a tab in the current project with a prompt. By default it submits the prompt; set `draft: true` to save it unsent without starting an agent turn. It accepts a provider, model, effort or other model settings, permission mode, and current checkout or new worktree choice. Omit `runtimeMode` to inherit the calling session's permission mode, or set it explicitly to override. It returns the new session ID as soon as the tab and prompt are accepted, so the agent can move it into a folder immediately.
- `sessions.list` shows project sessions. `sessions.read` returns up to three recent user/assistant exchanges, with a cursor for older exchanges and a per-message character cap. `sessions.send` submits a follow-up to an idle session, while `sessions.draft` saves an unsent message for the user to review. `folders.list` and `folders.move` organize project sessions in sidebar folders, including a new folder.
- `notes.list` returns titles and short previews; `notes.read` returns one full note by ID.

Orchestration workers keep their existing scoped `control` workflow and do not receive this app access.

Small, focused pull requests are welcome. Anything large is worth an issue first - see [CONTRIBUTING.md](CONTRIBUTING.md).

## Build from source

Supports macOS, Linux, and Windows.

Need Node.js 20+ and a current stable Rust toolchain. On Linux, ensure standard Tauri prerequisites are installed (e.g. `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `libsoup-3.0-dev`, `libjavascriptcoregtk-4.1-dev`). On Windows, the installer bootstraps the [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/) runtime when it is missing.

```bash
npm install
npm run tauri dev
```

### Ubuntu / Debian packages

On an Ubuntu/Debian workstation, the repository can install the native Tauri prerequisites and build distributable Linux packages directly:

```bash
npm run setup:linux:deb
npm ci
npm run build:linux
```

The Linux build emits `.deb` and AppImage bundles under `target/release/bundle/`.
Tauri loads `src-tauri/tauri.linux.conf.json` automatically for Linux development and builds.

### Windows packages

```bash
npm ci
npm run build:windows
```

The Windows build emits an NSIS installer under `target/release/bundle/nsis/`.
Tauri loads `src-tauri/tauri.windows.conf.json` automatically for Windows development and builds.

## License

[MIT](LICENSE). Provider names and logos are trademarks of their owners - see [NOTICE](NOTICE).
