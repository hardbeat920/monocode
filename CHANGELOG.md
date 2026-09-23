# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Fedora and Enterprise Linux 10 workstations (registered RHEL, Rocky, Alma, CentOS Stream, Oracle) can install native Tauri prerequisites with `npm run setup:linux:fedora`, which enables EPEL 10 and CRB automatically where needed, and build a distributable `.rpm` with `npm run build:fedora`. Each GitHub release ships that `.rpm` alongside the `.deb` and AppImage. In #449.

## [0.2.0] - 2026-09-25

### Added

- **BTW** opens a read-only side conversation on a completed agent response without changing the main thread. Use the response's BTW control or `/btw` in the composer. Side conversations support Claude, Codex, Cursor, Grok, OpenCode, Pi, and omp, retain their threads and model settings, and use the provider that produced the original turn even after a handoff. In #353.
- `/operator` gives an agent opt-in access to MonoCode in that thread through a local `app` CLI. It can inspect models, start or draft sessions, read and message project sessions, organize folders, and read notes. App access lasts for that thread; `/mono` and `/monocode` remain supported aliases. See [Agent access to MonoCode](README.md#agent-access-to-monocode). In #423.
- Claude and Codex account controls show the cached account's plan, email, and organization in Settings, the account picker, and the usage popover. Identity refreshes after reconnecting. In #372.
- A usage-limit notice shows the provider's reset time and countdown, pauses queued messages, and offers manual resume or automatic resume after the limit resets.
- The macOS Quick composer global shortcut can be changed in Settings → Keybindings. The default remains Command+Shift+Space.

### Changed

- The selected Workspace sidebar tab is remembered separately for each project, including when a project is renamed.
- BTW controls and turn metrics sit with the transcript's response metadata; their hover and focus styles and the BTW popover spacing have been refined.
- Source-control diff utilities now cover reusing unchanged items and pruning stale entries, with tests for both behaviors.
- Composer controls fit better in narrow layouts. In #414 by @sambhavthakkar.

### Fixed

- Resuming an interrupted Codex session preserves its saved model and settings while the model catalog loads, instead of temporarily selecting another provider's model. In #422.
- Closing the last window quits the app on Linux, as it already does on Windows. In #419 by @sambhavthakkar.
- Escape handling waits for later keydown listeners, so controls can prevent the window-level Escape action when they handle the key themselves.

## [0.1.56] - 2026-09-24

### Added

- On macOS, Quick composer opens a floating prompt over any app with Command+Shift+Space. Choose a project, provider, model, permissions, and working copy; attach files or capture a screenshot; then press Return to start a session in the background or Command+Return to open it. Enable it in Settings → General. In #398.
- GitHub pull requests in the Inbox show check results, expandable GitHub Actions jobs and steps, and failure details. Failed checks can be sent individually or together to an agent for repair, with progress and the linked conversation tracked in the pull request. In #364.
- Single-clicking a file, diff, or search result opens a reusable preview tab. Double-clicking its tab or source item, or editing the file, makes it permanent. Preview state survives workspace restoration. In #385.
- Settings → Providers can set default providers, models, and picker visibility globally or for a selected project. Project defaults apply when opening or moving a blank session into that project. In #395.
- Settings → Editor has a **Format on save** toggle for Prettier-supported files; it is enabled by default. Turn it off to save the text as typed, including quote style. In #396.
- Background effects include **Haze**, available globally and per project with a live preview. In #390.
- When the project rail is compact, its sidebar opens temporarily as a drawer and closes on Escape, an outside click, or session selection. Project menus are available from the project picker even while the rail is hidden, including by right-click or keyboard. In #389.

### Changed

- The collapsed project rail defaults to icon mode for new settings; the hidden rail remains available.
- Compact-rail live-agent cards have more bottom spacing, and empty sidebar action groups no longer take up space.

### Fixed

- Non-plan Full Access Codex turns now accept supported MCP elicitation confirmations without an additional approval prompt.
- Claude tool rows reconcile complete streamed input, keep consecutive assistant messages separate, and show background tasks while Claude yields and later resumes.
- Codex streamed assistant and reasoning text is deduplicated per item instead of repeating completed content.
- Pi and omp ignore late tool-progress updates after a tool finishes, so completed cards do not return to a running state. In #391.
- The terminal dock keeps its last chosen side across projects, restarts, and reloads. In #400.
- Clicking a file in the activity log opens the path shown in its label; mismatched preview paths no longer show an unrelated diff, and home-relative paths resolve correctly. In #330.

## [0.1.55] - 2026-09-23

### Added

- Jira Cloud joins the Inbox with site, email, and API-token connection settings; issue browsing; descriptions and comment threads; comment posting; and shared project filters. **Ask** and **Start work** include the ticket's description and Jira identifier, and Start work lets you choose a local project. This integration uses API tokens without scopes; scoped tokens and Jira Data Center are not supported. See [Jira setup](https://github.com/hardbeat920/monocode/blob/v0.1.55/docs/jira.md).
- Jira issues support background activity notifications, project-level mute controls scoped to each Jira site, and **Issue appeared** automation triggers that run in the automation's selected workspace. Project lists follow pagination, connection failures remain isolated from other Inbox providers, and disconnecting clears saved credentials and cached Jira content.
- Conversations have in-transcript Find with match highlighting, previous/next navigation, and Command/Ctrl+F, F3, and Command/Ctrl+G shortcuts. Global conversation-search results now jump to the matching transcript block.
- Command/Ctrl+Up and Command/Ctrl+Down switch to the previous or next session inside the focused tab. Sessions already visible elsewhere swap panes instead of mounting twice.
- The session sidebar can be shown or hidden independently through the title bar, View menu, or Command/Ctrl+Shift+B, with the choice remembered across launches.
- Background artwork supports Dither, ASCII, Halftone, and Scanlines effects, processed in a worker and cached by image revision and theme. Effects can be selected globally or overridden per project with a live preview. Initial background-effect support in #347 by @404khai.
- Claude Opus 5.5 sessions have a dedicated animated welcome scene that adapts to the available space around the Composer, alongside the existing Astra welcome screen.

### Changed

- Chat is the default transcript layout; saved choices of chat or full-width layout remain unchanged.
- Sent prompts rise into the transcript, and the Composer moves into its dock on the first message without changing width. Streaming replies reveal words progressively, while tool steps use paced, masked entrance animations to smooth bursts of updates. Motion respects reduced-motion preferences.
- Switching sessions preserves recently rendered transcripts and scroll positions, prefetches neighboring sessions, and avoids redundant transcript saves. Deferred cleanup, memoized sidebar cards, and staged initial transcript rendering reduce work during navigation and improve first paint.
- Sidebar diff statistics use tighter spacing around thousands separators.
- The project background dialog stays within the viewport with scrollable content and updated effect controls.

### Fixed

- Explorer sorts numbered files and folders naturally, placing names such as `chapter-2` before `chapter-10` while keeping folders first. In #356 by @404khai.
- Opening a file in its default application validates the path and reports launch failures. File and tab context-menu actions show actionable errors instead of silently failing.

## [0.1.54] - 2026-09-22

### Added

- The latest user message can be recalled, edited, rewound, and resent across Codex, OpenCode, Pi, and omp sessions, with attachments, drafts, and provider state kept consistent through failures. In #261 by @shxntanu.
- Sessions can be linked to GitHub issues or pull requests from the sidebar. Linked identities persist, and event automations restore their work-item links while refreshing session titles.
- Markdown files have a shared document preview with collapsible frontmatter plus in-preview Find, match navigation, case, whole-word, and regular-expression filters, highlighting, and keyboard shortcuts.
- Explorer can optionally show Git-excluded files, resolved through repository ignore rules. In #217 by @kartava.
- File-editor syntax highlighting now covers C, C++, C#, Java, PHP, SQL, XML, SVG, YAML, Go, Dart, Swift, Kotlin, Ruby, shell, TOML, Scala, Lua, R, Perl, PowerShell, Objective-C, Protocol Buffers, and Dockerfiles. In #348 by @404khai.
- The Changes tab can pull the tracked current branch, with shared Git-operation progress, success or error feedback, and automatic refresh of changed files and open editors. In #352 by @jonathanlamela.
- Claude Code catalogs include Claude Opus 5.5 with extended thinking, fast mode, and 1M-context support for Claude Code 2.1.280 and newer. In #355 by @kartava.

### Changed

- Diff addition and deletion counts use thousands separators throughout the project rail, sidebar, editor, file tree, review, and unified diff views.

### Fixed

- Hermes turns stay busy while detached subagents are running and resume automatically with their completed transcripts instead of requiring a manual Continue. Hermes ACP usage updates also populate the context meter. In #335.
- Pressing Enter in the branch picker selects the highlighted branch while preserving the create-branch action for unmatched searches.
- Composer drafts survive closing and reopening a session pane. In #336 by @bluzername.
- Edited-turn resend recovery restores the prior transcript and draft cleanly when a provider rejects or fails the rewind, and accepted rewinds retain the right conversation state.
- The native macOS Window menu again includes the standard tiling actions such as Fill, Center, and Move & Resize. In #344 by @sensitiky.
- Generic Claude live-catalog aliases now include the concrete version reported by Claude Code, such as `Opus 5.5 (1M context)`, while retaining the stable alias used to launch sessions. The resolver handles future Claude family and version identifiers without requiring another display-label update.

## [0.1.53] - 2026-09-21

### Added

- The workspace picker can attach a session to an existing worktree from a submenu, instead of only creating a new worktree or staying on the current checkout.

### Fixed

- Add-to-chat from a file-only workspace still opens a split session pane when no session tab is already open, seeding a replacement from the first known session instead of dropping the request. In #325.
- Renaming a project folder on disk keeps that project's sessions, settings, recents, and terminals attached instead of treating the new path as a different project.
- Agent markdown, code blocks, and diagrams pick up their intended styles after the frontend source-tree move.

## [0.1.52] - 2026-09-20

### Added

- Automations can run agents on hourly, daily, weekday, or weekly schedules; launch immediately with **Run now**; or react to GitHub, GitLab, Linear, and Azure DevOps Inbox events without keeping the window in the foreground. The new Automations surface includes starter templates, reusable or isolated workspaces, run history, and slash-triggered skill selection in prompts. In #327.
- Antigravity is available on macOS and Linux as a live ACP provider with its own vector mark, model discovery, file and image attachments, permission requests, access modes, persisted session resume, cancellation and process recovery, and discovery of skills from `~/.gemini/antigravity/skills`. In #314 by @elijah7x.
- Azure DevOps joins the Inbox with PAT authentication for Azure Boards work items and Azure Repos pull requests, including attention filters, details, threads, comments, reviewers, and textual diffs for cloud and HTTPS on-premises organizations. Azure DevOps work-item and pull-request activity can also trigger automations. In #317 by @jonathanlamela.
- Composer messages can be saved as persistent session drafts instead of being sent immediately. Drafts survive restarts, appear in session status, can be sent later, and can be removed without changing conversation history.
- New sessions can choose the current checkout, an existing worktree, or a draft workspace that creates its worktree and a descriptive branch on the first turn. Draft workspaces support branch renaming and expose their checkout identity in the Explorer.
- The Changes panel can amend the latest local commit when `HEAD` has not been pushed, and resets amend mode when the branch or commit changes. In #324 by @kartava.
- GitHub issue and pull-request URLs render as interactive work-item chips with hover and keyboard-focus previews for metadata, state, labels, and assignees. An optional authenticated prompt can also star MonoCode through the GitHub CLI.
- The file editor has a draggable CodeMirror scrollbar with change and diagnostic markers, plus a full-width sticky search toolbar.
- Files can open as standalone top-bar workspace tabs through a persistent setting, and tab opening and closing can use reduced-motion-aware animations through a separate opt-in setting.
- The project rail has a persistent compact mode that gives workspace content more room while retaining project navigation and status.

### Changed

- Model controls use consistent reasoning-option behavior across providers, can show model settings as beside-picker pills, open the relevant model list directly, and identify the provider behind every favorite. Effort, service-tier, and fast-mode settings are grouped consistently, with effort icons for Pi and omp thinking levels and a speed icon for service tier. In #323 by @D3nnis72.
- Orchestrated workers use recoverable worktrees seeded from the lead checkout, apply checkpoints with conflict and symlink safeguards, support retrying stopped workers, and clean up their temporary branches.
- Automation creation opens directly into the template picker, keeps its filters and templates in one scrolling region, and lets prompt fields grow beyond the default Composer height.
- Worktree deletion no longer requires typing the worktree name, while commit actions, modal titles and borders, and provider-setting controls use clearer states and lighter styling.
- Banked Codex reset details appear only when resets are actually available, and project mascots no longer use a separate unavailable-reset state.
- The frontend source tree is organized by application composition, product feature, provider integration, platform adapter, and shared code instead of the former `chrome`, `surfaces`, and catch-all `lib` directories. In #331.
- Session removal, resilient boolean preference storage, and repository-backed GitLab and Azure DevOps Inbox fetching now use shared lifecycle and data-access helpers.

### Fixed

- Removing the final session associated with a worktree asks about deletion only when an unused worktree actually exists.
- Removing a saved draft is serialized with session persistence so reusing a session ID cannot restore the deleted draft.
- Antigravity ignores malformed configuration updates without losing valid options, fails closed when access-mode changes are rejected, and retires stale or blocked transports so cancelled, forgotten, timed-out, or replaced sessions cannot leak output into a later turn.

## [0.1.51] - 2026-09-18

### Added

- Git worktrees provide independent working copies for parallel sessions. The working-copy picker can create a worktree from a new or existing branch, open another working copy in a new session, and recover sessions whose worktree was removed. Settings → Worktrees lists branch, status, unpublished commits, and associated sessions, with guarded deletion that preserves branches and sessions by default. In #319.
- Settings → Providers can rename and remove named Claude Code and Codex accounts. Removing an account deletes its stored credentials, stops its running turns, and retains existing conversations with a clear prompt to switch accounts before continuing.
- The Go to File dialog now doubles as a fuzzy command palette when opened with Command/Ctrl+Shift+P or a leading `>`. Its first action reloads MonoCode, also available with Command/Ctrl+Shift+R, with confirmation before discarding unsaved files. In #296 by @MichaelOgunjimi.
- User messages can be copied with their attachments or saved directly to Notes, and selected transcript text offers the same Notes action with success and error feedback. Message timestamps remain visible even for prompts that contain only a note, handoff, or second-opinion card. In #291 and #320 by @ognjeeen.
- Agent question options support full keyboard navigation with Arrow keys, Home, End, number shortcuts, Enter, and Space for both single- and multi-select prompts.

### Changed

- Add-to-chat actions from file-only workspaces open a new session pane and seed its focused Composer with the quoted or plain text, leaving the caret ready at the end.

### Fixed

- Open editors reliably reload after agent edits and external file changes, including updates that race the initial file watch or preserve the previous modification time.
- Stopping a turn or beginning the next one cancels unresolved approval requests, removes stale approval controls, and marks their unfinished tools as cancelled.
- Conversations created before named Claude Code and Codex accounts were introduced continue under the default account instead of losing their provider session association.
- Composer focus follows the visible active session in split layouts instead of being captured by a hidden session.

## [0.1.50] - 2026-09-17

### Added

- Hermes Agent is available as an ACP harness with live model discovery, image and file attachments, permission prompts, in-flight redirects, and persisted session resume. Install Hermes, configure a provider with `hermes model`, and MonoCode will add it to the model picker. In #282.
- Projects can be organized into persistent, collapsible groups in the project rail, with custom names, colors, and mascots. Projects can be assigned or returned to the ungrouped section from their context menu.
- On Windows, closing a window can hide it to the system tray so running agents continue; the behavior is enabled by default, configurable in Settings, and paired with tray actions to reopen or fully quit MonoCode. Full quits coordinate every window, count all running turns, ask once, wait for workspace saves, recover from stale confirmations, and abort safely if required persistence fails. In #224 by @goujandev.
- Claude Code and Codex support multiple named accounts. Add or switch accounts from the usage footer; each project remembers its selection, and existing conversations remain pinned to the account that started them. In #280.
- OpenCode Go usage appears in the status-bar footer with five-hour, weekly, and monthly limits and reset countdowns. Credential discovery supports environment overrides, JSON and JSONC configuration, and XDG data directories. In #263 by @D3nnis72.
- The branch picker can create a branch through a dedicated name dialog, with the new branch immediately available for selection.
- Files can be dragged from the Explorer into the Composer, with a drag preview and the same attachment handling as files added through the picker.
- Project menus can open a project in a detected external editor on macOS, Windows, and Linux.
- Binary and image viewers can copy the original file to the macOS clipboard from their toolbar or context menu, with temporary success feedback.
- GitHub pull-request headers include an action to copy the head branch name. Closes #248 in #273 by @bluzername.
- Contributors can set `MONOCODE_DEV_APP_NAME` to run a separately named macOS development app without changing the default bundle identity. Invalid names and path traversal are rejected. In #284 by @MichaelOgunjimi.

### Changed

- Workspace and file tabs have more consistent alignment, spacing, active-state highlighting, rounded corners, and cursor behavior, while the terminal dock uses a simpler trailing layout.
- Changing an OpenCode session's access mode updates its live permissions immediately, and switching to Full Access automatically resolves residual approval prompts.
- Orchestrated workers use private scratch directories and canonical write-path checks. Paused runs remain inspectable, interrupted work can be retried after resuming, and invalid orchestration proposals are repaired before results are published.

### Fixed

- Pasting or dropping files from Finder into the file tree works reliably on macOS, including safe handling of symlink aliases; drop targeting also accounts for Windows display scaling. In #264 by @kartava.
- The Composer regains focus after answering a question, finishing an agent turn, or returning to the MonoCode window without stealing focus from another Composer or an open picker. In #292 by @MichaelOgunjimi.
- Context menus can use their intrinsic height, tab-group menus open on the correct side, light-theme popovers remain opaque, and reorderable tabs use the default cursor.
- Unix orchestration scratch directories retain restrictive `0700` permissions when their ownership is transferred to the worker.

## [0.1.49] - 2026-09-16

### Added

- Usage controls open detailed Claude and Codex limit views with per-window progress, reset times, and refreshed status. Codex accounts can inspect and redeem banked rate-limit resets with confirmation, while project mascots reflect whether resets are available.
- The Inbox menu and Inbox view can mark all visible activity as read with persistent state. In #253 by @ognjeeen.
- GitHub Inbox aggregation discovers work items from both fork and parent repositories and keeps operations and caches scoped to the correct repository.
- Shift-click selects a range of sidebar sessions, while Command/Ctrl-click adds or removes individual sessions from the selection. In #259 by @ognjeeen.
- Provider footer controls can launch the official browser sign-in flow for Claude Code, Codex, Cursor, Grok Build, and fx when authentication is required. Submitting while signed out opens the same focused sign-in experience instead of printing the CLI error into the transcript.

### Changed

- Settled turns fold advisor interjections, status rows, and delegated runs into the work trail while keeping errors, interruptions, and failed runs visible. Folded prose is visually quieter and status-only groups use a clearer label. In #237 by @elijah7x.
- Linked work-item panels remain mounted across workspace-tab switches for instant restoration, and their external-link action now sits in the responsive panel header.
- The first editor opened beside a conversation is placed to its left, preserving the session on the right.
- Transparent macOS windows use native visual-effect backing, and stationary glass layers are isolated from modal, popover, and notice animations for more stable compositing.
- Markdown mode tabs, skill selection, and Orchestrator controls have simpler styling and improved contrast across light and dark themes.

### Fixed

- Orchestration waits wake as soon as a worker requests approval or other input, including when input was already pending, and unavailable approval controls stay hidden.
- GitHub pull-request actions accept successful commands that produce no stdout and then refresh the pull request state.
- Inbox alerts are suppressed for activity created by the current user across GitHub, GitLab, and Linear.
- PowerShell and Windows shell commands preserve quoted and partially quoted arguments, recognize option aliases, and stop wrapper-flag parsing at file scripts. In #233 by @notsapinho.
- The branch picker focuses its search field after the popover becomes visible. In #246 by @actuallyakshat.
- The Claude usage footer no longer refreshes or writes credentials owned by Claude Code, avoiding refresh-token rotation races that could force frequent reauthentication.

## [0.1.48] - 2026-09-16

### Added

- Drag a split pane into the title-bar tab strip to detach it as a separate workspace tab. Chat, editor, and terminal panes keep their contents and focus.
- Configure sounds, desktop banners, and sidebar indicators by category for each project from Settings or the project and Inbox menus. Projects can be muted for one, four, or eight hours, until a custom time, or until manually resumed; bulk controls and persistent mute indicators are included, and muting hides Inbox badges without clearing unread activity. In #242 by @ognjeeen.
- GitHub pull-request details include confirmed actions to merge, convert between draft and ready, close, and reopen the pull request, then refresh the Inbox with its new state.
- Title-bar tabs show a teal completion check for an unseen agent response until the tab is viewed, while active runs keep their busy indicator.

### Changed

- Linked GitHub issues and pull requests open in a resizable panel beside their session instead of replacing it with the Inbox. Item identity stays pinned while the details scroll as one view, related threads are omitted from the side-panel layout, and review actions remain visually distinct.
- The usage footer names the active terminal process in place of the generic terminal label and uses tighter control and icon spacing.
- Moving a note to another project now uses the same searchable, keyboard-accessible project picker as the sidebar.
- Selection highlights, structural separators, and primary actions use shared theme-aware styling across the interface, and the application icons have been refreshed.
- Terminals answer OSC color queries with resolved colors from the active theme, and the project terminal dock no longer applies its own background tint.

### Fixed

- Paragraphs and lists in agent replies have clear, consistent spacing without adding trailing whitespace to the message. Fixes #218 in #239 by @bluzername.

## [0.1.47] - 2026-09-15

### Added

- Settings can now be searched by name or keyword across every page. Results jump directly to the matching row, the navigation is organized into **App**, **Agents**, and **Workspace** groups, and transcript and composer preferences have their own **Chat** page.
- Drag an inactive workspace tab onto any edge of an open pane to merge its complete layout into that workspace. Files, terminals, split arrangements, and focus move together, while dropping onto a blank session replaces it.
- Settings → Appearance includes a persistent **Accent color** control with named presets and a custom picker. The chosen color is applied to the composer send button and user-message bubbles with an automatically legible foreground.
- Model flyouts for handoffs, second opinions, alternate Plan builds, and orchestration assignments open an adjacent effort picker when the hovered model supports reasoning levels, and apply the chosen model and effort together.
- Windows now delivers native toast notifications for completed turns, input requests, and reminders. Notification clicks restore a minimized window and open the related session, while blocked notifications are reported with a link to Windows Settings. In #220 by @ardevdevts.
- Inbox issue details show when the issue was created. In #231 by @ognjeeen.
- Orchestration lead cards show a compact subagent summary with per-task status in a hover or keyboard-focus tooltip, and expand their full controls while active, selected, or busy.
- The **Working agents** preview remains available in the sidebar when the project rail is collapsed, including project identity, elapsed time, current activity, status, selection, and expandable overflow.
- The GitHub issue chooser includes a structured feature-request template and again permits blank issues.

### Changed

- OpenCode models are grouped under readable provider names in the model picker, and provider names are included in search.
- Activating a session now loads that harness's live model catalog immediately instead of waiting for the model picker to open.
- Codex shell activity unwraps launcher commands and presents file inspection as readable **Read**, **Find**, and **List** steps while preserving the underlying command preview, including in saved transcripts.
- Project terminal creation and activation controls have moved from the title bar to the usage footer, leaving more room for workspace tabs.

### Fixed

- Paused orchestration runs explain why resumption is unavailable, link to another running session that is blocking the checkout, wait for interrupted work to stop, and keep composer text, attachments, and the selected mode when a submission is rejected.
- Orchestration scopes accept valid absolute paths by rebasing them to the project root, reject paths outside the project with a clear error, and compare Windows drive, UNC, separator, case, and extended-length path forms consistently.
- Codex installations managed by Bun are detected when locating the CLI. In #82 by @jagadhis.
- Expanding an orchestration card no longer shifts its header downward.

## [0.1.46] - 2026-09-14

### Added

- **Agent orchestration v1** lets a lead agent coordinate up to four workers in the same checkout. Choose worker providers and models, review and edit the proposed assignments before starting, and let the lead manage dependencies, review results, request corrections, and redirect supported workers mid-turn. Overlapping write scopes are queued, and worker approvals and questions are routed through the lead. In #228.
- Orchestration workers are grouped under their lead in the sidebar, with status, model, and expandable details. **View agents** opens their transcripts beside the lead. Run history survives restarts, interrupted work pauses for review, and stopping the lead stops its workers too.
- Completed agent turns show provider-reported token usage, cache metrics, and output rate in a hover or keyboard-focus preview, with metrics retained in saved conversations.
- Web links in user messages show compact page-title and favicon previews while preserving the surrounding text. Preview requests validate public destinations and keep fetching outside the webview.
- Settings → Appearance includes a **Dark-mode lightness** slider and remembers its value independently of light mode.
- Chat backgrounds have separate opacity controls for empty and active sessions, including project-specific overrides. In #191 by @shxntanu.
- GitHub pull-request reviews in the Inbox can switch between changed hunks and full-file context. In #189 by @UtkarshRahim.
- **Close All Tabs** is available from the menu and Command/Ctrl+Shift+W. It closes the active tab's editor files first, then closes the workspace tabs on a subsequent invocation, retaining a blank session and confirming unsaved files and running terminals. In #215 by @kartava.
- Notes can be moved between projects without changing their order or losing edits during navigation. In #187 by @ognjeeen.

### Changed

- Closed conversations are prefetched on hover or press and retained in a bounded cache for faster reopening, with safeguards against stale loads and duplicate tabs.
- Inbox cards mount progressively to keep large lists responsive. In #209 by @notsapinho.
- Reordering tabs and projects uses shared motion settings, with smoother scrolling and consecutive drag gestures. In #206 by @ognjeeen.
- Sidebar session cards place linked work-item and archive controls together in the footer, show a single provider icon, and retain the default cursor on reorderable items.
- Linked work-item update notices now sit inside their session pane, make the agent action more prominent, and offer clearer open and dismiss controls.

### Fixed

- OpenCode falls back to readable local paths for unsupported attachment formats instead of sending provider-rejected file parts, and repairs sessions already stuck on an unsupported file turn. Fixes #211.
- **Supervised** access explicitly sets Claude Code's permission mode, preventing local default settings from silently switching the session to automatic approvals or bypassed permissions. In #203 by @prkl78.
- OMP advisor interjections no longer fold away complete answers or break assistant streams. Saved conversations recover interjection boundaries and status-split continuations, while long interjections collapse by default. In #156 by @elijah7x.
- Continuing a conversation dismisses its due reminder and linked-update notices while preserving future reminders. Linked-activity sounds no longer repeat when the same update remounts.
- Opening the sidebar project picker focuses its search field reliably. In #226 by @actuallyakshat.
- Background GitHub CLI and related helper commands no longer flash console windows on Windows. In #221 by @korefs.
- Nested transcript scrollers keep receiving wheel gestures when the outer transcript reaches an edge, including gestures over SVG icons and containers that scroll on only one axis.
- Orchestration cleanup releases checkout reservations when a window closes and removes stale run and worker ownership records when conversations are deleted. Worker tabs wait for their lead to open, sidebar controls remain independently keyboard accessible, and completed or stopped runs release the Undo lock.

## [0.1.45] - 2026-09-13

### Added

- Subagent rows show the child's model beside its step count when the provider reports it, and retain it in saved conversations.
- Subagent trails now include OpenCode child sessions and Pi/omp progress, including separate rows for parallel and chained tasks. Cursor recovers child messages and tools from its local stores, names foreground tasks correctly, and restores expandable rows in saved conversations. Cursor, Grok Build, and fx also route child updates carrying a parent tool ID into the same trails. OpenCode pairs concurrent runs by session ID, and repeated progress updates merge into their existing steps.
- Subagents now get a row each in the transcript, sitting under the agent's own work with an animated mascot, a shimmering name while the run is live, and a count of the steps it has taken. Clicking a row opens the subagent's trail inline, grouped into phases the same way the main transcript groups work, so a long run reads as what it said and the calls that followed rather than one flat dump. The rows keep their place while the work above them folds and re-folds. Claude and Codex both report their subagents' work; Codex spawns are named from their brief, stay running until the agent itself reports otherwise, and one spawned agent no longer shows up as several rows.
- GitLab's **Needs attention** Inbox view now uses pending GitLab To-Dos to include assignments, mentions, and review requests from every accessible repository. Remote-only items support details, discussions, comments, and merge-request diffs without requiring a local checkout.
- Sessions linked to a GitHub issue or pull request now surface unseen comments, reviews, commits, and state changes in the sidebar and in an activity notice. Open the new activity directly, add it to the composer for an agent to address, or archive or delete the session after its issue closes or pull request closes or merges.
- Settings → General can show an optional standalone effort picker beside the model picker for quicker changes, and remembers the preference. When the standalone control is hidden, the combined model picker shows the selected effort beside the model name.
- Click a skill in Settings → Skills to inspect its `SKILL.md` in a responsive inline panel, switch between rendered Markdown and source, and expand its metadata. The preview supports keyboard focus restoration and Escape-to-close. In #172 by @ognjeeen.

### Changed

- While an agent is busy, entering a follow-up replaces the **Stop** action with **Send** instead of showing both controls at once.

### Fixed

- Clicking or dragging a session card no longer selects its text.
- Computer Use access confirmations are accepted automatically when Codex is already running in Full Access mode; other MCP confirmations still require an explicit decision.
- GitLab Inbox repository detection no longer flashes console windows when it invokes Git on Windows. In #200.
- Claude Sonnet 5 appears only with Claude Code 2.1.197 or newer, preventing older CLI versions from receiving an unsupported model argument. In #199 by @nulljosh.
- Claude model choices resolve consistently between the CLI's short live aliases and MonoCode's full startup model IDs, so relaunching no longer switches a saved session to a different model family.

## [0.1.44] - 2026-09-12

### Added

- Set a reminder from a session's menu using a preset or custom date and time. MonoCode persists scheduled reminders, delivers a notification when one is due, keeps reminder notices available in the session, and shows the scheduled time when cancelling one.
- The model picker remembers the six most recently used models. Right-click the current model or press Command/Ctrl+Period to switch among them quickly.
- Notes support normalized tags that can be edited and searched, and selected text from an agent transcript can be saved directly as a note.
- File and terminal tab menus include **Close Others**, with confirmation before closing unsaved files or running terminals.

### Changed

- Model selection and model settings now share one searchable, keyboard-accessible picker with nested menus, provider tabs, favorites, and inline setting controls.
- Returning to a project restores its last active session, editor, or terminal instead of choosing a different pane. In #144 by @kinsomicrote.
- Failed tool activity stays concise by default and can be expanded directly from its transcript summary to inspect the error.
- Transcripts preserve the provider and model used for each turn, including across reloads, handoffs, second opinions, and Claude model switches.

### Fixed

- Markdown file links distinguish document headings from local paths, handle bare and percent-encoded filenames, reject encoded network paths, and navigate to the requested source location reliably after reload. In #146 by @yankawai.
- Codex transport retries and fallback diagnostics no longer appear as transcript events, while terminal errors and unrelated runtime warnings remain visible.
- Background tabs preserve the composer's measured height, so returning to a tab no longer collapses a multi-line draft to one row. In #182 by @goujandev.
- File attachments now reach Codex, Claude Code, Pi, and omp through a local-path fallback when they cannot be sent inline, including attachment-only messages and mid-turn follow-ups. ACP resource links and OpenCode file parts keep their native formats. Fixes #174.
- Approvals and questions from nested Claude, Codex, and OpenCode sessions route to the correct active parent session, queue safely when several arrive, and surface reply failures instead of leaving the turn stuck.
- Files selected from search, the file picker, or the filesystem open by their exact path instead of being redirected by fuzzy path matching.
- Web links in agent messages now open in the system browser instead of relying on unavailable in-webview navigation. Fixes #175.

## [0.1.43] - 2026-09-11

### Added

- Settings → Inbox now reports whether the GitHub CLI is installed and authenticated, alongside the existing GitLab and Linear connection controls. The Inbox shows only connected sources, falls back safely when one is disconnected, and offers an **Add connection** menu that opens the matching Settings card. In #166 by @goujandev.
- Use `/add-to-folder` in the composer to place the current session in an existing sidebar folder or create a new folder without interrupting the prompt.
- Ungrouped pinned sessions now appear in a dedicated **Pinned** sidebar section that can be collapsed independently for each project and expands automatically while searching.
- Click an image attachment in the composer to inspect it in a full-screen preview; close it with Escape, the close button, or the backdrop, and focus returns to the attachment.
- Right-click file links, inline file paths, and code-block paths in agent messages to open them in MonoCode or the default app, reveal them in the system file manager, or copy their absolute or project-relative path.
- macOS releases now include separate signed packages for Apple Silicon and Intel Macs.

### Changed

- The session change-review card now appears after the latest completed reply instead of above the composer, stays hidden while a turn is running, summarizes total additions and deletions, and shows up to three changed files before offering to expand the list.
- File mentions created from an editor selection use the concise `@file (line…)` form, and the line location is highlighted as part of the mention.
- Inbox item identity, metadata, related threads, actions, and pull-request tabs remain pinned while descriptions, comments, and diffs scroll beneath them.
- Navigation, menu, tab, and file labels use tighter, more consistent line heights.
- Windows no longer shows a redundant centered title between the tab strip and native window controls.

### Fixed

- Codex Full Access approvals no longer block a turn, native Codex questions and supported MCP confirmations appear in the shared input UI, optional questions show their timeout and remain open after interaction, and access-mode changes made during a turn are applied to the next turn. Pending-input notifications also track concurrent requests individually and clear correctly when Codex resolves or cancels them. In #139 by @gettyeuro.
- A failed provider connection is retired so the next prompt can reconnect cleanly. In-progress tools and approvals are settled as failed or cancelled, while failed subagents expand automatically and show the provider's error details instead of leaving a session looking stuck or successfully completed.
- Truncated labels preserve letter descenders across tabs, navigation, menus, file views, and search results. In #119 by @ognjeeen.
- The project logo picker opens in the selected project's directory instead of an unrelated location. In #163 by @ognjeeen.

## [0.1.42] - 2026-09-10

### Added

- GitLab joins the Inbox alongside GitHub and Linear. Connect GitLab.com or a self-managed instance from Settings, then browse and filter issues and merge requests, inspect details, comments, assignees, labels, and diffs, post comments, and start or discuss work without leaving MonoCode.
- Sessions started from a GitHub Inbox item, or whose first prompt references a GitHub issue or pull request, remember that work item. Session cards link back to it, Inbox rows show related thread counts, and issue or pull request details link to every matching current or archived thread.
- Settings → Skills lists file-based skills from MonoCode, the current project, personal folders, and supported harnesses. Filter or refresh the catalog, enable and disable individual skills, create a starter project or personal `SKILL.md`, and copy or reveal a skill's path. In #137 by @imnakul.
- Edit and Write activity in agent transcripts shows the tool's exact diff or written content in an accessible hover and keyboard-focus preview; click through to open the full file or diff.
- Right-click file tabs to open a file in its default app, reveal it in the system file manager, copy its name or absolute or project-relative path, or close the tab.
- Selecting code in the editor opens an **Add to chat** action that inserts the file and selected line range into the composer without copying the code itself.
- Press a mouse's middle button or scroll wheel on a workspace, file, or terminal tab to close it without selecting a background tab first. Existing unsaved-file and running-terminal confirmations still apply, and the sole blank workspace tab remains open. In #154 by @50BytesOfJohn.

### Changed

- The Settings model picker now uses MonoCode's theme-aware popover, with keyboard navigation, active-option announcements, and reliable focus restoration instead of the operating system's native select menu. In #149 by @ardevdevts.
- Skill names, sources, paths, and creation controls use the same sans-serif interface typography as the rest of Settings.

### Fixed

- Renaming a session remains editable when its agent is working, including when the turn starts after rename mode opens. In #143 by @yankawai.
- Disabling a project skill allows an enabled personal skill with the same name to take its place; disabling either path no longer hides the wrong skill.
- Empty sessions update their displayed project label immediately when the tab group's custom label is changed or cleared.

## [0.1.41] - 2026-09-09

### Added

- OMP models support fast mode, including live RPC and configuration updates with a clear fallback when a model does not support it.

### Changed

- The file explorer avoids unnecessary rerenders and preserves unchanged file-icon DOM for smoother updates.
- The Changes panel header consistently shows its label instead of replacing it with diff counts.

### Fixed

- The sidebar update control stays hidden when no update is available and prevents duplicate installs from concurrent clicks. In #132 by @fobsouza.

## [0.1.40] - 2026-09-08

### Added

- Working-tree reviews now separate staged changes against `HEAD` from unstaged changes against the index, including partially staged files.
- The Changes panel can organize files into a collapsible directory tree, remembers the selected list or tree view, and can open every change in one review.

### Fixed

- Code block syntax highlighting follows MonoCode's appearance preference instead of the system color scheme. In #117 by @kartava.
- Split conversation panes share one continuous chat background instead of repeating the image in every pane.
- Project search safely treats include filters beginning with `-` as path patterns instead of Git options. In #125 by @Karajelly.
- Release publishing validates and uploads the expected versioned artifacts for every supported platform.

## [0.1.39] - 2026-09-08

### Added

- Windows releases now check for, download, and install signed updates through the same in-app update flow as macOS.

### Changed

- The prompt outline remains visible on slightly narrower windows.
- Removed the scrolled transcript's top-edge fade and blur effect.

## [0.1.38] - 2026-09-08

### Added

- The sidebar project picker is now searchable and keyboard navigable, shows each project's parent path, and includes actions for opening a new project or starting a new tab.
- Right-click a title-bar tab to close that tab, the other tabs, or every tab to its left or right. Bulk closing still protects unsaved files and running terminals.
- Shift-click conversations in the sidebar to select several at once, then pin, unpin, archive, unarchive, move into or out of folders, or delete them together.
- Settings → Appearance → Chat background adds an on-device image behind empty sessions or every conversation, with adjustable visibility. Each project can override the global image from its project-rail menu.
- Long transcripts have a vertical prompt outline for jumping between turns. Hover or keyboard-focus a marker to preview its prompt and reply. In #90 by @kartava.
- Drag image files into a note to copy them into MonoCode's local note storage and insert them into the note at the cursor.
- Archive the focused conversation with Shift+Command/Ctrl+A. The shortcut stays out of editors, terminals, diffs, and open overlays. In #89 by @kualta.

### Changed

- Scrolled transcripts fade and blur smoothly beneath the title bar, and popover backdrops now use theme-aware tints.
- Light mode uses an opaque native window for legibility, preserves the dark-mode glass settings, and gives the composer theme-specific shadows and send-button states.

### Fixed

- Enabling Sounds now plays the switch cue immediately. In #111 by @kartava.
- Sidebar multi-selection clears reliably when its menu closes or the pointer moves outside the selected conversation cards.
- Chat background changes appear across open session panes immediately, and the empty-session arcade stays hidden when a background is visible.
- Composer keyboard handlers ignore active IME composition, preventing Enter, Escape, and picker actions from firing while composing text.

## [0.1.37] - 2026-09-07

### Fixed

- Improved transcript performance by rendering collapsed work only when opened, skipping hidden-tab layout work, and reusing line-height measurements across reflows.
- Pending approval controls remain visible when completed transcript work folds, and switching tabs preserves transcript state.

## [0.1.36] - 2026-09-07

### Added

- Filter Linear Inbox issues by team and project, including issues with no project. Team filters stay in sync with Settings. In #103.

### Fixed

- Improved performance in tool-heavy conversations by avoiding repeated rescans when grouping transcript activity.
- Projects with the same folder name keep independent names, colors, logos, and mascots. Existing appearance settings migrate to each project, and shared logo files remain available while another project uses them. In #104.
- Terminal focus stays in place after changing directories with `cd`. In #94.
- Wrapped inline code grows to fit its content, and list markers stay beside it. In #93.
- GitHub pull request lookups qualify the head branch with its repository owner.
- The Windows installer uses the MonoCode icon.

## [0.1.35] - 2026-09-06

### Added

- Inbox items have an **Ask** panel for discussing and analyzing GitHub and Linear issues and pull requests without leaving the Inbox. Discussions remain available while switching items, can be restarted, and stay out of project history, recovery, and notifications.
- Settings → Appearance → Interface scale zooms the full UI from 50–200% and persists the choice. Use Command/Ctrl with `+`, `-`, or `0`, the View menu, or the settings slider. In #86 by @xaccefy.
- Settings: Notifications, off by default. With it on, a system notification appears when a turn finishes or an agent waits on an approval or question in a session that is not on screen, whether MonoCode is in the background or another session is open; clicking it jumps to that session. Turning it on asks macOS for permission, and a blocked state links to System Settings. The Sounds setting decides whether the notification plays a sound, and the in-app cue is skipped when the banner fires. In #62 by @emircan-sahin.
- Windows is a supported desktop target. Terminals, agent CLIs, and the rest of the macOS/Linux feature set run there, the window uses Tauri Acrylic in place of macOS vibrancy, and releases include an x86_64 NSIS installer. In #46.

### Changed

- Completed agent work folds into a concise summary in the transcript, keeping the prompt and final answer prominent. Expand the summary to inspect the reasoning and tool activity behind it.
- Unified diff code, line numbers, and hunk headers are vertically centered within their rows. In #72 by @tcmarkfeld.

### Fixed

- Archiving or deleting an open conversation closes its related workspace panes, safely stops in-progress work, preserves the latest output when archiving, and cannot be undone by a queued background save. Unrelated tabs and files remain open. In #70.
- The sole blank workspace tab no longer shows a close control. Closing while an auxiliary pane is focused closes that pane without removing the blank tab.
- Composer highlights for commands and mentions stay aligned when editing moves the textarea's scroll position.

## [0.1.34] - 2026-09-05

### Added

- Navigate sessions with Shift+Command/Ctrl+Up or Down and projects with Shift+Command/Ctrl+Left or Right. The shortcuts follow the visible sidebar order and also work from an empty composer. In #47 by @MisterWanted.
- Session cards show an Archive or Unarchive action on hover and keyboard focus.
- OMP's native commands and custom workflows appear in the `/` picker, with descriptions and argument hints. Commands run through OMP with their arguments intact, and workflow dialogs support choosing options and entering text. MonoCode keeps `/plan` and `/compact`; use `/omp:plan` and `/omp:compact` for OMP's versions.
- The `@` file picker supports files and folders whose paths contain spaces and refreshes when the workspace changes, so newly created paths appear without restarting MonoCode. Unsafe control and bidirectional formatting characters are excluded from mention tokens. In #67 by @elanchezhiyanr.

### Fixed

- Opening a file from the explorer preserves the unfinished composer draft. In #76 by @kartava.
- Pi extension status and notification labels no longer expose raw ANSI styling codes; interactive option values remain unchanged.
- OMP commands that finish locally display their output and release the composer without waiting for an agent turn. Command inventory updates refresh the active session's picker, and ongoing OMP workflows no longer finish early on a nonterminal agent event. In #73.

## [0.1.33] - 2026-09-04

### Added

- Selecting Astra in the composer celebrates it with a pane-wide solar animation: champagne-gold meteors, star glints, a glowing sun, and orbiting rings. The effect replays on every selection, fades out automatically, and respects reduced-motion preferences.
- Diff reviews can be annotated line by line in both Unified and Editor views. Use the comment action on a changed line to write a note and add its file, line number, and code context to the active composer; collect multiple comments and send them to the agent in one prompt.
- Compact session context manually with `/compact` or the context meter on supported agent harnesses.

### Fixed

- Agent markdown supports mixed right-to-left and left-to-right text while keeping code and Mermaid blocks left-to-right.
- Popover glass backgrounds stay stable during opening and closing animations.

## [0.1.32] - 2026-09-04

### Added

- Plan mode is available from the composer’s + menu or with `/plan`. Supported agents produce a reviewable Plan card instead of starting implementation; open it to inspect or edit the full markdown, then approve the exact plan with Build.
- The Plan card and expanded markdown view have a split Build button. Use its model picker to implement the approved plan with another model or agent harness; cross-harness builds carry the session context through the existing handoff flow.
- Settings → General → Follow-up behavior can queue prompts sent during an active turn and dispatch them in order when the agent finishes. Queued prompts can be edited, removed, or sent immediately with Steer; interrupting a turn pauses the queue until you resume it. In #56 by @tcmarkfeld.
- Grok Build accepts image attachments in prompts.
- Live task lists from supported agent harnesses appear as a separate Tasks card with per-item status and a completion count, while provider-internal todo calls stay out of the activity feed. Partial task updates preserve the full checklist and its labels, and stopping a turn resets unfinished spinners. Task progress is saved in session history, searchable, and included in handoffs and second opinions.
- Session checkpoint Review opens a read-only unified diff of the exact before-and-after changes made by that session, with session-scoped file and line counts.

### Changed

- Diff reviews load files concurrently, prioritize the focused file, and render large changes progressively. Embedded pull request diffs use collapsible file cards, and large patches are no longer silently capped at 2,000 rendered lines.
- Sync Changes starts its pull and push without a separate push confirmation.
- The composer hides its internal scrollbar, and a disabled attachment button names the active harness that does not support attachments.
- Pull request CI cancels superseded runs while main-branch and other non-PR runs remain independent. In #57 by @tcmarkfeld.
- The README uses a higher-resolution application screenshot.

### Fixed

- Cursor background subagents stay visibly active until their result is delivered instead of making the session look stalled. In #61 by @D3nnis72.
- Session Undo preserves changes that existed before the agent turn and is disabled when another running session or a later edit makes restoration unsafe. Checkpoint operations are serialized so overlapping review, keep, and undo actions cannot race.
- Vertical wheel gestures over a horizontally scrollable unified diff code pane continue scrolling the surrounding review.
- Reordering the visible tabs for one project no longer moves hidden tabs belonging to other projects.
- Copying a code block no longer adds its final newline to the clipboard.
- ⌘W / Ctrl+W closes the active workspace tab or pane when the project terminal has focus instead of closing a terminal from the project-wide dock.
- The close button remains available on the last workspace tab.

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

[Unreleased]: https://github.com/hardbeat920/monocode/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/hardbeat920/monocode/compare/v0.1.56...v0.2.0
[0.1.56]: https://github.com/hardbeat920/monocode/compare/v0.1.55...v0.1.56
[0.1.55]: https://github.com/hardbeat920/monocode/compare/v0.1.54...v0.1.55
[0.1.54]: https://github.com/hardbeat920/monocode/compare/v0.1.53...v0.1.54
[0.1.53]: https://github.com/hardbeat920/monocode/compare/v0.1.52...v0.1.53
[0.1.52]: https://github.com/hardbeat920/monocode/compare/v0.1.51...v0.1.52
[0.1.51]: https://github.com/hardbeat920/monocode/compare/v0.1.50...v0.1.51
[0.1.50]: https://github.com/hardbeat920/monocode/compare/v0.1.49...v0.1.50
[0.1.49]: https://github.com/hardbeat920/monocode/compare/v0.1.48...v0.1.49
[0.1.48]: https://github.com/hardbeat920/monocode/compare/v0.1.47...v0.1.48
[0.1.47]: https://github.com/hardbeat920/monocode/compare/v0.1.46...v0.1.47
[0.1.46]: https://github.com/hardbeat920/monocode/compare/v0.1.45...v0.1.46
[0.1.45]: https://github.com/hardbeat920/monocode/compare/v0.1.44...v0.1.45
[0.1.44]: https://github.com/hardbeat920/monocode/compare/v0.1.43...v0.1.44
[0.1.43]: https://github.com/hardbeat920/monocode/compare/v0.1.42...v0.1.43
[0.1.42]: https://github.com/hardbeat920/monocode/compare/v0.1.41...v0.1.42
[0.1.41]: https://github.com/hardbeat920/monocode/compare/v0.1.40...v0.1.41
[0.1.40]: https://github.com/hardbeat920/monocode/compare/v0.1.39...v0.1.40
[0.1.39]: https://github.com/hardbeat920/monocode/compare/v0.1.38...v0.1.39
[0.1.38]: https://github.com/hardbeat920/monocode/compare/v0.1.37...v0.1.38
[0.1.37]: https://github.com/hardbeat920/monocode/compare/v0.1.36...v0.1.37
[0.1.36]: https://github.com/hardbeat920/monocode/compare/v0.1.35...v0.1.36
[0.1.35]: https://github.com/hardbeat920/monocode/compare/v0.1.34...v0.1.35
[0.1.34]: https://github.com/hardbeat920/monocode/compare/v0.1.33...v0.1.34
[0.1.33]: https://github.com/hardbeat920/monocode/compare/v0.1.32...v0.1.33
[0.1.32]: https://github.com/hardbeat920/monocode/compare/v0.1.31...v0.1.32
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
