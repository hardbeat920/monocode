# Provider setup

Install and sign in to at least one supported agent before using MonoCode. MonoCode runs the CLIs you install and usually reuses their existing logins. Antigravity's ACP server has its own sign-in.

Follow each provider’s linked installation guide for platform requirements and installation options. Use the installation and login commands below where provided.

- [Claude Code](https://claude.com/product/claude-code) - `claude auth login`
- [Codex](https://developers.openai.com/codex/cli) - `codex login`
- [Cursor CLI](https://cursor.com/cli) - `agent login`
- [Grok Build](https://docs.x.ai/build/overview) - `curl -fsSL https://x.ai/cli/install.sh | bash` then `grok login`
- [OpenCode](https://opencode.ai) - `opencode auth login`
- [Antigravity](https://antigravity.google/docs/cli-install) - on macOS/Linux, `curl -fsSL https://antigravity.google/cli/install.sh | bash`. On Windows, set the absolute path to the native `agy_acp_server.exe` in **Settings → Providers → Antigravity** (automatic discovery is not available).
- [Pi](https://pi.dev/) - `npm install -g @earendil-works/pi-coding-agent`
- [omp](https://omp.sh) - `curl -fsSL https://omp.sh/install | sh`
- [fx](https://fx.sh) - `curl -fsSL https://fx.sh/setup.sh | bash` then `fx login`
- [Hermes Agent](https://github.com/NousResearch/hermes-agent) - macOS/Linux: `curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash`; Windows PowerShell: `iex (irm https://hermes-agent.nousresearch.com/install.ps1)`; then run `hermes model`
- [Devin CLI](https://docs.devin.ai/cli) - macOS/Linux: `curl -fsSL https://cli.devin.ai/install.sh | bash` (or `brew install --cask devin-cli`); Windows PowerShell: `irm https://static.devin.ai/cli/setup.ps1 | iex`; then run `devin auth login` (MonoCode reuses that login)

Antigravity's ACP server supports Google OAuth sign-in. When authentication is required, MonoCode opens the sign-in page and retries the session after login. On Windows, configure the path to `agy_acp_server.exe` in **Settings → Providers → Antigravity**.

One provider is enough. MonoCode probes for each CLI at startup and disables the ones it can’t find, with a hint about how to install them, so a missing Codex doesn’t stop you from working on anything else.

Once a provider is installed and signed in, open a project folder in MonoCode, choose that agent, and send a message. For sessions on another machine, install and authenticate the providers on the host as described in [remote access](remote-access.md).

To run MonoCode from a checkout, see [building from source](building.md). Return to the [README](../README.md) for desktop downloads.
