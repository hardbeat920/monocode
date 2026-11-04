# Monocode Native 0.16.0-preview.13

This experimental Monocode Native preview is built with Rust + GPUI. React/Tauri remains the production app.

## Changes since preview 12

### Tabs and terminals

- **Pin title tabs** from their context menu to keep them at the left edge of the tab strip. Up to three pins sit in a row; larger collections stack into a deck that fans out on hover and shows each tab's title. Pins are saved with the workspace, and tab shortcuts and cycling visit them first.
- **Drag a terminal out of the dock** onto a pane edge to split it beside your chat, or onto the title strip to give it its own tab. The running shell moves with it, so commands keep running. Terminal panes restore their directory after restarting the app, with a new shell.
- Tabs use solid, raised icon tiles. Split tabs stack their panes' tiles and shuffle the focused pane to the front; Mono, Search, app, and review tabs use the same styling. Pane focus motion follows the system's Reduce Motion setting independently of the Tab animations preference.
- Composers, search fields, message editors, the file editor, and terminals have **right-click editing menus**. Inputs offer Cut, Copy, Paste, and Select All as applicable; the file editor also offers Undo and Redo. Terminal menus offer Copy, Paste, and Select All, and composer Paste still accepts clipboard images.

### Chat and change reviews

- A compact **changes pill above the composer**, beside Commit, shows added and removed line counts and opens the session's review card. Keep, Undo, and file review are available there without scrolling back to the latest reply.
- Unresolved file changes **carry forward across turns** until kept, undone, or committed, retaining their original baseline. Files discarded elsewhere, including from the Changes panel, leave the pending review and stay dismissed if edited again. Recorded shell edits also make the Commit pill available.
- **Settings → Chat → Collapse tool groups** keeps groups closed even while they run, until opened manually. **Fold finished work** controls whether completed work folds under the "worked for" header or keeps text and tool groups inline. Defaults preserve automatic expansion while running and folding after completion.
- Tool groups, individual tool calls, and work headers use matching tiles and connected activity rails. Hovering a group's tile redraws that tile instead of the whole transcript.
- The composer button expands into a labelled pill and animates between **Send, Queue, Steer, Save draft, and Stop** as the action changes.
- Mouse-wheel notches glide through their full distance, with queued motion cancelled when resetting the transcript or returning to the latest reply. Trackpad scrolling keeps its native pixel motion, and Reduce Motion disables the wheel glide. Transcript rows do not stretch elastically while scrolling.
- Reading position stays within the same turn when the row being read folds away, using the full row key to find its replacement. Reopening a transcript that was unloaded from memory restores its reading position, including remote chats.
- The prompt outline appears once the transcript learns its width. Width changes request an extra redraw only when the outline's visibility changes.
- Unlabelled and `text`, `plaintext`, or `txt` code fences render as plain text, preventing paths and apostrophes in prose from picking up JavaScript syntax colors.

### Search and settings

- **Settings → General → Search as command panel** opens global search as a floating panel over the current workspace. It includes recent conversations and files before typing, grouped results, scope filters, and keyboard navigation. The existing Search page remains the default.
- Settings has a **collapsible navigation sidebar**, with Back to app and search at the top. Collapsing it moves search into the title bar; reopening it restores the sidebar without changing the project's sidebar preference. Search results support arrow-key navigation.
- Long settings dropdowns have a filter field and a scrollable list.
- Model-picker rows have a **favorite star**, including default-model rows and remote model lists. Clicking it adds or removes the favorite without selecting the model, changing recent choices, or closing the picker.
- A dismissible **Star on GitHub** prompt appears in the sidebar for signed-in GitHub CLI accounts that have not starred the repository. It stays hidden when GitHub CLI is unavailable, signed out, or offline.

### Models, remote sessions, and reliability

- New chats consistently choose their provider and model across the sidebar, app views, Quick Composer, and phone. Selection follows the project default, app default, most recent choice, then an available fallback, respecting hidden providers for local projects.
- Phone-started chats use the configured model for an explicitly chosen provider, even when that provider differs from the desktop default. Phone model lists respect project-specific hidden providers, while remote projects retain their available remote providers.
- Titles changed on a remote host update chats that are already open, along with their tabs and sidebar entries, without writing the received title back to the host.
- Provider processes retry brief Unix **Text file busy (`ETXTBSY`)** startup failures instead of immediately failing to start.
- macOS windows recover repainting after their display link stalls, keep watching for stalls while frames are needed, and coordinate live-resize redraws with the window layer.
- Windows terminals resolve shells against the child process's PATH, honor an installed native executable named by `SHELL`, and otherwise prefer PowerShell 7, then Windows PowerShell, then `COMSPEC`/`cmd.exe`. Missing executables and Unix-only shell paths are ignored.
- Dismissing an update notice no longer opens **What's new**.
- The sidebar update control uses a compact **Update to v…** row with the installed version alongside it, an accent highlight when an update is available, and an animated progress indicator while working.

### Monos

- A Mono's **Settings → General → Sessions it starts** section can choose a separate model and effort for its background sessions. **Same as the Mono** inherits its own settings. These defaults apply when the Mono does not explicitly choose a provider, model, or effort for the launch, and can also be edited from the phone.
- New Mono chats start at **low effort** for quick conversation; existing chats keep their selected effort. Standing instructions ask Monos to reply before using tools, delegate longer work, and save memory after replying.
- Monos can call the app directly through an **MCP app tool** with Claude, Codex, and ACP providers when app access is available, avoiding a shell launch for each app action. The app CLI remains available as a fallback.
- Mono activity labels describe the current action, such as remembering a fact, starting a session, checking CI, or looking at pull requests, using tool descriptions and short reasoning headings where available.
- Claude and Codex receive standing Mono rules through their system or developer instructions, while changing soul and memory content remains in turn context. Resumed Codex threads receive updated rules in the message when their saved instructions are older, including after the app executable moves.
- A Mono no longer picks up a message already held by its own running turn, avoiding duplicate steering or interruption just after a provider switch.

### Paired phones and companion app

The following features need an updated phone companion as well as the desktop changes in this preview. Companion builds are distributed separately from the native desktop packages.

- Sessions and Mono chats paused by a usage limit show the desktop's reset time and let the phone **Continue** when eligible or enable/disable **Resume at reset** while waiting. Automatic continuation requires the desktop app running with the session loaded; the native app saves the preference and resumes scheduling when that session loads again. Remote sessions and removed working copies must be recovered on their desktop. Stale or duplicate phone actions are rejected when the session changes.
- **Settings → Layout → Monos only** switches to a messaging-style Mono home with search and unread indicators. A new creation sheet chooses a name, mascot, and color before opening the new Mono's chat. Mono profiles show five recent documents and a full document list.
- In Monos-only mode, a Mono's color supplies the chat accent when no chat or app accent was chosen. The mobile dark theme uses lighter base and surface colors.
- Wide phone, tablet, and Android windows show a persistent project/recent-session sidebar when there are at least 740 points of usable width. Resizing keeps the conversation and draft mounted.
- On **iPhone Duo with iOS 27.1 or newer**, folding in portrait to about 120° or less focuses the visible composer. Opening past 150° rearms it. Dismissing the keyboard keeps it dismissed until leaving and re-entering that posture; folding while a sheet or menu is open does not defer a focus request.
- iOS Mono chats animate row movement, departing rows, reaction tapbacks, and the thinking mascot. Brief replies avoid flashing a Thinking indicator, and Mono body text is larger. Closing a Mono chat releases the keyboard, and keyboard transitions no longer briefly reuse the previous keyboard height.
- Android menus, pickers, sliders, and layout controls have Android implementations. Choice sheets group actions with icons, selection checks, and destructive styling. Mono profiles open as sheets with corrected header spacing and titles, including when editing a newly created Mono.

### Development and coverage

- A scripted Claude stream and profiling runner provide repeatable CPU captures on temporary development data without calling a provider. See the [performance guide](https://github.com/hardbeat920/monocode/blob/native-v0.16.0-preview.13/scripts/perf/README.md).
- Mobile render-timing logs are opt-in through `EXPO_PUBLIC_RENDER_TIMING=1`. iOS companion builds now require Xcode 27.1 for hinge observation, with the EAS build image pinned accordingly.
- Regression coverage was added for model favorites and new-chat defaults, remote titles and reading positions, review carry-over and external discards, wheel scrolling, search and settings navigation, pinned tabs, terminal drags and editing menus, Mono instructions and launch defaults, mobile usage recovery, adaptive layouts, and keyboard posture handling. Formatting, Clippy, and mobile test setup were also corrected.
- The [complete change audit](https://github.com/hardbeat920/monocode/blob/native-v0.16.0-preview.13/docs/native/preview-13-change-audit.md) records the reviewed commit range, merged contributions, and React/Tauri-only fixes that do not ship in the native binary.

Thanks to @Juns-g, @nwoolls, @404khai, @knot2006, @visualfusion, @keyCat, and @oskar-albers for their contributions since preview 12.

## Installation and testing

Choose the download for your platform: macOS Apple Silicon or Intel, Windows x64, or Linux x64 (AppImage, Debian/Ubuntu `.deb`, or Fedora/Enterprise Linux `.rpm`). Install it alongside your existing Monocode application.

**Testing status:** macOS is the primary tested platform. A Linux ARM64 source build was exercised in an Ubuntu VM with a graphics workaround, and a Windows 10 source build passed manual title-bar checks. The downloadable Linux x64 and Windows packages still need broader runtime testing. Native system notifications currently support macOS only. HTML panels and media support remain limited on Linux and Windows.

Local automated checks, the native dependency audit, and the long-history CPU comparison are recorded in [verification](https://github.com/hardbeat920/monocode/blob/native-v0.16.0-preview.13/docs/native/verification.md).

**Data:** this preview uses your existing Monocode database and saves changes there. Back up your Monocode data with the app closed before testing. Native preferences are stored separately. Avoid editing the same conversations in both apps at once.

**Updates:** use Settings → General → About to check for updates and install the next native preview. Packaged upgrades still need runtime testing; if updating fails, download the new installer manually. Linux `.deb`/`.rpm` installs use the system authentication prompt (`pkexec`) to install the downloaded package. Cancelling authentication leaves the current version installed. Native previews receive native preview updates; production Tauri releases use their own feed.

The initial installation requires a download. macOS completion/approval notifications, sounds, and Dock badges still need testing in the distributed app.

Feature and platform parity are still being tested. Read [preview limitations](https://github.com/hardbeat920/monocode/blob/native-v0.16.0-preview.13/docs/native/parity.md) at this release's tag for known limitations. A successfully built package does not establish that every feature works on that platform.

Please report the preview version, operating system/architecture, installation format, provider, and reproduction steps with any issue. Remove credentials and private conversation content from logs and screenshots.
