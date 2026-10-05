# Remote access

MonoCode Host runs provider processes and stores their conversations on another macOS, Linux, or Windows machine. Closing the desktop or losing its network connection does not stop a host turn. The native host is a Rust executable and does not require Node.

Remote access remains experimental. [Native qualification](../reports/native-qualification.md) separates loopback protocol, SSH, and recovery tests from actual paired desktop and live-provider checks.

## Install and pair a host

Use the host version shown in Settings, Connections, Add machine. Native SSH setup downloads the matching `monocode-host_<version>_<Rust-target>.tar.gz` and `SHA256SUMS`, verifies the archive, and checks the executable's version before running it. The release must publish those files before SSH setup can succeed.

On the intended host, install the release through the repository's native installer script. Replace `0.6.0` with the version shown by the desktop:

```sh
sh scripts/install-native-host.sh 0.6.0
```

On Windows:

```powershell
powershell -File scripts/install-native-host.ps1 -Version 0.6.0
```

These commands download the native executable, run `connect`, and may install or update the host's login service. They belong on the machine you intend to configure. Native release archives must exist at the configured release URL. `MONOCODE_HOST_RELEASE_BASE_URL` selects an HTTPS release directory for development.

`connect` installs a persistent runtime under `~/.monocode-host`, registers a systemd user service on Linux, a launch agent on macOS, or a per-user Task Scheduler task on Windows, and prints a one-time pairing link. The default host port is 3774. Other computers connect over TLS to the certificate pinned by the link.

Paste the link in Settings, Connections, Add machine, Pairing link. The code works once and expires after 15 minutes. Run `monocode-host connect pair` to create another link. Opening a `monocode://pair?...` URL also routes to the native app.

## Set up through SSH

Choose SSH in Add machine and enter `user@host` or an alias from your OpenSSH configuration. MonoCode uses the system SSH client and the existing keys, agent, and host verification. Password or passphrase prompts use the native askpass window. The app does not save those responses.

If the desktop cannot reach the host's published network addresses, it opens an SSH forward to the host's loopback port. This also supports hosts configured with `--local-only`. Reconnect can restart a stopped host and restore the forward. Host updates may interrupt running turns and require the existing confirmation path.

Each provider must be installed and authenticated under the host account. Provider requirements remain separate from the native host. Linux login services require systemd user services. macOS and Windows hosts need the account's desktop login. Keep the machine awake. Signing out or rebooting can interrupt turns until the service starts again.

## Use a remote project

Open a folder on a connected machine through the project rail. Its identity is a `remote://` path, and sessions use that machine's provider catalog and checkout. The host owns the working files. MonoCode does not synchronize a local working directory to the host.

The native code routes Explorer, text editing, project search, Git status, diffs, history, staging, commits, push, pull, and pull requests to host commands. Text file operations have a 1 MiB limit. Attachments are copied to the host when supported and have a 20 MiB per-file limit. Plan and draft behavior depend on the advertised host capabilities.

The host protocol also supports branch and worktree operations. Their actual native picker interactions need a paired-window check before a supported release claim. Workspace commands unavailable on the host return an error and do not execute against a local directory.

Remote terminals, operator mode, local provider account management, local automation and orchestration flows, worktree deletion, and local worktree settings are outside the current remote UI contract. Generated images that expose only a host-local metadata path still cannot be opened on the desktop. A host catalog cannot make an incompatible or unavailable provider CLI usable.

## Manage the host

The native launcher is `~/.monocode-host/bin/monocode-host` on macOS and Linux and `%USERPROFILE%\.monocode-host\bin\monocode-host.exe` on Windows.

```sh
monocode-host connect status
monocode-host connect pair
monocode-host connect disable
monocode-host devices
monocode-host revoke DEVICE_ID
monocode-host stop
monocode-host service uninstall
```

`connect disable` returns the host to loopback access. Existing SSH routes can still reach it. `--bind <address>`, `--local-only`, `--port`, and `--name` set network and naming behavior. `--no-service` starts a detached host without registering a login service. `--json` prints machine-readable output and sends progress to stderr.

Removing a machine from this desktop closes its forward and removes the saved connection. Revoking access also invalidates that desktop's host credential. Removing a connection does not stop the host or delete its sessions. Service uninstall stops the host and removes its login service but preserves its data directory.

## Security and recovery

The pairing link carries the expected certificate fingerprint. The client pins that identity and receives a per-device credential. A device credential grants host-user control, including provider turns and project file access. Revoke credentials you no longer want to authorize.

The host rejects browser Origin requests. Plain HTTP and the authenticated lifecycle endpoint are local-only. Limit listening addresses or firewall access when appropriate. The host data directory belongs to its account and contains sessions, credentials, logs, and TLS material.

Connection checks refresh addresses and prefer the last working route. Safe route retries do not repeat a request that may already have reached the host. Command IDs and the outbox resolve uncertain sends. Long-poll change notifications and bounded transcript transfers let the desktop recover after disconnection or host restart.

## Development and checks

Use a scratch directory and no login service for a local test host:

```sh
cargo run -p monocode-host --bin monocode-host --locked -- connect --no-service --json --bind 127.0.0.1 --data-dir /tmp/monocode-host-test
cargo run -p monocode-host --bin monocode-host --locked -- connect status --data-dir /tmp/monocode-host-test
cargo run -p monocode-host --bin monocode-host --locked -- stop --data-dir /tmp/monocode-host-test
```

Run the native checks:

```sh
cargo test -p monocode-host -p monocode-remote --tests --locked
cargo test -p monocode-app --test ssh_askpass --all-features --locked
cargo build -p monocode-app -p monocode-host --bins --locked
python3 scripts/test-remote-ssh.py
```

The SSH runner uses a disposable loopback SSH daemon, private temporary keys and known-hosts file, and an isolated host. It leaves personal SSH configuration and login services intact. It needs an OpenSSH server executable on the test machine. A loopback pass does not qualify setup on another computer or a paid provider turn.

The native migration tests retain the Node host wire and data contracts. The explicit Node comparison fixture still needs the retained Node artifact. Keep that migration evidence when removing legacy source. Normal Rust provider transport tests also use inline Node fixture scripts. Native CI must provide Node for those tests. The shipped desktop and host do not use it.
