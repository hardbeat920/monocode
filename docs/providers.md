# Provider setup

Install and sign in to at least one supported agent before using MonoCode. MonoCode runs the CLIs you install and reuses their existing logins.

Follow each provider’s linked installation guide for platform requirements and installation options. Use the installation and login commands below where provided.

- [Claude Code](https://claude.com/product/claude-code) - `claude auth login`
- [Codex](https://developers.openai.com/codex/cli) - `codex login`
- [Cursor CLI](https://cursor.com/cli) - `agent login`
- [Grok Build](https://docs.x.ai/build/overview) - `curl -fsSL https://x.ai/cli/install.sh | bash` then `grok login`
- [OpenCode](https://opencode.ai) - `opencode auth login`
- [Antigravity](https://antigravity.google/docs/cli/acp) - install the current CLI on Windows, macOS, or Linux, confirm `agy acp --help` works, then run `agy` or `agy acp --login` once to sign in. MonoCode launches `agy acp` and reuses the CLI login. Existing standalone ACP installs also work: keep `agy_acp_server.exe` (Windows) or `agy_acp_server.par` (macOS/Linux) beside `localharness_external`; these retain their own configuration and take precedence over CLI auto-detection. Set the Antigravity CLI path to `agy.exe` / `agy` to select the CLI explicitly.
- [Pi](https://pi.dev/) - `npm install -g @earendil-works/pi-coding-agent`
- [omp](https://omp.sh) - `curl -fsSL https://omp.sh/install | sh`
- [fx](https://fx.sh) - `curl -fsSL https://fx.sh/setup.sh | bash` then `fx login`
- [Hermes Agent](https://github.com/NousResearch/hermes-agent) - macOS/Linux: `curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash`; Windows PowerShell: `iex (irm https://hermes-agent.nousresearch.com/install.ps1)`; then run `hermes model`
- [Devin CLI](https://docs.devin.ai/cli) - macOS/Linux: `curl -fsSL https://cli.devin.ai/install.sh | bash` (or `brew install --cask devin-cli`); Windows PowerShell: `irm https://static.devin.ai/cli/setup.ps1 | iex`; then run `devin auth login` (MonoCode reuses that login)

One provider is enough. MonoCode probes for each CLI at startup and disables the ones it can’t find, with a hint about how to install them, so a missing Codex doesn’t stop you from working on anything else.

Once a provider is installed and signed in, open a project folder in MonoCode, choose that agent, and send a message. For sessions on another machine, install and authenticate the providers on the host as described in [remote access](remote-access.md).

Both the production desktop app and the GPUI native preview support Antigravity's built-in `agy acp` server, including Windows auto-detection from `%LOCALAPPDATA%\agy\bin`. In either app, an explicit CLI path can select `agy` / `agy.exe` when a standalone ACP server is also installed. The CLI reuses its cached sign-in; standalone servers keep their own authentication configuration.

To run MonoCode from a checkout, see [building from source](building.md). Return to the [README](../README.md) for desktop downloads.
