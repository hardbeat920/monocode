# Monocode Native 0.16.0-preview.8

This experimental Monocode Native preview is built with Rust + GPUI. React/Tauri remains the production app.

## Changes since preview 7

- Native charts and dashboard components can appear inline in assistant replies.
- Mono tabs can join session splits as panes. The terminal panel hides when its last terminal closes, and the sidebar adjusts as the surrounding layout changes.
- New `/stage` and `/commit` commands handle session Git actions, with compact summaries of Git turns.
- GitHub issue and pull request links in prompts appear as chips with hover cards and can be linked to the session.
- The Inbox opens scoped to the active project, uses configured project names, and clears stale selection and detail requests when switching projects.
- Window size, fullscreen and maximized state survive relaunches. Wayland users gain a decoration setting, window controls, title-bar dragging and edge resizing.
- Expanded user messages keep a consistent bubble width. Windows and Linux builds resolve a real monospace font and show the app icon.
- Idle conversations switch accounts in place, stored floating-point values reload accurately, and composer autocorrect is off by default.
- Missing-provider hints name the runtime actually needed, including the Antigravity ACP server.

Thanks to @parkgogogo, @knot2006, @Kaloyanes and @Hazem-Samak for their contributions in this preview.

## Installation and testing

Choose the download for your platform: macOS Apple Silicon or Intel, Windows x64, or Linux x64 (AppImage, Debian/Ubuntu `.deb`, or Fedora/Enterprise Linux `.rpm`). Install it alongside your existing Monocode application.

**Testing status:** macOS is the primary tested platform. A Linux ARM64 source build was launched and exercised in an Ubuntu VM with a graphics workaround; the downloadable Linux x64 packages and Windows app still need runtime testing. Native system notifications currently support macOS only. HTML panels and media support remain limited on Linux and Windows.

**Data:** this preview uses your existing Monocode database and saves changes there. Back up your Monocode data with the app closed before testing. Native preferences are stored separately. Avoid editing the same conversations in both apps at once.

**Updates:** use Settings → General → About to check for updates and install the next native preview. Packaged upgrades still need runtime testing; if updating fails, download the new installer manually. Linux `.deb`/`.rpm` installs use the system authentication prompt (`pkexec`) to install the downloaded package. Cancelling authentication leaves the current version installed. Native previews receive native preview updates; production Tauri releases use their own feed.

The initial installation requires a download. macOS completion/approval notifications, sounds, and Dock badges still need testing in the distributed app.

Feature and platform parity are still being tested. Read `docs/native/parity.md` at this release's tag for known limitations. A successfully built package does not establish that every feature works on that platform.

Please report the preview version, operating system/architecture, installation format, provider, and reproduction steps with any issue. Remove credentials and private conversation content from logs and screenshots.
