# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.31] - 2026-09-03

### Added

- Changes: a git graph under the working tree (swimlanes, merge arcs, HEAD ring). Click a commit for a read-only unified diff of that revision. The list is HEAD, its upstream, and the default branch — the newest 200 commits, not stashes or unmerged local branches. Drag the sash to resize; the Graph header collapses the pane.
- Opening an image file shows a viewer with zoom, dimensions, and file size instead of the text editor. The view reloads when the file changes.
- Inbox issue and pull request markdown shows GitHub and Linear images and videos inline.
- Changes: discard every unstaged file from the section header, with a native confirm.
- File → New Tab (`⌘T` / `Ctrl+T`), Inbox in the app menu, and a copy control on agent markdown code blocks. In #54 by @tcmarkfeld.
- Transcript turn status names the model and shows the harness icon while a turn is working, waiting, or done.

### Fixed

- Unified diff: every added or deleted line in a hunk can be staged, not only the first. The line-number gutter stays put while the code scrolls sideways, and the per-line stage control appears on hover.
- Closing other tabs with unsaved files uses a native confirm. `window.confirm` was swallowed when a macOS menu accelerator fired, so Close Other Tabs skipped the discard prompt. In #54 by @tcmarkfeld.

## [0.1.30] - 2026-09-02

### Added

- Settings → General → Diff view: Editor or Unified. Unified stacks every working-tree change in one **Changes** tab — GitHub-style review, editor syntax colours, sticky file headers and line numbers, and a single horizontal scroll that stops at the end of the line. Editor keeps the previous per-file working-tree tabs.

### Fixed

- Escape stops the in-flight agent turn you are focused on. Modals, pickers, search, and the editor still consume Escape first, and a terminal still uses Ctrl+C — Escape is not a PTY interrupt. In #44 by @MisterWanted.

## [0.1.29] - 2026-09-02

### Added

- Inbox rows show whether an issue or pull request is open, draft, merged, or closed. The icon changes with the status (not only the colour), and the detail header uses the same marks, so closed items are no longer the same grey as drafts. In #49 by @emircan-sahin.

### Changed

- Classic layout and the zen-mode toggle are gone. The workspace is always the project rail plus scoped tabs, and the transcript always folds tool work into phases above the final answer.
- Session folder menus show the same saturation picker as project colors, and the picker stays open.
- Agent and Task tools read as subagent work in the transcript — "Running a subagent" while they run — instead of a generic tool row. Codex nested-agent activity shows up the same way.

### Fixed

- Claude's AskUserQuestion (and the same clarifying-question flow on Cursor, Grok, and OpenCode) now opens a form above the composer. Questions come one at a time — answer or skip, then the next — instead of an Allow/Deny prompt that silently chose the first option.
- Agent CLIs no longer leak after a quit or a crash. `cursor-agent` survived as orphaned `node` processes because quit sent SIGTERM and exited before the delayed SIGKILL could land. Quit now waits for those trees to die, and the next launch reaps leftover agent processes from a previous run. Terminals close with the app; programs you started from a terminal are left alone.
- A Claude turn no longer looks finished while a background subagent is still running. Completion waits until those tasks settle.
- Clearing Merged or Closed on the inbox filter no longer snaps the list back to open items a moment later. An unfiltered inbox also fetches a longer page so open work is not crowded out by closed history. In #49 by @emircan-sahin.

## [0.1.28] - 2026-09-01

### Added

- Sidebar: group sessions into folders. Folders sit above pinned chats. Right-click a session to create a folder or add it to one, drag a session onto another to make a folder, or drag into an existing folder. Drag folders to reorder them. An open folder has a New session button that starts a chat in that folder.

### Changed

- Claude Code models are fetched from the CLI instead of a hardcoded list, so the picker matches what your install and account can run.

## [0.1.27] - 2026-09-01

### Changed

- Opening a project is faster. Session cards no longer compute +N/−N diffs on first paint, and the sidebar mounts a page of chats then loads more as you scroll.
- Session history is on the rail when the window appears. Recently opened chats stay cached so clicking a card paints without waiting on disk.

### Fixed

- Switching tabs keeps the transcript where you left it — the turn you were reading and the scroll position.
- Title bar tabs and window controls use the arrow pointer instead of a grab cursor.

## [0.1.26] - 2026-09-01

### Changed

- After an in-app update, a card on the project rail above Check for updates names the version. Click it for a What's new modal with the changelog. Settings → General still has What's new.

### Fixed

- A busy terminal could freeze the window. Output is batched before it reaches the UI, hidden windows skip extra work, and a killed agent cannot keep spawning.
- Installed Claude Code plugin skills now show up in the skill list. In #43.

## [0.1.25] - 2026-09-01

### Added

- Inbox: comment on GitHub pull requests and issues, and on Linear issues, from the detail pane. Reply stays in a GitHub review thread or Linear comment thread.
- Check for Updates lives in the app menu — next to Settings on macOS and in the File menu elsewhere — so you can check anytime; Settings → General has it too.
- Sidebar: pin a session from the context menu to keep it at the top until you unpin it.
- Settings → General: Empty session games. Turn it off to hide pac-man and snake from the empty pane. On by default.
- GitHub Releases ship a `.deb` and an AppImage for Linux (x86_64).

### Changed

- Second opinion uses overlapping chat bubbles instead of a fork/split glyph, so it no longer looks like branching a chat.

- The empty-session grid now slides between pac-man and snake on its own. Pac-man is chased by four project mascots through a maze that runs edge to edge; snake still hunts pellets and provider logos. Hover pauses the slider, the dots jump to a game, and take control plays whichever is on screen — three lives on pac-man, the same low/mid/hard speeds on both. The grid's random cell flicker is gone.

- Launch holds the logo until the restored workspace is ready, then fades to that first paint. The window stays up through the Dock bounce so the mark is visible instead of a blank or shifting chrome.

### Fixed

- Reloading a file keeps the editor's scroll position and selection instead of swapping the whole document. Find and replace run in the visible editor, not a hidden tab.

## [0.1.24] - 2026-09-01

### Added

- Handoff: a card-exchange icon next to Copy and Second opinion on a finished turn opens another provider in a split pane. A composer card holds the recap so you can add context before sending. The original session keeps its model.
- Status bar: when a terminal is running a command (a dev server, tests, …) a chip on the right shows three orange bars lighting in sequence, plus the process name. Click it to show or hide that terminal.

### Changed

- Zen tool-call rows no longer show a spinning dashed ring on the right while a step is running.

### Fixed

- Launch is one frame: the window stays hidden and opaque until the splash logo is painted, then glass turns on as that overlay dissolves. The empty-session grid fades in.

## [0.1.23] - 2026-08-31

### Added

- After an in-app update, MonoCode shows a notification with a **What's new** action. It opens the bundled notes in a read-only tab only when requested. The notes remain available under Settings → General → About.
- Inbox pull request and issue details load the conversation (comments, reviews, and review threads) in the background, so the description still appears first.
- Grok Build joins the provider list. Install it with `curl -fsSL https://x.ai/cli/install.sh | bash` and run `grok login` (or set `XAI_API_KEY`). MonoCode runs `grok agent stdio` like the other ACP harnesses: live turns, supervised approvals, model catalog, reasoning effort, context usage, skills from `.grok/skills`, and titles / commit / PR text.
- Search sits next to Inbox and Notes in the sidebar project picker, with the same ⌘K / Ctrl+K hint.

## [0.1.22] - 2026-08-31

### Added

- Settings → Appearance: System theme. The picker was Dark and Light only, so the app never followed the OS. System tracks the appearance while MonoCode is open; Dark stays the default, so existing installs do not flip. In #36 by @emircan-sahin.
- Search, Inbox, and Notes show Back and a sidebar toggle in the title bar when the project rail is closed, so you can get the rail back without leaving the overlay.

### Changed

- Deck layout: the project picker — label, logo or mascot, and switcher — lives in the sidebar header instead of the title bar. Inbox and Notes actions move there too when a project is open. The title bar picker remains only when Deck has no project.
- Live zen tool calls stay in a short autoscrolling window. A long research run no longer grows the transcript without bound; the open phase stays pinned to the newest step, then expands again when you reopen the group after the turn settles.
- Successful tool calls no longer show a checkmark. Failed and rejected rows stay marked, in red.
- Working-agent titles on the project rail are plain text instead of a shimmer, and idle status is muted.

### Fixed

- Drag the window from any empty spot in the header — title bar, sidebar, project rail, and the Inbox, Notes, Search, and Settings rows. Labels and gaps were dead zones, and a double click highlighted tab text instead of maximizing. In #37 by @emircan-sahin.
- Deleting a session in Deck stays on that project: a sibling tab opens if one exists, otherwise the emptied tab is replaced instead of jumping to another project's tab.
- Deck title bar tabs fill the available width instead of growing to a fixed size, and labels stay 13px until the tab is wide enough for the meta text.

## [0.1.21] - 2026-08-30

### Added

- Working agents on the project rail. When two or more chats are in flight — including parked ones from other projects — a card above Check for updates lists them so you can jump across. Finished turns stay until you open that session. Settings → General has a Working agents toggle (on by default) to hide the card.

## [0.1.20] - 2026-08-30

### Added

- Pi sessions load skills from Pi itself instead of scanning skill folders. The slash picker lists `/skill:name` the way Pi expects, and typing that prefix still finds the row. In #35 by @kinsomicrote.
- Settings → General: Anchor prompts to top. When you send, the new prompt sits at the top of the transcript and the reply grows into the space below. Off keeps the classic layout, with the latest message on the composer.
- Line numbers in the notes markdown source editor.

### Changed

- Shell tool rows that are really a read, search, or listing (`cat`, `grep`, `ls`, and similar) now show as `Read path`, `Find query`, or `List path` instead of the raw command.
- Icons across the chrome and transcript use a single stroke catalog. Fold/unfold marks are stroke-only so they match the rest of the set.

## [0.1.19] - 2026-08-30

### Added

- Notes: a markdown notebook on the project rail. Save a finished turn from the transcript, write your own, then mention it later with `@note` or add it to chat — the note shows as a card, the same way Inbox issues do. Settings → General has a Notes toggle (on by default) to hide it from the UI.
- Claude Code hooks now run. MonoCode used to launch the CLI with `disableAllHooks`, so every hook in your `settings.json` — command rewrites, blocks, notifications — was silently skipped. Settings → General has a "Claude Code hooks" toggle (on by default) to turn them back off if one misbehaves. MonoCode's own helper spawns, like title generation, stay hook-free. In #25.

### Changed

- Zen mode no longer ticks through one tool at a time. Related tool calls fold under the line the agent wrote to introduce them — a phase that stays open while it runs and collapses to a labelled header once the agent moves on, leaving an outline above the final answer. Click any group to read it back.
- The usage footer only shows on a session, and only polls the provider that session is using. Search, Inbox, and Settings hide it.
- Non-git folders keep the branch picker in the composer toolbar, labelled "No repo", so the bar does not jump when you open a plain directory.
- Title bar tabs are rounded pills without dividers. The project rail's search field and project cards share a fixed height.
- `#` headings are coloured in the notes editor and in markdown source preview.

### Fixed

- A permission prompt that a `PermissionRequest` hook resolves before you do is no longer labelled "Rejected" in the transcript.

## [0.1.18] - 2026-08-29

### Added

- Sounds in Settings → General: a short cue when a turn finishes, when a new inbox item lights the project-rail dot, or when an update is available. Switches click, and Copy on a finished turn plays a scan. Off mutes every cue.

### Fixed

- Skill tool rows only said `Skill`. They now show `Skill /name`, the same way reads show the file. In #32.
- Shell tool rows only showed the tool name (`bash` / `Bash`) for Claude, Pi, and some other providers, so you could not see or cancel the command that was about to run. The activity ticker now shows the command itself, matching Codex. In #32.
- The `@` mention picker in the composer only offered files, so you could not point the agent at a folder. Directories from the project tree are selectable now. In #34.
- Codex turns looked finished while the agent was still working: “Worked for” froze and the composer stop button went back to send, even though tools and text kept arriving. The turn now stays live until Codex actually completes it.

## [0.1.17] - 2026-08-29

### Added

- Second opinion: the scale icon next to Copy on a finished turn sends that work to another provider in a split pane. Hover a provider to pick its model. The reviewing session shows a compact card instead of the review prompt.

### Changed

- Provider CLIs stay warm for five minutes after a turn so follow-ups stay instant, then park. Title/commit one-shots no longer leave a second process running. The usage footer only polls Claude/Codex when a session in this window actually uses them.

### Fixed

- `npm run set-version` left `package-lock.json` behind, so the lockfile still called itself 0.1.0 sixteen releases on. The script now updates both of the version fields it carries, and the lockfile is back in sync.
- Closing a tab in Deck keeps the active tab in the current project when another tab from that project is open. In #22 by @kinsomicrote.
- Closing the last tab of a project in Deck no longer jumps to another project. Command+W stays where you are; Classic layout still flows across mixed-project tabs.
- Unused provider CLIs no longer start at launch. Catalog probes run only for harnesses in the restored workspace, or when you open that provider in the model picker. Pi/omp probes skip extensions so a leftover `pi` process cannot sit at ~1GB while you work in Codex or Claude. Diagnosed in #19.
- A Pi or omp turn that fails now reports why. Pi puts the failure on the assistant message (`stopReason: "error"`) instead of an error frame, so an expired provider token ended empty and looked like the agent ignoring you. In #23 by @emircan-sahin.
- The context meter stayed at zero for a Pi or omp turn. Usage lives on the assistant message (and the streaming partial), not the top of the frame. In #23 by @emircan-sahin.

## [0.1.16] - 2026-08-28

### Added

- Toggle zen mode with `⌘⌥Z` (`Ctrl+Alt+Z` on Windows and Linux).

### Changed

- Zen mode improvements.
- Finished turns show the time they completed next to the copy button.

### Fixed

- Full-width transcript layout: a user prompt no longer sits flush against the tool call under it.

## [0.1.15] - 2026-08-28

### Changed

- Sidebar tabs read as rounded segments inset from the row rather than full-height boxes with dividers: Sessions, Explorer, and Changes in both layouts, and the inbox's GitHub and Linear tabs.
- The classic sidebar's project header no longer floats over the session list, and the Explorer's toolbar and root folder row stay pinned while the tree scrolls under them.

### Fixed

- Zen mode: expanding a settled turn's toolchain no longer folds it again when the turn's earlier tool calls were already open. The summary and the `+N previous` disclosure now track their own state.

## [0.1.14] - 2026-08-28

### Added

- Zen mode in Settings → General quiets a noisy transcript. While a turn runs, edits collapse into the same one-line activity list as reads and searches instead of stacking full diff cards; once the turn settles the whole toolchain folds behind a single `12 tool calls · 4 files edited` line, leaving the agent's closing answer. Edits waiting on approval still show their diff, since you cannot judge a change you cannot see. Off by default.
- Transcript layout in Settings → General: Full width keeps user prompts as a spanning card, Chat aligns them to the right with a max width.
- Copy button on completed agent turns, copying the assistant prose and any plan as Markdown.
- Inbox pull requests have Summary and Code tabs, with the `gh`-backed diff rendered as highlighted file changes (large diffs are truncated).
- Inbox shows a dot on the project rail and on cards for items that are new or updated since you last looked.

### Changed

- Edit rows read as `Edit src/lib/appearance.ts` with a file-type icon and a clickable path, matching how reads and searches already render.
- Inbox authors and assignees show avatars from GitHub and Linear, falling back to initials.
- Inbox author moved into the detail metadata row alongside assignees and time, instead of repeating in the body.
- New workspaces default to the Deck sidebar layout.

### Fixed

- Streamed Markdown keeps headings, blank lines, tables, and repeated characters. Completed Claude/Codex snapshots no longer paste the same reply twice. Diagnosed in #15 by @kinsomicrote.

## [0.1.13] - 2026-08-27

### Added

- Show in picker in Settings → Providers: hide an installed provider from the model picker without removing it from Settings.

### Changed

- The model picker only shows providers whose CLI is installed. Uninstalled harnesses stay listed in Settings → Providers.

### Fixed

- Claude usage in the footer refreshes OAuth tokens before they expire and retries on 401, so the chip stays signed in. Failed sign-in shows expired instead of a generic error.
- GitHub inbox and `gh` subprocesses work when MonoCode is launched from Finder, by resolving the CLI through the login-shell PATH the same way harnesses do.

## [0.1.12] - 2026-08-27

### Added

- Inbox: assigned GitHub issues and pull requests from `gh`, plus Linear issues after you paste a personal API key in Settings → General. Start an issue in a local project; pull requests open on GitHub instead of starting a session from an untrusted branch. Linear Start includes the issue description, since agents cannot fetch Linear pages.
- Starting from Inbox shows a Linear or GitHub card above the composer — logo, identifier, title, team or repo — instead of pasting the issue into the textarea. Send still includes the issue for the agent; you can add a note or dismiss the card.
- Claude and Codex plan usage (5-hour and weekly) shows in a footer under the main pane: percent used and time until reset. If a provider isn't installed or signed in, the chip says not connected and isn't polled again until you refresh or relaunch.
- While a turn is running, the project's pixel mascot patrols the composer (or the changes bar), hops the jump-to-latest control, and occasionally grabs a coin. Turn it off in Settings → General.

### Fixed

- Inbox still lists the other source when GitHub or Linear fails, with the error on that tab instead of a blank inbox.
- Linear team hiding and status filters apply to the active tab, and Start errors show in the issue detail pane.

### Security

- Linear personal API keys stay on this Mac: written to the app data folder (`~/Library/Application Support/com.monocode.desktop/linear-token`) with owner-only permissions (`0600`). They are sent only to Linear's API to list and read issues, never to MonoCode servers, and never placed in the agent prompt. Disconnect deletes the file. GitHub uses the `gh` login already on the machine; MonoCode does not store a GitHub token.

## [0.1.11] - 2026-08-27

### Added

- Select text in a finished agent response and choose **Add to chat** to quote that excerpt in the same session's composer.
- Projects with no conversations yet show an empty sessions state instead of a blank list.
- Switching or creating a branch prompts to stash or commit when checkout would overwrite local changes.

### Changed

- Branch picker stays put with a loading skeleton while git lookup settles.
- Session history is cached across projects and refreshed in the background, so switching back is instant.
- Startup is lighter: Material icons and Mermaid load on demand, and git, harness, and model probes are cached.

### Fixed

- Switching projects no longer flashes a loading spinner over the session list.

## [0.1.10] - 2026-08-26

### Added

- omp ([oh-my-pi](https://omp.sh)) joins the provider list. Install it with `curl -fsSL https://omp.sh/install | sh` and log in, and MonoCode runs it like any other harness: live turns, steering, approvals, model catalog, and skills from `.omp/skills`.
- Check for updates in the classic sidebar footer.

### Changed

- Pi and omp share one adapter core. omp is a fork of Pi and speaks the same `--mode rpc` protocol, so both run on the same code path instead of two copies that drift apart.
- Classic layout opens the full settings page with `⌘,` instead of the appearance popover in the title bar. Section navigation lives in the sidebar while settings are open, and Settings sits at the bottom next to Check for updates.
- Deck layout shows Settings and Check for updates in the sidebar footer when the project rail is collapsed.
- Composer branch picker creates and checks out a branch in this folder.

### Fixed

- Deck layout no longer duplicates Settings and Check for updates when the project rail is open, or shows a title bar settings button while a project is selected.

## [0.1.8] - 2026-08-26

### Added

- Project rail context menu: Archive takes a project off the rail and keeps its conversations; Delete asks first, then also removes saved chats. Archived projects show up in Settings → Archive, where you can restore them to the rail or delete them. The folder on disk is left alone either way.
- Session branches: switching or creating a branch in a session checks it out in a git worktree, so the project's HEAD stays put. Sidebar changes and diffs follow that session's working copy, and the branch comes back when you restore the session.
- Per-provider default models in Settings → Providers. The model beside each provider is what new conversations use when that provider is selected.

### Changed

- Global search placeholder reads “Search everything…”, with tighter scope buttons and a clearer hover state on unselected scopes.
- Delete project confirmation states that all project conversations will be removed, with a separate count when saved conversations exist.

### Fixed

- Wide code blocks in the transcript scroll horizontally instead of clipping.

## [0.1.7] - 2026-08-26

### Added

- Deck layout: a second window layout, opt-in and off by default. Switch between Classic and Deck under Layout in the appearance menu. Deck puts a project rail down the left edge with every project you have opened, and scopes the title-bar tabs to the selected project instead of mixing all of them together. `⌘B` shows and hides the rail.
- Project rail cards show live state: an animated pixel mascot per project, a spinner and a shimmering name while a turn is running, and `+n -n` for uncommitted changes. Pick a different mascot for a project from its context menu.
- Project terminal dock (deck layout): terminals belong to the project rather than to one tab, and dock to the top, left, right, or bottom edge. `⌘J` hides and shows the dock, and the layout survives tab switches and restarts.
- Changes is a sidebar tab in deck layout, next to Sessions and Explorer, with the working-tree diff stats on the tab itself.
- Global search with `⌘K`: one field across files, projects, and past conversations, including the text of messages inside them.
- Settings page with `⌘,`: general, appearance, keybindings, providers, and archived sessions in one place.
- Sessions can start without a project in deck layout. The session opens with a project picker and you choose the folder when you are ready.
- Projects can be removed from the rail, with an option to also delete their saved chats and appearance settings. The folder on disk is left alone either way.
- Session archive: right-click a session in the sidebar to archive or unarchive it. Archived sessions are hidden by default and stay archived across restarts.
- Session sidebar filters: filter by provider, status (working, needs approval, done), and time (today, last 7 days, last 30 days). Toggle archived sessions from the filter menu. Filter choices persist across restarts.

Thanks [@Queaxtra](https://github.com/Queaxtra) for the filter and archive ideas.

### Changed

- A paused turn shows “Waiting for approval” in place of the timer instead of dropping the row, so the transcript no longer shifts while you decide.
- The changes view can be opened from the file tree header as well as the title bar. The title-bar control shows a diff icon when there are no uncommitted changes yet, and the close button was removed from the changes pane — use either toggle to show or hide it.
- Composer placeholder mentions `@` for file references.

## [0.1.6] - 2026-08-25

### Added

- Handoff: switching providers mid-session continues the chat on the next send. The new message goes to the incoming provider with a short recap of what happened and any files this chat edited. The divider shows a spinner and “Preparing a handoff” until that provider starts, then its logo and name.

### Fixed

- Read and Find rows show the file or search query next to the verb, instead of a bare Read/Find. Every provider uses the same nested-arg extraction; Cursor also recovers Glob/Grep from its session store when ACP sends empty input.
- Provider CLIs installed through a Node version manager (nvm, fnm, mise, Volta) no longer show as unavailable when MonoCode is launched from Finder. Detection reads PATH from an interactive login shell, so anything set up in `.zshrc` is found, and a disabled provider now says its CLI was not found instead of implying it needs to be authenticated.
- Codex works when only the Codex desktop app is installed. MonoCode falls back to the CLI bundled inside `Codex.app` when no standalone `codex` is on PATH, preferring a real install whenever one exists.

## [0.1.5] - 2026-08-23

### Fixed

- Updater archives now use immutable, versioned URLs so Cloudflare cannot pair a cached previous release with the latest signature.

## [0.1.4] - 2026-08-23

### Added

- Project files now show their Git status with color in the file tree.

### Fixed

- fx sessions no longer stall after the first turn or when starting another session; fast ACP responses are registered before they can be delivered, and failed transports are recycled cleanly.
- fx now exposes the model selected by its TUI even when `fx models --json` omits it, including GLM 5.2.
- fx tool activity shows useful file, search, command, output, and failure details instead of empty or misleading rows.
- Finder-launched builds pass the user environment and available Gateway credentials to fx instead of hanging on an invisible Keychain prompt.
- The access-mode control is hidden for fx because fx always runs in its automatic mode.

## [0.1.3] - 2026-08-23

### Added

- fx as a harness: if `fx` is installed and logged in, it shows up next to Claude Code, Codex, Cursor, OpenCode, and Pi. Live sessions spawn `fx acp` and talk Agent Client Protocol. fx does not accept image or audio attachments, so the attach button is disabled with a tooltip. Follow-up messages while a turn is running are not steered - wait for the turn to finish.
- Model picker shortcuts: `⌘.` (`Ctrl+.`) opens or closes it, and left/right arrows move between provider tabs.

### Fixed

- Closing a title-bar tab no longer flashes the sidebar session list. The cards stay on screen while history refreshes instead of disappearing and popping back.
- Git diff gutter and the Changes sidebar update live when files are modified externally, including after discarding a change, without closing and reopening the tab.
- The title-bar `+n -n` badge clears when the Changes sidebar shows no uncommitted files, instead of keeping stale addition/deletion counts.
- Launch no longer flashes a fully clear window: the boot splash uses the same `background-base` / glass tint as the loaded app.

## [0.1.2] - 2026-08-22

### Added

- Editor diff hunks show a centered gutter pill with revert and stage. Plus stages that hunk (or the selected lines) so you can commit some changes and leave the rest unstaged.
- Pi Coding Agent as a harness: if `pi` is installed, it shows up next to Claude Code, Codex, Cursor, and OpenCode. Live sessions spawn `pi --mode rpc` with the user's existing config and extensions loaded, so globally installed Pi packages (todos, subagents, custom tools) still run. Project-local `.pi` resources follow Pi's saved trust file. TUI-only widgets do not appear in MonoCode; extension confirm/select dialogs use the existing approval UI. MonoCode's runtime-mode control does not gate Pi tools - Pi has no native permission prompts.
- Closing the window no longer kills a running chat: MonoCode hides instead, and reopening the app brings the same window back mid-turn.
- Quit (⌘Q) asks first if chats are still running, then restores those sessions the next time you open the app and continues the turn.
- Reopening the app restores the last window: tabs, splits, and open file or terminal panes, instead of always starting on a blank homepage.

### Fixed

- Quitting during a later turn still resumes: a previous interrupt note no longer blocks Continue on the next quit.
- Opening a file scrolls its tab into view when the pane's tab strip overflows.
- Editor syntax lint no longer underlines valid TypeScript (arrow type predicates, typed `catch`, JSX comments, `typeof import()`) or Tailwind `@source` rules. Rust files are still highlighted but are not linted - the highlighter grammar was marking real code as errors.

## [0.1.1] - 2026-08-21

### Added

- Light mode: toggle Dark/Light in the appearance panel. Terminal, editor, markdown (including Mermaid), and sidebar all follow the scheme; preference persists across restarts.
- Editor syntax linting for supported source files (JavaScript, TypeScript, JSON, CSS, HTML, Rust, and Python): lightweight diagnostics straight from the Lezer parse tree, with wavy red underlines and hover tooltips. Catches unclosed brackets, stray quotes, and other typo-class mistakes - not a type checker or language server.
- File tabs show syntax problems: the label turns red and the tooltip appends a problem count.
- Context meter in the composer: a ring showing how much of the model context window the session is using, with exact token counts on hover. It turns amber at 75% and red at 90%.
- Context usage is read from each CLI rather than estimated, so the window matches whatever model the session actually runs. Claude Code, Codex, and OpenCode report it; Cursor does not expose token usage over ACP, so no meter is shown for Cursor sessions.
- The last context reading is stored with the session, so reopening a closed session shows its meter right away instead of waiting for the next turn.
- Tab back/forward, like a browser: ⌘[ and ⌘] walk the tabs you actually visited, not the order they sit in the strip. Buttons live in the sidebar header, or in the title bar when the sidebar is closed. View menu: Go Back / Go Forward. Closed tabs drop out of the stack; visiting a different tab after going back clears forward.
- Empty terminal panes grow a tiny snake on the grid. It hunts provider logos and pops a pixel speech bubble when it catches one.

### Fixed

- A tab is removed from its group when its session's project no longer matches the other tabs in that group.

## [0.1.0] - 2026-08-20

First public release. macOS (Apple Silicon) only.

### Added

- Desktop UI for the coding agent CLIs already installed on your machine: Claude Code, Codex, Cursor, and OpenCode. Tabs are sessions, the composer is the input.
- Project file tree, editor with diff view, and full-text search.
- Git surface: staged and unstaged diffs, commit, push, pull, branch switching, and pull request creation.
- Session checkpoints with undo.
- Embedded terminal panes.
- In-app updater.

### Security

- `harness_exec` only runs resolver-produced harness CLIs with a fixed argument allowlist.
- Content Security Policy enabled on the webview. Production CSP excludes the Vite dev server; `devCsp` covers `tauri dev`.
- Agent markdown does not load remote images (`data:` images still work).
- Updater endpoint and minisign public key are injected at release time rather than committed, so forks do not inherit the maintainer's update channel.
- macOS release builds sign with `APPLE_SIGNING_IDENTITY` via a config overlay; the committed default remains ad-hoc `-` for community builds.

[Unreleased]: https://github.com/hardbeat920/monocode/compare/v0.1.31...HEAD
[0.1.31]: https://github.com/hardbeat920/monocode/compare/v0.1.30...v0.1.31
[0.1.30]: https://github.com/hardbeat920/monocode/compare/v0.1.29...v0.1.30
[0.1.29]: https://github.com/hardbeat920/monocode/compare/v0.1.28...v0.1.29
[0.1.28]: https://github.com/hardbeat920/monocode/compare/v0.1.27...v0.1.28
[0.1.27]: https://github.com/hardbeat920/monocode/compare/v0.1.26...v0.1.27
[0.1.26]: https://github.com/hardbeat920/monocode/compare/v0.1.25...v0.1.26
[0.1.25]: https://github.com/hardbeat920/monocode/compare/v0.1.24...v0.1.25
[0.1.24]: https://github.com/hardbeat920/monocode/compare/v0.1.23...v0.1.24
[0.1.23]: https://github.com/hardbeat920/monocode/compare/v0.1.22...v0.1.23
[0.1.22]: https://github.com/hardbeat920/monocode/compare/v0.1.21...v0.1.22
[0.1.21]: https://github.com/hardbeat920/monocode/compare/v0.1.20...v0.1.21
[0.1.20]: https://github.com/hardbeat920/monocode/compare/v0.1.19...v0.1.20
[0.1.19]: https://github.com/hardbeat920/monocode/compare/v0.1.18...v0.1.19
[0.1.18]: https://github.com/hardbeat920/monocode/compare/v0.1.17...v0.1.18
[0.1.17]: https://github.com/hardbeat920/monocode/compare/v0.1.16...v0.1.17
[0.1.16]: https://github.com/hardbeat920/monocode/compare/v0.1.15...v0.1.16
[0.1.15]: https://github.com/hardbeat920/monocode/compare/v0.1.14...v0.1.15
[0.1.14]: https://github.com/hardbeat920/monocode/compare/v0.1.13...v0.1.14
[0.1.13]: https://github.com/hardbeat920/monocode/compare/v0.1.12...v0.1.13
[0.1.12]: https://github.com/hardbeat920/monocode/compare/v0.1.11...v0.1.12
[0.1.11]: https://github.com/hardbeat920/monocode/compare/v0.1.10...v0.1.11
[0.1.10]: https://github.com/hardbeat920/monocode/compare/v0.1.9...v0.1.10
[0.1.9]: https://github.com/hardbeat920/monocode/compare/v0.1.8...v0.1.9
[0.1.8]: https://github.com/hardbeat920/monocode/compare/v0.1.7...v0.1.8
[0.1.7]: https://github.com/hardbeat920/monocode/compare/v0.1.6...v0.1.7
[0.1.6]: https://github.com/hardbeat920/monocode/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/hardbeat920/monocode/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/hardbeat920/monocode/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/hardbeat920/monocode/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/hardbeat920/monocode/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/hardbeat920/monocode/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/hardbeat920/monocode/releases/tag/v0.1.0
