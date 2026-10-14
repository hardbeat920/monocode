import type { Translations } from "./types";

export const en = {
  "settings.notifications.mutedCount": "{muted} of {total} projects muted",
  "settings.notifications.saveFailed":
    "Could not save notification preferences. Please try again.",
  "settings.notifications.resume": "Resume notifications",
  "settings.notifications.changeMuteDuration": "Change mute duration",
  "settings.notifications.mute": "Mute notifications",
  "settings.notifications.muteTitle":
    "Mute pauses all project notifications without changing your category choices.",
  "settings.notifications.mutedLabel": "Muted",
  "settings.notifications.muteLabel": "Mute",
  "settings.notifications.muteAllFor": "Mute all notifications for",
  "settings.notifications.muteDialog": "Mute project notifications",
  "settings.notifications.invalidDate": "Choose a valid date and time.",
  "settings.notifications.pastDate": "Choose a date and time in the future.",
  "settings.notifications.muteAllUntil": "Mute all notifications until",
  "settings.notifications.cancel": "Cancel",
  "settings.notifications.muteUntilThen": "Mute until then",
  "settings.notifications.mutedUntilResumed": "Muted until resumed",
  "settings.notifications.mutedUntil": "Muted until {date}",
  "settings.notifications.muteHours": {
    one: "{count} hour",
    other: "{count} hours",
  },
  "settings.notifications.untilResumed": "Until resumed",
  "settings.notifications.chooseDateTime": "Choose date and time",
  "settings.notifications.tomorrow": "Tomorrow, ",
  "settings.notifications.question": "Question",
  "settings.notifications.category.pullRequests":
    "Pull requests / Merge requests",
  "settings.notifications.category.issues": "Issues and Linear tasks",
  "settings.notifications.category.agentFinished": "Agent finished",
  "settings.notifications.category.agentInput": "Agent approvals and questions",
  "settings.notifications.category.reminders": "Reminders",
  "settings.notifications.banner.question": "{harness} has a question for you",
  "settings.notifications.banner.approve": "Approve: {what}",
  "settings.notifications.banner.needsApproval":
    "{harness} needs your approval",
  "settings.notifications.banner.finished": "{harness} finished",
  "settings.notifications.reminderTitle": "Reminder",
  "settings.notifications.projects.title": "Project notifications",
  "settings.notifications.projects.description":
    "Choose sounds, banners and sidebar indicators by category. Mute pauses them without changing your choices. Unread items stay marked in Inbox.",
  "settings.notifications.projects.done": "Done",
  "settings.notifications.projects.select": "Select projects",
  "settings.notifications.projects.empty":
    "Open a project or connect an Inbox provider to configure its notifications.",
  "settings.notifications.projects.selectAll": "Select all projects",
  "settings.notifications.projects.selectedCount": "{count} selected",
  "settings.notifications.projects.muteSelected": "Mute selected projects",
  "settings.notifications.projects.selectOne": "Select {name}",
  "settings.notifications.projects.categoriesFor":
    "Notification categories for {name}",
  "settings.notifications.projects.local": "Local project · ",
  "settings.notifications.projects.paused": "All notifications paused",
  "settings.notifications.projects.allEnabled": "All categories enabled",
  "settings.notifications.projects.someEnabled": "{enabled} of {total} enabled",
  "settings.notifications.projects.mutedHint":
    "Your category choices apply when notifications resume. You can edit them while muted.",
  "settings.notifications.projects.categoryFor": "{category} for {project}",
  "settings.jira.checking": "Checking Jira connection…",
  "settings.jira.disconnecting": "Disconnecting",
  "settings.jira.disconnect": "Disconnect",
  "settings.jira.intro":
    "Connect your Jira Cloud site using your Atlassian email and an API token without scopes. Disconnect deletes the saved credentials.",
  "settings.jira.site": "Jira site",
  "settings.jira.email": "Atlassian email",
  "settings.jira.token": "Jira API token",
  "settings.jira.tokenPlaceholder": "API token",
  "settings.jira.connecting": "Connecting",
  "settings.jira.connect": "Connect",
  "settings.jira.createToken": "Create API token",
  "settings.jira.projects": "Projects",
  "settings.jira.refreshProjects": "Refresh projects",
  "settings.jira.hiddenHint": "Unchecked projects stay out of the inbox.",
  "settings.group.app": "App",
  "settings.group.agents": "Agents",
  "settings.group.workspace": "Workspace",
  "settings.section.general.label": "General",
  "settings.section.general.description":
    "The build you are running, how MonoCode reaches you, and the panels it shows.",
  "settings.section.connections.label": "Connections",
  "settings.section.connections.description":
    "Connect your machines and run agents remotely through SSH.",
  "settings.section.appearance.label": "Appearance",
  "settings.section.appearance.description":
    "Theme, tint, translucency, workspace layout, and conversation backgrounds.",
  "settings.section.keybindings.label": "Keybindings",
  "settings.section.keybindings.description":
    "Every shortcut the workspace handles, from the app menu and the key handler.",
  "settings.section.chat.label": "Chat",
  "settings.section.chat.description":
    "How transcripts read, what the composer does with a follow-up, how files save, and how diffs open.",
  "settings.section.providers.label": "Providers",
  "settings.section.providers.description":
    "Provider accounts, agent CLIs MonoCode can drive, and the model new sessions start with.",
  "settings.section.mcp.description":
    "Find MCP servers across providers and manage their connections.",
  "settings.section.skills.label": "Skills",
  "settings.section.skills.description":
    "Discover and manage file skills from project, personal, and harness folders.",
  "settings.section.monos.label": "Monos",
  "settings.section.monos.description":
    "The resident agent beside your tabs, and which projects have one.",
  "settings.section.inbox.label": "Inbox",
  "settings.section.inbox.description":
    "Manage Inbox services and notification preferences for each project.",
  "settings.section.archive.label": "Archive",
  "settings.section.archive.description":
    "Projects and conversations you have archived.",
  "settings.section.worktrees.label": "Worktrees",
  "settings.section.worktrees.description":
    "Manage additional worktrees for each project.",
  "settings.index.remote-machines": "Your machines",
  "settings.index.mcp-servers": "MCP servers",
  "settings.index.monos-enabled": "Show monos",
  "settings.index.rail-monos-pinned": "Pin monos to the icon rail",
  "settings.index.mono-list": "Your monos",
  "settings.index.project-worktrees": "Project worktrees",
  "settings.index.update": "Version",
  "settings.index.sounds": "Sounds",
  "settings.index.notifications": "Notifications",
  "settings.index.notes": "Notes",
  "settings.index.quick-composer": "Quick composer",
  "settings.index.working-agents": "Working agents",
  "settings.index.file-tabs": "File tabs",
  "settings.index.tab-animations": "Tab animations",
  "settings.index.close-to-tray": "Close to tray",
  "settings.index.theme": "Theme",
  "settings.index.accent-color": "Accent color",
  "settings.index.diff-colors": "Diff colors",
  "settings.index.hue": "Hue",
  "settings.index.saturation": "Saturation",
  "settings.index.dark-lightness": "Dark-mode lightness",
  "settings.index.sidebar-opacity": "Sidebar opacity",
  "settings.index.blur": "Blur radius",
  "settings.index.main-pane-glass": "Main pane glass",
  "settings.index.interface-scale": "Interface scale",
  "settings.index.collapsed-project-rail": "Collapsed project rail",
  "settings.index.show-excluded-files": "Show excluded files",
  "settings.index.chat-background": "Chat background",
  "settings.index.transcript-layout": "Transcript layout",
  "settings.index.anchor-prompts": "Anchor prompts to top",
  "settings.index.follow-up": "Follow-up behavior",
  "settings.index.model-controls": "Model controls",
  "settings.index.composer-autocorrect": "Autocorrect",
  "settings.index.composer-mascot": "Composer mascot",
  "settings.index.format-on-save": "Format on save",
  "settings.index.diff-view": "Diff view",
  "settings.index.empty-session-games": "Empty session games",
  "settings.index.agent-clis": "Agent CLIs",
  "settings.index.provider-accounts": "Provider accounts",
  "settings.index.show-remaining-usage": "Show remaining usage",
  "settings.index.mask-emails": "Mask account emails",
  "settings.index.claude-hooks": "Claude Code hooks",
  "settings.index.idle-agents": "Keep idle agents ready",
  "settings.index.project-notifications": "Project notifications",
  "settings.index.show-archived": "Show archived in the sidebar",
  "settings.keybindings.invalidShortcut":
    "That combination is not a valid shortcut",
  "settings.keybindings.activateNeedsNumber":
    "Tab: Activate 1–8 needs a number key from 1 to 8",
  "settings.keybindings.alreadyUsed": "Already used by {owner}",
  "settings.keybindings.saveFailed": "Could not save shortcuts",
  "settings.keybindings.when.always": "Always",
  "settings.keybindings.when.anywhere": "Anywhere",
  "settings.keybindings.when.projectWithMono": "Project with a mono",
  "settings.keybindings.when.draftComposer": "Draft session composer",
  "settings.appearance.effect.none.label": "None",
  "settings.appearance.effect.dither.label": "Dither",
  "settings.appearance.effect.ascii.label": "ASCII",
  "settings.appearance.effect.halftone.label": "Halftone",
  "settings.appearance.effect.scanlines.label": "Scanlines",
  "settings.appearance.effect.gradient-blur.label": "Haze",
  "settings.appearance.effect.none.description": "Shows the original artwork.",
  "settings.appearance.effect.dither.description":
    "Rebuilds the artwork with a dithered color palette.",
  "settings.appearance.effect.ascii.description":
    "Recreates the artwork with colored characters on black.",
  "settings.appearance.effect.halftone.description":
    "Recreates the artwork with colored print dots on black.",
  "settings.appearance.effect.scanlines.description":
    "Adds a pronounced horizontal display-line texture.",
  "settings.appearance.effect.gradient-blur.description":
    "Blurs and fades the artwork into the background below.",
  "settings.mcp.scope.local": "Local",
  "settings.mcp.scope.project": "Project",
  "settings.mcp.scope.user": "User",
  "settings.mcp.status.disabled": "Disabled",
  "settings.mcp.status.configured": "Configured",
  "settings.mcp.notObject": "Configuration must be a JSON object",
  "settings.mcp.add.title": "Add MCP server",
  "settings.mcp.add.description":
    "Paste a server configuration and choose where to add it.",
  "settings.mcp.add.provider": "Provider",
  "settings.mcp.add.scope": "Scope",
  "settings.mcp.add.name": "Name",
  "settings.mcp.add.nameOptional": "(optional for an mcpServers block)",
  "settings.mcp.add.config": "JSON configuration",
  "settings.mcp.add.hint":
    "Paste one entry from an mcpServers block, or a single server object with a name above.",
  "settings.mcp.add.cancel": "Cancel",
  "settings.mcp.add.adding": "Adding…",
  "settings.mcp.add.submit": "Add server",
  "settings.mcp.removePrompt": "Remove {name} from {scope} scope?",
  "settings.mcp.removeTitle": "Remove MCP server",
  "settings.mcp.connections": "MCP connections",
  "settings.mcp.connectionsDescription":
    "Configured servers for the selected project and your provider accounts.",
  "settings.mcp.refresh": "Refresh",
  "settings.mcp.showAvailable": "Show available providers",
  "settings.mcp.showAll": "Show all providers",
  "settings.mcp.showingAll": "Showing all providers",
  "settings.mcp.showingAvailable": "Showing available providers",
  "settings.mcp.filter": "Filter MCP servers by provider",
  "settings.mcp.all": "All",
  "settings.mcp.claudeUnavailable":
    "Claude connection status unavailable: {error}",
  "settings.mcp.checking": "Checking servers…",
  "settings.mcp.empty": "No MCP servers configured for this provider.",
  "settings.mcp.signIn": "Sign in",
  "settings.mcp.scopeToRemove": "Scope to remove {name} from",
  "settings.mcp.remove": "Remove",
  "settings.mcp.showConfig": "Show config",
  "settings.mcp.footer":
    "Claude Code status comes from its CLI. Other providers show configured entries. Sign in opens your browser when supported.",
  "settings.title": "Settings",
  "settings.restoreDefaults": "Restore defaults",
  "settings.search.placeholder": "Search settings",
  "settings.search.clear": "Clear settings search",
  "settings.search.results": "Settings search results",
  "settings.search.none": "No matching settings",
  "settings.search.page": "Page",
  "settings.general.alerts.title": "Alerts",
  "settings.general.alerts.description":
    "How MonoCode reaches you while you are looking somewhere else.",
  "settings.general.sounds.description":
    "Short cues for project activity, finished turns, and available updates. Choose project notification categories in Inbox settings. Switches and Copy on a finished turn also play.",
  "settings.general.notifications.description":
    "Notify when a reminder is due, or when an agent finishes or needs input in another session or while MonoCode is in the background. Click the notification to open that session.",
  "settings.general.notifications.unsupported":
    "Not available on this platform",
  "settings.general.workspace.title": "Workspace",
  "settings.general.workspace.description":
    "How project navigation and workspace tabs behave.",
  "settings.general.fileTabs.description":
    "Open files beside the active chat, or give each file a normal tab in the top bar. Top-bar files can still be combined into split panes.",
  "settings.general.fileTabs.pane": "Beside chat",
  "settings.general.fileTabs.workspace": "Top bar",
  "settings.general.tabAnimations.description":
    "Animate tabs as they open and close. Turn this off for instant tab changes.",
  "settings.general.notes.description":
    "A global markdown notebook on the project rail. Save a finished turn from the transcript, then mention it later with @note or add it to chat.",
  "settings.general.quickComposer.description":
    "Press {shortcut} in any app to float a prompt over it and start a session without switching to MonoCode. Change the shortcut in Keybindings. Return starts it in the background; ⌘Return starts it and brings the session forward.",
  "settings.general.workingAgents.description":
    "When two or more chats are in flight, a card on the project rail lists them so you can jump across projects. Finished turns stay until you open that session.",
  "settings.general.closeToTray.description":
    "Closing a window hides it to the system tray instead of quitting, so running agents keep going. Reopen from the tray icon, and quit for real from its menu. Turn this off to have close end the window.",
  "settings.general.about": "About",
  "settings.chat.transcript.title": "Transcript",
  "settings.chat.transcript.description":
    "How a conversation reads as it grows.",
  "settings.chat.layout.description":
    "Full width keeps user prompts as a spanning card. Chat aligns them to the right with a max width, like a messaging app.",
  "settings.chat.layout.full": "Full width",
  "settings.chat.layout.chat": "Chat",
  "settings.chat.anchor.description":
    "When you send, the new prompt sits at the top of the transcript and the reply grows into the space below. Turn this off to keep the classic layout, with the latest message resting on the composer.",
  "settings.chat.composer.title": "Composer",
  "settings.chat.composer.description":
    "What the composer does with what you type.",
  "settings.chat.followUp.description":
    "Queue follow-ups until the active turn finishes, or steer the active turn immediately.",
  "settings.chat.followUp.queue": "Queue",
  "settings.chat.followUp.steer": "Steer",
  "settings.chat.modelControls.description":
    "Show model options beside the picker instead of inside the model menu.",
  "settings.chat.modelControls.menu": "Menu",
  "settings.chat.modelControls.beside": "Beside",
  "settings.chat.autocorrect.description":
    "Spell check and autocorrect prompts in session and mono composers. Turn this off to keep the text exactly as typed.",
  "settings.chat.editor.title": "Editor",
  "settings.chat.editor.description":
    "What happens when you save a file in the workspace editor.",
  "settings.chat.formatOnSave.description":
    "Run Prettier on supported files before writing. Off keeps the text you typed, including quote style.",
  "settings.chat.review.title": "Code review",
  "settings.chat.review.description":
    "Where a turn's changes open when you go to read them.",
  "settings.chat.diffView.description":
    "Editor keeps working-tree changes in the file. Unified stacks every changed file in one review, with sticky headers and collapsed unchanged lines.",
  "settings.chat.diffView.editor": "Editor",
  "settings.chat.diffView.unified": "Unified",
  "settings.chat.extras.title": "Extras",
  "settings.chat.extras.description":
    "Idle animation, and nothing else. Turn both off for a still workspace.",
  "settings.chat.mascot.description":
    "When a turn is running, the project mascot runs along the composer, bonks the scroll-to-latest button the first time, then jumps it, and sometimes grabs a coin.",
  "settings.chat.games.description":
    "Pac-man and snake idle on the empty-session grid. Hover the band to take control of whichever is on screen. Turn this off to keep the pane still.",
  "settings.inbox.github.description":
    "Pull requests, reviews, and issues, read through the GitHub CLI.",
  "settings.inbox.gitlab.description":
    "Merge requests from GitLab.com or a self-managed instance.",
  "settings.inbox.ado.description":
    "Pull requests and Boards work items from your ADO organization.",
  "settings.inbox.jira.description":
    "Jira Cloud issues from the projects you pick.",
  "settings.inbox.linear.description":
    "Issues assigned to you, from the teams you pick.",
  "settings.inbox.connection": "Connection",
  "settings.inbox.disconnect": "Disconnect",
  "settings.inbox.connect": "Connect",
  "settings.inbox.saving": "Saving",
  "settings.inbox.github.connected":
    "GitHub CLI is installed and authenticated. MonoCode uses it for GitHub inbox items.",
  "settings.inbox.github.signIn":
    "Run gh auth login in a terminal, complete the sign-in flow, then check again.",
  "settings.inbox.github.install":
    "Install GitHub CLI from cli.github.com, run gh auth login in a terminal, then check again.",
  "settings.inbox.github.checking": "Checking",
  "settings.inbox.github.statusConnected": "Connected",
  "settings.inbox.github.statusSignIn": "Sign in required",
  "settings.inbox.github.statusMissing": "Not installed",
  "settings.inbox.github.guide": "Installation guide",
  "settings.inbox.github.checkAgain": "Check again",
  "settings.inbox.gitlab.connection":
    "Connect GitLab.com or a self-managed GitLab instance. Use a personal access token with API access; the token is stored locally and Disconnect deletes it.",
  "settings.inbox.gitlab.url": "GitLab URL",
  "settings.inbox.gitlab.token": "GitLab access token",
  "settings.inbox.ado.connection":
    "Connect your ADO organization with a personal access token (Boards + Repos read & write for comments). The token is stored locally and Disconnect deletes it.",
  "settings.inbox.ado.url": "Azure DevOps organization URL",
  "settings.inbox.ado.token": "Azure DevOps personal access token",
  "settings.inbox.linear.apiKey": "API key",
  "settings.inbox.linear.apiKeyDescription":
    "Create a personal API key in Linear → Settings → Security & Access. Disconnect deletes it.",
  "settings.inbox.linear.apiKeyLabel": "Linear API key",
  "settings.inbox.linear.teams": "Teams",
  "settings.inbox.linear.hiddenHint": "Unchecked teams stay out of the inbox.",
  "settings.update.available": "Version {version} is available.",
  "settings.update.downloadingPercent": "Downloading {progress}%",
  "settings.update.downloading": "Downloading…",
  "settings.update.checking": "Checking for updates…",
  "settings.update.current": "You're on the latest version.",
  "settings.update.failed": "Update check failed.",
  "settings.update.selfUpdates":
    "MonoCode updates itself from the release feed.",
  "settings.update.whatsNew": "What's new",
  "settings.update.download": "Download",
  "settings.update.check": "Check for updates",
  "settings.language.title": "Language",
  "settings.language.description":
    "Use your system language, or pick one. The change applies right away.",
  "settings.language.system": "System default",
  "settings.index.language": "Language",
  "settings.appearance.theme.description":
    "Dark and light share the same tint, so the color settings below apply to both.",
  "settings.appearance.theme.row": "System follows the OS appearance.",
  "settings.appearance.theme.system": "System",
  "settings.appearance.theme.dark": "Dark",
  "settings.appearance.theme.light": "Light",
  "settings.appearance.accent.description":
    "Used for the composer send button and your message bubbles.",
  "settings.appearance.accent.default": "Default",
  "settings.appearance.accent.blue": "Blue",
  "settings.appearance.accent.violet": "Violet",
  "settings.appearance.accent.pink": "Pink",
  "settings.appearance.accent.red": "Red",
  "settings.appearance.accent.orange": "Orange",
  "settings.appearance.accent.green": "Green",
  "settings.appearance.diff.description":
    "Colors for added and removed lines. Colorblind and High contrast use blue and orange instead of green and red; High contrast adds stronger tints and text.",
  "settings.appearance.diff.default": "Default",
  "settings.appearance.diff.colorblind": "Colorblind",
  "settings.appearance.diff.highContrast": "High contrast",
  "settings.appearance.color.title": "Color",
  "settings.appearance.color.description":
    "Hue and saturation tint every surface. Lightness only moves the dark theme.",
  "settings.appearance.hue.description":
    "Base hue for accents and tinted surfaces.",
  "settings.appearance.saturation.description":
    "How strongly the hue tints the interface. Zero keeps it neutral.",
  "settings.appearance.darkLightness.disabled":
    "This only affects dark mode. Your dark-mode value is preserved.",
  "settings.appearance.darkLightness.description":
    "Base brightness of the dark theme. Lower values are darker; zero is true black.",
  "settings.appearance.translucency.title": "Translucency",
  "settings.appearance.translucency.disabled":
    "Light mode always uses an opaque window, so these are off. Your dark-mode values are preserved.",
  "settings.appearance.translucency.description":
    "How much of the desktop shows through MonoCode. Blur costs more to composite the higher it goes.",
  "settings.appearance.sidebarOpacity.description":
    "Applies to the project rail and the other glass panes.",
  "settings.appearance.blur.description": "Background blur behind the window.",
  "settings.appearance.mainPaneGlass.description":
    "Extend the translucent treatment to the main pane behind sessions and editors.",
  "settings.appearance.layout.title": "Layout",
  "settings.appearance.rail.description":
    "Keep project navigation available as a compact icon rail, or hide the rail completely.",
  "settings.appearance.rail.compact": "Icon rail",
  "settings.appearance.rail.hidden": "Hidden",
  "settings.appearance.scale.description":
    "Zoom the whole interface. You can also use Ctrl+=, Ctrl+-, and Ctrl+0 (Cmd on macOS).",
  "settings.appearance.excluded.description":
    "Show files and folders Git excludes, such as build output and dependencies, in the explorer.",
  "settings.appearance.background.description":
    "An image behind your chat panes. It stays on this device.",
  "settings.appearance.background.preview": "Empty chat preview at {percent}%",
  "settings.appearance.background.choose": "Choose an image",
  "settings.appearance.background.change": "Change",
  "settings.appearance.background.remove": "Remove",
  "settings.appearance.background.effect": "Background effect",
  "settings.appearance.background.showOn": "Show on",
  "settings.appearance.background.showOnDescription":
    "Empty sessions only, or every conversation.",
  "settings.appearance.background.showOnLabel": "Show background on",
  "settings.appearance.background.emptyOnly": "Empty only",
  "settings.appearance.background.allSessions": "All sessions",
  "settings.appearance.background.emptyVisibility": "Empty chat visibility",
  "settings.appearance.background.emptyVisibilityDescription":
    "Background strength before a chat has messages.",
  "settings.appearance.background.emptyVisibilityLabel":
    "Empty chat background visibility",
  "settings.appearance.background.sessionVisibility": "Session visibility",
  "settings.appearance.background.sessionVisibilityDescription":
    "Background strength once the conversation has messages.",
  "settings.appearance.background.sessionVisibilityLabel":
    "Session background visibility",
  "settings.shortcut.change": "Change {name} shortcut",
  "settings.shortcut.reset": "Reset {name} shortcut",
  "settings.shortcut.record": "Record…",
  "settings.shortcut.disabled": "Disabled",
  "settings.shortcut.hint": "Del disables · Esc cancels",
  "settings.shortcut.quickComposerNeedsModifier":
    "Quick Composer needs ⌘ or Ctrl as a global hotkey",
  "settings.keybindings.title": "Shortcuts",
  "settings.keybindings.description":
    "Click a shortcut to record new keys. Press Delete while recording to disable it.",
  "settings.keybindings.count": {
    one: "{count} binding",
    other: "{count} bindings",
  },
  "settings.keybindings.filter": "Filter",
  "settings.keybindings.filterLabel": "Filter keybindings",
  "settings.keybindings.command": "Command",
  "settings.keybindings.keybinding": "Keybinding",
  "settings.keybindings.when": "When",
  "settings.keybindings.none": "No matching bindings",
  "settings.providers.binary.invalidCodex":
    "Codex CLI returned an invalid version.",
  "settings.providers.binary.invalidOpencode":
    "OpenCode CLI returned an invalid version.",
  "settings.providers.binary.tooOld":
    "OpenCode v{version} is too old. Upgrade to v{minimum} or newer.",
  "settings.providers.binary.saveFailed": "Could not save the binary path.",
  "settings.providers.binary.showDetailsRestart":
    "Show {title} CLI details, restart required",
  "settings.providers.binary.showDetails": "Show {title} CLI details",
  "settings.providers.binary.pathTitle": "{title} CLI path",
  "settings.providers.binary.pathTitleRestart":
    "{title} CLI path — restart required",
  "settings.providers.binary.details": "{title} CLI details",
  "settings.providers.binary.globalPath": "Global path",
  "settings.providers.binary.needsAttention": "Needs attention",
  "settings.providers.binary.restartRequired": "Restart required",
  "settings.providers.binary.configured": "Configured",
  "settings.providers.binary.autoDetected": "Auto-detected",
  "settings.providers.binary.cliPath": "CLI path",
  "settings.providers.binary.autoPathPlaceholder": "Auto-detected path",
  "settings.providers.binary.hint":
    "Enter the absolute path to the CLI executable. Changes apply after restarting MonoCode.",
  "settings.providers.binary.cancel": "Cancel",
  "settings.providers.binary.useAuto": "Use auto-detected path",
  "settings.providers.binary.save": "Save path",
  "settings.providers.binary.unresolved": "CLI could not be resolved",
  "settings.providers.binary.checkingCli": "Checking the selected CLI…",
  "settings.providers.binary.retryHint": "Retry to check this CLI",
  "settings.providers.binary.checkingVersion": "Checking version…",
  "settings.providers.binary.revealFailed":
    "Could not open the CLI location: {error}",
  "settings.providers.binary.retryConfiguredLabel":
    "Retry {title} configured path",
  "settings.providers.binary.retryAutoLabel": "Retry {title} auto-detect",
  "settings.providers.binary.retryConfigured": "Retry configured path",
  "settings.providers.binary.retryAuto": "Retry auto-detect",
  "settings.providers.binary.openLocationLabel": "Open {title} CLI location",
  "settings.providers.binary.openLocation": "Open location",
  "settings.providers.binary.editLabel": "Edit {title} CLI path",
  "settings.providers.binary.edit": "Edit path",
  "settings.providers.global": "Global",
  "settings.providers.scope": "Provider defaults scope",
  "settings.providers.projectDescription":
    "These defaults apply to {project} only. A provider with Show in picker off is also kept out of new conversations started in this project. CLI paths remain global for MonoCode.",
  "settings.providers.globalDescription":
    "A provider is listed as installed once its CLI is found on your PATH. Uninstalled CLIs stay listed but are left out of the model picker, as are installed ones with Show in picker off. The model beside a provider is what its new conversations start with; Use by default picks the provider itself. CLI paths are global for MonoCode and apply to every project.",
  "settings.providers.advanced": "Advanced",
  "settings.providers.idle.description":
    "A finished conversation keeps its agent CLI running for a few minutes, so a follow-up starts instantly. Each one can hold a few hundred MB of memory. Past this many, the one idle longest stops early; it resumes on your next message after a short restart. Choose None on a machine short on memory.",
  "settings.providers.idle.none": "None",
  "settings.providers.hooks.description":
    "Run the hooks configured in your settings.json files — PreToolUse command rewrites, blocks, notifications, and the rest — just as the Claude Code CLI would. Turn this off if a hook is misbehaving and you need the session back. Takes effect on the next turn.",
  "settings.providers.usage.title": "Usage and privacy",
  "settings.providers.usage.remaining":
    "Fill usage meters with what is left in each limit instead of what has been used.",
  "settings.providers.usage.mask":
    "Blur account emails in Settings and the usage popover until you click one, so they stay out of screenshots.",
  "settings.providers.accounts.saveFailed": "Could not save this account",
  "settings.providers.accounts.removeFailed": "Could not remove this account",
  "settings.providers.accounts.removeConfirm":
    "Remove “{label}”? Its stored credentials will be deleted and any running turns for this account will stop. Existing conversations stay in history, but cannot continue until you switch accounts.",
  "settings.providers.accounts.removeTitle": "Remove {title} account",
  "settings.providers.accounts.removeOk": "Remove account",
  "settings.providers.accounts.cancel": "Cancel",
  "settings.providers.accounts.title": "Accounts",
  "settings.providers.accounts.description":
    "Create isolated sign-ins for providers that support account profiles. Account switching stays available from the usage control in the footer.",
  "settings.providers.accounts.count": {
    one: "{count} account",
    other: "{count} accounts",
  },
  "settings.providers.accounts.add": "Add account",
  "settings.providers.accounts.cliProfile": "Provider CLI profile",
  "settings.providers.accounts.isolated": "Isolated profile",
  "settings.providers.default": "Default",
  "settings.providers.accounts.renameLabel": "Rename {label}",
  "settings.providers.accounts.renameTitle": "Rename account",
  "settings.providers.accounts.removeLabel": "Remove {label}",
  "settings.providers.accounts.name": "Account name",
  "settings.providers.accounts.namePlaceholder": "Work or Personal",
  "settings.providers.accounts.newLabel": "New {title} account",
  "settings.providers.accounts.renameEditLabel": "Rename {title} account",
  "settings.providers.accounts.waiting": "Waiting for browser…",
  "settings.providers.accounts.signInAdd": "Sign in and add",
  "settings.providers.accounts.save": "Save",
  "settings.providers.row.models": {
    one: "{count} model available.",
    other: "{count} models available.",
  },
  "settings.providers.row.modelLabel": "{title} model",
  "settings.providers.row.useDefault": "Use by default",
  "settings.providers.row.hiddenGlobally": "Hidden globally",
  "settings.providers.row.showInPicker": "Show in picker",
  "settings.providers.row.showInPickerLabel":
    "Show {title} in the model picker",
  "settings.archive.projects.title": "Archived projects",
  "settings.archive.projects.description":
    "Archive a project from the rail to keep its chats without listing it in the sidebar.",
  "settings.archive.projects.none": "No archived projects.",
  "settings.archive.restore": "Restore",
  "settings.archive.delete": "Delete",
  "settings.archive.inProject": "Archived in {project}",
  "settings.archive.conversations": "Archived conversations",
  "settings.archive.showDescription":
    "Keep archived conversations listed alongside the active ones.",
  "settings.archive.openProject":
    "Open a project to see its archived conversations.",
  "settings.archive.noneInProject":
    "No archived conversations in this project.",
  "settings.archive.unarchive": "Unarchive",
  "settings.monos.enabled.description":
    "Agents of your own on the project rail. Each works on the projects you give it, remembers what matters and picks up habits it runs on its own. Turn this off to hide them.",
  "settings.monos.pinned.description":
    "When the project rail is collapsed to icons, show each Mono at the top of the rail, above a divider, instead of inside the project picker.",
  "settings.monos.menuBar.title": "Menu bar icon",
  "settings.monos.menuBar.description":
    "Chat with a Mono or open the quick composer from the macOS menu bar. Turn this off to hide the icon.",
  "settings.monos.list.description":
    "Choose whether new sessions started by each Mono appear in the sidebar. Hidden sessions remain saved and can be opened from the Mono's chat. Add a Mono with the plus on the rail and choose its projects from its details.",
  "settings.monos.none": "No monos yet.",
  "settings.monos.worksOn": "Works on {projects}",
  "settings.monos.noProjects": "No projects yet",
  "settings.monos.showSessions": "Show Mono spawned session on the sidebar",
  "settings.monos.showSessionsLabel":
    "Show sessions started by {name} in sidebar",
  "settings.monos.reset.label": "Reset Mono",
  "settings.monos.reset.title": "Reset {name} to its defaults?",
  "settings.monos.reset.body":
    "Its soul goes back to the default and its name to {defaultName}. Changes to its soul can't be recovered.",
  "settings.monos.reset.kept":
    "Its conversation, projects, memory and habits will be kept.",
  "settings.monos.reset.failure": "Could not reset the Mono.",
  "settings.monos.reset.title2": "Reset to defaults",
  "settings.monos.reset.aria": "Reset {name} to defaults",
  "settings.general.notifications.permission": "Permission needed",
  "settings.general.notifications.openSystem": "Open System Settings",
} as const;

export const tr: Translations<typeof en> = {
  "settings.notifications.mutedCount":
    "{total} projeden {muted} tanesi sessize alındı",
  "settings.notifications.saveFailed":
    "Bildirim tercihleri kaydedilemedi. Lütfen tekrar dene.",
  "settings.notifications.resume": "Bildirimleri sürdür",
  "settings.notifications.changeMuteDuration": "Sessiz süresini değiştir",
  "settings.notifications.mute": "Bildirimleri sessize al",
  "settings.notifications.muteTitle":
    "Sessize alma, kategori seçimlerini değiştirmeden tüm proje bildirimlerini duraklatır.",
  "settings.notifications.mutedLabel": "Sessizde",
  "settings.notifications.muteLabel": "Sessize al",
  "settings.notifications.muteAllFor": "Tüm bildirimleri şu süreyle sessize al",
  "settings.notifications.muteDialog": "Proje bildirimlerini sessize al",
  "settings.notifications.invalidDate": "Geçerli bir tarih ve saat seç.",
  "settings.notifications.pastDate": "Gelecekte bir tarih ve saat seç.",
  "settings.notifications.muteAllUntil":
    "Tüm bildirimleri şu zamana kadar sessize al",
  "settings.notifications.cancel": "İptal",
  "settings.notifications.muteUntilThen": "O zamana kadar sessize al",
  "settings.notifications.mutedUntilResumed": "Sürdürülene kadar sessizde",
  "settings.notifications.mutedUntil": "{date} tarihine kadar sessizde",
  "settings.notifications.muteHours": {
    one: "{count} saat",
    other: "{count} saat",
  },
  "settings.notifications.untilResumed": "Sürdürülene kadar",
  "settings.notifications.chooseDateTime": "Tarih ve saat seç",
  "settings.notifications.tomorrow": "Yarın, ",
  "settings.notifications.question": "Soru",
  "settings.notifications.category.pullRequests":
    "Pull request / Merge request",
  "settings.notifications.category.issues": "Issue ve Linear görevleri",
  "settings.notifications.category.agentFinished": "Agent bitti",
  "settings.notifications.category.agentInput": "Agent onayları ve soruları",
  "settings.notifications.category.reminders": "Hatırlatıcılar",
  "settings.notifications.banner.question": "{harness} sana bir soru soruyor",
  "settings.notifications.banner.approve": "Onayla: {what}",
  "settings.notifications.banner.needsApproval": "{harness} onayını bekliyor",
  "settings.notifications.banner.finished": "{harness} bitti",
  "settings.notifications.reminderTitle": "Hatırlatıcı",
  "settings.notifications.projects.title": "Proje bildirimleri",
  "settings.notifications.projects.description":
    "Sesleri, banner'ları ve kenar çubuğu göstergelerini kategoriye göre seç. Sessize alma, seçimlerini değiştirmeden bunları duraklatır. Okunmamış öğeler Gelen kutusunda işaretli kalır.",
  "settings.notifications.projects.done": "Tamam",
  "settings.notifications.projects.select": "Proje seç",
  "settings.notifications.projects.empty":
    "Bildirimlerini ayarlamak için bir proje aç veya bir Gelen kutusu sağlayıcısı bağla.",
  "settings.notifications.projects.selectAll": "Tüm projeleri seç",
  "settings.notifications.projects.selectedCount": "{count} seçildi",
  "settings.notifications.projects.muteSelected": "Seçili projeleri sessize al",
  "settings.notifications.projects.selectOne": "{name} projesini seç",
  "settings.notifications.projects.categoriesFor":
    "{name} için bildirim kategorileri",
  "settings.notifications.projects.local": "Yerel proje · ",
  "settings.notifications.projects.paused": "Tüm bildirimler duraklatıldı",
  "settings.notifications.projects.allEnabled": "Tüm kategoriler açık",
  "settings.notifications.projects.someEnabled":
    "{total} kategoriden {enabled} tanesi açık",
  "settings.notifications.projects.mutedHint":
    "Kategori seçimlerin bildirimler sürdürüldüğünde geçerli olur. Sessizdeyken de düzenleyebilirsin.",
  "settings.notifications.projects.categoryFor": "{project} için {category}",
  "settings.jira.checking": "Jira bağlantısı kontrol ediliyor…",
  "settings.jira.disconnecting": "Bağlantı kesiliyor",
  "settings.jira.disconnect": "Bağlantıyı kes",
  "settings.jira.intro":
    "Atlassian e-postanı ve kapsamsız bir API token kullanarak Jira Cloud sitene bağlan. Bağlantıyı kesmek kayıtlı kimlik bilgilerini siler.",
  "settings.jira.site": "Jira sitesi",
  "settings.jira.email": "Atlassian e-postası",
  "settings.jira.token": "Jira API token",
  "settings.jira.tokenPlaceholder": "API token",
  "settings.jira.connecting": "Bağlanıyor",
  "settings.jira.connect": "Bağlan",
  "settings.jira.createToken": "API token oluştur",
  "settings.jira.projects": "Projeler",
  "settings.jira.refreshProjects": "Projeleri yenile",
  "settings.jira.hiddenHint":
    "İşaretlenmeyen projeler Gelen kutusunda görünmez.",
  "settings.group.app": "Uygulama",
  "settings.group.agents": "Agent'lar",
  "settings.group.workspace": "Çalışma alanı",
  "settings.section.general.label": "Genel",
  "settings.section.general.description":
    "Çalıştırdığın sürüm, MonoCode'un sana nasıl ulaştığı ve gösterdiği paneller.",
  "settings.section.connections.label": "Bağlantılar",
  "settings.section.connections.description":
    "Makinelerini bağla ve agent'ları SSH üzerinden uzaktan çalıştır.",
  "settings.section.appearance.label": "Görünüm",
  "settings.section.appearance.description":
    "Tema, renk tonu, saydamlık, çalışma alanı düzeni ve konuşma arka planları.",
  "settings.section.keybindings.label": "Kısayollar",
  "settings.section.keybindings.description":
    "Çalışma alanının işlediği tüm kısayollar: uygulama menüsü ve tuş işleyicisi.",
  "settings.section.chat.label": "Sohbet",
  "settings.section.chat.description":
    "Konuşmaların nasıl görüneceği, mesaj alanının takip mesajıyla ne yapacağı, dosyaların nasıl kaydedileceği ve diff'lerin nasıl açılacağı.",
  "settings.section.providers.label": "Sağlayıcılar",
  "settings.section.providers.description":
    "Sağlayıcı hesapları, MonoCode'un çalıştırabildiği agent CLI'ları ve yeni oturumların başladığı model.",
  "settings.section.mcp.description":
    "Sağlayıcılar genelindeki MCP sunucularını bul ve bağlantılarını yönet.",
  "settings.section.skills.label": "Skill'ler",
  "settings.section.skills.description":
    "Proje, kişisel ve harness klasörlerindeki dosya skill'lerini keşfet ve yönet.",
  "settings.section.monos.label": "Monolar",
  "settings.section.monos.description":
    "Sekmelerinin yanındaki yerleşik agent ve hangi projelerde olduğu.",
  "settings.section.inbox.label": "Gelen kutusu",
  "settings.section.inbox.description":
    "Gelen kutusu hizmetlerini ve her proje için bildirim tercihlerini yönet.",
  "settings.section.archive.label": "Arşiv",
  "settings.section.archive.description":
    "Arşivlediğin projeler ve konuşmalar.",
  "settings.section.worktrees.label": "Worktree'ler",
  "settings.section.worktrees.description":
    "Her proje için ek worktree'leri yönet.",
  "settings.index.remote-machines": "Makinelerin",
  "settings.index.mcp-servers": "MCP sunucuları",
  "settings.index.monos-enabled": "Monoları göster",
  "settings.index.rail-monos-pinned": "Monoları simge çubuğuna sabitle",
  "settings.index.mono-list": "Monoların",
  "settings.index.project-worktrees": "Proje worktree'leri",
  "settings.index.update": "Sürüm",
  "settings.index.sounds": "Sesler",
  "settings.index.notifications": "Bildirimler",
  "settings.index.notes": "Notlar",
  "settings.index.quick-composer": "Hızlı mesaj alanı",
  "settings.index.working-agents": "Çalışan agent'lar",
  "settings.index.file-tabs": "Dosya sekmeleri",
  "settings.index.tab-animations": "Sekme animasyonları",
  "settings.index.close-to-tray": "Tepsiye kapat",
  "settings.index.theme": "Tema",
  "settings.index.accent-color": "Vurgu rengi",
  "settings.index.diff-colors": "Diff renkleri",
  "settings.index.hue": "Ton",
  "settings.index.saturation": "Doygunluk",
  "settings.index.dark-lightness": "Koyu mod parlaklığı",
  "settings.index.sidebar-opacity": "Kenar çubuğu opaklığı",
  "settings.index.blur": "Bulanıklık yarıçapı",
  "settings.index.main-pane-glass": "Ana bölme camı",
  "settings.index.interface-scale": "Arayüz ölçeği",
  "settings.index.collapsed-project-rail": "Daraltılmış proje çubuğu",
  "settings.index.show-excluded-files": "Hariç tutulan dosyaları göster",
  "settings.index.chat-background": "Sohbet arka planı",
  "settings.index.transcript-layout": "Konuşma düzeni",
  "settings.index.anchor-prompts": "Prompt'ları üste sabitle",
  "settings.index.follow-up": "Takip mesajı davranışı",
  "settings.index.model-controls": "Model kontrolleri",
  "settings.index.composer-autocorrect": "Otomatik düzeltme",
  "settings.index.composer-mascot": "Mesaj alanı maskotu",
  "settings.index.format-on-save": "Kaydederken biçimlendir",
  "settings.index.diff-view": "Diff görünümü",
  "settings.index.empty-session-games": "Boş oturum oyunları",
  "settings.index.agent-clis": "Agent CLI'ları",
  "settings.index.provider-accounts": "Sağlayıcı hesapları",
  "settings.index.show-remaining-usage": "Kalan kullanımı göster",
  "settings.index.mask-emails": "Hesap e-postalarını gizle",
  "settings.index.claude-hooks": "Claude Code hook'ları",
  "settings.index.idle-agents": "Boştaki agent'ları hazır tut",
  "settings.index.project-notifications": "Proje bildirimleri",
  "settings.index.show-archived": "Kenar çubuğunda arşivlenenleri göster",
  "settings.keybindings.invalidShortcut":
    "Bu tuş kombinasyonu geçerli bir kısayol değil",
  "settings.keybindings.activateNeedsNumber":
    "Tab: Activate 1–8 için 1 ile 8 arasında bir rakam tuşu gerekir",
  "settings.keybindings.alreadyUsed": "Zaten {owner} tarafından kullanılıyor",
  "settings.keybindings.saveFailed": "Kısayollar kaydedilemedi",
  "settings.keybindings.when.always": "Her zaman",
  "settings.keybindings.when.anywhere": "Her yerde",
  "settings.keybindings.when.projectWithMono": "Mono olan proje",
  "settings.keybindings.when.draftComposer": "Taslak oturum mesaj alanı",
  "settings.appearance.effect.none.label": "Yok",
  "settings.appearance.effect.dither.label": "Dither",
  "settings.appearance.effect.ascii.label": "ASCII",
  "settings.appearance.effect.halftone.label": "Yarım ton",
  "settings.appearance.effect.scanlines.label": "Tarama çizgileri",
  "settings.appearance.effect.gradient-blur.label": "Pus",
  "settings.appearance.effect.none.description": "Özgün görseli gösterir.",
  "settings.appearance.effect.dither.description":
    "Görseli dither uygulanmış bir renk paletiyle yeniden oluşturur.",
  "settings.appearance.effect.ascii.description":
    "Görseli siyah zemin üzerinde renkli karakterlerle yeniden oluşturur.",
  "settings.appearance.effect.halftone.description":
    "Görseli siyah zemin üzerinde renkli baskı noktalarıyla yeniden oluşturur.",
  "settings.appearance.effect.scanlines.description":
    "Belirgin bir yatay ekran çizgisi dokusu ekler.",
  "settings.appearance.effect.gradient-blur.description":
    "Görseli bulanıklaştırıp alttaki arka plana doğru soldurur.",
  "settings.mcp.scope.local": "Yerel",
  "settings.mcp.scope.project": "Proje",
  "settings.mcp.scope.user": "Kullanıcı",
  "settings.mcp.status.disabled": "Devre dışı",
  "settings.mcp.status.configured": "Yapılandırıldı",
  "settings.mcp.notObject": "Yapılandırma bir JSON nesnesi olmalı",
  "settings.mcp.add.title": "MCP sunucusu ekle",
  "settings.mcp.add.description":
    "Bir sunucu yapılandırması yapıştır ve nereye ekleneceğini seç.",
  "settings.mcp.add.provider": "Sağlayıcı",
  "settings.mcp.add.scope": "Kapsam",
  "settings.mcp.add.name": "Ad",
  "settings.mcp.add.nameOptional": "(mcpServers bloğu için isteğe bağlı)",
  "settings.mcp.add.config": "JSON yapılandırması",
  "settings.mcp.add.hint":
    "mcpServers bloğundan bir giriş yapıştır veya yukarıya bir ad yazıp tek bir sunucu nesnesi yapıştır.",
  "settings.mcp.add.cancel": "İptal",
  "settings.mcp.add.adding": "Ekleniyor…",
  "settings.mcp.add.submit": "Sunucu ekle",
  "settings.mcp.removePrompt":
    "{name} sunucusu {scope} kapsamından kaldırılsın mı?",
  "settings.mcp.removeTitle": "MCP sunucusunu kaldır",
  "settings.mcp.connections": "MCP bağlantıları",
  "settings.mcp.connectionsDescription":
    "Seçili proje ve sağlayıcı hesapların için yapılandırılmış sunucular.",
  "settings.mcp.refresh": "Yenile",
  "settings.mcp.showAvailable": "Mevcut sağlayıcıları göster",
  "settings.mcp.showAll": "Tüm sağlayıcıları göster",
  "settings.mcp.showingAll": "Tüm sağlayıcılar gösteriliyor",
  "settings.mcp.showingAvailable": "Mevcut sağlayıcılar gösteriliyor",
  "settings.mcp.filter": "MCP sunucularını sağlayıcıya göre filtrele",
  "settings.mcp.all": "Tümü",
  "settings.mcp.claudeUnavailable": "Claude bağlantı durumu alınamadı: {error}",
  "settings.mcp.checking": "Sunucular kontrol ediliyor…",
  "settings.mcp.empty": "Bu sağlayıcı için yapılandırılmış MCP sunucusu yok.",
  "settings.mcp.signIn": "Giriş yap",
  "settings.mcp.scopeToRemove": "{name} sunucusunun kaldırılacağı kapsam",
  "settings.mcp.remove": "Kaldır",
  "settings.mcp.showConfig": "Yapılandırmayı göster",
  "settings.mcp.footer":
    "Claude Code durumu kendi CLI'ından gelir. Diğer sağlayıcılar yapılandırılmış girişleri gösterir. Giriş yap, destekleniyorsa tarayıcını açar.",
  "settings.title": "Ayarlar",
  "settings.restoreDefaults": "Varsayılanları geri yükle",
  "settings.search.placeholder": "Ayarlarda ara",
  "settings.search.clear": "Ayar aramasını temizle",
  "settings.search.results": "Ayar arama sonuçları",
  "settings.search.none": "Eşleşen ayar yok",
  "settings.search.page": "Sayfa",
  "settings.general.alerts.title": "Uyarılar",
  "settings.general.alerts.description":
    "Başka bir yere bakarken MonoCode'un sana nasıl ulaşacağı.",
  "settings.general.sounds.description":
    "Proje etkinliği, biten turlar ve mevcut güncellemeler için kısa sesler. Proje bildirim kategorilerini Gelen kutusu ayarlarından seç. Anahtarlar ve biten bir turda Kopyala da ses çalar.",
  "settings.general.notifications.description":
    "Bir hatırlatıcının vakti geldiğinde veya bir agent başka bir oturumda ya da MonoCode arka plandayken bittiğinde veya girdi beklediğinde bildirim gönder. O oturumu açmak için bildirime tıkla.",
  "settings.general.notifications.unsupported": "Bu platformda kullanılamıyor",
  "settings.general.workspace.title": "Çalışma alanı",
  "settings.general.workspace.description":
    "Proje gezinmesinin ve çalışma alanı sekmelerinin nasıl davranacağı.",
  "settings.general.fileTabs.description":
    "Dosyaları etkin sohbetin yanında aç veya her dosyaya üst çubukta normal bir sekme ver. Üst çubuktaki dosyalar yine de bölünmüş bölmelerde birleştirilebilir.",
  "settings.general.fileTabs.pane": "Sohbetin yanında",
  "settings.general.fileTabs.workspace": "Üst çubuk",
  "settings.general.tabAnimations.description":
    "Sekmeler açılıp kapanırken animasyon göster. Sekme değişikliklerinin anında olması için bunu kapat.",
  "settings.general.notes.description":
    "Proje çubuğunda genel bir markdown not defteri. Biten bir turu konuşmadan kaydet, sonra @note ile an veya sohbete ekle.",
  "settings.general.quickComposer.description":
    "Herhangi bir uygulamada {shortcut} tuşlarına basarak üzerinde bir prompt aç ve MonoCode'a geçmeden oturum başlat. Kısayolu Kısayollar bölümünden değiştir. Return oturumu arka planda başlatır; ⌘Return başlatır ve öne getirir.",
  "settings.general.workingAgents.description":
    "İki veya daha fazla sohbet sürerken proje çubuğundaki bir kart bunları listeler, böylece projeler arasında geçiş yapabilirsin. Biten turlar, o oturumu açana kadar kalır.",
  "settings.general.closeToTray.description":
    "Bir pencereyi kapatmak çıkmak yerine onu sistem tepsisine gizler, böylece çalışan agent'lar devam eder. Tepsi simgesinden yeniden aç, gerçekten çıkmak için menüsünü kullan. Kapatmanın pencereyi sonlandırması için bunu kapat.",
  "settings.general.about": "Hakkında",
  "settings.chat.transcript.title": "Konuşma",
  "settings.chat.transcript.description":
    "Bir konuşmanın büyürken nasıl göründüğü.",
  "settings.chat.layout.description":
    "Tam genişlik, kullanıcı prompt'larını tüm genişliği kaplayan bir kart olarak tutar. Sohbet, onları bir mesajlaşma uygulaması gibi en fazla genişlikle sağa hizalar.",
  "settings.chat.layout.full": "Tam genişlik",
  "settings.chat.layout.chat": "Sohbet",
  "settings.chat.anchor.description":
    "Gönderdiğinde yeni prompt konuşmanın en üstünde durur ve yanıt alttaki boşluğa doğru büyür. Son mesajın mesaj alanının üzerinde durduğu klasik düzeni korumak için bunu kapat.",
  "settings.chat.composer.title": "Mesaj alanı",
  "settings.chat.composer.description":
    "Mesaj alanının yazdıklarınla ne yapacağı.",
  "settings.chat.followUp.description":
    "Takip mesajlarını etkin tur bitene kadar kuyruğa al veya etkin turu hemen yönlendir.",
  "settings.chat.followUp.queue": "Kuyruğa al",
  "settings.chat.followUp.steer": "Yönlendir",
  "settings.chat.modelControls.description":
    "Model seçeneklerini model menüsünün içinde değil, seçicinin yanında göster.",
  "settings.chat.modelControls.menu": "Menü",
  "settings.chat.modelControls.beside": "Yanında",
  "settings.chat.autocorrect.description":
    "Oturum ve Mono mesaj alanlarındaki prompt'larda yazım denetimi ve otomatik düzeltme yap. Metnin yazdığın gibi kalması için bunu kapat.",
  "settings.chat.editor.title": "Düzenleyici",
  "settings.chat.editor.description":
    "Çalışma alanı düzenleyicisinde bir dosyayı kaydettiğinde ne olacağı.",
  "settings.chat.formatOnSave.description":
    "Yazmadan önce desteklenen dosyalarda Prettier çalıştır. Kapalıyken tırnak stili dahil yazdığın metin korunur.",
  "settings.chat.review.title": "Kod incelemesi",
  "settings.chat.review.description":
    "Bir turun değişikliklerini okumaya gittiğinde nerede açılacağı.",
  "settings.chat.diffView.description":
    "Düzenleyici, çalışma ağacı değişikliklerini dosyanın içinde tutar. Birleşik, değişen tüm dosyaları yapışkan başlıklar ve daraltılmış değişmeyen satırlarla tek bir incelemede toplar.",
  "settings.chat.diffView.editor": "Düzenleyici",
  "settings.chat.diffView.unified": "Birleşik",
  "settings.chat.extras.title": "Ekstralar",
  "settings.chat.extras.description":
    "Boşta animasyon, başka bir şey yok. Hareketsiz bir çalışma alanı için ikisini de kapat.",
  "settings.chat.mascot.description":
    "Bir tur çalışırken proje maskotu mesaj alanı boyunca koşar, ilk seferde en alta kaydır düğmesine çarpar, sonra üzerinden atlar ve ara sıra bir jeton kapar.",
  "settings.chat.games.description":
    "Pac-man ve yılan, boş oturum ızgarasında kendi kendine oynar. Ekrandakini kontrol etmek için şeridin üzerine gel. Bölmenin hareketsiz kalması için bunu kapat.",
  "settings.inbox.github.description":
    "GitHub CLI üzerinden okunan pull request'ler, incelemeler ve issue'lar.",
  "settings.inbox.gitlab.description":
    "GitLab.com veya kendi barındırdığın bir örnekten merge request'ler.",
  "settings.inbox.ado.description":
    "ADO kuruluşundaki pull request'ler ve Boards iş öğeleri.",
  "settings.inbox.jira.description":
    "Seçtiğin projelerdeki Jira Cloud issue'ları.",
  "settings.inbox.linear.description":
    "Seçtiğin takımlardan sana atanan issue'lar.",
  "settings.inbox.connection": "Bağlantı",
  "settings.inbox.disconnect": "Bağlantıyı kes",
  "settings.inbox.connect": "Bağlan",
  "settings.inbox.saving": "Kaydediliyor",
  "settings.inbox.github.connected":
    "GitHub CLI kurulu ve kimliği doğrulanmış. MonoCode bunu GitHub Gelen kutusu öğeleri için kullanır.",
  "settings.inbox.github.signIn":
    "Bir terminalde gh auth login çalıştır, giriş akışını tamamla, sonra yeniden kontrol et.",
  "settings.inbox.github.install":
    "GitHub CLI'ı cli.github.com adresinden kur, bir terminalde gh auth login çalıştır, sonra yeniden kontrol et.",
  "settings.inbox.github.checking": "Kontrol ediliyor",
  "settings.inbox.github.statusConnected": "Bağlı",
  "settings.inbox.github.statusSignIn": "Giriş gerekli",
  "settings.inbox.github.statusMissing": "Kurulu değil",
  "settings.inbox.github.guide": "Kurulum kılavuzu",
  "settings.inbox.github.checkAgain": "Yeniden kontrol et",
  "settings.inbox.gitlab.connection":
    "GitLab.com'a veya kendi barındırdığın bir GitLab örneğine bağlan. API erişimi olan bir kişisel erişim token'ı kullan; token yerel olarak saklanır ve Bağlantıyı kes onu siler.",
  "settings.inbox.gitlab.url": "GitLab URL'si",
  "settings.inbox.gitlab.token": "GitLab erişim token'ı",
  "settings.inbox.ado.connection":
    "ADO kuruluşunu bir kişisel erişim token'ı ile bağla (yorumlar için Boards + Repos okuma ve yazma). Token yerel olarak saklanır ve Bağlantıyı kes onu siler.",
  "settings.inbox.ado.url": "Azure DevOps kuruluş URL'si",
  "settings.inbox.ado.token": "Azure DevOps kişisel erişim token'ı",
  "settings.inbox.linear.apiKey": "API anahtarı",
  "settings.inbox.linear.apiKeyDescription":
    "Linear → Settings → Security & Access bölümünde kişisel bir API anahtarı oluştur. Bağlantıyı kes onu siler.",
  "settings.inbox.linear.apiKeyLabel": "Linear API anahtarı",
  "settings.inbox.linear.teams": "Takımlar",
  "settings.inbox.linear.hiddenHint":
    "İşaretlenmeyen takımlar Gelen kutusunda görünmez.",
  "settings.update.available": "{version} sürümü mevcut.",
  "settings.update.downloadingPercent": "İndiriliyor %{progress}",
  "settings.update.downloading": "İndiriliyor…",
  "settings.update.checking": "Güncellemeler kontrol ediliyor…",
  "settings.update.current": "En son sürümü kullanıyorsun.",
  "settings.update.failed": "Güncelleme kontrolü başarısız oldu.",
  "settings.update.selfUpdates": "MonoCode kendini sürüm akışından günceller.",
  "settings.update.whatsNew": "Yenilikler",
  "settings.update.download": "İndir",
  "settings.update.check": "Güncellemeleri kontrol et",
  "settings.language.title": "Dil",
  "settings.language.description":
    "Sistem dilini kullan veya birini seç. Değişiklik hemen uygulanır.",
  "settings.language.system": "Sistem varsayılanı",
  "settings.index.language": "Dil",
  "settings.appearance.theme.description":
    "Koyu ve açık tema aynı renk tonunu paylaşır, bu yüzden aşağıdaki renk ayarları ikisine de uygulanır.",
  "settings.appearance.theme.row": "Sistem, işletim sistemi görünümünü izler.",
  "settings.appearance.theme.system": "Sistem",
  "settings.appearance.theme.dark": "Koyu",
  "settings.appearance.theme.light": "Açık",
  "settings.appearance.accent.description":
    "Mesaj alanındaki gönder düğmesi ve mesaj balonların için kullanılır.",
  "settings.appearance.accent.default": "Varsayılan",
  "settings.appearance.accent.blue": "Mavi",
  "settings.appearance.accent.violet": "Mor",
  "settings.appearance.accent.pink": "Pembe",
  "settings.appearance.accent.red": "Kırmızı",
  "settings.appearance.accent.orange": "Turuncu",
  "settings.appearance.accent.green": "Yeşil",
  "settings.appearance.diff.description":
    "Eklenen ve silinen satırların renkleri. Renk körü ve Yüksek kontrast, yeşil ve kırmızı yerine mavi ve turuncu kullanır; Yüksek kontrast daha güçlü tonlar ve metin ekler.",
  "settings.appearance.diff.default": "Varsayılan",
  "settings.appearance.diff.colorblind": "Renk körü",
  "settings.appearance.diff.highContrast": "Yüksek kontrast",
  "settings.appearance.color.title": "Renk",
  "settings.appearance.color.description":
    "Ton ve doygunluk her yüzeyi renklendirir. Parlaklık yalnızca koyu temayı etkiler.",
  "settings.appearance.hue.description":
    "Vurgular ve renklendirilmiş yüzeyler için temel ton.",
  "settings.appearance.saturation.description":
    "Tonun arayüzü ne kadar güçlü renklendireceği. Sıfır, arayüzü nötr tutar.",
  "settings.appearance.darkLightness.disabled":
    "Bu yalnızca koyu modu etkiler. Koyu mod değerin korunur.",
  "settings.appearance.darkLightness.description":
    "Koyu temanın temel parlaklığı. Düşük değerler daha koyudur; sıfır tam siyahtır.",
  "settings.appearance.translucency.title": "Saydamlık",
  "settings.appearance.translucency.disabled":
    "Açık mod her zaman opak bir pencere kullanır, bu yüzden bunlar kapalıdır. Koyu mod değerlerin korunur.",
  "settings.appearance.translucency.description":
    "Masaüstünün MonoCode'dan ne kadar görüneceği. Bulanıklık yükseldikçe birleştirmesi daha pahalıdır.",
  "settings.appearance.sidebarOpacity.description":
    "Proje çubuğuna ve diğer cam bölmelere uygulanır.",
  "settings.appearance.blur.description":
    "Pencerenin arkasındaki arka plan bulanıklığı.",
  "settings.appearance.mainPaneGlass.description":
    "Saydam görünümü oturumların ve düzenleyicilerin arkasındaki ana bölmeye de uygula.",
  "settings.appearance.layout.title": "Düzen",
  "settings.appearance.rail.description":
    "Proje gezinmesini kompakt bir simge çubuğu olarak açık tut veya çubuğu tamamen gizle.",
  "settings.appearance.rail.compact": "Simge çubuğu",
  "settings.appearance.rail.hidden": "Gizli",
  "settings.appearance.scale.description":
    "Tüm arayüzü yakınlaştır. Ctrl+=, Ctrl+- ve Ctrl+0 (macOS'ta Cmd) kısayollarını da kullanabilirsin.",
  "settings.appearance.excluded.description":
    "Git'in hariç tuttuğu dosya ve klasörleri, örneğin derleme çıktısı ve bağımlılıkları, gezginde göster.",
  "settings.appearance.background.description":
    "Sohbet bölmelerinin arkasında bir görsel. Bu cihazda kalır.",
  "settings.appearance.background.preview": "Boş sohbet önizlemesi, %{percent}",
  "settings.appearance.background.choose": "Görsel seç",
  "settings.appearance.background.change": "Değiştir",
  "settings.appearance.background.remove": "Kaldır",
  "settings.appearance.background.effect": "Arka plan efekti",
  "settings.appearance.background.showOn": "Şurada göster",
  "settings.appearance.background.showOnDescription":
    "Yalnızca boş oturumlar veya her konuşma.",
  "settings.appearance.background.showOnLabel": "Arka planı şurada göster",
  "settings.appearance.background.emptyOnly": "Yalnızca boş",
  "settings.appearance.background.allSessions": "Tüm oturumlar",
  "settings.appearance.background.emptyVisibility": "Boş sohbet görünürlüğü",
  "settings.appearance.background.emptyVisibilityDescription":
    "Sohbette mesaj olmadan önce arka planın gücü.",
  "settings.appearance.background.emptyVisibilityLabel":
    "Boş sohbet arka planı görünürlüğü",
  "settings.appearance.background.sessionVisibility": "Oturum görünürlüğü",
  "settings.appearance.background.sessionVisibilityDescription":
    "Konuşmada mesajlar olduğunda arka planın gücü.",
  "settings.appearance.background.sessionVisibilityLabel":
    "Oturum arka planı görünürlüğü",
  "settings.shortcut.change": "{name} kısayolunu değiştir",
  "settings.shortcut.reset": "{name} kısayolunu sıfırla",
  "settings.shortcut.record": "Kaydediliyor…",
  "settings.shortcut.disabled": "Devre dışı",
  "settings.shortcut.hint": "Del devre dışı bırakır · Esc iptal eder",
  "settings.shortcut.quickComposerNeedsModifier":
    "Quick Composer için genel kısayol olarak ⌘ veya Ctrl gerekir",
  "settings.keybindings.title": "Kısayollar",
  "settings.keybindings.description":
    "Yeni tuşları kaydetmek için bir kısayola tıkla. Devre dışı bırakmak için kaydederken Delete'e bas.",
  "settings.keybindings.count": {
    one: "{count} kısayol",
    other: "{count} kısayol",
  },
  "settings.keybindings.filter": "Filtrele",
  "settings.keybindings.filterLabel": "Kısayolları filtrele",
  "settings.keybindings.command": "Komut",
  "settings.keybindings.keybinding": "Kısayol",
  "settings.keybindings.when": "Ne zaman",
  "settings.keybindings.none": "Eşleşen kısayol yok",
  "settings.providers.binary.invalidCodex":
    "Codex CLI geçersiz bir sürüm döndürdü.",
  "settings.providers.binary.invalidOpencode":
    "OpenCode CLI geçersiz bir sürüm döndürdü.",
  "settings.providers.binary.tooOld":
    "OpenCode v{version} çok eski. v{minimum} veya daha yenisine yükselt.",
  "settings.providers.binary.saveFailed": "CLI yolu kaydedilemedi.",
  "settings.providers.binary.showDetailsRestart":
    "{title} CLI ayrıntılarını göster, yeniden başlatma gerekli",
  "settings.providers.binary.showDetails": "{title} CLI ayrıntılarını göster",
  "settings.providers.binary.pathTitle": "{title} CLI yolu",
  "settings.providers.binary.pathTitleRestart":
    "{title} CLI yolu — yeniden başlatma gerekli",
  "settings.providers.binary.details": "{title} CLI ayrıntıları",
  "settings.providers.binary.globalPath": "Genel yol",
  "settings.providers.binary.needsAttention": "Dikkat gerekli",
  "settings.providers.binary.restartRequired": "Yeniden başlatma gerekli",
  "settings.providers.binary.configured": "Yapılandırıldı",
  "settings.providers.binary.autoDetected": "Otomatik algılandı",
  "settings.providers.binary.cliPath": "CLI yolu",
  "settings.providers.binary.autoPathPlaceholder": "Otomatik algılanan yol",
  "settings.providers.binary.hint":
    "CLI çalıştırılabilir dosyasının mutlak yolunu gir. Değişiklikler MonoCode yeniden başlatıldıktan sonra uygulanır.",
  "settings.providers.binary.cancel": "İptal",
  "settings.providers.binary.useAuto": "Otomatik algılanan yolu kullan",
  "settings.providers.binary.save": "Yolu kaydet",
  "settings.providers.binary.unresolved": "CLI bulunamadı",
  "settings.providers.binary.checkingCli": "Seçili CLI kontrol ediliyor…",
  "settings.providers.binary.retryHint":
    "Bu CLI'ı kontrol etmek için yeniden dene",
  "settings.providers.binary.checkingVersion": "Sürüm kontrol ediliyor…",
  "settings.providers.binary.revealFailed": "CLI konumu açılamadı: {error}",
  "settings.providers.binary.retryConfiguredLabel":
    "{title} yapılandırılmış yolunu yeniden dene",
  "settings.providers.binary.retryAutoLabel":
    "{title} otomatik algılamayı yeniden dene",
  "settings.providers.binary.retryConfigured":
    "Yapılandırılmış yolu yeniden dene",
  "settings.providers.binary.retryAuto": "Otomatik algılamayı yeniden dene",
  "settings.providers.binary.openLocationLabel": "{title} CLI konumunu aç",
  "settings.providers.binary.openLocation": "Konumu aç",
  "settings.providers.binary.editLabel": "{title} CLI yolunu düzenle",
  "settings.providers.binary.edit": "Yolu düzenle",
  "settings.providers.global": "Genel",
  "settings.providers.scope": "Sağlayıcı varsayılanları kapsamı",
  "settings.providers.projectDescription":
    "Bu varsayılanlar yalnızca {project} için geçerlidir. Seçicide göster kapalı olan bir sağlayıcı, bu projede başlatılan yeni konuşmalardan da hariç tutulur. CLI yolları MonoCode için genel kalır.",
  "settings.providers.globalDescription":
    "Bir sağlayıcının CLI'ı PATH'inde bulunduğunda kurulu olarak listelenir. Kurulu olmayan CLI'lar listede kalır ancak model seçicinin dışında bırakılır; Seçicide göster kapalı olan kurulu CLI'lar da öyle. Sağlayıcının yanındaki model, yeni konuşmalarının başladığı modeldir; Varsayılan olarak kullan sağlayıcının kendisini seçer. CLI yolları MonoCode için geneldir ve her projeye uygulanır.",
  "settings.providers.advanced": "Gelişmiş",
  "settings.providers.idle.description":
    "Biten bir konuşma, agent CLI'ını birkaç dakika çalışır halde tutar, böylece bir takip mesajı anında başlar. Her biri birkaç yüz MB bellek tutabilir. Bu sayıyı aşınca en uzun süredir boşta olan erken durur; bir sonraki mesajında kısa bir yeniden başlatmanın ardından devam eder. Belleği az olan bir makinede Yok'u seç.",
  "settings.providers.idle.none": "Yok",
  "settings.providers.hooks.description":
    "settings.json dosyalarında yapılandırılan hook'ları (PreToolUse komut yeniden yazmaları, engellemeler, bildirimler ve diğerleri) Claude Code CLI'ın çalıştıracağı gibi çalıştır. Bir hook sorun çıkarıyorsa ve oturumu geri almak istiyorsan bunu kapat. Bir sonraki turda etkili olur.",
  "settings.providers.usage.title": "Kullanım ve gizlilik",
  "settings.providers.usage.remaining":
    "Kullanım göstergelerini kullanılan miktar yerine her limitte kalan miktarla doldur.",
  "settings.providers.usage.mask":
    "Hesap e-postalarını, birine tıklayana kadar Ayarlar'da ve kullanım açılır penceresinde bulanıklaştır; böylece ekran görüntülerinde görünmezler.",
  "settings.providers.accounts.saveFailed": "Bu hesap kaydedilemedi",
  "settings.providers.accounts.removeFailed": "Bu hesap kaldırılamadı",
  "settings.providers.accounts.removeConfirm":
    "“{label}” kaldırılsın mı? Kayıtlı kimlik bilgileri silinecek ve bu hesapla çalışan turlar duracak. Mevcut konuşmalar geçmişte kalır ancak hesap değiştirene kadar devam edemez.",
  "settings.providers.accounts.removeTitle": "{title} hesabını kaldır",
  "settings.providers.accounts.removeOk": "Hesabı kaldır",
  "settings.providers.accounts.cancel": "İptal",
  "settings.providers.accounts.title": "Hesaplar",
  "settings.providers.accounts.description":
    "Hesap profillerini destekleyen sağlayıcılar için yalıtılmış girişler oluştur. Hesap değiştirme, alt çubuktaki kullanım denetiminden yapılmaya devam eder.",
  "settings.providers.accounts.count": {
    one: "{count} hesap",
    other: "{count} hesap",
  },
  "settings.providers.accounts.add": "Hesap ekle",
  "settings.providers.accounts.cliProfile": "Sağlayıcı CLI profili",
  "settings.providers.accounts.isolated": "Yalıtılmış profil",
  "settings.providers.default": "Varsayılan",
  "settings.providers.accounts.renameLabel": "{label} adını değiştir",
  "settings.providers.accounts.renameTitle": "Hesabı yeniden adlandır",
  "settings.providers.accounts.removeLabel": "{label} hesabını kaldır",
  "settings.providers.accounts.name": "Hesap adı",
  "settings.providers.accounts.namePlaceholder": "İş veya Kişisel",
  "settings.providers.accounts.newLabel": "Yeni {title} hesabı",
  "settings.providers.accounts.renameEditLabel":
    "{title} hesabını yeniden adlandır",
  "settings.providers.accounts.waiting": "Tarayıcı bekleniyor…",
  "settings.providers.accounts.signInAdd": "Giriş yap ve ekle",
  "settings.providers.accounts.save": "Kaydet",
  "settings.providers.row.models": {
    one: "{count} model mevcut.",
    other: "{count} model mevcut.",
  },
  "settings.providers.row.modelLabel": "{title} modeli",
  "settings.providers.row.useDefault": "Varsayılan olarak kullan",
  "settings.providers.row.hiddenGlobally": "Genel olarak gizli",
  "settings.providers.row.showInPicker": "Seçicide göster",
  "settings.providers.row.showInPickerLabel":
    "{title} sağlayıcısını model seçicide göster",
  "settings.archive.projects.title": "Arşivlenen projeler",
  "settings.archive.projects.description":
    "Sohbetlerini koruyup kenar çubuğunda listelememek için bir projeyi çubuktan arşivle.",
  "settings.archive.projects.none": "Arşivlenen proje yok.",
  "settings.archive.restore": "Geri yükle",
  "settings.archive.delete": "Sil",
  "settings.archive.inProject": "{project} içinde arşivlenenler",
  "settings.archive.conversations": "Arşivlenen konuşmalar",
  "settings.archive.showDescription":
    "Arşivlenen konuşmaları etkin olanların yanında listede tut.",
  "settings.archive.openProject":
    "Arşivlenen konuşmalarını görmek için bir proje aç.",
  "settings.archive.noneInProject": "Bu projede arşivlenen konuşma yok.",
  "settings.archive.unarchive": "Arşivden çıkar",
  "settings.monos.enabled.description":
    "Proje çubuğunda sana ait agent'lar. Her biri verdiğin projelerde çalışır, önemli olanı hatırlar ve kendi başına yürüttüğü alışkanlıklar edinir. Gizlemek için bunu kapat.",
  "settings.monos.pinned.description":
    "Proje çubuğu simgelere daraltıldığında her Mono'yu proje seçicinin içinde değil, çubuğun en üstünde, bir ayırıcının üzerinde göster.",
  "settings.monos.menuBar.title": "Menü çubuğu simgesi",
  "settings.monos.menuBar.description":
    "macOS menü çubuğundan bir Mono ile sohbet et veya hızlı mesaj alanını aç. Simgeyi gizlemek için bunu kapat.",
  "settings.monos.list.description":
    "Her Mono'nun başlattığı yeni oturumların kenar çubuğunda görünüp görünmeyeceğini seç. Gizli oturumlar kayıtlı kalır ve Mono'nun sohbetinden açılabilir. Çubuktaki artı ile bir Mono ekle ve projelerini ayrıntılarından seç.",
  "settings.monos.none": "Henüz Mono yok.",
  "settings.monos.worksOn": "{projects} üzerinde çalışır",
  "settings.monos.noProjects": "Henüz proje yok",
  "settings.monos.showSessions":
    "Mono'nun başlattığı oturumları kenar çubuğunda göster",
  "settings.monos.showSessionsLabel":
    "{name} tarafından başlatılan oturumları kenar çubuğunda göster",
  "settings.monos.reset.label": "Mono'yu sıfırla",
  "settings.monos.reset.title": "{name} varsayılanlarına sıfırlansın mı?",
  "settings.monos.reset.body":
    "Ruhu varsayılana, adı {defaultName} olarak sıfırlanır. Ruhunda yaptığın değişiklikler geri alınamaz.",
  "settings.monos.reset.kept":
    "Konuşması, projeleri, belleği ve alışkanlıkları korunur.",
  "settings.monos.reset.failure": "Mono sıfırlanamadı.",
  "settings.monos.reset.title2": "Varsayılanlara sıfırla",
  "settings.monos.reset.aria": "{name} varsayılanlara sıfırla",
  "settings.general.notifications.permission": "İzin gerekli",
  "settings.general.notifications.openSystem": "Sistem Ayarları'nı aç",
};
