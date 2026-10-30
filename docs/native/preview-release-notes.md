# Monocode Native 0.16.0-preview.10

This experimental Monocode Native preview is built with Rust + GPUI. React/Tauri remains the production app.

## Changes since preview 9

### Monos and background work

- A new **Overview** tab shows a Mono's current activity, a contribution calendar, today's merged pull requests, lines changed, sessions and habit runs, plus upcoming habits and new documents. The session list is now labeled **Sub-agents**, with cards showing the model, provider, checkout and current state.
- Contributions are recovered from saved session history and recorded as agents commit code or open, merge and review pull requests through Git, the GitHub CLI or GitHub MCP tools. Repeated records are deduplicated, amended commits replace their earlier entry, and pull requests the Mono opened are checked for later merges. Read-only review lookups do not count. Commit line counts follow explicit tool working directories and `git -C`, including commands that commit in multiple checkouts. Contribution data is stored in the native sidecar database.
- **Heartbeats** let an idle Mono review recent work, sessions left unattended, notes and inbox items in its assigned projects, then suggest up to two follow-ups. They are enabled by default after 30 minutes of quiet; the Mono's settings offer an off switch and 15-minute, 30-minute, 1-hour and 2-hour intervals. Quiet checks leave no chat report, avoid repeating recent suggestions and gradually back off to four hours. Checks keep the Mono's selected permissions, use its provider account and select a smaller triage model when one is available.
- Monos are prompted to show a short opening reply while work continues and delegate longer tasks to background sessions. Completion reports retain the final answer while hiding routine notification commentary, and launched sessions stay in the background until opened.
- Suggested replies appear as individual line cards. Choosing one sends that reply; the selected choice stays checked after answering, and the other choices become inactive.
- Sent photos in Mono chats retain their full shape above the message bubble and open in the image viewer. The Mono composer and its toolbar controls use rounded styling, including a pill-shaped input for a single line.

### Native visuals and commands

- Use **`/visualize`** in an idle local desktop conversation to visualize the current topic, or **`/visualize <request>`** to specify what to show. The command asks the agent to read the visual reference, validate its program and publish an inline visual. Submitted requests have a dither animation; rejected requests preserve the draft and attachments.
- Claude Code and Codex receive guidance on when a visual will help. Claude's session-scoped native visual tools are allowed without an extra permission prompt; the executable fallback remains available for providers without injected MCP tools.
- Visuals published in a Mono chat also appear in **Documents** and open in the native visual reader. Publishing the same visual ID updates that document. **New HTML artifacts are no longer accepted** by the Mono app API: use `artifacts.write` with `kind: "document"` for Markdown, or `visual_show` for interactive native content.
- Slash-command completions have distinct icons and colors that follow the active light or dark theme, including `/btw`, `/compact`, `/stage`, `/commit` and `/visualize`.

### Transcript and workspace fixes

- Fixed a transcript crash when updates merge messages while older rows are animating out.
- Steering messages remain attached to their running turn after reopening a conversation. Once a Mono has replied in words, later steers and queued messages stay in chat order.
- A Mono's opening sentence stays visible as its run finishes, without replaying its entrance animation. Transcript spacing and activity labels are more consistent, and the phone transcript retains final Mono reports.
- Day separators appear when the local calendar day changes, rather than after an hour-long pause. Message timestamps retain the time of day.
- Hermes Agent receives the reasoning effort selected in the model picker when its server advertises that option.
- Mono conversations no longer crash when showing a usage-limit notice.
- The terminal panel's **New Terminal** button sits directly after its tabs.

### Documentation and checks

- The main README links native preview downloads and explains shared-database backups and precautions when switching between the native and production apps.
- Native preview formatting, a strict Clippy warning and an outdated Overview tab assertion are fixed. The complete native check and macOS development-profile build passed locally with Rust 1.99.1; see `docs/native/verification.md` for counts and remaining runtime checks.

Thanks to @Juns-g for the transcript crash and Hermes reasoning-effort fixes in #1216 and #1217.

## Installation and testing

Choose the download for your platform: macOS Apple Silicon or Intel, Windows x64, or Linux x64 (AppImage, Debian/Ubuntu `.deb`, or Fedora/Enterprise Linux `.rpm`). Install it alongside your existing Monocode application.

**Testing status:** macOS is the primary tested platform. A Linux ARM64 source build was exercised in an Ubuntu VM with a graphics workaround, and a Windows 10 source build passed manual title-bar checks. The downloadable Linux x64 and Windows packages still need broader runtime testing. Native system notifications currently support macOS only. HTML panels and media support remain limited on Linux and Windows.

**Data:** this preview uses your existing Monocode database and saves changes there. Back up your Monocode data with the app closed before testing. Native preferences are stored separately. Avoid editing the same conversations in both apps at once.

**Updates:** use Settings → General → About to check for updates and install the next native preview. Packaged upgrades still need runtime testing; if updating fails, download the new installer manually. Linux `.deb`/`.rpm` installs use the system authentication prompt (`pkexec`) to install the downloaded package. Cancelling authentication leaves the current version installed. Native previews receive native preview updates; production Tauri releases use their own feed.

The initial installation requires a download. macOS completion/approval notifications, sounds, and Dock badges still need testing in the distributed app.

Feature and platform parity are still being tested. Read `docs/native/parity.md` at this release's tag for known limitations. A successfully built package does not establish that every feature works on that platform.

Please report the preview version, operating system/architecture, installation format, provider, and reproduction steps with any issue. Remove credentials and private conversation content from logs and screenshots.
