# Monocode Native 0.16.0-preview.14

This experimental Monocode Native preview is built with Rust + GPUI. React/Tauri remains the production app.

## Changes since preview 13

### Terminals, tabs, and split sessions

- **Settings → General → Terminal** offers **Bottom, Side panel, or Title bar** placement. Your previous side-panel preference is preserved. Side-panel placement uses the Focused layout; title-bar terminals are available for local projects. With Title bar selected, **Cmd/Ctrl+J** opens a fresh terminal tab, and the title-strip plus button offers New Session or New Terminal.
- **Split Terminal Right**, available from the app menu and **Cmd/Ctrl+Shift+J**, opens a shell beside the focused pane when using Title bar placement. In Bottom or Side panel placement, it adds a terminal there instead.
- Dragging the last terminal out of the side panel into a split or title tab keeps the panel open with a fresh shell. The original shell moves with its running commands.
- Closing a terminal pane or title tab **asks for confirmation when a foreground process is running**. Bulk tab closes list the affected running terminals together; Cancel keeps the tabs open. Idle shells close without a prompt.
- **Close All and Close Others shortcuts preserve pinned tabs**, matching the tab menu.
- Title tabs containing a chat with a **saved draft** show a dashed border, including chats inside split tabs.
- New split chats use the selected provider's model choice, account, preferred effort, and default access mode, just like new chat tabs. Their working-copy scope is preserved.

### Composers, sidebar, and settings

- Session composers start **one line tall** and grow with the message. Transcript text lines up with the composer, and clicking its toolbar keeps the input focused.
- Mono chats use the same composer layout and action button as sessions. Model, model settings, and access controls sit inside the toolbar; attachment and emoji controls remain beneath it, with the context meter aligned below the action button.
- An **amber bolt** on model controls shows when a supported model's fast mode is active, including Mono chats and Quick Composer.
- The Focused sidebar has a **New** navigation row, a live **Notes count**, and a compact **Search** button beside Sidebar options. New is highlighted while an empty chat is open, and hints, counts, unread dots, and section controls have consistent alignment.
- A collapsed sidebar now **peeks in the boxy Focused layout** as well as the rounded layout when hovering at the left edge or sidebar toggle. Glass blur eases with the slide animation.
- Settings keeps **Search at the top** of its navigation and **Back to app at the bottom**, with the pages scrolling between them.
- Mono transcript signatures use mascot tiles. Menu text has more line height to prevent clipped letter descenders on Windows.

### Claude sign-in and resumed replies

- On Unix, provider cleanup and app quit give processes time to finish credential writes before forceful termination. This addresses Claude Code being signed out when a rotating refresh token was saved to macOS Keychain.
- Claude model catalogs are cached for **six hours**, reducing cold CLI starts. CLI updates invalidate every account's catalog, and Claude reloads it for the account in use.
- An expired Claude access token with a refresh token keeps the last usage reading instead of asking you to sign in again; usage updates after the CLI renews it on its next run. A cleared macOS Keychain login correctly shows Sign in, even if an old credentials file remains.
- A browser sign-in that is still waiting can be **cancelled** from the usage footer or the add-account editor. Returning to the app gives an unfinished login a short grace period, then resets it so you can try again.
- Resuming a Claude session no longer lets a replayed background-task notification end the real turn early or leave behind a stale usage-limit error. The actual reply or error can arrive normally.

### Rendering and macOS reliability

- Streaming reply updates share the animation clock's redraws while it is running. Sized CoreText fonts are cached, and per-frame notification checks avoid copying preferences, reducing repeated rendering work.
- macOS windows keep retrying outstanding frame requests after a display-link stall, including when recovery was temporarily blocked by occlusion or a missing screen. Hidden windows remain stopped.
- Screenshot-thumbnail drops no longer rely on a two-buffer Metal layer that could block the main thread waiting for a drawable. The renderer also tracks whether drawn frames reached the screen and attempts recovery when an active window's frames go unshown.
- GPUI warnings and macOS repaint diagnostics are saved to **`~/Library/Logs/Monocode Native/gpui.log`**, including slow renderer waits and frame summaries after file drops. The log rotates and suppresses repeated messages.

### Phone companion

This change requires a separately distributed phone companion update; installing the native desktop preview alone does not update the phone UI.

- You can **keep writing while the desktop reconnects**. The composer shows a reconnecting placeholder, keeps the draft editable, and leaves dictation available. Send and Stop wait for reconnection; Send shows a pulsing status dot and an accessibility hint when a draft is ready. Session errors wait until the desktop is back before replacing the field.

### Development and coverage

- **`MONOCODE_FRAME_LOG`** provides opt-in frame counts, draw times, main-thread CPU on macOS, view reuse, redraw origins, transcript measurement time, and text-shaping counts. **`scripts/perf/frame-run.sh`** runs repeatable scripted streaming or scrolling captures with isolated development data and optional stack profiling. See the [performance guide](https://github.com/hardbeat920/monocode/blob/native-v0.16.0-preview.14/scripts/perf/README.md).
- Regression coverage was added or updated for Claude replay and credential handling, graceful provider shutdown, sign-in cancellation, terminal placement/splitting/drag-out/close confirmation, new-session defaults, pinned tabs, composer focus and layout, sidebar navigation and peeking, frame-demand recovery, and offline phone composition. Process-escalation fixtures tolerate delayed group exit, and logger setup is included in test builds for strict Clippy.
- The root README now separates Native and Tauri downloads and refreshes its screenshot and image links. The [complete change audit](https://github.com/hardbeat920/monocode/blob/native-v0.16.0-preview.14/docs/native/preview-14-change-audit.md) records every commit since preview 13, merged contributions, and the parallel React/Tauri Claude usage fixes.

Thanks to @keyCat and @Kaloyanes for their contributions since preview 13.

## Installation and testing

Choose the download for your platform: macOS Apple Silicon or Intel, Windows x64, or Linux x64 (AppImage, Debian/Ubuntu `.deb`, or Fedora/Enterprise Linux `.rpm`). Install it alongside your existing Monocode application.

**Testing status:** macOS is the primary tested platform. A Linux ARM64 source build was exercised in an Ubuntu VM with a graphics workaround, and a Windows 10 source build passed manual title-bar checks. The downloadable Linux x64 and Windows packages still need broader runtime testing. Native system notifications currently support macOS only. HTML panels and media support remain limited on Linux and Windows.

Local automated checks, the native dependency audit, and the long-history CPU comparison are recorded in [verification](https://github.com/hardbeat920/monocode/blob/native-v0.16.0-preview.14/docs/native/verification.md).

**Data:** this preview uses your existing Monocode database and saves changes there. Back up your Monocode data with the app closed before testing. Native preferences are stored separately. Avoid editing the same conversations in both apps at once.

**Updates:** use Settings → General → About to check for updates and install the next native preview. Packaged upgrades still need runtime testing; if updating fails, download the new installer manually. Linux `.deb`/`.rpm` installs use the system authentication prompt (`pkexec`) to install the downloaded package. Cancelling authentication leaves the current version installed. Native previews receive native preview updates; production Tauri releases use their own feed.

The initial installation requires a download. macOS completion/approval notifications, sounds, and Dock badges still need testing in the distributed app.

Feature and platform parity are still being tested. Read [preview limitations](https://github.com/hardbeat920/monocode/blob/native-v0.16.0-preview.14/docs/native/parity.md) at this release's tag for known limitations. A successfully built package does not establish that every feature works on that platform.

Please report the preview version, operating system/architecture, installation format, provider, and reproduction steps with any issue. Remove credentials and private conversation content from logs and screenshots.
