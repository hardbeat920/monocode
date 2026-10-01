# Remote access (experimental)

MonoCode can run Codex and Claude Code sessions on a separate Windows, Linux, or macOS machine. The machine runs MonoCode Host, which owns the provider processes and the session database. Closing the desktop, closing a session tab, or losing the network does not stop a host session.

A folder on a connected machine is a project in the rail, marked with a globe. Every session in it runs on that machine, in the same session view and composer as a local session. The Sessions sidebar lists that machine's sessions for the project.

## Connect a machine

On the machine, run the command shown in **Settings → Connections → Add machine**. It names the host version that matches your desktop:

```sh
npx monocode-host@0.4.3 connect
```

`connect` does four things:

1. Copies the host into `~/.monocode-host` (`%USERPROFILE%\.monocode-host` on Windows). npm may clear its npx cache, so the service never runs from it.
2. Installs a login service: a systemd user service on Linux, a launch agent on macOS, or a per-user Task Scheduler task on Windows.
3. Turns on network access. The host listens on port 3774 on every interface. Other computers must use TLS with the host's self-signed certificate, which the host creates on first run. Plain HTTP is accepted only from the machine itself.
4. Prints a pairing link, such as `monocode://pair?v=1&name=studio&...`. It works once and expires after 15 minutes.

In MonoCode, open **Settings → Connections → Add machine**, keep **Pairing link** selected, paste the link, and click **Pair**. The link lists the host's addresses: LAN addresses first, then Tailscale and other overlay addresses, then the Tailscale MagicDNS name when Tailscale is running. The desktop tries each address, checks that the host presents the certificate whose SHA-256 fingerprint is in the link, and exchanges the one-time code for a device credential. The credential stays in the desktop's native connection store and never reaches the renderer.

Run `connect` again whenever you need another link, for example for a second desktop. A running host of the same or a newer version keeps running. An older host is replaced with this version. If the older host has running turns, `connect` asks before restarting it; `--yes` skips the question.

### Set up over SSH

Choose **SSH** under **Add machine** and enter an SSH address (`user@my-mac-mini`) or an alias from your SSH config. MonoCode runs the same `connect` command on the machine through npx, reads the pairing link from its output, and pairs this desktop. It uses the desktop's OpenSSH client with your normal SSH config, keys, and agent. SSH host verification and password or passphrase prompts appear in Settings. MonoCode uses a password or passphrase only for the current authentication and does not save it.

If this computer cannot reach the machine's network addresses, MonoCode pairs and connects through an SSH forward to the host's loopback port instead. This covers WSL2 in NAT mode, cloud machines that only open port 22, and hosts started with `--local-only`. Machines set up over SSH keep the forward as a fallback route.

For a machine set up over SSH, **Reconnect** runs `connect` again, which starts the host if it stopped, and reopens the forward. **Update Host** installs the desktop's host version and restarts the host, which interrupts active turns. For a machine paired with a link, run the command from Settings on the machine to update it.

### Requirements

- Node.js 22.13 or newer for the account that runs the host. Over SSH, setup looks for `npx` on the login shell's PATH, then in Volta, mise, asdf, nvm, Homebrew, and `/usr/local/bin`.
- Codex and/or Claude Code installed and signed in under the same account. The host finds provider CLIs on the PATH it was installed with, plus `~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin`, and `/usr/bin`. Run `connect --restart` after installing a provider.
- The desktop must reach TCP port 3774 on one of the host's addresses, or use SSH setup. Allow the port in the host's firewall if needed. On a tailnet, no extra setup is needed.
- Linux needs systemd user services. Setup runs `loginctl enable-linger` for the account so the host survives logout. If that needs administrator access, `connect` prints the recovery command and starts the host detached until the next reboot.
- macOS needs an active desktop login. Keep that Mac signed in and awake.
- Windows needs the same account signed in at the desktop. Locking the desktop and disconnecting SSH are fine; signing out or rebooting interrupts agents, and the task starts again at the next login. The task uses an interactive logon token because S4U tasks cannot access network or encrypted files ([Microsoft task logon documentation](https://learn.microsoft.com/en-us/windows/win32/taskschd/principal-logontype)). Windows provider discovery supports native `.exe` installations and standard npm installations of `@openai/codex` and `@anthropic-ai/claude-code`.

### Routes and updates

The desktop keeps the route that last answered. When a request cannot connect, the desktop tries the machine's other addresses and then the SSH forward, and sends the request on the first route that answers. A request that may have reached the host is never resent; the renderer confirms it with its command ID, as before. After every route fails, the desktop waits 5 seconds before trying them again. **Retry** in Settings tries at once. The host reports its current addresses on each connection check, so a changed IP address updates the saved list.

The desktop holds one `changes.wait` request open per machine. The host answers as soon as a session is saved, batching writes that arrive within 40 ms, and otherwise after 25 seconds. Open sessions and session lists then load only the blocks that changed. Polling remains as a fallback every 5 to 30 seconds, and at the old 0.75 to 3 second rate for hosts older than this release.

## Start a session

1. In the project rail, click **+** next to Projects and choose **Open folder on a machine…**.
2. Choose the machine, browse to an existing checkout (or type its absolute path, such as `/home/me/code/my-app`), and click **Open**.
3. Send a message. The first message creates the session on the host with the model, reasoning effort, and permission mode shown in the composer.

The composer's model picker lists the host's Codex and Claude Code models. Model, effort, and permission changes apply to the host session directly, and the next turn uses them; a change made during a running turn is applied when that turn finishes. If the host cannot load its model list, or no longer lists the session's model, the session's saved settings stay visible and editable. The normal workspace and branch pickers use the host checkout through the shared file and Git commands. Choose an existing host worktree or create one from a branch before the first message; the session then runs in that working copy. Once the conversation starts, its worktree is fixed, as in a local session. Start a new session to use another worktree. The branch picker can search local and remote branches, create a local branch, and switch the current working copy when it is clean and the project's sessions are idle. Machine connection state appears on its project rail dot.

The **Explorer** sidebar and Go to File use the normal file views for host folders and files. Opening a text file uses the normal editor tabs. Saving writes back to the host; remote reads and edits are limited to 1 MiB text files. File creation, rename, deletion, and project search use the same controls as local projects. The **Changes** sidebar and Git history graph use the normal Git views, including file diffs, staging, discarding, commits, pushing, and pull request creation through `gh` on the host. Remote pull requests use the host's commit list and diff summary for their title and body. An ordinary folder outside Git shows an empty Changes state. Git changes are refreshed every few seconds. Generated commit messages are not available remotely yet.

The composer’s **+** menu supports file and image attachments, Plan mode, and saved drafts when the host advertises these capabilities. Attachments are copied to the host’s private data directory before the turn or draft is recorded; each file is limited to 20 MiB. Image previews are restored from the host when you reopen a conversation. A draft can be sent or removed from its transcript card. Plan mode uses the host provider and produces a reviewable plan card whose Build action continues on the host with the session’s current model. Update older hosts to enable these menu actions.

Features that read or run on this computer are not available in these projects: `@` file mentions, skills and slash commands other than `/plan` and `/compact`, operator mode, and terminals. Worktree deletion and the local worktree settings page are not available remotely yet. Plans from the transcript open normal read-only plan tabs. Source files stay on the host; this feature shares host-owned sessions, not working-directory synchronization.

## Manage the host

Run these on the machine. The launcher is `~/.monocode-host/bin/monocode-host`, or `%USERPROFILE%\.monocode-host\bin\monocode-host.cmd` on Windows.

```sh
monocode-host connect status     # version, addresses, certificate, paired desktops
monocode-host connect pair       # a new one-time pairing link
monocode-host connect disable    # loopback only; SSH routes keep working
monocode-host devices            # paired desktops
monocode-host revoke DEVICE_ID
monocode-host stop
monocode-host service uninstall
```

`connect` accepts `--bind <address>` to listen on one address, such as the Tailscale IP, instead of all interfaces. `--local-only` keeps the host on loopback, so only SSH routes reach it. `--port` changes the port, `--name` changes the name shown in MonoCode, and `--no-service` starts the host detached instead of installing a login service. `--json` prints one JSON line for scripts and sends progress to stderr.

**Remove** in Settings asks for confirmation and offers two choices. **Remove from this desktop only** deletes the saved connection and closes its forward. The host keeps running, and this desktop's credential stays valid on it. **Revoke access and remove** first asks the host to revoke the credential this desktop uses, then removes the connection. It needs the machine to be reachable, and if revocation fails the connection is kept. Neither option stops the host, affects other desktops, or deletes sessions. Pairing the same machine again reconnects its projects, tabs, and history, and revokes this desktop's previous credential.

`service uninstall` removes the systemd user service, the launch agent, or the scheduled task, then stops the host and interrupts running turns. It never deletes the data directory; sessions, logs, device credentials, and the TLS certificate stay in `~/.monocode-host` until you delete it. On Linux, the command prints how to turn off lingering if nothing else needs it. Deleting `~/.monocode-host/tls` creates a new certificate at the next start, and every desktop must pair again.

State and logs live in the data directory, readable only by its owner. On Windows, ACLs restrict it to the current user, SYSTEM, and Administrators. Only one host may own a data directory. Use `--data-dir` to run a separate host.

## Security

- Remote clients reach the host only over TLS. The desktop pins the certificate fingerprint from the pairing link and verifies the handshake signature, so another machine at the same address cannot impersonate the host. Hostnames and certificate authorities play no part.
- A pairing code has 256 bits, works once, and expires after 15 minutes. The host keeps only its hash. Without a device credential, a client can call nothing except the pairing exchange, and the host answers at most 30 failed pairing attempts per minute.
- Each desktop gets its own device credential. It grants control of the host as its OS user, including running providers and reading project files. Revoke it from Settings or with `monocode-host revoke`.
- Anyone who can reach port 3774 can attempt to pair. Use `--bind` or a firewall to limit who can reach it, or `--local-only` to require SSH.
- The host rejects requests that carry a browser `Origin` header. Its lifecycle endpoint answers only local requests that carry a secret from its data directory.

## Scope

Supported: persistent remote text conversations, existing Codex/Claude adapters, follow-up turns, approvals, questions, cancellation, per-device revocation, reconnect, remote file browsing and text editing, Git status, file diffs, and history, staging and commits, branch selection and creation, worktree selection and creation, and a tracked Git diff against HEAD. Views download only the transcript blocks that changed since their last update. The desktop rejects any single host response over 16 MiB. A sync above 4 MiB, such as reopening a very long transcript or one very large tool output, is sent as a series of bounded pieces of one consistent revision, so the response cap does not limit transcript size. The host writes streamed output in 120 ms batches and keeps a bounded event journal.

Remote history appears in the Sessions sidebar of each project on a machine. Host snapshots also populate the app's normal session state while the tab is open; they are not written to the local session store. Remote `/compact` uses the provider's context compaction, and `/plan` selects the host provider's plan mode. Queued follow-ups and editing the last message still need host commands. Other local slash commands and skill expansion are not yet available remotely. Worktree deletion, terminals, generated image output, named provider accounts, `/operator`, automations, and orchestration are not implemented yet. MonoCode has no relay service: the desktop connects to the host directly or through SSH, and machines do not appear without pairing.

The headless host runs the reused TypeScript adapters with a Node process backend.

## Development

`npm run host:npm` builds the `monocode-host` package in `build/host-npm` and packs it as `build/monocode-host-<version>.tgz`. Publishing to npm is a manual step: `npm publish build/host-npm`. Until a version is published, `npx monocode-host@<version>` fails, so install development builds one of these ways:

- `npm run host:install -- <ssh-target>` packs this checkout, copies the tarball to the machine, and runs `connect` there. It prints the pairing link.
- Set `MONOCODE_HOST_PACKAGE` when starting the desktop to change what its SSH setup installs, for example a tarball path on the host or a tarball URL.
- On the machine itself, `npx --yes --package ./monocode-host-<version>.tgz monocode-host connect`.

## Verify

```sh
npm run host:build
npm run test:host
npm run check:web
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test
```

Host tests use fake provider executables and temporary loopback servers. They do not contact paid models. They cover both real adapter transports, cross-client reattachment, duplicate sends, approval races, interruption recovery, device revocation, path checks, pairing, pushed changes, the TLS listener, and a full `connect` run that installs, pairs over pinned TLS, reuses, restarts, and disables network access. `connect` tests pass `--no-service`, which never touches the login service. On Node 26, use `NODE_OPTIONS=--no-experimental-webstorage npm run check:web` to avoid its experimental global storage interfering with the existing happy-dom tests.

The Rust tests pair against a local TLS host that uses a certificate generated by `host/tls.ts`. To check the desktop client against a real host, start one and pass its link to the ignored test:

```sh
node build/host/monocode-host.mjs connect --no-service --json --bind 127.0.0.1 --data-dir "$(mktemp -d)"
MONOCODE_TEST_PAIRING_LINK='monocode://pair?...' cargo test --lib real_host -- --ignored
```

For the real OpenSSH transport and native askpass smoke test on Linux/macOS, run `npm run host:build` and `cargo build --bin monocode`, then `python3 scripts/test-remote-ssh.py`. It uses a disposable loopback sshd, temporary keys and known-hosts file, and an isolated host. It leaves personal SSH configuration, provider credentials, and OS services untouched.

Host CI runs on Windows, macOS, and Linux. Windows-specific tests cover ACL inheritance, Task Scheduler definitions, parsing of the SSH connect script, and provider child-process cleanup. They use temporary data and mocked task registration so normal test runs do not install or replace a real user's background task. Full SSH-to-Task-Scheduler setup must also be validated on a signed-in Windows host before a supported release.
