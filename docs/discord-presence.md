# Verify Discord presence

MonoCode publishes Rich Presence over Discord's local IPC socket. The toggle
lives in **Settings → General → Workspace → Discord presence** and is off by
default. Its status label polls the native client every three seconds.

## Preconditions

- Discord desktop is running and signed in.
- Discord → User Settings → Activity Privacy → **Display current activity as a
  status message** is on.
- The Discord application `1574386892118560891` has Rich Presence enabled in
  the Developer Portal.
- Your own client may not render your activity on every screen. If the app
  reports `Connected` but you see nothing, check from a second account or ask a
  friend; presence is often only visible to other users.

To confirm Discord exposes its IPC endpoint on Windows:

```powershell
[System.IO.Directory]::GetFiles("\\.\\pipe\\") | Where-Object { $_ -like 'discord-ipc*' }
```

On Linux/macOS look for `discord-ipc-0` in `$XDG_RUNTIME_DIR` or `/tmp`.

## End-to-end check

```bash
npm run tauri dev
```

1. Open Settings → General, find **Discord presence** under Workspace.
2. Turn it on. The label should go `Off → Connecting… → Connected`.
3. Start a turn or switch sessions. Presence details should become that
   session's title, with the state line showing the harness and project; an
   idle session ends in `· idle`. The MonoCode logo appears as the large image
   (`MonoCode` on hover); Discord fetches the HTTPS URL, so an internet
   connection is needed for the image to show.
4. With no session open, presence reads `Browsing MonoCode` (or `N agents
   running` when other chats are busy).
5. Quit MonoCode. The rich presence entry should disappear.

## Diagnostics

Two environment variables are available for local verification:

| Variable | Effect |
| --- | --- |
| `MONOCODE_DISCORD_DEBUG=1` | Prints IPC paths, connect/handshake failures, each `SET_ACTIVITY` (state and details), send errors, and the clear frame to the terminal running Tauri. |
| `MONOCODE_DISCORD_IPC_PORT=N` | Tries only socket `N` (0–9) instead of scanning all ten. Lets a mock server run beside real Discord. |
| `MONOCODE_DISCORD_LOGO_URL=...` | Replaces the large image with a different HTTPS URL or a Discord asset key. Defaults to `https://usemono.dev/icon.png`. |

```powershell
$env:MONOCODE_DISCORD_DEBUG="1"
npm run tauri dev
```

In dev builds the browser devtools console also logs each
`discord_presence_set` command, its payload, and the returned status via
`console.debug`.

## Deterministic IPC check (without Discord's UI)

The mock server accepts a handshake and prints every frame MonoCode sends.
This proves the transport independently of Discord's display settings.

Windows (real Discord keeps port 0; use port 9 for the mock):

```powershell
# Terminal 1
npm run mock:discord:win -- 9

# Terminal 2
$env:MONOCODE_DISCORD_IPC_PORT="9"
$env:MONOCODE_DISCORD_DEBUG="1"
npm run tauri dev
```

Linux/macOS:

```bash
# Terminal 1
npm run mock:discord -- 9

# Terminal 2
MONOCODE_DISCORD_IPC_PORT=9 MONOCODE_DISCORD_DEBUG=1 npm run tauri dev
```

Expected mock output:

- a handshake frame containing `1574386892118560891` when presence is enabled;
- `SET_ACTIVITY` frames with `state` / `details` when you switch sessions or
  start a turn;
- a `SET_ACTIVITY` frame with `"activity":null` when you disable presence or
  quit.

Stop the mock with Ctrl+C; it removes its Unix socket on exit.

## Troubleshooting

| Symptom | Likely cause | What to do |
| --- | --- | --- |
| Label stays `Off` after toggling | Frontend command failed | Check devtools console and the debug terminal; confirm `discord_presence_set` is invoked. |
| `Discord not running` | No IPC socket found | Start Discord; check the endpoint commands above; note WSL/containers do not share the host socket. |
| `Connection lost` | Socket closed mid-session | Discord restarted; the worker retries every five seconds. |
| `Connected`, nothing shown in Discord | Discord settings or portal config | Check Activity Privacy, Rich Presence enablement for the app ID, and verify from another account. |
| Mock receives no frames | Port mismatch | Match `MONOCODE_DISCORD_IPC_PORT` to the mock's argument and keep real Discord on port 0. |
