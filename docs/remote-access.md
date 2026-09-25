# Remote access (experimental)

MonoCode can run Codex and Claude Code sessions on a separate Windows, Linux, or macOS host. The host owns the provider processes and session database. Closing the desktop, closing its remote view, or losing the SSH tunnel does not stop a host session.

Select your project in the existing project rail. The **This computer** control in an empty session chooses the machine. Remote sessions appear in that machine's session selector; reopening the project and selecting the machine lets you return to them from any connected desktop.

## Connect through SSH

In **Settings → Connections → Add machine**, enter an SSH address (`user@my-mac-mini`) or an alias from your SSH config and click **Connect**. The machine picker also links to this Settings page. An optional name and SSH port are available.

MonoCode downloads the host package matching the desktop release and remote architecture, verifies its checksum, installs a background service, pairs this desktop, and opens a private SSH forward. Node is included in the host package; users do not build the host, install Node, copy tokens, or run a tunnel command. Existing running hosts are reused without interrupting their agents.

Prerequisites:

- SSH must already be enabled and reachable on the host. MonoCode uses the desktop's OpenSSH client and normal SSH config, keys, and agent. Windows clients need the OpenSSH Client feature installed.
- Hosts: Windows 10/11 or Server 2019+, Linux, or macOS, on x64 or arm64. Mac/Linux need `curl` or `wget`, `tar`, and `shasum` or `sha256sum`. Windows needs Windows PowerShell 5.1, OpenSSH Server, and Task Scheduler; no WSL or Unix shell is required. Setup detects the remote platform through SSH.
- Install and authenticate Codex and/or Claude Code on the host under the connecting OS account. Its non-interactive login shell must find the provider CLIs.
- Linux needs systemd user services. Setup enables lingering so the host survives logout; if this requires administrator access, Settings displays the recovery command. macOS needs an active desktop login; keep that Mac signed in and awake.
- Windows uses a per-user Task Scheduler task, with no time limit, under the connecting user's normal permissions. Sign in to that same account at the Windows desktop and keep it signed in and awake. Locking the desktop and disconnecting SSH are fine; signing out or rebooting interrupts agents. The task starts again at the next login. No Windows password is stored for scheduling. This uses an interactive logon token because S4U tasks cannot access network or encrypted files. [Microsoft task logon documentation](https://learn.microsoft.com/en-us/windows/win32/taskschd/principal-logontype).
- Windows provider discovery supports native `.exe` installations and standard npm installations of `@openai/codex` and `@anthropic-ai/claude-code`. npm entry points run with the bundled Node runtime; arbitrary custom `.cmd` wrappers are not supported. SSH aliases can supply Windows domain/user names through the normal SSH config.

SSH host verification and password/passphrase prompts appear in Settings. Changed host keys are rejected by OpenSSH. Passwords/passphrases are used only for the current authentication, not saved. When key/agent authentication is available, opening the remote view restores a lost tunnel automatically. If SSH needs another prompt, use **Reconnect** in Settings. Reconnecting uses the saved device credential and checks the host's identity.

The forward binds to a temporary port on the laptop's loopback interface. Removing a connection or quitting the desktop closes only that forward. It does not stop the host service or its agent sessions.

## Start a session

1. Select your project in the existing project rail.
2. In an empty session, use **This computer** to choose the saved machine.
3. Link that project to an existing checkout's absolute path on the host, such as `/home/me/code/my-app`.
4. Create a Codex or Claude session and send a prompt.

The mapping is remembered per host. Paths may differ between your laptop and host. A project must currently exist in the laptop's rail; remote-only rail entries are a later integration step. Source files stay on the host; this feature shares host-owned sessions, not working-directory synchronization.

## Manual connection (advanced / development)

Use Node.js 24 or newer on the host. Install and sign in to Codex and/or Claude Code under the same OS account that runs the host. The host uses that account's default provider credentials and searches its PATH, `~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin`, and `/usr/bin`.

From a checkout of this version of MonoCode on the host:

```sh
npm ci
npm run host:build
node build/host/monocode-host.mjs start
node build/host/monocode-host.mjs pair --name "My laptop"
```

`start` launches the host independently of the terminal. `pair` prints a device ID and a device token; copy the token to the receiving desktop. Create a separate credential for each desktop. Tokens grant control of the host as its OS user, including provider execution and workspace reads.

The host binds only to `127.0.0.1:3774`. State and logs live in `~/.monocode-host` (`%USERPROFILE%\.monocode-host` on Windows). Unix permissions restrict access to the owner; Windows ACLs restrict it to the current user, SYSTEM, and Administrators. Use `--data-dir` and `--port` to override them. `serve` runs in the foreground for debugging.

Open an SSH tunnel from your laptop:

```sh
ssh -N -L 3774:127.0.0.1:3774 user@your-host
```

Under **Settings → Connections → Connect to an existing host by URL**, enter `http://127.0.0.1:3774` and the device token.

When the laptop wakes, restore the SSH tunnel. The remote view reconnects automatically and loads the host's latest persisted snapshot. An uncertain send retains its draft and request ID; **Retry request** asks the host for that same command's receipt, so a lost response does not submit the prompt again.

HTTPS endpoints can also be entered if you operate a reverse proxy to the loopback host. Plain HTTP is accepted only for loopback endpoints. Native desktop HTTP carries the saved credential; it is not exposed to the renderer or browser storage. The native connection store is `remote-machines.json` in the desktop app's data directory, with mode 0600 on Unix. This initial implementation does not use the OS keychain.

## Manage the host

```sh
node build/host/monocode-host.mjs status
node build/host/monocode-host.mjs devices
node build/host/monocode-host.mjs revoke DEVICE_ID
node build/host/monocode-host.mjs stop
```

SSH setup names each device credential after the desktop's computer name. Removing a saved connection from the desktop does not stop the host or revoke the device token. Use `revoke` on the host to remove access. Stopping the host interrupts active turns; restarting retains their transcripts and marks them interrupted. No uncertain provider operation is automatically replayed after a host crash.

The machine must remain awake. Manual `start` launches a detached process. `service install` installs a user service; SSH setup runs it automatically. Only one host may own a data directory. Restart the host after changing its provider installation or PATH. Service installs preserve an already-running host, including one previously started manually.

SSH-installed hosts have a launcher at `~/.monocode-host/bin/monocode-host`; use it in place of `node build/host/monocode-host.mjs` in management commands. Linux services are named `monocode-host.service`; macOS uses `com.monocode.host`. A service manager can restart a stopped process: to stop a service permanently, use `systemctl --user disable --now monocode-host.service` on Linux, or `launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/com.monocode.host.plist` on macOS and remove that plist to disable the next login's startup.

On Windows, the launcher is `%USERPROFILE%\.monocode-host\bin\monocode-host.cmd`. The scheduled task is named `MonoCode Host-<user SID>`. To disable automatic startup, disable that task in Task Scheduler and then run the launcher's `stop` command. Normal cancellation and shutdown stop the provider's process tree. A plain manual `start` is detached, but use `service install` for SSH-hosted Windows sessions so Task Scheduler owns the process independently of the SSH login.

## Release packaging

`npm run host:package` builds a self-contained package for the current Windows/Mac/Linux architecture. `npm run host:package -- --all` builds all six archives, using pinned official Node binaries and checksums. Archives contain the host bundle, runtime, launcher, and licenses. `host/package.mjs` pins the runtime version. Cross-packaging Windows on Unix requires `zip` and `unzip`; native Windows packaging uses PowerShell.

The release workflow publishes `monocode-host-{darwin,linux}-{arm64,x64}.tar.gz`, `monocode-host-win32-{arm64,x64}.zip`, and their `.sha256` files alongside the desktop release. SSH setup downloads from the exact desktop version's GitHub release, then installs under `~/.monocode-host/runtime`.

**Unreleased development builds:** automatic first-time installation requires those archives to be published for that version. This code change does not publish a release or retrofit assets onto an existing tag. Until the matching release is available, use the manual development connection above. A missing archive produces an explicit error in Settings. No fallback to an arbitrary latest release or unverified download is used. Connecting does not automatically upgrade a running host.

## Scope of this first version

Supported: persistent remote text conversations, existing Codex/Claude adapters, follow-up turns, approvals, questions, cancellation, per-device revocation, reconnect, tracked Git diffs against HEAD, and bounded text-file previews. The desktop polls the host and downloads only transcript blocks that changed since its last update (every 0.75 s while a session runs, 3 s otherwise). The host writes streamed output in 120 ms batches and keeps a bounded event journal.

Remote history currently lives in the remote view's session selector, rather than the main local session sidebar. Local sessions retain their existing lifecycle. Remote attachments, worktree creation, editing files, terminals, model catalog discovery, named provider accounts, `/operator`, automations, orchestration, host upgrades from Settings, and account-based tunnels are not implemented yet. Remote prompts are sent directly to the provider; MonoCode's local slash-command workflow is not applied.

The headless host runs the reused TypeScript adapters with a Node process backend. It proves the execution boundary without introducing the planned Rust daemon/worker IPC yet. Node is included in host release archives, separately from the desktop application.

## Verify

```sh
npm run host:build
npm run test:host
npm run check:web
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test
```

Host tests use fake provider executables and temporary loopback servers. They do not contact paid models. They cover both real adapter transports, cross-client reattachment, duplicate sends, approval races, interruption recovery, device revocation, path checks, and detached host lifecycle. On Node 26, use `NODE_OPTIONS=--no-experimental-webstorage npm run check:web` to avoid its experimental global storage interfering with the existing happy-dom tests.

For the real OpenSSH transport and native askpass smoke test on Linux/macOS, build with `npm run host:package` and `cargo build --bin monocode`, then run `python3 scripts/test-remote-ssh.py`. It uses a disposable loopback sshd, temporary keys and known-hosts file, and an isolated packaged host. It leaves personal SSH configuration, provider credentials, and OS services untouched.

Host CI runs on Windows, macOS, and Linux. Windows-specific tests cover ACL inheritance, Task Scheduler definitions, bootstrap parsing/installation, and provider child-process cleanup. They use temporary data and mocked task registration so normal test runs do not install or replace a real user's background task. Full SSH-to-Task-Scheduler setup must also be validated on a signed-in Windows host before a supported release.
