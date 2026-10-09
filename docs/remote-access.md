# Remote access (experimental)

MonoCode can run Claude Code, Codex, Cursor, Grok Build, OpenCode, Pi, OMP, fx, Hermes Agent, and Antigravity sessions on a separate Windows, Linux, or macOS host. The host owns the provider processes and session database. Closing the desktop, closing a session tab, or losing the connection does not stop a host session.

A folder on a connected machine is a project in the rail, marked with a globe. Every session in it runs on that machine, in the same session view and composer as a local session. The Sessions sidebar lists that machine's sessions for the project.

## Connect a machine

There are two ways to add a machine. **Set up over SSH** works from Settings and installs a host package that includes Node. **Pair with a link** needs Node.js 22.13 or newer on the machine and no SSH access from this computer. Both run the same `connect` command on the machine, and both give the desktop a direct TLS route to the host when one exists.

### Set up over SSH

In **Settings → Connections → Add machine**, choose **SSH**, enter an SSH address (`user@my-mac-mini`) or an alias from your SSH config, and click **Set up over SSH**. The machine picker also links to this Settings page. An optional name and SSH port are available.

MonoCode downloads the host package matching the desktop release and remote architecture, verifies its checksum, and runs its `connect` command. `connect` installs a background service, turns on network access over TLS, and prints a one-time pairing link that the desktop redeems. Node is included in the host package; users do not build the host, install Node, copy tokens, or run a tunnel command. A running host of the same version keeps running, and its agents are not interrupted. If this computer cannot reach the host's network addresses, MonoCode pairs and connects through a private SSH forward to the host's loopback port instead, for example for WSL2 in NAT mode or a cloud machine that only opens port 22.

Prerequisites:

- SSH must already be enabled and reachable on the host for SSH setup. MonoCode uses the desktop's OpenSSH client and normal SSH config, keys, and agent. Windows clients need the OpenSSH Client feature installed.
- Hosts: Windows 10/11 or Server 2019+, Linux, or macOS, on x64 or arm64. Mac/Linux need `curl` or `wget`, `tar`, and `shasum` or `sha256sum`. Windows needs Windows PowerShell 5.1, OpenSSH Server, and Task Scheduler; no WSL or Unix shell is required. Setup detects the remote platform through SSH.
- Install and authenticate each provider you want to use on the host under the connecting OS account. The host must be able to find its CLI on PATH or in its standard install directory. Antigravity's ACP server is available only on macOS and Linux.
- Linux needs systemd user services. Setup runs `loginctl enable-linger` for the SSH account so the host survives logout. Lingering applies to all of that account's user services, and `service uninstall` leaves it enabled. If enabling it requires administrator access, Settings displays the recovery command. macOS needs an active desktop login; keep that Mac signed in and awake.
- Windows uses a per-user Task Scheduler task, with no time limit, under the connecting user's normal permissions. Sign in to that same account at the Windows desktop and keep it signed in and awake. Locking the desktop and disconnecting SSH are fine; signing out or rebooting interrupts agents. The task starts again at the next login. No Windows password is stored for scheduling. This uses an interactive logon token because S4U tasks cannot access network or encrypted files. [Microsoft task logon documentation](https://learn.microsoft.com/en-us/windows/win32/taskschd/principal-logontype).
- Windows provider discovery supports native `.exe` installations for supported providers other than Antigravity, and standard npm installations of `@openai/codex` and `@anthropic-ai/claude-code`. Those npm entry points run with the bundled Node runtime; arbitrary custom `.cmd` wrappers are not supported. SSH aliases can supply Windows domain/user names through the normal SSH config.
- The desktop reaches the host directly on TCP port 3774 when one of the host's addresses is reachable from this computer, such as on the same network or tailnet. Otherwise it uses the SSH forward. Allow the port in the host's firewall to use the direct route.

SSH host verification and password/passphrase prompts appear in Settings. Changed host keys are rejected by OpenSSH. Passwords/passphrases are used only for the current authentication, not saved. When key/agent authentication is available, a lost forward is restored automatically. If SSH needs another prompt, use **Reconnect** in Settings. Reconnecting uses the saved device credential and checks the host's identity.

The forward binds to a temporary port on the laptop's loopback interface. Quitting the desktop closes only that forward. It does not stop the host service or its agent sessions.

### Pair with a link

On the machine, run the command shown under **Add machine → Pairing link**. It names the host version that matches your desktop:

```sh
npx monocode-host@<desktop version> connect
```

`connect` does four things:

1. Copies the host into `~/.monocode-host` (`%USERPROFILE%\.monocode-host` on Windows). npm may clear its npx cache, so the service never runs from it.
2. Installs a login service: a systemd user service on Linux, a launch agent on macOS, or a per-user Task Scheduler task on Windows.
3. Turns on network access. The host listens on port 3774 on every interface. Other computers must use TLS with the host's self-signed certificate, which the host creates on first run. Plain HTTP is accepted only from the machine itself.
4. Prints a pairing link, such as `monocode://pair?v=1&name=studio&...`. It works once and expires after 15 minutes.

In MonoCode, paste the link under **Add machine → Pairing link** and click **Pair**. The link lists the host's addresses: LAN addresses first, then Tailscale and other overlay addresses, then the Tailscale MagicDNS name when Tailscale is running. The desktop tries each address, checks that the host presents the certificate whose SHA-256 fingerprint is in the link, and exchanges the one-time code for a device credential.

Run `connect` again whenever you need another link, for example for a second desktop. A running host of the same or a newer version keeps running. An older host is replaced with this version; if it may have running turns, `connect` asks first, and `--yes` skips the question. Over SSH, **Update Host** passes `--yes`.

### Routes and updates

The desktop keeps the route that last answered. When a request cannot connect, the desktop tries the machine's other addresses and then the SSH forward, and sends the request on the first route that answers. A request that may have reached the host is never resent; the renderer confirms it with its command ID. After every route fails, the desktop waits 5 seconds before trying them again. **Retry** in Settings tries at once. The host reports its current addresses on each connection check, so a changed IP address updates the saved list.

The desktop holds one `changes.wait` request open per machine. The host answers as soon as a session is saved, batching writes that arrive within 40 ms, and otherwise after 25 seconds. Open sessions and session lists then load only the blocks that changed. Polling remains as a fallback every 5 to 30 seconds, and at the previous 0.75 to 3 second rate for hosts that predate `changes.wait`.

**Remove** in Settings asks for confirmation and offers two choices. **Remove from this desktop only** deletes the saved connection and closes its forward. The host keeps running, and this desktop's device credential stays valid on it. **Revoke access and remove** first asks the host to revoke the credential this desktop is using, then removes the connection. It needs the machine to be reachable, and if revocation fails the connection is kept. Neither option stops the host, affects other desktops' credentials, or deletes sessions. Adding or pairing the same machine again reconnects its projects, tabs, and history, and revokes the credential this desktop used before.

## Start a session

1. In the project rail, click **+** next to Projects and choose **Open folder on a machine…**.
2. Choose the machine, browse to an existing checkout (or type its absolute path, such as `/home/me/code/my-app`), and click **Open**.
3. Send a message. The first message creates the session on the host with the model, reasoning effort, and permission mode shown in the composer.

The composer's model picker lists models for the providers installed on the host. Model, provider-specific settings, and permission changes apply to the host session directly, and the next turn uses them; a change made during a running turn is applied when that turn finishes. If the host cannot load its model list, or no longer lists the session's model, the session's saved settings remain visible. The normal workspace and branch pickers use the host checkout through the shared file and Git commands. Choose an existing host worktree or create one from a branch before the first message; the session then runs in that working copy. Once the conversation starts, its worktree is fixed, as in a local session. Start a new session to use another worktree. The branch picker can search local and remote branches, create a local branch, and switch the current working copy when it is clean and the project's sessions are idle. Machine connection state appears on its project rail dot.

The **Explorer** sidebar and Go to File use the normal file views for host folders and files. Opening a text file uses the normal editor tabs. Saving writes back to the host; remote reads and edits are limited to 1 MiB text files. File creation, rename, deletion, and project search use the same controls as local projects. The **Changes** sidebar and Git history graph use the normal Git views, including file diffs, staging, discarding, commits, pushing, and pull request creation through `gh` on the host. Remote pull requests use the host's commit list and diff summary for their title and body. An ordinary folder outside Git shows an empty Changes state. Git changes are refreshed every few seconds. Generated commit messages are not available remotely yet.

The composer’s **+** menu supports file and image attachments, Plan mode, and saved drafts when the host advertises these capabilities. Attachments are copied to the host’s private data directory before the turn or draft is recorded; each file is limited to 20 MiB. Image previews are restored from the host when you reopen a conversation. A draft can be sent or removed from its transcript card. Plan mode uses the host provider and produces a reviewable plan card whose Build action continues on the host with the session’s current model. Update older hosts to enable these menu actions.

Features that read or run on this computer are not available in these projects: `@` file mentions, skills and slash commands other than `/plan` and `/compact`, operator mode, and terminals. Worktree deletion and the local worktree settings page are not available remotely yet. Plans from the transcript open normal read-only plan tabs. Source files stay on the host; this feature shares host-owned sessions, not working-directory synchronization.

## Development connection

Use Node.js 22.13 or newer on the host. Install and sign in to the providers you want under the same OS account that runs the host. The host uses that account's default provider credentials and searches its PATH and common per-user and system installation directories.

From a checkout of this version of MonoCode on the host:

```sh
npm ci
npm run host:build
node build/host/monocode-host.mjs connect
```

Paste the printed link under **Add machine → Pairing link**. `npm run host:npm` packs the same host as `build/monocode-host-<version>.tgz`; copy it to a machine and run `npx --yes --package ./monocode-host-<version>.tgz monocode-host connect` there to test the npm install path.

When the laptop wakes, the remote view reconnects automatically and loads the host's latest persisted snapshot. An uncertain request retains its original command ID, including creation of a new session and its first message. **Retry** asks the host for that same command’s receipt, so a lost response does not create another conversation or submit the prompt again. Pending requests survive restarting the desktop and belong to the tab that created them.

Native desktop HTTP carries the saved credential; it is not exposed to the renderer or browser storage. The native connection store is `remote-machines.json` in the desktop app's data directory, with mode 0600 on Unix. This initial implementation does not use the OS keychain.

## Manage the host

```sh
node build/host/monocode-host.mjs connect status     # version, addresses, certificate, paired desktops
node build/host/monocode-host.mjs connect pair       # a new one-time pairing link
node build/host/monocode-host.mjs connect disable    # loopback only; SSH routes keep working
node build/host/monocode-host.mjs status
node build/host/monocode-host.mjs devices
node build/host/monocode-host.mjs revoke DEVICE_ID
node build/host/monocode-host.mjs stop
node build/host/monocode-host.mjs service uninstall
```

`connect` accepts `--bind <address>` to listen on one address, such as the Tailscale IP, instead of all interfaces. `--local-only` keeps the host on loopback, so only SSH routes reach it. `--port` changes the port, `--name` changes the name shown in MonoCode, and `--no-service` starts the host detached instead of installing a login service. `--json` prints one JSON line for scripts and sends progress to stderr.

Pairing names each device credential after the desktop's computer name. Removing a saved connection from the desktop does not stop the host. It revokes the device credential only when you choose **Revoke access and remove**. Otherwise, use `devices` and `revoke` on the host to remove access. Stopping the host interrupts active turns; restarting retains their transcripts and marks them interrupted. No uncertain provider operation is automatically replayed after a host crash.

`service uninstall` is the cleanup path for a host you no longer want running. It removes the systemd user service, the LaunchAgent, or this user's scheduled task. It then stops the host, including a manually started one, and interrupts any running turns. It never deletes the data directory. Sessions, logs, device credentials, and the TLS certificate stay in `~/.monocode-host` until you delete that directory yourself. Deleting `~/.monocode-host/tls` creates a new certificate at the next start, and every desktop must pair again. To remove access without stopping the host, use `revoke` instead. On Linux, the command prints how to turn off lingering if nothing else needs it.

The machine must remain awake. Manual `start` launches a detached process. `service install` installs a user service; `connect` runs it automatically. Only one host may own a data directory. Restart the host after changing its provider installation or PATH. Service installs preserve an already-running host, including one previously started manually.

Installed hosts have a launcher at `~/.monocode-host/bin/monocode-host`; use it in place of `node build/host/monocode-host.mjs` in management commands. Linux services are named `monocode-host.service`; macOS uses `com.monocode.host`. A service manager can restart a stopped process, so use `service uninstall` rather than `stop` to keep the host stopped.

On Windows, the launcher is `%USERPROFILE%\.monocode-host\bin\monocode-host.cmd`. The scheduled task is named `MonoCode Host-<user SID>`; `service uninstall` unregisters it and stops the host. Normal cancellation and shutdown stop the provider's process tree. A plain manual `start` is detached, but use `service install` for SSH-hosted Windows sessions so Task Scheduler owns the process independently of the SSH login.

## Security

- Remote clients reach the host only over TLS. The desktop pins the certificate fingerprint from the pairing link, or from the `connect` output it received over SSH, and verifies the handshake signature. Hostnames and certificate authorities play no part.
- A pairing code has 256 bits, works once, and expires after 15 minutes. The host keeps only its hash. Without a device credential, a client can call nothing except the pairing exchange, and the host answers at most 30 failed pairing attempts per minute.
- Each desktop gets its own device credential. It grants control of the host as its OS user, including running providers and reading project files. Revoke it from Settings or with `revoke`.
- Anyone who can reach port 3774 can attempt to pair. Use `--bind` or a firewall to limit who can reach it, or `--local-only` to require SSH.
- The host rejects requests that carry a browser `Origin` header. Its lifecycle endpoint answers only local requests that carry a secret from its data directory.

## Release packaging

`npm run host:package` builds a self-contained package for the current Windows/Mac/Linux architecture. `npm run host:package -- --all` builds all six archives, using pinned official Node binaries and checksums. Archives contain the host bundle, runtime, launcher, and licenses. `host/package.mjs` pins the runtime version. Cross-packaging Windows on Unix requires `zip` and `unzip`; native Windows packaging uses PowerShell.

The release workflow publishes `monocode-host-{darwin,linux}-{arm64,x64}.tar.gz`, `monocode-host-win32-{arm64,x64}.zip`, and their `.sha256` files alongside the desktop release. SSH setup downloads from the exact desktop version's GitHub release, then installs under `~/.monocode-host/runtime`. SSH setup reinstalls when the installed launcher reports another version, without restarting the running host; `connect` decides whether to restart it.

`npm run host:npm` packs the `monocode-host` npm package, which contains the same host bundle without a Node runtime. The release workflow requires the matching `monocode-host@<version>` to be publicly available on npm before it uploads desktop packages, publishes the GitHub release, or updates the feed. If that version is absent, an `NPM_TOKEN` secret with publish access to `monocode-host` is required. A missing token, failed publication, or failed registry check blocks the desktop release. The npm tarball is also attached to the release. Until a development version is on npm, use the development connection above.

**Unreleased development builds:** automatic first-time installation and **Update Host** require host archives published for the desktop version. Release builds from v0.5.0 onward include the matching archives; an unreleased checkout may not have them. Until the matching release is available, use the development connection above. A missing archive produces an explicit error in Settings. No fallback to an arbitrary latest release or unverified download is used. SSH setup does not restart a host that may have running turns. When an SSH host is older than the desktop, Settings → Connections offers **Update Host**. This downloads and verifies the matching package, restarts the host through `connect --yes`, and reconnects using the existing device credential. The restart interrupts active agent turns; sessions and history remain on the host. Machines paired with a link are updated by running `connect` on the machine.

## Scope

Supported: persistent remote text conversations with all ten local provider adapters, follow-up turns, approvals, questions where the provider offers them, cancellation, per-device revocation, reconnect, remote file browsing and text editing, Git status, file diffs, and history, staging and commits, branch selection and creation, worktree selection and creation, and a tracked Git diff against HEAD. OpenCode's server and event stream stay on the host's loopback interface. Cursor's optional enrichment from its native session database is not available on the headless host; basic transcript and subagent events still work. The desktop learns about session writes from `changes.wait` and downloads only transcript blocks that changed since its last update. The desktop rejects any single host response over 16 MiB. A sync above 4 MiB, such as reopening a very long transcript or one very large tool output, is sent as a series of bounded pieces of one consistent revision. Transcript size is therefore not limited by the response cap. The host writes streamed output in 120 ms batches and keeps a bounded event journal.

Remote history appears in the Sessions sidebar of each project on a machine. Host snapshots also populate the app's normal session state while the tab is open; they are not written to the local session store. Remote `/compact` uses the provider's context compaction, and `/plan` selects the host provider's plan mode. Queued follow-ups and editing the last message still need host commands. Other local slash commands and skill expansion are not yet available remotely. Worktree deletion, terminals, generated image output, named provider accounts, `/operator`, automations, orchestration, LAN discovery, and account-based relays are not implemented yet; a machine appears only after it is paired. Other remote prompts are sent directly to the provider.

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

Host tests use fake provider executables and temporary loopback servers. They do not contact paid models. They cover the Codex and Claude transports, Pi/OMP RPC, ACP providers, OpenCode HTTP and event streaming, cross-client reattachment, duplicate sends, approval races, interruption recovery, device revocation, path checks, and detached host lifecycle. On Node 26, use `NODE_OPTIONS=--no-experimental-webstorage npm run check:web` to avoid its experimental global storage interfering with the existing happy-dom tests.

Connect tests cover pairing codes, the change feed, the TLS listener, the certificate, and a full `connect` run that installs, pairs over pinned TLS, reuses, restarts, and disables network access. They pass `--no-service`, which never touches a login service. The Rust tests pair against a local TLS server that uses a certificate generated by `host/tls.ts`. To check the desktop client against a real host, start one and pass its link to the ignored test:

```sh
node build/host/monocode-host.mjs connect --no-service --json --bind 127.0.0.1 --data-dir "$(mktemp -d)"
MONOCODE_TEST_PAIRING_LINK='monocode://pair?...' cargo test --lib real_host -- --ignored
```

For the real OpenSSH transport and native askpass smoke test on Linux/macOS, build with `npm run host:package` and `cargo build --bin monocode`, then run `python3 scripts/test-remote-ssh.py`. It uses a disposable loopback sshd, temporary keys and known-hosts file, and an isolated packaged host. It leaves personal SSH configuration, provider credentials, and OS services untouched.

Host CI runs on Windows, macOS, and Linux. Windows-specific tests cover ACL inheritance, Task Scheduler definitions, bootstrap parsing/installation, and provider child-process cleanup. They use temporary data and mocked task registration so normal test runs do not install or replace a real user's background task. Full SSH-to-Task-Scheduler setup must also be validated on a signed-in Windows host before a supported release.
