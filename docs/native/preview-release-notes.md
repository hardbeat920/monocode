# Monocode Native 0.16.0-preview.9

This experimental Monocode Native preview is built with Rust + GPUI. React/Tauri remains the production app.

## Changes since preview 8

- Faster startup and fewer full-window redraws during animations, with GPU memory released when the app goes quiet.
- Mermaid diagrams render natively in Markdown, resize with the panel and wrap sequence labels while preserving their controls.
- Cmd/Ctrl+Enter sends a mid-turn message the opposite way from your usual follow-up preference: steering when you normally queue, or queueing when you normally steer. The send button reflects the selected action.
- Composer file mentions show file icons and insert short names. Terminal links open on click, and terminal text prefers a Nerd Font.
- The workspace picker includes All checkouts, while selecting a project folder keeps that folder as the session scope.
- Per-project theme colours are available from the project menu. Buttons, tree rails and diff overlays follow the active theme more consistently, including the send button on light themes.
- Windows gains an integrated title bar and glass backdrops. macOS bundles use the Liquid Glass app icon.
- Pasted images are stored per conversation and cleaned up with that conversation; existing shared legacy attachments are preserved.
- Plain-link prompts wrap inside their bubbles, and the Edit GitHub link dialog keeps its Update button inside the dialog.
- Disabling mobile access or shutting down cancels pending mobile startup. Host packaging retries interrupted runtime downloads and reuses verified cached downloads.

Thanks to @Kaloyanes, @donnes, @ognjeeen and @404khai for their contributions in this preview.

## Installation and testing

Choose the download for your platform: macOS Apple Silicon or Intel, Windows x64, or Linux x64 (AppImage, Debian/Ubuntu `.deb`, or Fedora/Enterprise Linux `.rpm`). Install it alongside your existing Monocode application.

**Testing status:** macOS is the primary tested platform. A Linux ARM64 source build was exercised in an Ubuntu VM with a graphics workaround, and a Windows 10 source build passed manual title-bar checks. The downloadable Linux x64 and Windows packages still need broader runtime testing. Native system notifications currently support macOS only. HTML panels and media support remain limited on Linux and Windows.

**Data:** this preview uses your existing Monocode database and saves changes there. Back up your Monocode data with the app closed before testing. Native preferences are stored separately. Avoid editing the same conversations in both apps at once.

**Updates:** use Settings → General → About to check for updates and install the next native preview. Packaged upgrades still need runtime testing; if updating fails, download the new installer manually. Linux `.deb`/`.rpm` installs use the system authentication prompt (`pkexec`) to install the downloaded package. Cancelling authentication leaves the current version installed. Native previews receive native preview updates; production Tauri releases use their own feed.

The initial installation requires a download. macOS completion/approval notifications, sounds, and Dock badges still need testing in the distributed app.

Feature and platform parity are still being tested. Read `docs/native/parity.md` at this release's tag for known limitations. A successfully built package does not establish that every feature works on that platform.

Please report the preview version, operating system/architecture, installation format, provider, and reproduction steps with any issue. Remove credentials and private conversation content from logs and screenshots.
