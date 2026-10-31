# Monocode Native 0.16.0-preview.11

This experimental Monocode Native preview is built with Rust + GPUI. React/Tauri remains the production app.

## Changes since preview 10

### Projects and conversations

- Projects can be reordered by dragging their spaces, using the project menu's **Move** actions, or the **Project: Move Back / Forward** shortcuts. The order survives restarts, and Previous / Next follows the order shown in the project rail.
- New worktree branches receive descriptive names from the first message. Naming stays with the selected provider and account; unsupported or unavailable providers leave the temporary name in place. Manual branch names and existing destination branches are preserved.
- Empty sessions offer **Ink**, **Games**, and **Off**. The ink canvas responds to pointer movement and presses, starts its animation on first display, and stops repainting when quiet. Existing empty-session choices survive the upgrade and desktop preference syncing. Reduced motion and chat backgrounds suppress the canvas.
- Sessions opened in a Mono's details panel keep their history visible. Sub-agent lists put running and waiting sessions first, followed by recent activity. Choice cards fit their labels and wrap longer text.

### Inbox and phones

- Hiding a Linear team immediately removes its cached Inbox issues and clears the selection when no visible issue remains.
- Inbox details have **Copy link** with a copied confirmation. **Send to agent** is now labeled **Open in new session**. Pairing-link copies also show a confirmation.

### Provider options and security

- An experimental **Identify as Monocode to Claude** setting controls Claude's client label. It is off by default.
- Native dependencies are updated for known timer, TLS certificate validation, cryptographic timing, compression, and Markdown parsing advisories. Phone identity and enrollment-proof compatibility is covered by a fixture from the previous signing library.

Thanks to @nwoolls and @Kaloyanes for their contributions in #1175, #1193, #1206, and #1226.

## Installation and testing

Choose the download for your platform: macOS Apple Silicon or Intel, Windows x64, or Linux x64 (AppImage, Debian/Ubuntu `.deb`, or Fedora/Enterprise Linux `.rpm`). Install it alongside your existing Monocode application.

**Testing status:** macOS is the primary tested platform. A Linux ARM64 source build was exercised in an Ubuntu VM with a graphics workaround, and a Windows 10 source build passed manual title-bar checks. The downloadable Linux x64 and Windows packages still need broader runtime testing. Native system notifications currently support macOS only. HTML panels and media support remain limited on Linux and Windows.

Local automated checks, the native dependency audit, and the long-history CPU comparison are recorded in [verification](verification.md).

**Data:** this preview uses your existing Monocode database and saves changes there. Back up your Monocode data with the app closed before testing. Native preferences are stored separately. Avoid editing the same conversations in both apps at once.

**Updates:** use Settings → General → About to check for updates and install the next native preview. Packaged upgrades still need runtime testing; if updating fails, download the new installer manually. Linux `.deb`/`.rpm` installs use the system authentication prompt (`pkexec`) to install the downloaded package. Cancelling authentication leaves the current version installed. Native previews receive native preview updates; production Tauri releases use their own feed.

The initial installation requires a download. macOS completion/approval notifications, sounds, and Dock badges still need testing in the distributed app.

Feature and platform parity are still being tested. Read `docs/native/parity.md` at this release's tag for known limitations. A successfully built package does not establish that every feature works on that platform.

Please report the preview version, operating system/architecture, installation format, provider, and reproduction steps with any issue. Remove credentials and private conversation content from logs and screenshots.
