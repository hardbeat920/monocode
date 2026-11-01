# Monocode Native 0.16.0-preview.12

This experimental Monocode Native preview is built with Rust + GPUI. React/Tauri remains the production app.

## Changes since preview 11

### Phones

- Paired phones get push notifications while the Monocode app is closed. Sessions started or messaged from the phone notify on approvals, questions, and finishing; Monos notify on their replies and on the reports their habits post. Nothing is sent for a session open in front of you on the desktop. The phone chooses which kinds it receives in its Settings. The desktop sends notifications through Expo's push service; Android phones keep their in-app alerts for now.
- Phones can answer an agent's pending questions, page back through the full history of long conversations, and reopen the last project or Mono on launch.
- Monos can be browsed from the phone's side drawer alongside projects, with search, filters, and unread replies marked. Phones can create Monos, edit a Mono's name, mascot, color, model, projects, and its soul and memory files, and reset or delete a Mono. Saving a soul or memory file is refused if it changed on the desktop since it was opened.
- Mono chats on the phone show reply bubbles, reactions as tapbacks, suggested replies, document cards, and the sessions a Mono launched.

### Background tasks

- A **Background model** setting (Settings → Providers → Background tasks) chooses the model that writes session titles, worktree branch names, commit messages, and pull request text, on the desktop and for phone git actions. **Automatic** keeps using each session's provider and its quick model.

### Editor

- Zig files get syntax highlighting and fold their struct, enum, union, error-set, and switch blocks. ZON files are not supported.

Thanks to @404khai for their contribution in #1246.

## Installation and testing

Choose the download for your platform: macOS Apple Silicon or Intel, Windows x64, or Linux x64 (AppImage, Debian/Ubuntu `.deb`, or Fedora/Enterprise Linux `.rpm`). Install it alongside your existing Monocode application.

**Testing status:** macOS is the primary tested platform. A Linux ARM64 source build was exercised in an Ubuntu VM with a graphics workaround, and a Windows 10 source build passed manual title-bar checks. The downloadable Linux x64 and Windows packages still need broader runtime testing. Native system notifications currently support macOS only. HTML panels and media support remain limited on Linux and Windows.

Local automated checks, the native dependency audit, and the long-history CPU comparison are recorded in [verification](verification.md).

**Data:** this preview uses your existing Monocode database and saves changes there. Back up your Monocode data with the app closed before testing. Native preferences are stored separately. Avoid editing the same conversations in both apps at once.

**Updates:** use Settings → General → About to check for updates and install the next native preview. Packaged upgrades still need runtime testing; if updating fails, download the new installer manually. Linux `.deb`/`.rpm` installs use the system authentication prompt (`pkexec`) to install the downloaded package. Cancelling authentication leaves the current version installed. Native previews receive native preview updates; production Tauri releases use their own feed.

The initial installation requires a download. macOS completion/approval notifications, sounds, and Dock badges still need testing in the distributed app.

Feature and platform parity are still being tested. Read `docs/native/parity.md` at this release's tag for known limitations. A successfully built package does not establish that every feature works on that platform.

Please report the preview version, operating system/architecture, installation format, provider, and reproduction steps with any issue. Remove credentials and private conversation content from logs and screenshots.
