import type { Translations } from "./types";

export const en = {
  // Release notes / What's new
  "app.releaseNotes.ariaLabel": "Release notes",
  "app.releaseNotes.unavailable":
    "Release notes for this version are not available in this build.",
  "app.whatsNew.title": "What's new",
  "app.update.updatedTo": "Updated to {version}",
  "app.update.dismissNotification": "Dismiss update notification",
  "app.update.downloading": "Downloading…",
  "app.update.downloadingProgress": "Downloading {progress}%",
  "app.update.updateTo": "Update to {version}",
  // Settings rail
  "app.settingsRail.ariaLabel": "Settings",
  "app.settingsRail.back": "Back",
  // Release notes model
  "app.releaseNotes.title": "What's new in MonoCode {version}",
  // Updater
  "app.updater.packageHintDeb":
    "Download one .deb from {url} and run: sudo apt install ./MonoCode_X.Y.Z_amd64.deb\nReplace the file name with the one you downloaded.",
  "app.updater.packageHintRpm":
    "Download one .rpm from {url} and run: sudo dnf install ./MonoCode-X.Y.Z-1.x86_64.rpm\nReplace the file name with the one you downloaded.",
  "app.updater.latest": "You're on the latest version.",
  "app.updater.availablePrompt":
    "MonoCode {version} is available (you have {current}).{detail}\n\nInstall now?",
  "app.updater.availableTitle": "Update available",
  "app.updater.notConfigured":
    "Automatic updates aren't configured for this build.\n\nDownload releases at {url}",
  "app.updater.notAvailable":
    "Automatic updates aren't available for this install yet.\n\nDownload releases at {url}",
  "app.updater.checkFailed": "Couldn't check for updates.\n\n{error}",
  "app.updater.installFailed": "Couldn't install the update.\n\n{error}",
  // App lifecycle
  "app.lifecycle.quit": "Quit",
  "app.lifecycle.reloadPrompt": "Reload MonoCode and discard unsaved changes?",
  "app.lifecycle.reload": "Reload",
  "app.lifecycle.closeWindowPrompt":
    "Close this window and stop its running chats? Other windows will stay open.",
  "app.lifecycle.closeWindow": "Close window",
  // Mono habits
  "app.habits.approvalUnanswered": "Nobody answered its approval request.",
  "app.habits.timedOut": "It took too long and was stopped.",
  "app.habits.runUnfinished": "The run did not finish.",
  // Pi usage
  "app.piUsage.sendMessageHint":
    "Send a message so Pi can report its configured provider.",
  "app.piUsage.unsupportedProvider":
    "Subscription usage is not supported for this Pi provider.",
  "app.piUsage.unavailable": "Usage unavailable",
  "app.piUsage.sourceLabel": "Pi's saved OAuth account",
  "app.piUsage.refresh": "Refresh Pi usage",
  // Window controls
  "app.window.minimize": "Minimize",
  "app.window.minimizeWindow": "Minimize window",
  "app.window.restore": "Restore",
  "app.window.maximize": "Maximize",
  "app.window.restoreWindow": "Restore window",
  "app.window.maximizeWindow": "Maximize window",
  "app.window.close": "Close",
  // GitHub star prompt
  "app.githubStar.ariaLabel": "Star MonoCode on GitHub",
  "app.githubStar.starring": "Starring…",
  "app.githubStar.star": "Star on GitHub",
  "app.githubStar.dontShowAgain": "Don't show again",
  "app.githubStar.dismiss": "Dismiss GitHub star prompt",
  // Mono rail
  "app.monoRail.newReplies": {
    one: "{count} new reply",
    other: "{count} new replies",
  },
  "app.monoRail.title": "Monos",
  "app.monoRail.newMono": "New mono",
  "app.monoRail.noProjects": "No projects yet",
  "app.monoRail.options": "Mono options",
  "app.monoRail.optionsFor": "{name} options",
  "app.monoRail.name": "Mono name",
  "app.monoRail.backgroundImage": "Background image",
  "app.monoRail.delete": "Delete mono…",
  // Menu bar
  "app.menuBar.newTab": "New Tab",
  "app.menuBar.newTerminal": "New Terminal",
  "app.menuBar.newWindow": "New Window",
  "app.menuBar.autosave": "Autosave",
  "app.menuBar.openProject": "Open Project…",
  "app.menuBar.search": "Search…",
  "app.menuBar.goToFile": "Go to File…",
  "app.menuBar.findInFiles": "Find in Files…",
  "app.menuBar.closePane": "Close Pane",
  "app.menuBar.closeOtherTabs": "Close Other Tabs",
  "app.menuBar.closeAllTabs": "Close All Tabs",
  "app.menuBar.checkForUpdates": "Check for Updates…",
  "app.menuBar.toggleSidebar": "Toggle Sidebar",
  "app.menuBar.toggleSessionSidebar": "Toggle Session Sidebar",
  "app.menuBar.inbox": "Inbox",
  "app.menuBar.notes": "Notes",
  "app.menuBar.toggleTerminal": "Toggle Terminal",
  "app.menuBar.switchModel": "Switch Model…",
  "app.menuBar.toggleChanges": "Toggle Changes",
  "app.menuBar.zoomIn": "Zoom In",
  "app.menuBar.zoomOut": "Zoom Out",
  "app.menuBar.resetZoom": "Reset Zoom",
  "app.menuBar.file": "File",
  "app.menuBar.view": "View",
  "app.menuBar.terminal": "Terminal",
  "app.menuBar.menuAriaLabel": "{menu} menu",
  // Project menu
  "app.projectMenu.revealFinder": "Reveal in Finder",
  "app.projectMenu.revealExplorer": "Reveal in File Explorer",
  "app.projectMenu.openContainingFolder": "Open Containing Folder",
  "app.projectMenu.newGroup": "New group…",
  "app.projectMenu.ungrouped": "Ungrouped",
  "app.projectMenu.backgroundImage": "Background image",
  "app.projectMenu.moveToGroup": "Move to group",
  "app.projectMenu.unpin": "Unpin project",
  "app.projectMenu.pin": "Pin project",
  "app.projectMenu.openInEditor": "Open in editor",
  "app.projectMenu.lookingForEditors": "Looking for editors…",
  "app.projectMenu.noEditors": "No supported editors found",
  "app.projectMenu.mute": "Mute notifications",
  "app.projectMenu.notificationSettings": "Notification settings…",
  "app.projectMenu.archive": "Archive",
  "app.projectMenu.delete": "Delete",
  "app.projectMenu.resume": "Resume notifications",
  "app.projectMenu.deleteGroup": "Delete group",
  "app.projectMenu.deleteGroupDescription": "Projects will become ungrouped",
  "app.projectMenu.saveNotificationsFailed":
    "Could not save notification preferences. Please try again.",
  "app.projectMenu.groupActions": "Project group actions",
  "app.projectMenu.muteProjectNotifications": "Mute project notifications",
  // Usage footer
  "app.usageFooter.terminal": "Terminal",
  "app.usageFooter.newTerminal": "New Terminal ({shortcut})",
  "app.usageFooter.providerUsage": "Provider usage",
  "app.usageFooter.terminals": "Terminals",
  "app.usageFooter.session": "Session",
  "app.usageFooter.removedAccount": "This conversation uses a removed account",
  "app.usageFooter.codexResetFailed": "Could not use Codex reset",
  "app.usageFooter.signInFailed": "Could not complete sign-in",
  "app.usageFooter.refreshUsage": "Refresh usage",
  "app.usageFooter.signInRequired": "{provider} sign-in required",
  "app.usageFooter.signIn": "sign in",
  "app.usageFooter.signInDialog": "{provider} sign-in",
  "app.usageFooter.processIn": '"{process}" in {terminal}',
  "app.usageFooter.hideProcess": "Hide {process}",
  "app.usageFooter.showProcess": "Show {process}",
  "app.usageFooter.hideRunningTerminals": "Hide running terminals",
  "app.usageFooter.terminalsRunning": {
    one: "{count} terminals are running processes",
    other: "{count} terminals are running processes",
  },
  "app.usageFooter.runningTerminals": "Running terminals",
  // Usage provider chip
  "app.usageChip.removedAccount": "Removed account",
  "app.usageChip.useResetFailed": "Could not use this reset",
  "app.usageChip.usageDetailsFor": "{provider} usage details",
  "app.usageChip.notConnected": "Not connected",
  "app.usageChip.loadingUsage": "Loading usage…",
  "app.usageChip.usageDetails": "Usage details",
  "app.usageChip.notConnectedShort": "not connected",
  "app.usageChip.providerUsage": "{provider} usage",
  "app.usageChip.switchProviderAccount": "Switch {provider} account",
  "app.usageChip.updating": "Updating",
  "app.usageChip.refreshFailed":
    "Couldn’t refresh. Showing the last available snapshot.",
  "app.usageChip.extraUsageBalance": "Extra usage balance",
  "app.usageChip.switchAccountFrom": "Switch account from {account}",
  "app.usageChip.switch": "Switch",
  "app.usageChip.backToUsage": "Back to usage",
  "app.usageChip.providerAccounts": "{provider} accounts",
  "app.usageChip.accountsHint":
    "Each conversation stays pinned to the account that started it.",
  "app.usageChip.addAccount": "Add account",
  "app.usageChip.manageAccounts": "Manage accounts…",
  "app.usageChip.outOfUsage": "Out of usage",
  "app.usageChip.runningLow": "Running low",
  "app.usageChip.switchTo": "switch to",
  "app.usageChip.addAccountFailed": "Could not add this account",
  "app.usageChip.backToAccounts": "Back to accounts",
  "app.usageChip.addProviderAccount": "Add {provider} account",
  "app.usageChip.addAccountHint":
    "Give this account a local name, then finish sign-in in your browser.",
  "app.usageChip.accountName": "Account name",
  "app.usageChip.accountNamePlaceholder": "Work or Personal",
  "app.usageChip.waitingForBrowser": "Waiting for browser…",
  "app.usageChip.signInAndAdd": "Sign in and add account",
  "app.usageChip.dailyLimit": "Daily limit",
  "app.usageChip.fiveHourLimit": "5-hour limit",
  "app.usageChip.weeklyLimit": "Weekly limit",
  "app.usageChip.monthlyLimit": "Monthly limit",
  "app.usageChip.windowLimit": "{window} limit",
  "app.usageChip.remaining": "remaining",
  "app.usageChip.used": "used",
  "app.usageChip.window": "{window} window",
  "app.usageChip.bankedResets": "Banked resets",
  "app.usageChip.resetsAvailable": {
    one: "{count} reset available",
    other: "{count} resets available",
  },
  "app.usageChip.availableBankedResets": "Available banked resets",
  "app.usageChip.bankedReset": "Banked reset {index}",
  "app.usageChip.expiryNotProvided": "Expiry not provided",
  "app.usageChip.expiresNow": "Expires now",
  "app.usageChip.expiresIn": "Expires in {duration}",
  "app.usageChip.applying": "Applying…",
  "app.usageChip.useReset": "Use reset",
  "app.usageChip.spendResetConfirm": "Spend this reset now?",
  "app.usageChip.cancel": "Cancel",
  "app.usageChip.confirm": "Confirm",
  "app.usageChip.rateLimitDetails": "Rate-limit details",
  "app.usageChip.updatedJustNow": "Updated just now",
  "app.usageChip.updatedMinutesAgo": "Updated {minutes}m ago",
  "app.usageChip.updatedHoursAgo": "Updated {hours}h ago",
  "app.usageChip.outcomeReset": "Codex usage was reset.",
  "app.usageChip.outcomeNothingToReset": "There’s no active usage to reset.",
  "app.usageChip.outcomeNoCredit": "No banked resets are available.",
  "app.usageChip.outcomeAlreadyUsed": "That reset was already used.",
  "app.usageChip.expired": "expired",
  // Title bar
  "app.titleBar.sessionCount": {
    one: "{count} session",
    other: "{count} sessions",
  },
  "app.titleBar.newSession": "New session",
  "app.titleBar.unsavedChanges": "Unsaved changes",
  "app.titleBar.responseComplete": "Response complete",
  "app.titleBar.closeTab": "Close Tab",
  "app.titleBar.closeNamed": "Close {name}",
  "app.titleBar.scrollTabsLeft": "Scroll tabs left",
  "app.titleBar.scrollTabsRight": "Scroll tabs right",
  "app.titleBar.developmentBuild": "Development build",
  "app.titleBar.development": "Development",
  "app.titleBar.back": "Back ({shortcut})",
  "app.titleBar.forward": "Forward ({shortcut})",
  "app.titleBar.toggleProjects": "Toggle Projects",
  "app.titleBar.toggleSidebar": "Toggle Sidebar ({shortcut})",
  "app.titleBar.closeOtherTabs": "Close Other Tabs",
  "app.titleBar.closeTabsRight": "Close Tabs to the Right",
  "app.titleBar.closeTabsLeft": "Close Tabs to the Left",
  "app.titleBar.archive": "Archive",
  "app.titleBar.archiveAll": "All {count} conversations in this tab",
  "app.titleBar.delete": "Delete",
  "app.titleBar.deleteAll":
    "Permanently delete all {count} conversations in this tab",
  "app.titleBar.hideOverview": "Hide overview",
  "app.titleBar.showOverview": "Show overview",
  "app.titleBar.overview": "Overview",
  "app.titleBar.monoSettings": "{name} settings",
  "app.titleBar.inbox": "Inbox",
  "app.titleBar.notes": "Notes",
  "app.titleBar.settings": "Settings ({shortcut})",
  "app.titleBar.toggleSessionSidebar": "Toggle Session Sidebar ({shortcut})",
  "app.titleBar.noProject": "No project",
  "app.titleBar.tabActions": "Tab actions for {name}",
  // Project rail
  "app.projectRail.projects": "Projects",
  "app.projectRail.search": "Search",
  "app.projectRail.searchShortcut": "Search ({shortcut})",
  "app.projectRail.inbox": "Inbox",
  "app.projectRail.inboxNewItems": "Inbox, new items",
  "app.projectRail.notes": "Notes",
  "app.projectRail.automations": "Automations",
  "app.projectRail.pinned": "Pinned",
  "app.projectRail.groups": "Groups",
  "app.projectRail.noProjects": "No projects yet",
  "app.projectRail.settings": "Settings",
  "app.projectRail.settingsShortcut": "Settings ({shortcut})",
  "app.projectRail.resize": "Resize project sidebar",
  "app.projectRail.newGroup": "New project group",
  "app.projectRail.projectCount": {
    one: "{count} project",
    other: "{count} projects",
  },
  "app.projectRail.groupOptions": "Group options",
  "app.projectRail.groupOptionsFor": "{name} group options",
  "app.projectRail.machineNotConnected":
    "Machine not connected on this computer",
  "app.projectRail.connecting": "Connecting",
  "app.projectRail.connected": "Connected",
  "app.projectRail.reconnecting": "Reconnecting",
  "app.projectRail.remoteTitle": "{path} on {machine} ({connection})",
  "app.projectRail.anotherMachine": "another machine",
  "app.projectRail.nameOnMachine": "{name} on {machine}",
  "app.projectRail.projectOptions": "Project options",
  "app.projectRail.uncommitted": "{label} uncommitted",
  "app.projectRail.working": "Working",
  "app.projectRail.filesChanged": {
    one: "{count} file changed",
    other: "{count} files changed",
  },
  "app.projectRail.workingLower": "working",
  "app.projectRail.openProject": "Open project",
  "app.projectRail.openFolder": "Open folder…",
  "app.projectRail.openFolderOnMachine": "Open folder on a machine…",
  // Sidebar
  "app.sidebar.tabSessions": "Sessions",
  "app.sidebar.tabInbox": "Inbox",
  "app.sidebar.tabExplorer": "Explorer",
  "app.sidebar.tabChanges": "Changes",
  "app.sidebar.connectToChange":
    "Connect this project's machine to change its sessions.",
  "app.sidebar.updateFailed": "Could not update this session.\n\n{error}",
  "app.sidebar.connectToDelete":
    "Connect this project's machine to delete its sessions.",
  "app.sidebar.deleteConfirm": {
    one: "Delete this conversation? This can’t be undone.",
    other: "Delete {count} conversations? This can’t be undone.",
  },
  "app.sidebar.deleteFailed": "Could not delete this session.\n\n{error}",
  // Sidebar menus
  "app.sidebar.rename": "Rename",
  "app.sidebar.ungroup": "Ungroup",
  "app.sidebar.cancelReminder": "Cancel reminder",
  "app.sidebar.multipleReminderTimes": "Multiple reminder times",
  "app.sidebar.unpin": "Unpin",
  "app.sidebar.pin": "Pin",
  "app.sidebar.copySessionId": "Copy session ID",
  "app.sidebar.harnessSessionId": "Harness session ID",
  "app.sidebar.monocodeSessionId": "MonoCode session ID",
  "app.sidebar.editWorkItemLink": "Edit GitHub issue or PR link…",
  "app.sidebar.linkWorkItem": "Link GitHub issue or PR…",
  "app.sidebar.remindMe": "Remind me",
  "app.sidebar.newFolder": "New folder",
  "app.sidebar.addToFolder": "Add to {name}",
  "app.sidebar.removeFromFolders": "Remove from folders",
  "app.sidebar.removeFromFolder": "Remove from folder",
  "app.sidebar.unarchive": "Unarchive",
  "app.sidebar.archive": "Archive",
  "app.sidebar.delete": "Delete",
  // Sidebar sessions
  "app.sidebar.searchPlaceholder": "Search conversations...",
  "app.sidebar.searchConversations": "Search conversations",
  "app.sidebar.workspace": "Workspace",
  "app.sidebar.noProjectFolder": "No project folder",
  "app.sidebar.filterSessions": "Filter sessions",
  "app.sidebar.loadFailed": "Couldn’t load sessions",
  "app.sidebar.noMatching": "No matching sessions",
  "app.sidebar.noFilterMatch": "No sessions match these filters",
  "app.sidebar.machineNotConnected":
    "This project’s machine isn’t connected on this computer.",
  "app.sidebar.emptyMessage": "Sessions you start will show up here",
  "app.sidebar.reminders": "Reminders",
  "app.sidebar.pinned": "Pinned",
  "app.sidebar.newSession": "New session",
  "app.sidebar.settings": "Settings",
  "app.sidebar.settingsShortcut": "Settings ({shortcut})",
  "app.sidebar.selectedSessionActions": {
    one: "{count} selected session actions",
    other: "{count} selected session actions",
  },
  "app.sidebar.sessionActions": "Session actions",
  "app.sidebar.folderActions": "Folder actions",
  // Sidebar rails
  "app.sidebar.resize": "Resize sidebar",
  "app.sidebar.newTab": "New tab ({shortcut})",
  "app.sidebar.search": "Search ({shortcut})",
  "app.sidebar.inboxNewItems": "Inbox, new items",
  "app.sidebar.notes": "Notes",
  "app.sidebar.automations": "Automations",
  "app.sidebar.projectShortcuts": "Project shortcuts",
  "app.sidebar.expandProjects": "Expand projects",
  "app.sidebar.goToFile": "Go to File ({shortcut})",
  "app.sidebar.newSessionShortcut": "New session ({shortcut})",
  // Session card
  "app.sidebar.needsInput": "Needs input",
  "app.sidebar.needApproval": "Need approval",
  "app.sidebar.working": "Working...",
  "app.sidebar.done": "Done",
  "app.sidebar.draft": "Draft",
  "app.sidebar.linkedPrUpdated": "Linked PR updated since this session",
  "app.sidebar.linkedIssueUpdated": "Linked issue updated since this session",
  "app.sidebar.linkedWorkItemUpdated": "Linked work item updated",
  "app.sidebar.openWorkItemTitle":
    "Open {kind} #{number} beside this session ({mod}-click for GitHub)",
  "app.sidebar.openWorkItem": "Open {kind} #{number}",
  "app.sidebar.startedByAutomation": "Started by an automation",
  "app.sidebar.orchestratorLabel": {
    one: "Orchestrator, {count} subagent, {done} done",
    other: "Orchestrator, {count} subagents, {done} done",
  },
  "app.sidebar.subagents": "Subagents",
  "app.sidebar.subagentsDone": "{done}/{total} done",
  // Relative time
  "app.sidebar.relNow": "now",
  "app.sidebar.relMinutes": "{minutes}m",
  "app.sidebar.relHoursMinutes": "{hours}h {minutes}m",
  "app.sidebar.relHours": "{hours}h",
  "app.sidebar.relDays": "{days}d",
  "app.monoRail.idle": "idle",
  // App: unsaved changes
  "app.unsaved.closeTab": "Close this tab with unsaved files?",
  "app.unsaved.closeTabs": "Close these tabs with unsaved files?",
  "app.unsaved.closeOtherTabs": "Close other tabs with unsaved files?",
  "app.unsaved.closeConversation":
    "Close this conversation with unsaved files?",
  "app.unsaved.closeAllFiles": "Close all open files with unsaved changes?",
  "app.unsaved.closeAllTabs": "Close all tabs with unsaved files?",
  // App: sessions
  "app.session.askTitle": "Ask · {title}",
  // App: errors
  "app.errors.monoConversationUnavailable":
    "This Mono's conversation is no longer available.",
  "app.errors.conversationResetting":
    "This conversation is already being reset.",
  "app.errors.sessionUnavailable": "This session is no longer available.",
  "app.errors.conversationUnavailable":
    "This conversation is no longer available.",
  "app.errors.reminderNeedsMessage":
    "Send a message in this conversation before setting a reminder.",
  // App: worktrees
  "app.worktree.alreadyDeleting": "This worktree is already being deleted.",
  "app.worktree.beingSelected":
    "A session is selecting this worktree. Try deleting it again once selection finishes.",
  "app.worktree.unavailable": "This worktree is no longer available.",
  "app.worktree.moveSessionsFirst":
    "Move or delete the sessions using this worktree first.",
  "app.worktree.waitForSessions":
    "Wait for these sessions to finish changing before deleting the worktree.",
  // App: removal
  "app.unsaved.archiveConversation":
    "Archive this conversation with unsaved files?",
  "app.unsaved.deleteConversation":
    "Delete this conversation with unsaved files?",
  "app.removal.worktreeKept":
    "The session was deleted. Its worktree was kept.\n\n{error}\n\nYou can manage it in Settings → Worktrees.",
  "app.removal.archiveFailed":
    "Could not archive this conversation.\n\n{detail}",
  "app.removal.deleteFailed": "Could not delete this conversation.\n\n{detail}",
  "app.removal.unarchiveFailed":
    "Could not unarchive this conversation.\n\n{error}",
  "app.removal.linkUpdateFailed":
    "Could not update this conversation's GitHub link.\n\n{error}",
  "app.removal.deleteMonoConfirm":
    "Delete {name}? Its conversation will be deleted and its habits stop. This can’t be undone.",
  "app.removal.deleteSelectedConfirm": {
    one: "Delete {count} selected conversations? This can’t be undone.",
    other: "Delete {count} selected conversations? This can’t be undone.",
  },
  "app.worktree.waitBeforeSwitch":
    "Wait for this session to finish before changing working copies.",
  "app.worktree.beingDeleted":
    "This worktree is being deleted. Select another working copy.",
  "app.worktree.clearQueued":
    "Clear queued messages before changing working copies.",
  "app.worktree.stopOrchestration":
    "Stop this orchestration run before changing working copies.",
  "app.worktree.refreshPicker":
    "This worktree is no longer available. Refresh the picker.",
  "app.worktree.sessionChanged":
    "The session changed. Try selecting the working copy again.",
  // App: submit
  "app.submit.stopOrchestrationFirst":
    "Stop the current orchestration run before preparing another proposal.",
  "app.submit.removedAccount":
    "This conversation uses a removed provider account. Switch accounts from the usage control to start a new conversation.",
  "app.submit.operatorOutsideOrchestration":
    "Use /operator from a regular session turn, outside an orchestration run.",
  "app.submit.operatorNewTurn":
    "/operator starts a new turn after the current turn finishes.",
  "app.submit.noFollowUpMidTurn":
    "{harness} cannot take a follow-up mid-turn — wait for this turn to finish, or stop it first.",
  "app.submit.steerFailed": "{harness} could not steer the active turn",
  "app.submit.projectFolderFailed": "The project folder could not be opened.",
  "app.submit.harnessNotConnected":
    "{harness} is not connected yet — install and sign in to that provider, then retry.",
  // App: orchestration
  "app.orchestration.repairFailed": "The lead could not repair the proposal.",
  "app.orchestration.planningFailed": "The lead could not finish planning.",
  // App: automations
  "app.automation.habitRunFailed": "The habit's run could not start.",
  "app.automation.sessionRunFailed":
    "The selected agent session could not start this run.",
  // App: queue
  "app.queue.deliveryUnavailable":
    "Delivery is unavailable. Retry when the agent is ready.",
  "app.queue.planningAfterTurn":
    "Orchestration planning will start after the current turn finishes.",
  // App: side questions
  "app.sideQuestion.needsProject":
    "A project working directory is required for this question.",
  "app.sideQuestion.codexModelUnavailable":
    "The selected Codex model is unavailable.",
  "app.sideQuestion.turnUnavailable":
    "The completed turn is no longer available.",
  "app.sideQuestion.emptyAnswer": "{harness} returned an empty side answer.",
  "app.sideQuestion.answerFailed":
    "{harness} could not answer this side question.",
  // App: compaction
  "app.compact.unsupported":
    "{harness} does not support manual context compaction.",
  "app.compact.compacting": "Compacting context…",
  "app.compact.compacted": "Compacted context",
  "app.compact.failed": "{harness} could not compact this context",
  "app.orchestration.waitForProposal":
    "Wait for the proposal to finish before confirming.",
  "app.orchestration.leadModelChanged":
    "The lead model has changed. Switch back to the model shown on this card, or generate a new proposal.",
  // App: CI repair
  "app.ciRepair.chooseProject": "Choose a local project for this PR first.",
  "app.ciRepair.chooseChat": "Choose a chat from this project.",
  "app.ciRepair.chatBusy":
    "This chat is busy. Choose another chat or start a new one.",
  "app.ciRepair.sessionTitle": "Fix CI #{number}: {title}",
  "app.session.newRemote": "New remote session",
  "app.sidebar.searchLabel": "Search",
  "app.sidebar.settingsLabel": "Settings",
  "app.sidebar.sessionsNewReplies": "Sessions, new replies",
} as const;

export const tr: Translations<typeof en> = {
  // Release notes / What's new
  "app.releaseNotes.ariaLabel": "Sürüm notları",
  "app.releaseNotes.unavailable": "Bu sürümün notları bu derlemede bulunmuyor.",
  "app.whatsNew.title": "Yenilikler",
  "app.update.updatedTo": "{version} sürümüne güncellendi",
  "app.update.dismissNotification": "Güncelleme bildirimini kapat",
  "app.update.downloading": "İndiriliyor…",
  "app.update.downloadingProgress": "İndiriliyor %{progress}",
  "app.update.updateTo": "{version} sürümüne güncelle",
  // Settings rail
  "app.settingsRail.ariaLabel": "Ayarlar",
  "app.settingsRail.back": "Geri",
  // Release notes model
  "app.releaseNotes.title": "MonoCode {version} sürümündeki yenilikler",
  // Updater
  "app.updater.packageHintDeb":
    "{url} adresinden bir .deb dosyası indirip şunu çalıştırın: sudo apt install ./MonoCode_X.Y.Z_amd64.deb\nDosya adını indirdiğiniz dosyanın adıyla değiştirin.",
  "app.updater.packageHintRpm":
    "{url} adresinden bir .rpm dosyası indirip şunu çalıştırın: sudo dnf install ./MonoCode-X.Y.Z-1.x86_64.rpm\nDosya adını indirdiğiniz dosyanın adıyla değiştirin.",
  "app.updater.latest": "En güncel sürümü kullanıyorsunuz.",
  "app.updater.availablePrompt":
    "MonoCode {version} kullanılabilir (sizdeki sürüm: {current}).{detail}\n\nŞimdi yüklensin mi?",
  "app.updater.availableTitle": "Güncelleme mevcut",
  "app.updater.notConfigured":
    "Bu derleme için otomatik güncellemeler yapılandırılmamış.\n\nSürümleri {url} adresinden indirebilirsiniz",
  "app.updater.notAvailable":
    "Bu kurulum için otomatik güncellemeler henüz kullanılamıyor.\n\nSürümleri {url} adresinden indirebilirsiniz",
  "app.updater.checkFailed": "Güncellemeler denetlenemedi.\n\n{error}",
  "app.updater.installFailed": "Güncelleme yüklenemedi.\n\n{error}",
  // App lifecycle
  "app.lifecycle.quit": "Çık",
  "app.lifecycle.reloadPrompt":
    "MonoCode yeniden yüklensin ve kaydedilmemiş değişiklikler silinsin mi?",
  "app.lifecycle.reload": "Yeniden yükle",
  "app.lifecycle.closeWindowPrompt":
    "Bu pencere kapatılsın ve içinde çalışan sohbetler durdurulsun mu? Diğer pencereler açık kalacak.",
  "app.lifecycle.closeWindow": "Pencereyi kapat",
  // Mono habits
  "app.habits.approvalUnanswered": "Onay isteğine kimse yanıt vermedi.",
  "app.habits.timedOut": "Çok uzun sürdüğü için durduruldu.",
  "app.habits.runUnfinished": "Çalıştırma tamamlanmadı.",
  // Pi usage
  "app.piUsage.sendMessageHint":
    "Pi'nin yapılandırılmış sağlayıcısını bildirebilmesi için bir mesaj gönderin.",
  "app.piUsage.unsupportedProvider":
    "Bu Pi sağlayıcısı için abonelik kullanımı desteklenmiyor.",
  "app.piUsage.unavailable": "Kullanım bilgisi yok",
  "app.piUsage.sourceLabel": "Pi'nin kayıtlı OAuth hesabı",
  "app.piUsage.refresh": "Pi kullanımını yenile",
  // Window controls
  "app.window.minimize": "Simge durumuna küçült",
  "app.window.minimizeWindow": "Pencereyi simge durumuna küçült",
  "app.window.restore": "Önceki boyut",
  "app.window.maximize": "Ekranı kapla",
  "app.window.restoreWindow": "Pencereyi önceki boyutuna getir",
  "app.window.maximizeWindow": "Pencereyi ekranı kaplayacak şekilde büyüt",
  "app.window.close": "Kapat",
  // GitHub star prompt
  "app.githubStar.ariaLabel": "MonoCode'a GitHub'da yıldız ver",
  "app.githubStar.starring": "Yıldız veriliyor…",
  "app.githubStar.star": "GitHub'da yıldız ver",
  "app.githubStar.dontShowAgain": "Bir daha gösterme",
  "app.githubStar.dismiss": "GitHub yıldız önerisini kapat",
  // Mono rail
  "app.monoRail.newReplies": {
    one: "{count} yeni yanıt",
    other: "{count} yeni yanıt",
  },
  "app.monoRail.title": "Monolar",
  "app.monoRail.newMono": "Yeni Mono",
  "app.monoRail.noProjects": "Henüz proje yok",
  "app.monoRail.options": "Mono seçenekleri",
  "app.monoRail.optionsFor": "{name} seçenekleri",
  "app.monoRail.name": "Mono adı",
  "app.monoRail.backgroundImage": "Arka plan görseli",
  "app.monoRail.delete": "Mono'yu sil…",
  // Menu bar
  "app.menuBar.newTab": "Yeni sekme",
  "app.menuBar.newTerminal": "Yeni terminal",
  "app.menuBar.newWindow": "Yeni pencere",
  "app.menuBar.autosave": "Otomatik kaydet",
  "app.menuBar.openProject": "Proje aç…",
  "app.menuBar.search": "Ara…",
  "app.menuBar.goToFile": "Dosyaya git…",
  "app.menuBar.findInFiles": "Dosyalarda bul…",
  "app.menuBar.closePane": "Bölmeyi kapat",
  "app.menuBar.closeOtherTabs": "Diğer sekmeleri kapat",
  "app.menuBar.closeAllTabs": "Tüm sekmeleri kapat",
  "app.menuBar.checkForUpdates": "Güncellemeleri denetle…",
  "app.menuBar.toggleSidebar": "Kenar çubuğunu aç/kapat",
  "app.menuBar.toggleSessionSidebar": "Oturum kenar çubuğunu aç/kapat",
  "app.menuBar.inbox": "Gelen kutusu",
  "app.menuBar.notes": "Notlar",
  "app.menuBar.toggleTerminal": "Terminali aç/kapat",
  "app.menuBar.switchModel": "Model değiştir…",
  "app.menuBar.toggleChanges": "Değişiklikleri aç/kapat",
  "app.menuBar.zoomIn": "Yakınlaştır",
  "app.menuBar.zoomOut": "Uzaklaştır",
  "app.menuBar.resetZoom": "Yakınlaştırmayı sıfırla",
  "app.menuBar.file": "Dosya",
  "app.menuBar.view": "Görünüm",
  "app.menuBar.terminal": "Terminal",
  "app.menuBar.menuAriaLabel": "{menu} menüsü",
  // Project menu
  "app.projectMenu.revealFinder": "Finder'da göster",
  "app.projectMenu.revealExplorer": "Dosya Gezgini'nde göster",
  "app.projectMenu.openContainingFolder": "İçeren klasörü aç",
  "app.projectMenu.newGroup": "Yeni grup…",
  "app.projectMenu.ungrouped": "Grupsuz",
  "app.projectMenu.backgroundImage": "Arka plan görseli",
  "app.projectMenu.moveToGroup": "Gruba taşı",
  "app.projectMenu.unpin": "Projeyi sabitlemeyi kaldır",
  "app.projectMenu.pin": "Projeyi sabitle",
  "app.projectMenu.openInEditor": "Düzenleyicide aç",
  "app.projectMenu.lookingForEditors": "Düzenleyiciler aranıyor…",
  "app.projectMenu.noEditors": "Desteklenen düzenleyici bulunamadı",
  "app.projectMenu.mute": "Bildirimleri sessize al",
  "app.projectMenu.notificationSettings": "Bildirim ayarları…",
  "app.projectMenu.archive": "Arşivle",
  "app.projectMenu.delete": "Sil",
  "app.projectMenu.resume": "Bildirimleri sürdür",
  "app.projectMenu.deleteGroup": "Grubu sil",
  "app.projectMenu.deleteGroupDescription": "Projeler grupsuz kalacak",
  "app.projectMenu.saveNotificationsFailed":
    "Bildirim tercihleri kaydedilemedi. Lütfen tekrar deneyin.",
  "app.projectMenu.groupActions": "Proje grubu işlemleri",
  "app.projectMenu.muteProjectNotifications": "Proje bildirimlerini sessize al",
  // Usage footer
  "app.usageFooter.terminal": "Terminal",
  "app.usageFooter.newTerminal": "Yeni terminal ({shortcut})",
  "app.usageFooter.providerUsage": "Sağlayıcı kullanımı",
  "app.usageFooter.terminals": "Terminaller",
  "app.usageFooter.session": "Oturum",
  "app.usageFooter.removedAccount":
    "Bu konuşma kaldırılmış bir hesabı kullanıyor",
  "app.usageFooter.codexResetFailed": "Codex sıfırlaması kullanılamadı",
  "app.usageFooter.signInFailed": "Oturum açma tamamlanamadı",
  "app.usageFooter.refreshUsage": "Kullanımı yenile",
  "app.usageFooter.signInRequired": "{provider} için oturum açılması gerekiyor",
  "app.usageFooter.signIn": "oturum aç",
  "app.usageFooter.signInDialog": "{provider} oturum açma",
  "app.usageFooter.processIn": '{terminal} içinde "{process}"',
  "app.usageFooter.hideProcess": "{process} gizle",
  "app.usageFooter.showProcess": "{process} göster",
  "app.usageFooter.hideRunningTerminals": "Çalışan terminalleri gizle",
  "app.usageFooter.terminalsRunning": {
    one: "{count} terminalde çalışan işlem var",
    other: "{count} terminalde çalışan işlem var",
  },
  "app.usageFooter.runningTerminals": "Çalışan terminaller",
  // Usage provider chip
  "app.usageChip.removedAccount": "Kaldırılmış hesap",
  "app.usageChip.useResetFailed": "Bu sıfırlama kullanılamadı",
  "app.usageChip.usageDetailsFor": "{provider} kullanım ayrıntıları",
  "app.usageChip.notConnected": "Bağlı değil",
  "app.usageChip.loadingUsage": "Kullanım yükleniyor…",
  "app.usageChip.usageDetails": "Kullanım ayrıntıları",
  "app.usageChip.notConnectedShort": "bağlı değil",
  "app.usageChip.providerUsage": "{provider} kullanımı",
  "app.usageChip.switchProviderAccount": "{provider} hesabını değiştir",
  "app.usageChip.updating": "Güncelleniyor",
  "app.usageChip.refreshFailed":
    "Yenilenemedi. Son alınan anlık görüntü gösteriliyor.",
  "app.usageChip.extraUsageBalance": "Ek kullanım bakiyesi",
  "app.usageChip.switchAccountFrom": "{account} hesabından geçiş yap",
  "app.usageChip.switch": "Değiştir",
  "app.usageChip.backToUsage": "Kullanıma dön",
  "app.usageChip.providerAccounts": "{provider} hesapları",
  "app.usageChip.accountsHint": "Her konuşma, onu başlatan hesaba bağlı kalır.",
  "app.usageChip.addAccount": "Hesap ekle",
  "app.usageChip.manageAccounts": "Hesapları yönet…",
  "app.usageChip.outOfUsage": "Kullanım hakkı bitti",
  "app.usageChip.runningLow": "Azalıyor",
  "app.usageChip.switchTo": "şuna geç",
  "app.usageChip.addAccountFailed": "Bu hesap eklenemedi",
  "app.usageChip.backToAccounts": "Hesaplara dön",
  "app.usageChip.addProviderAccount": "{provider} hesabı ekle",
  "app.usageChip.addAccountHint":
    "Bu hesaba yerel bir ad verin, ardından tarayıcınızda oturum açmayı tamamlayın.",
  "app.usageChip.accountName": "Hesap adı",
  "app.usageChip.accountNamePlaceholder": "İş veya Kişisel",
  "app.usageChip.waitingForBrowser": "Tarayıcı bekleniyor…",
  "app.usageChip.signInAndAdd": "Oturum aç ve hesap ekle",
  "app.usageChip.dailyLimit": "Günlük limit",
  "app.usageChip.fiveHourLimit": "5 saatlik limit",
  "app.usageChip.weeklyLimit": "Haftalık limit",
  "app.usageChip.monthlyLimit": "Aylık limit",
  "app.usageChip.windowLimit": "{window} limiti",
  "app.usageChip.remaining": "kaldı",
  "app.usageChip.used": "kullanıldı",
  "app.usageChip.window": "{window} aralığı",
  "app.usageChip.bankedResets": "Biriktirilmiş sıfırlamalar",
  "app.usageChip.resetsAvailable": {
    one: "{count} sıfırlama kullanılabilir",
    other: "{count} sıfırlama kullanılabilir",
  },
  "app.usageChip.availableBankedResets":
    "Kullanılabilir biriktirilmiş sıfırlamalar",
  "app.usageChip.bankedReset": "Biriktirilmiş sıfırlama {index}",
  "app.usageChip.expiryNotProvided": "Son kullanma tarihi belirtilmemiş",
  "app.usageChip.expiresNow": "Şimdi sona eriyor",
  "app.usageChip.expiresIn": "{duration} içinde sona eriyor",
  "app.usageChip.applying": "Uygulanıyor…",
  "app.usageChip.useReset": "Sıfırlamayı kullan",
  "app.usageChip.spendResetConfirm": "Bu sıfırlama şimdi kullanılsın mı?",
  "app.usageChip.cancel": "İptal",
  "app.usageChip.confirm": "Onayla",
  "app.usageChip.rateLimitDetails": "Kullanım sınırı ayrıntıları",
  "app.usageChip.updatedJustNow": "Az önce güncellendi",
  "app.usageChip.updatedMinutesAgo": "{minutes} dk önce güncellendi",
  "app.usageChip.updatedHoursAgo": "{hours} sa önce güncellendi",
  "app.usageChip.outcomeReset": "Codex kullanımı sıfırlandı.",
  "app.usageChip.outcomeNothingToReset": "Sıfırlanacak etkin kullanım yok.",
  "app.usageChip.outcomeNoCredit":
    "Kullanılabilir biriktirilmiş sıfırlama yok.",
  "app.usageChip.outcomeAlreadyUsed": "Bu sıfırlama zaten kullanılmış.",
  "app.usageChip.expired": "süresi doldu",
  // Title bar
  "app.titleBar.sessionCount": {
    one: "{count} oturum",
    other: "{count} oturum",
  },
  "app.titleBar.newSession": "Yeni oturum",
  "app.titleBar.unsavedChanges": "Kaydedilmemiş değişiklikler",
  "app.titleBar.responseComplete": "Yanıt tamamlandı",
  "app.titleBar.closeTab": "Sekmeyi kapat",
  "app.titleBar.closeNamed": "{name} kapat",
  "app.titleBar.scrollTabsLeft": "Sekmeleri sola kaydır",
  "app.titleBar.scrollTabsRight": "Sekmeleri sağa kaydır",
  "app.titleBar.developmentBuild": "Geliştirme derlemesi",
  "app.titleBar.development": "Geliştirme",
  "app.titleBar.back": "Geri ({shortcut})",
  "app.titleBar.forward": "İleri ({shortcut})",
  "app.titleBar.toggleProjects": "Projeleri aç/kapat",
  "app.titleBar.toggleSidebar": "Kenar çubuğunu aç/kapat ({shortcut})",
  "app.titleBar.closeOtherTabs": "Diğer sekmeleri kapat",
  "app.titleBar.closeTabsRight": "Sağdaki sekmeleri kapat",
  "app.titleBar.closeTabsLeft": "Soldaki sekmeleri kapat",
  "app.titleBar.archive": "Arşivle",
  "app.titleBar.archiveAll": "Bu sekmedeki {count} konuşmanın tümü",
  "app.titleBar.delete": "Sil",
  "app.titleBar.deleteAll":
    "Bu sekmedeki {count} konuşmanın tümünü kalıcı olarak sil",
  "app.titleBar.hideOverview": "Genel bakışı gizle",
  "app.titleBar.showOverview": "Genel bakışı göster",
  "app.titleBar.overview": "Genel bakış",
  "app.titleBar.monoSettings": "{name} ayarları",
  "app.titleBar.inbox": "Gelen kutusu",
  "app.titleBar.notes": "Notlar",
  "app.titleBar.settings": "Ayarlar ({shortcut})",
  "app.titleBar.toggleSessionSidebar":
    "Oturum kenar çubuğunu aç/kapat ({shortcut})",
  "app.titleBar.noProject": "Proje yok",
  "app.titleBar.tabActions": "{name} için sekme işlemleri",
  // Project rail
  "app.projectRail.projects": "Projeler",
  "app.projectRail.search": "Ara",
  "app.projectRail.searchShortcut": "Ara ({shortcut})",
  "app.projectRail.inbox": "Gelen kutusu",
  "app.projectRail.inboxNewItems": "Gelen kutusu, yeni öğeler",
  "app.projectRail.notes": "Notlar",
  "app.projectRail.automations": "Otomasyonlar",
  "app.projectRail.pinned": "Sabitlenenler",
  "app.projectRail.groups": "Gruplar",
  "app.projectRail.noProjects": "Henüz proje yok",
  "app.projectRail.settings": "Ayarlar",
  "app.projectRail.settingsShortcut": "Ayarlar ({shortcut})",
  "app.projectRail.resize": "Proje kenar çubuğunu yeniden boyutlandır",
  "app.projectRail.newGroup": "Yeni proje grubu",
  "app.projectRail.projectCount": {
    one: "{count} proje",
    other: "{count} proje",
  },
  "app.projectRail.groupOptions": "Grup seçenekleri",
  "app.projectRail.groupOptionsFor": "{name} grup seçenekleri",
  "app.projectRail.machineNotConnected": "Makine bu bilgisayarda bağlı değil",
  "app.projectRail.connecting": "Bağlanıyor",
  "app.projectRail.connected": "Bağlı",
  "app.projectRail.reconnecting": "Yeniden bağlanıyor",
  "app.projectRail.remoteTitle": "{machine} üzerinde {path} ({connection})",
  "app.projectRail.anotherMachine": "başka bir makine",
  "app.projectRail.nameOnMachine": "{machine} üzerinde {name}",
  "app.projectRail.projectOptions": "Proje seçenekleri",
  "app.projectRail.uncommitted": "{label} commit edilmemiş",
  "app.projectRail.working": "Çalışıyor",
  "app.projectRail.filesChanged": {
    one: "{count} dosya değişti",
    other: "{count} dosya değişti",
  },
  "app.projectRail.workingLower": "çalışıyor",
  "app.projectRail.openProject": "Proje aç",
  "app.projectRail.openFolder": "Klasör aç…",
  "app.projectRail.openFolderOnMachine": "Bir makinede klasör aç…",
  // Sidebar
  "app.sidebar.tabSessions": "Oturumlar",
  "app.sidebar.tabInbox": "Gelen kutusu",
  "app.sidebar.tabExplorer": "Gezgin",
  "app.sidebar.tabChanges": "Değişiklikler",
  "app.sidebar.connectToChange":
    "Bu projenin oturumlarını değiştirmek için makinesini bağlayın.",
  "app.sidebar.updateFailed": "Bu oturum güncellenemedi.\n\n{error}",
  "app.sidebar.connectToDelete":
    "Bu projenin oturumlarını silmek için makinesini bağlayın.",
  "app.sidebar.deleteConfirm": {
    one: "Bu konuşma silinsin mi? Bu işlem geri alınamaz.",
    other: "{count} konuşma silinsin mi? Bu işlem geri alınamaz.",
  },
  "app.sidebar.deleteFailed": "Bu oturum silinemedi.\n\n{error}",
  // Sidebar menus
  "app.sidebar.rename": "Yeniden adlandır",
  "app.sidebar.ungroup": "Grubu dağıt",
  "app.sidebar.cancelReminder": "Hatırlatıcıyı iptal et",
  "app.sidebar.multipleReminderTimes": "Birden fazla hatırlatma zamanı",
  "app.sidebar.unpin": "Sabitlemeyi kaldır",
  "app.sidebar.pin": "Sabitle",
  "app.sidebar.copySessionId": "Oturum kimliğini kopyala",
  "app.sidebar.harnessSessionId": "Harness oturum kimliği",
  "app.sidebar.monocodeSessionId": "MonoCode oturum kimliği",
  "app.sidebar.editWorkItemLink": "GitHub issue veya PR bağlantısını düzenle…",
  "app.sidebar.linkWorkItem": "GitHub issue veya PR bağla…",
  "app.sidebar.remindMe": "Bana hatırlat",
  "app.sidebar.newFolder": "Yeni klasör",
  "app.sidebar.addToFolder": "{name} klasörüne ekle",
  "app.sidebar.removeFromFolders": "Klasörlerden çıkar",
  "app.sidebar.removeFromFolder": "Klasörden çıkar",
  "app.sidebar.unarchive": "Arşivden çıkar",
  "app.sidebar.archive": "Arşivle",
  "app.sidebar.delete": "Sil",
  // Sidebar sessions
  "app.sidebar.searchPlaceholder": "Konuşmalarda ara...",
  "app.sidebar.searchConversations": "Konuşmalarda ara",
  "app.sidebar.workspace": "Çalışma alanı",
  "app.sidebar.noProjectFolder": "Proje klasörü yok",
  "app.sidebar.filterSessions": "Oturumları filtrele",
  "app.sidebar.loadFailed": "Oturumlar yüklenemedi",
  "app.sidebar.noMatching": "Eşleşen oturum yok",
  "app.sidebar.noFilterMatch": "Bu filtrelerle eşleşen oturum yok",
  "app.sidebar.machineNotConnected":
    "Bu projenin makinesi bu bilgisayarda bağlı değil.",
  "app.sidebar.emptyMessage": "Başlattığınız oturumlar burada görünecek",
  "app.sidebar.reminders": "Hatırlatıcılar",
  "app.sidebar.pinned": "Sabitlenenler",
  "app.sidebar.newSession": "Yeni oturum",
  "app.sidebar.settings": "Ayarlar",
  "app.sidebar.settingsShortcut": "Ayarlar ({shortcut})",
  "app.sidebar.selectedSessionActions": {
    one: "Seçili {count} oturum için işlemler",
    other: "Seçili {count} oturum için işlemler",
  },
  "app.sidebar.sessionActions": "Oturum işlemleri",
  "app.sidebar.folderActions": "Klasör işlemleri",
  // Sidebar rails
  "app.sidebar.resize": "Kenar çubuğunu yeniden boyutlandır",
  "app.sidebar.newTab": "Yeni sekme ({shortcut})",
  "app.sidebar.search": "Ara ({shortcut})",
  "app.sidebar.inboxNewItems": "Gelen kutusu, yeni öğeler",
  "app.sidebar.notes": "Notlar",
  "app.sidebar.automations": "Otomasyonlar",
  "app.sidebar.projectShortcuts": "Proje kısayolları",
  "app.sidebar.expandProjects": "Projeleri genişlet",
  "app.sidebar.goToFile": "Dosyaya git ({shortcut})",
  "app.sidebar.newSessionShortcut": "Yeni oturum ({shortcut})",
  // Session card
  "app.sidebar.needsInput": "Yanıt bekliyor",
  "app.sidebar.needApproval": "Onay gerekiyor",
  "app.sidebar.working": "Çalışıyor...",
  "app.sidebar.done": "Tamam",
  "app.sidebar.draft": "Taslak",
  "app.sidebar.linkedPrUpdated": "Bağlı PR bu oturumdan sonra güncellendi",
  "app.sidebar.linkedIssueUpdated":
    "Bağlı issue bu oturumdan sonra güncellendi",
  "app.sidebar.linkedWorkItemUpdated": "Bağlı iş öğesi güncellendi",
  "app.sidebar.openWorkItemTitle":
    "{kind} #{number} bu oturumun yanında aç (GitHub için {mod}-tıkla)",
  "app.sidebar.openWorkItem": "{kind} #{number} aç",
  "app.sidebar.startedByAutomation": "Bir otomasyon tarafından başlatıldı",
  "app.sidebar.orchestratorLabel": {
    one: "Orkestratör, {count} alt ajan, {done} tamamlandı",
    other: "Orkestratör, {count} alt ajan, {done} tamamlandı",
  },
  "app.sidebar.subagents": "Alt ajanlar",
  "app.sidebar.subagentsDone": "{done}/{total} tamamlandı",
  // Relative time
  "app.sidebar.relNow": "şimdi",
  "app.sidebar.relMinutes": "{minutes}dk",
  "app.sidebar.relHoursMinutes": "{hours}sa {minutes}dk",
  "app.sidebar.relHours": "{hours}sa",
  "app.sidebar.relDays": "{days}g",
  "app.monoRail.idle": "boşta",
  // App: unsaved changes
  "app.unsaved.closeTab":
    "Kaydedilmemiş dosyalar içeren bu sekme kapatılsın mı?",
  "app.unsaved.closeTabs":
    "Kaydedilmemiş dosyalar içeren bu sekmeler kapatılsın mı?",
  "app.unsaved.closeOtherTabs":
    "Kaydedilmemiş dosyalar içeren diğer sekmeler kapatılsın mı?",
  "app.unsaved.closeConversation":
    "Kaydedilmemiş dosyalar varken bu konuşma kapatılsın mı?",
  "app.unsaved.closeAllFiles":
    "Kaydedilmemiş değişiklikler içeren tüm açık dosyalar kapatılsın mı?",
  "app.unsaved.closeAllTabs":
    "Kaydedilmemiş dosyalar içeren tüm sekmeler kapatılsın mı?",
  // App: sessions
  "app.session.askTitle": "Sor · {title}",
  // App: errors
  "app.errors.monoConversationUnavailable":
    "Bu Mono'nun konuşması artık kullanılamıyor.",
  "app.errors.conversationResetting": "Bu konuşma zaten sıfırlanıyor.",
  "app.errors.sessionUnavailable": "Bu oturum artık kullanılamıyor.",
  "app.errors.conversationUnavailable": "Bu konuşma artık kullanılamıyor.",
  "app.errors.reminderNeedsMessage":
    "Hatırlatıcı ayarlamadan önce bu konuşmaya bir mesaj gönderin.",
  // App: worktrees
  "app.worktree.alreadyDeleting": "Bu worktree zaten siliniyor.",
  "app.worktree.beingSelected":
    "Bir oturum bu worktree'yi seçiyor. Seçim tamamlanınca silmeyi tekrar deneyin.",
  "app.worktree.unavailable": "Bu worktree artık kullanılamıyor.",
  "app.worktree.moveSessionsFirst":
    "Önce bu worktree'yi kullanan oturumları taşıyın veya silin.",
  "app.worktree.waitForSessions":
    "Worktree'yi silmeden önce bu oturumlardaki değişikliklerin bitmesini bekleyin.",
  // App: removal
  "app.unsaved.archiveConversation":
    "Kaydedilmemiş dosyalar varken bu konuşma arşivlensin mi?",
  "app.unsaved.deleteConversation":
    "Kaydedilmemiş dosyalar varken bu konuşma silinsin mi?",
  "app.removal.worktreeKept":
    "Oturum silindi. worktree'si korundu.\n\n{error}\n\nBunu Ayarlar → Worktree'ler bölümünden yönetebilirsiniz.",
  "app.removal.archiveFailed": "Bu konuşma arşivlenemedi.\n\n{detail}",
  "app.removal.deleteFailed": "Bu konuşma silinemedi.\n\n{detail}",
  "app.removal.unarchiveFailed": "Bu konuşma arşivden çıkarılamadı.\n\n{error}",
  "app.removal.linkUpdateFailed":
    "Bu konuşmanın GitHub bağlantısı güncellenemedi.\n\n{error}",
  "app.removal.deleteMonoConfirm":
    "{name} silinsin mi? Konuşması silinecek ve alışkanlıkları duracak. Bu işlem geri alınamaz.",
  "app.removal.deleteSelectedConfirm": {
    one: "Seçili {count} konuşma silinsin mi? Bu işlem geri alınamaz.",
    other: "Seçili {count} konuşma silinsin mi? Bu işlem geri alınamaz.",
  },
  "app.worktree.waitBeforeSwitch":
    "Çalışma kopyasını değiştirmeden önce bu oturumun bitmesini bekleyin.",
  "app.worktree.beingDeleted":
    "Bu worktree siliniyor. Başka bir çalışma kopyası seçin.",
  "app.worktree.clearQueued":
    "Çalışma kopyasını değiştirmeden önce sıradaki mesajları temizleyin.",
  "app.worktree.stopOrchestration":
    "Çalışma kopyasını değiştirmeden önce bu orkestrasyon çalıştırmasını durdurun.",
  "app.worktree.refreshPicker":
    "Bu worktree artık kullanılamıyor. Seçiciyi yenileyin.",
  "app.worktree.sessionChanged":
    "Oturum değişti. Çalışma kopyasını yeniden seçmeyi deneyin.",
  // App: submit
  "app.submit.stopOrchestrationFirst":
    "Başka bir öneri hazırlamadan önce mevcut orkestrasyon çalıştırmasını durdurun.",
  "app.submit.removedAccount":
    "Bu konuşma kaldırılmış bir sağlayıcı hesabını kullanıyor. Yeni bir konuşma başlatmak için kullanım denetiminden hesap değiştirin.",
  "app.submit.operatorOutsideOrchestration":
    "/operator komutunu orkestrasyon çalıştırması dışında, normal bir oturum turunda kullanın.",
  "app.submit.operatorNewTurn":
    "/operator, mevcut tur bittikten sonra yeni bir tur başlatır.",
  "app.submit.noFollowUpMidTurn":
    "{harness} tur sırasında ek mesaj alamaz; bu turun bitmesini bekleyin veya önce durdurun.",
  "app.submit.steerFailed": "{harness} etkin turu yönlendiremedi",
  "app.submit.projectFolderFailed": "Proje klasörü açılamadı.",
  "app.submit.harnessNotConnected":
    "{harness} henüz bağlı değil; bu sağlayıcıyı yükleyip oturum açın, ardından yeniden deneyin.",
  // App: orchestration
  "app.orchestration.repairFailed": "Lider öneriyi onaramadı.",
  "app.orchestration.planningFailed": "Lider planlamayı tamamlayamadı.",
  // App: automations
  "app.automation.habitRunFailed": "Alışkanlığın çalıştırması başlatılamadı.",
  "app.automation.sessionRunFailed":
    "Seçilen ajan oturumu bu çalıştırmayı başlatamadı.",
  // App: queue
  "app.queue.deliveryUnavailable":
    "İletim kullanılamıyor. Ajan hazır olduğunda yeniden deneyin.",
  "app.queue.planningAfterTurn":
    "Orkestrasyon planlaması mevcut tur bittikten sonra başlayacak.",
  // App: side questions
  "app.sideQuestion.needsProject":
    "Bu soru için bir proje çalışma dizini gerekiyor.",
  "app.sideQuestion.codexModelUnavailable":
    "Seçilen Codex modeli kullanılamıyor.",
  "app.sideQuestion.turnUnavailable": "Tamamlanan tur artık kullanılamıyor.",
  "app.sideQuestion.emptyAnswer": "{harness} boş bir yan yanıt döndürdü.",
  "app.sideQuestion.answerFailed": "{harness} bu yan soruyu yanıtlayamadı.",
  // App: compaction
  "app.compact.unsupported":
    "{harness} elle bağlam sıkıştırmayı desteklemiyor.",
  "app.compact.compacting": "Bağlam sıkıştırılıyor…",
  "app.compact.compacted": "Bağlam sıkıştırıldı",
  "app.compact.failed": "{harness} bu bağlamı sıkıştıramadı",
  "app.orchestration.waitForProposal":
    "Onaylamadan önce önerinin tamamlanmasını bekleyin.",
  "app.orchestration.leadModelChanged":
    "Lider model değişti. Bu kartta gösterilen modele geri dönün veya yeni bir öneri oluşturun.",
  // App: CI repair
  "app.ciRepair.chooseProject": "Önce bu PR için yerel bir proje seçin.",
  "app.ciRepair.chooseChat": "Bu projeden bir sohbet seçin.",
  "app.ciRepair.chatBusy":
    "Bu sohbet meşgul. Başka bir sohbet seçin veya yenisini başlatın.",
  "app.ciRepair.sessionTitle": "CI'ı düzelt #{number}: {title}",
  "app.session.newRemote": "Yeni uzak oturum",
  "app.sidebar.searchLabel": "Ara",
  "app.sidebar.settingsLabel": "Ayarlar",
  "app.sidebar.sessionsNewReplies": "Oturumlar, yeni yanıtlar",
};
