# Contributing

MonoCode is early and I’m the only maintainer, so small and focused lands much faster than large and ambitious. Past that, the door is open - bug reports and fixes are genuinely welcome.

## Get it running

You need Node.js 20+, a current stable Rust toolchain, and at least one provider CLI installed and logged in:

- [Claude Code](https://claude.com/product/claude-code) - `claude auth login`
- [Codex](https://developers.openai.com/codex/cli) - `codex login`
- [Cursor CLI](https://cursor.com/cli) - `agent login`
- [Grok Build](https://docs.x.ai/build/overview) - `curl -fsSL https://x.ai/cli/install.sh | bash` then `grok login`
- [OpenCode](https://opencode.ai) - `opencode auth login`
- [Pi](https://pi.dev/) - `npm install -g @earendil-works/pi-coding-agent`
- [omp](https://omp.sh) - `curl -fsSL https://omp.sh/install | sh`
- [fx](https://fx.sh) - `curl -fsSL https://fx.sh/setup.sh | bash` then `fx login`

macOS and Linux are supported targets. On Debian/Ubuntu, `npm run setup:linux:deb` installs the native Tauri build dependencies.

```bash
npm install
npm run tauri dev
```

One provider is enough. MonoCode probes for each CLI at startup and disables the ones it can’t find, with a hint about how to install them, so a missing Codex doesn’t stop you from working on anything else.

## Where things live

- `src/chrome/` - the window frame: title bar, sidebar, composer, tabs, model picker
- `src/surfaces/` - the panes inside a tab: transcript, file editor, diff, terminal
- `src/lib/harness/` - one adapter per provider, plus the registry they plug into
- `src-tauri/src/` - the Rust side: PTYs, filesystem and git, session storage, native window

`src/lib/harness/` is the most useful place to start if you want to fix something real. Each provider has an adapter (`claudeAdapter.ts`) that implements the shared `HarnessAdapter` lifecycle from `registry.ts`, and a protocol module (`claudeProtocol.ts`) that translates the CLI’s output into MonoCode’s own event types. The protocol modules are pure functions with unit tests beside them, so you can fix a Codex parsing bug with only Claude Code installed.

## Before you push

```bash
npm run check
```

That runs what CI runs: vitest, `tsc --noEmit`, `cargo fmt`, `cargo clippy`, and `cargo test`. If it’s green locally it should be green on GitHub. `npm run check:web` and `npm run check:rust` run the two halves separately when you only touched one side.

## Pull requests

Keep a PR to one thing, and say what changed and why. The [PR template](.github/pull_request_template.md) covers the rest. If it changes the UI, a before/after screenshot helps a lot.

For anything that moves product direction - a new surface, new provider behavior, a refactor that changes the shape of the app - open an issue first. That’s not gatekeeping, I’d just rather you hear “I’m already halfway through that” before you write it than after.

I might close a PR, ask you to shrink it, or end up implementing the idea differently. That’s a call about scope and timing, not about you or the quality of your work.

Be kind: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Security reports: [SECURITY.md](SECURITY.md).
