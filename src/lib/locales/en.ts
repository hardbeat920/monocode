/**
 * English locale template for MonoCode.
 *
 * Every key maps to itself. This file serves as a reference for translators —
 * it shows all available keys and the default English text.
 */

export const en: Record<string, string> = {
  // ── General / Common ──────────────────────────────────────────────────
  "MonoCode": "MonoCode",
  "Close": "Close",
  "Cancel": "Cancel",
  "Delete": "Delete",
  "Remove": "Remove",
  "Copy": "Copy",
  "Cut": "Cut",
  "Paste": "Paste",
  "Rename": "Rename",
  "Duplicate": "Duplicate",
  "Save": "Save",
  "Back": "Back",
  "Search": "Search",
  "Settings": "Settings",
  "Create": "Create",
  "Retry": "Retry",
  "Allow": "Allow",
  "Deny": "Deny",
  "Continue": "Continue",
  "Skip": "Skip",
  "Loading…": "Loading…",
  "Loading": "Loading",
  "No results": "No results",
  "Untitled": "Untitled",
  "Question": "Question",
  "Approval": "Approval",
  "Send": "Send",
  "Stop": "Stop",
  "Plan": "Plan",
  "Resume": "Resume",
  "Done": "Done",
  "Working": "Working",
  "Open": "Open",
  "Closed": "Closed",
  "Draft": "Draft",
  "Merged": "Merged",
  "New": "New",
  "Default": "Default",
  "Refresh": "Refresh",
  "Archive": "Archive",
  "Unarchive": "Unarchive",
  "Restore": "Restore",
  "Download": "Download",

  // ── TitleBar.tsx ──────────────────────────────────────────────────────
  "New session": "New session",
  "{0} sessions": "{0} sessions",
  "Unsaved changes": "Unsaved changes",
  "{0} — MonoCode": "{0} — MonoCode",
  "{0} — {1} — MonoCode": "{0} — {1} — MonoCode",
  "Development build": "Development build",
  "Development": "Development",
  "Back ({0}[)": "Back ({0}[)",
  "Forward ({0}])": "Forward ({0}])",
  "Toggle Projects": "Toggle Projects",
  "Toggle Sidebar ({0}B)": "Toggle Sidebar ({0}B)",
  "Inbox": "Inbox",
  "Notes": "Notes",
  "Go to File ({0}P)": "Go to File ({0}P)",
  "New session ({0}T)": "New session ({0}T)",
  "New Terminal": "New Terminal",
  "New Terminal ({0}`)": "New Terminal ({0}`)",
  "Terminal": "Terminal",
  "Settings ({0},)": "Settings ({0},)",
  "No project": "No project",
  "Scroll tabs left": "Scroll tabs left",
  "Scroll tabs right": "Scroll tabs right",
  "Close Tab": "Close Tab",
  "Close {0}": "Close {0}",

  // ── MenuBar.tsx ───────────────────────────────────────────────────────
  "New Tab": "New Tab",
  "New Window": "New Window",
  "Open Project…": "Open Project…",
  "Search…": "Search…",
  "Go to File…": "Go to File…",
  "Find in Files…": "Find in Files…",
  "Close Pane": "Close Pane",
  "Close Other Tabs": "Close Other Tabs",
  "Check for Updates…": "Check for Updates…",
  "Toggle Sidebar": "Toggle Sidebar",
  "Switch Model…": "Switch Model…",
  "Toggle Changes": "Toggle Changes",
  "Toggle Terminal": "Toggle Terminal",
  "File": "File",
  "View": "View",

  // ── Sidebar.tsx ───────────────────────────────────────────────────────
  "Sessions": "Sessions",
  "Explorer": "Explorer",
  "Changes": "Changes",
  "Search conversations...": "Search conversations...",
  "Search conversations": "Search conversations",
  "Filter sessions": "Filter sessions",
  "No project folder": "No project folder",
  "Couldn't load sessions": "Couldn't load sessions",
  "No matching sessions": "No matching sessions",
  "No sessions match these filters": "No sessions match these filters",
  "Sessions you start will show up here": "Sessions you start will show up here",
  "Session actions": "Session actions",
  "Folder actions": "Folder actions",
  "Workspace": "Workspace",
  "Go to File": "Go to File",
  "{0} files changed": "{0} files changed",
  "1 file changed": "1 file changed",
  "{0} uncommitted": "{0} uncommitted",
  "Resize sidebar": "Resize sidebar",
  "Working...": "Working...",
  "Need approval": "Need approval",
  "Ungroup": "Ungroup",
  "New folder": "New folder",
  "Add to {0}": "Add to {0}",
  "Remove from folder": "Remove from folder",

  // ── Composer.tsx ──────────────────────────────────────────────────────
  "Queue paused because you interrupted": "Queue paused because you interrupted",
  "Steer": "Steer",
  "Edit queued message": "Edit queued message",
  "Save queued message": "Save queued message",
  "Cancel queued message edit": "Cancel queued message edit",
  "Remove queued message": "Remove queued message",
  "Add a note, or send to start…": "Add a note, or send to start…",
  "Add a message, or send…": "Add a message, or send…",
  "Add context, or send to continue…": "Add context, or send to continue…",
  "Ask, build, / for commands, @ for references... ": "Ask, build, / for commands, @ for references... ",
  "Add files or choose a mode": "Add files or choose a mode",
  "Add to message": "Add to message",
  "Upload file": "Upload file",
  "Attach files or images to this message": "Attach files or images to this message",
  "{0} does not support attachments": "{0} does not support attachments",
  "Plan mode": "Plan mode",
  "Create a plan to review before building": "Create a plan to review before building",
  "Turn off Plan mode": "Turn off Plan mode",
  "Drop files to attach": "Drop files to attach",
  "{0} attachments": "{0} attachments",
  "1 attachment": "1 attachment",

  // ── ContextMeter.tsx ──────────────────────────────────────────────────
  "Context usage": "Context usage",
  "{0}, {1}. Open context actions": "{0}, {1}. Open context actions",
  "Compact now": "Compact now",
  "Wait for the current operation to finish": "Wait for the current operation to finish",
  "Compact this conversation's context": "Compact this conversation's context",

  // ── CwdPicker.tsx ─────────────────────────────────────────────────────
  "Project {0}": "Project {0}",
  "Current project": "Current project",
  "Recent projects": "Recent projects",
  "More Projects": "More Projects",
  "New terminal": "New terminal",

  // ── FileTree.tsx ──────────────────────────────────────────────────────
  "Reveal in Finder": "Reveal in Finder",
  "Reveal in File Explorer": "Reveal in File Explorer",
  "Open Containing Folder": "Open Containing Folder",
  "New File": "New File",
  "New Folder": "New Folder",
  "Copy Path": "Copy Path",
  "Copy Relative Path": "Copy Relative Path",
  "Open in Terminal": "Open in Terminal",
  "Delete folder \"{0}\" and everything inside it?": "Delete folder \"{0}\" and everything inside it?",
  "Delete \"{0}\"?": "Delete \"{0}\"?",
  "Cannot paste a folder into itself.": "Cannot paste a folder into itself.",
  "Collapse All": "Collapse All",
  "Search in files ({0}Shift+F)": "Search in files ({0}Shift+F)",
  "{0} files": "{0} files",
  "Hide changes": "Hide changes",
  "Show changes": "Show changes",
  "{0} file changed": "{0} file changed",
  "A file or folder name must be provided.": "A file or folder name must be provided.",
  "A file or folder name cannot start with a slash.": "A file or folder name cannot start with a slash.",
  "A file or folder {0} already exists at this location. Please choose a different name.": "A file or folder {0} already exists at this location. Please choose a different name.",
  "The name {0} is not valid as a file or folder name. Please choose a different name.": "The name {0} is not valid as a file or folder name. Please choose a different name.",
  "Leading or trailing whitespace detected in file or folder name.": "Leading or trailing whitespace detected in file or folder name.",
  "Type file name. Press Enter to confirm or Escape to cancel.": "Type file name. Press Enter to confirm or Escape to cancel.",

  // ── FilePicker.tsx ────────────────────────────────────────────────────
  "Open a project to search files": "Open a project to search files",
  "Indexing files…": "Indexing files…",
  "No files found": "No files found",
  "No matching files": "No matching files",
  "Type a file name to search": "Type a file name to search",
  "Files": "Files",

  // ── FileMentionPicker.tsx ─────────────────────────────────────────────
  "No matching files or notes": "No matching files or notes",
  "No matching files or folders": "No matching files or folders",
  "No files or notes found": "No files or notes found",
  "No files or folders found": "No files or folders found",
  "Files and notes": "Files and notes",
  "Files and folders": "Files and folders",
  "Note": "Note",

  // ── FilePreview.tsx ───────────────────────────────────────────────────
  "+{0}": "+{0}",
  "-{0}": "-{0}",

  // ── ApprovalToasts.tsx ────────────────────────────────────────────────
  // (Question, Approval, Allow, Deny already covered above)

  // ── AttachmentChip.tsx ────────────────────────────────────────────────
  // (Remove already covered above)

  // ── BranchPicker.tsx ──────────────────────────────────────────────────
  "detached {0}": "detached {0}",
  "No repo": "No repo",
  "Loading branch…": "Loading branch…",
  "No git repository": "No git repository",
  "Loading branch": "Loading branch",
  "Branch {0}": "Branch {0}",
  "Search or create a branch...": "Search or create a branch...",
  "Search or create a branch": "Search or create a branch",
  "No matching branches": "No matching branches",
  "No branches": "No branches",
  "Branches": "Branches",
  "Create and checkout {0}": "Create and checkout {0}",
  "WIP before switching to {0}": "WIP before switching to {0}",

  // ── ColorPickerPopover.tsx ────────────────────────────────────────────
  "Color {0}": "Color {0}",
  "Custom color": "Custom color",
  "Saturation and brightness": "Saturation and brightness",
  "Hue": "Hue",
  "Hex color": "Hex color",

  // ── ExplorerMenu.tsx ──────────────────────────────────────────────────
  "File actions": "File actions",

  // ── GitChangesPanel.tsx ───────────────────────────────────────────────
  "Message ({0}↩ to commit)": "Message ({0}↩ to commit)",
  "Generate commit message": "Generate commit message",
  "Commit": "Commit",
  "Commit options": "Commit options",
  "Commit & Push": "Commit & Push",
  "Commit, Push & Create PR": "Commit, Push & Create PR",
  "Publish Branch": "Publish Branch",
  "Sync Changes": "Sync Changes",
  "Create PR": "Create PR",
  "View PR": "View PR",
  "View PR #{0}": "View PR #{0}",
  "Synchronizing Changes...": "Synchronizing Changes...",
  "Publish Branch \"{0}\"": "Publish Branch \"{0}\"",
  "Pull {0} and push {1} commits between {2}": "Pull {0} and push {1} commits between {2}",
  "Pull {0} commit from {1}": "Pull {0} commit from {1}",
  "Pull {0} commits from {1}": "Pull {0} commits from {1}",
  "Push {0} commit to {1}": "Push {0} commit to {1}",
  "Push {0} commits to {1}": "Push {0} commits to {1}",
  "Create a pull request into {0}": "Create a pull request into {0}",
  "Create pull request": "Create pull request",
  "View PR #{0}: {1}": "View PR #{0}: {1}",
  "View pull request": "View pull request",
  "Create a pull request from default branch \"{0}\"?": "Create a pull request from default branch \"{0}\"?",
  "Push to default branch \"{0}\"?": "Push to default branch \"{0}\"?",
  "Delete untracked file {0}?": "Delete untracked file {0}?",
  "Discard changes in {0}? This cannot be undone.": "Discard changes in {0}? This cannot be undone.",
  "Discard": "Discard",
  "No uncommitted changes": "No uncommitted changes",
  "Loading changes…": "Loading changes…",
  "{0} unpushed commit": "{0} unpushed commit",
  "{0} unpushed commits": "{0} unpushed commits",
  "{0} incoming commit": "{0} incoming commit",
  "{0} incoming commits": "{0} incoming commits",
  "No files": "No files",
  "Staged Changes": "Staged Changes",
  "Unstage All Changes": "Unstage All Changes",
  "Discard All Changes": "Discard All Changes",
  "Stage All Changes": "Stage All Changes",
  "Discard Changes": "Discard Changes",
  "Unstage Changes": "Unstage Changes",
  "Stage Changes": "Stage Changes",

  // ── GitHistoryGraph.tsx ───────────────────────────────────────────────
  "Collapse graph": "Collapse graph",
  "Expand graph": "Expand graph",
  "Graph": "Graph",
  "No commits yet": "No commits yet",
  "Resize graph": "Resize graph",

  // ── HandoffMiniCard.tsx ───────────────────────────────────────────────
  "Handoff": "Handoff",
  "{0} file": "{0} file",
  "Remove handoff": "Remove handoff",

  // ── InboxFiltersMenu.tsx ──────────────────────────────────────────────
  "All time": "All time",
  "Today": "Today",
  "Last 7 days": "Last 7 days",
  "Last 30 days": "Last 30 days",
  "Issues": "Issues",
  "Pull requests": "Pull requests",
  "Assigned to me": "Assigned to me",
  "Status": "Status",
  "Time": "Time",
  "Type": "Type",
  "Projects": "Projects",
  "Clear filters": "Clear filters",
  "Filter inbox": "Filter inbox",

  // ── InboxMiniCard.tsx ─────────────────────────────────────────────────
  "Pull request": "Pull request",
  "Issue": "Issue",
  "Open in {0}": "Open in {0}",
  "Open {0} {1} in {2}": "Open {0} {1} in {2}",
  "Remove {0} {1}": "Remove {0} {1}",

  // ── MarkdownModeToggle.tsx ────────────────────────────────────────────
  "Markdown view": "Markdown view",
  "Preview": "Preview",
  "Source": "Source",

  // ── Modal.tsx ─────────────────────────────────────────────────────────
  // (Close already covered above)

  // ── ModelPicker.tsx ───────────────────────────────────────────────────
  "Search models...": "Search models...",
  "Search models": "Search models",
  "Providers": "Providers",
  "Favorites": "Favorites",
  "No favorite models": "No favorite models",
  "Loading Codex models…": "Loading Codex models…",
  "No matching models": "No matching models",
  "Remove from favorites": "Remove from favorites",
  "Add to favorites": "Add to favorites",
  "Models": "Models",

  // ── NoteMiniCard.tsx ──────────────────────────────────────────────────
  "Remove note {0}": "Remove note {0}",

  // ── PlanPreview.tsx ───────────────────────────────────────────────────
  "Building…": "Building…",
  "Built": "Built",
  "Build": "Build",
  "Open in pane": "Open in pane",
  "Open plan in pane": "Open plan in pane",
  "Build this plan": "Build this plan",

  // ── ProjectRail.tsx ───────────────────────────────────────────────────
  "Unpin project": "Unpin project",
  "Pin project": "Pin project",
  "Inbox, new items": "Inbox, new items",
  "Pinned": "Pinned",
  "No projects yet": "No projects yet",
  "Working agents": "Working agents",
  "Show less": "Show less",
  "{0} more": "{0} more",
  "Open project": "Open project",
  "Project options": "Project options",
  "Resize project sidebar": "Resize project sidebar",

  // ── ProjectSearch.tsx ─────────────────────────────────────────────────
  "Back to files": "Back to files",
  "Search in files": "Search in files",
  "Match case": "Match case",
  "Match whole word": "Match whole word",
  "Use regular expression": "Use regular expression",
  "files to include": "files to include",
  "files to exclude": "files to exclude",
  "Searching…": "Searching…",
  "{0} result in {1} file": "{0} result in {1} file",
  "{0} results in {1} file": "{0} results in {1} file",
  "{0} results in {1} files": "{0} results in {1} files",
  " (limited)": " (limited)",
  "Type to search across the project": "Type to search across the project",

  // ── QuestionForm.tsx ──────────────────────────────────────────────────
  "{0} of {1}": "{0} of {1}",
  "Select all that apply": "Select all that apply",
  "Type your answer": "Type your answer",
  "Other": "Other",

  // ── RemoveProjectDialog.tsx ───────────────────────────────────────────
  "All conversations for this project will be deleted. It also leaves the sidebar. The folder on disk stays put, and opening it again brings the project back empty.": "All conversations for this project will be deleted. It also leaves the sidebar. The folder on disk stays put, and opening it again brings the project back empty.",
  "1 saved conversation will be removed.": "1 saved conversation will be removed.",
  "{0} saved conversations will be removed.": "{0} saved conversations will be removed.",

  // ── SecondOpinionButton.tsx ───────────────────────────────────────────
  "Install another provider to hand off": "Install another provider to hand off",
  "Hand this session to another agent to continue the work.": "Hand this session to another agent to continue the work.",
  "Hand this session to another agent": "Hand this session to another agent",
  "Build with another model": "Build with another model",
  "No build providers are available": "No build providers are available",
  "Choose the model and provider that should build this plan.": "Choose the model and provider that should build this plan.",
  "Build this plan with another model or provider": "Build this plan with another model or provider",
  "Second opinion": "Second opinion",
  "Install another provider for a second opinion": "Install another provider for a second opinion",
  "Send this turn to another agent to review the work.": "Send this turn to another agent to review the work.",
  "Send this turn to another agent": "Send this turn to another agent",

  // ── SecondOpinionCard.tsx ─────────────────────────────────────────────
  // (Handoff, Second opinion, file/files already covered above)

  // ── SessionFiltersMenu.tsx ────────────────────────────────────────────
  "Archived": "Archived",
  "Needs approval": "Needs approval",

  // ── SessionReview.tsx ─────────────────────────────────────────────────
  "Collapse files": "Collapse files",
  "Expand files": "Expand files",
  "{0} Files": "{0} Files",
  "Undo all session changes": "Undo all session changes",
  "Undo is unavailable while another session is running in this project": "Undo is unavailable while another session is running in this project",
  "Undo is unavailable because a file changed outside this session": "Undo is unavailable because a file changed outside this session",
  "Undo All": "Undo All",
  "Keep all session changes": "Keep all session changes",
  "Keep All": "Keep All",
  "Review changes": "Review changes",
  "Review": "Review",
  "Shared file": "Shared file",

  // ── SettingsRail.tsx ──────────────────────────────────────────────────
  // (Settings, Back already covered above)

  // ── SkillPicker.tsx ───────────────────────────────────────────────────
  "No matching commands or skills": "No matching commands or skills",
  "No commands yet": "No commands yet",
  "Commands and skills": "Commands and skills",
  "New skill": "New skill",
  "Writes a starter SKILL.md you can edit.": "Writes a starter SKILL.md you can edit.",
  "skill-name": "skill-name",
  "Skill name": "Skill name",
  "Project": "Project",
  ".agents/skills": ".agents/skills",
  "Personal": "Personal",
  "~/.agents/skills": "~/.agents/skills",
  "Use lowercase letters, numbers, and hyphens.": "Use lowercase letters, numbers, and hyphens.",
  "Creating…": "Creating…",
  "monocode": "monocode",
  "personal": "personal",
  "project": "project",

  // ── SurfaceTabs.tsx ───────────────────────────────────────────────────
  "Working tree changes": "Working tree changes",
  "Session Changes": "Session Changes",
  "Changes captured for this session only": "Changes captured for this session only",
  "{0} (Working Tree)": "{0} (Working Tree)",
  "{0} — {1}": "{0} — {1}",
  "{0} problem": "{0} problem",
  "{0} problems": "{0} problems",
  "Open files": "Open files",
  "Drag to reorder pane": "Drag to reorder pane",

  // ── SwitchBranchDialog.tsx ────────────────────────────────────────────
  "Create {0}": "Create {0}",
  "Switch to {0}": "Switch to {0}",
  "Uncommitted changes": "Uncommitted changes",
  "Creating \"{0}\" would overwrite your local changes. Stash them for later, or commit them on this branch first.": "Creating \"{0}\" would overwrite your local changes. Stash them for later, or commit them on this branch first.",
  "Switching to \"{0}\" would overwrite your local changes. Stash them for later, or commit them on this branch first.": "Switching to \"{0}\" would overwrite your local changes. Stash them for later, or commit them on this branch first.",
  "Commit message": "Commit message",
  "Commit & switch": "Commit & switch",
  "Stash & switch": "Stash & switch",

  // ── TabGroupMenu.tsx ──────────────────────────────────────────────────
  "New tab in group": "New tab in group",
  "Move group to new window": "Move group to new window",
  "Close group": "Close group",
  "Delete group": "Delete group",
  "Tab group actions": "Tab group actions",
  "Group name": "Group name",
  "Change project logo": "Change project logo",
  "Add project logo": "Add project logo",
  "Project logo": "Project logo",
  "Shown in tabs and composer": "Shown in tabs and composer",
  "Optional — replaces folder icon": "Optional — replaces folder icon",
  "Remove project logo": "Remove project logo",
  "Mascot": "Mascot",
  "Mascot {0}": "Mascot {0}",

  // ── TaskListPreview.tsx ───────────────────────────────────────────────
  "Task progress": "Task progress",
  "Tasks": "Tasks",
  "Completed": "Completed",
  "In progress": "In progress",
  "Cancelled": "Cancelled",
  "Pending": "Pending",

  // ── UpdateRailCard.tsx ────────────────────────────────────────────────
  "Updated to {0}": "Updated to {0}",
  "What's new": "What's new",
  "Dismiss update notification": "Dismiss update notification",

  // ── UsageFooter.tsx ───────────────────────────────────────────────────
  "Provider usage": "Provider usage",
  "Terminals": "Terminals",
  "Session": "Session",
  "Refresh usage": "Refresh usage",
  "Hide {0}": "Hide {0}",
  "Show {0}": "Show {0}",
  "Hide running terminals": "Hide running terminals",
  "{0} terminals are running processes": "{0} terminals are running processes",
  "Running terminals": "Running terminals",
  "not connected": "not connected",
  "Loading usage…": "Loading usage…",
  "Not connected": "Not connected",
  "expired": "expired",

  // ── WhatsNewDialog.tsx ────────────────────────────────────────────────
  "MonoCode {0}": "MonoCode {0}",
  "Release notes for this version are not available in this build.": "Release notes for this version are not available in this build.",

  // ── WindowControls.tsx ────────────────────────────────────────────────
  "Minimize": "Minimize",
  "Minimize window": "Minimize window",
  "Maximize": "Maximize",
  "Restore window": "Restore window",
  "Maximize window": "Maximize window",
  "Close window": "Close window",

  // ── SidebarUpdate.tsx ─────────────────────────────────────────────────
  "Update to {0}": "Update to {0}",
  "Downloading{0}": "Downloading{0}",
  "Checking…": "Checking…",
  "Check for updates": "Check for updates",

  // ══════════════════════════════════════════════════════════════════════
  // surfaces/
  // ══════════════════════════════════════════════════════════════════════

  // ── AgentMarkdown.tsx ─────────────────────────────────────────────────
  "Copied": "Copied",
  "Copy code": "Copy code",

  // ── AgentTranscript.tsx ───────────────────────────────────────────────
  "Load earlier messages": "Load earlier messages",
  "Waiting for answers": "Waiting for answers",
  "Thinking…": "Thinking…",
  "Waiting for approval": "Waiting for approval",
  "Subagent is running": "Subagent is running",
  "Agent is working": "Agent is working",
  "Copy response": "Copy response",
  "Saved to Notes": "Saved to Notes",
  "Save as note": "Save as note",
  "Preparing a handoff": "Preparing a handoff",
  "Preparing a handoff to {0}": "Preparing a handoff to {0}",
  "Continued with {0}": "Continued with {0}",
  "Accepted": "Accepted",
  "Rejected": "Rejected",
  "Hide the steps for {0}": "Hide the steps for {0}",
  "Show the steps for {0}": "Show the steps for {0}",
  "Hide thinking": "Hide thinking",
  "Show thinking: {0}": "Show thinking: {0}",
  "Hide the full note": "Hide the full note",
  "Agent said: {0}": "Agent said: {0}",
  "Tool call: {0}": "Tool call: {0}",
  "{0} tool call: {1}": "{0} tool call: {1}",
  "Read": "Read",
  "Find": "Find",
  "Skill": "Skill",
  "List": "List",
  "Edit": "Edit",
  "Write": "Write",
  "Worked": "Worked",
  "worked": "worked",
  "Subagent running": "Subagent running",
  "subagent running": "subagent running",
  "working": "working",
  "{0} subagent is running": "{0} subagent is running",
  "{0} is working": "{0} is working",
  "file": "file",

  // ── BinaryFileView.tsx ────────────────────────────────────────────────
  "Opening {0}…": "Opening {0}…",
  "Couldn't open {0}": "Couldn't open {0}",
  "{0} · not a readable image": "{0} · not a readable image",
  "Zoom out": "Zoom out",
  "Fit to window": "Fit to window",
  "Fit": "Fit",
  "Zoom in": "Zoom in",
  "Reveal": "Reveal",
  "—": "—",

  // ── CommitDiff.tsx ────────────────────────────────────────────────────
  "Couldn't load diff: {0}": "Couldn't load diff: {0}",
  "No textual diff": "No textual diff",
  "Couldn't load commit": "Couldn't load commit",

  // ── DiffCommentComposer.tsx ───────────────────────────────────────────
  "Cancel comment": "Cancel comment",
  "Leave a comment…": "Leave a comment…",
  "{0}↩ to add": "{0}↩ to add",
  "Add to chat": "Add to chat",
  "Comment on {0}": "Comment on {0}",

  // ── EmptySession.tsx ──────────────────────────────────────────────────
  "What should we work on in {0}?": "What should we work on in {0}?",
  "What should we work on?": "What should we work on?",

  // ── FileEditor.tsx ────────────────────────────────────────────────────
  "Saving…": "Saving…",
  "Saved": "Saved",
  "Save failed: {0}": "Save failed: {0}",
  "Can't stage this file": "Can't stage this file",
  "Jump between changes": "Jump between changes",
  "Previous change": "Previous change",
  "Next change": "Next change",
  "Plan markdown": "Plan markdown",

  // ── FilePane.tsx ──────────────────────────────────────────────────────
  "This plan is no longer in the session.": "This plan is no longer in the session.",

  // ── InboxComments.tsx ─────────────────────────────────────────────────
  "1 comment": "1 comment",
  "{0} comments": "{0} comments",
  "Latest comments · more on {0}": "Latest comments · more on {0}",
  "Cancel reply": "Cancel reply",
  "Write a reply ({0}↩)": "Write a reply ({0}↩)",
  "Leave a comment ({0}↩)": "Leave a comment ({0}↩)",
  "Posting...": "Posting...",
  "Reply": "Reply",
  "Comment": "Comment",
  "Loading comments": "Loading comments",
  "Resolved": "Resolved",
  "Open in Linear": "Open in Linear",
  "Open on GitHub": "Open on GitHub",

  // ── InboxMedia.tsx ────────────────────────────────────────────────────
  "Image": "Image",

  // ── InboxPrDiff.tsx ───────────────────────────────────────────────────
  "Patch unavailable because this pull request is too large": "Patch unavailable because this pull request is too large",

  // ── InboxView.tsx ─────────────────────────────────────────────────────
  "Inbox source": "Inbox source",
  "Mark all as read": "Mark all as read",
  "No matching Linear issues": "No matching Linear issues",
  "No matching issues or pull requests": "No matching issues or pull requests",
  "No Linear issues match these filters": "No Linear issues match these filters",
  "No issues or pull requests match these filters": "No issues or pull requests match these filters",
  "No Linear issues": "No Linear issues",
  "Open a project to fill the inbox": "Open a project to fill the inbox",
  "Select an issue or pull request": "Select an issue or pull request",
  "Pull request sections": "Pull request sections",
  "Summary": "Summary",
  "Code": "Code",
  "No file changes": "No file changes",
  "No description": "No description",
  "Unassigned": "Unassigned",
  "Updated {0}": "Updated {0}",
  "Send to agent": "Send to agent",
  "Sending...": "Sending...",
  "Review on GitHub": "Review on GitHub",
  "Choose project": "Choose project",
  "Linear": "Linear",
  "GitHub": "GitHub",
  "new": "new",
  "Resize inbox list": "Resize inbox list",

  // ── NotesView.tsx ─────────────────────────────────────────────────────
  "New note": "New note",
  "No matching notes": "No matching notes",
  "No notes yet. Save a turn from the transcript, or create one here.": "No notes yet. Save a turn from the transcript, or create one here.",
  "Select a note": "Select a note",
  "Note sections": "Note sections",
  "Write markdown…": "Write markdown…",
  "Note title": "Note title",
  "Resize notes list": "Resize notes list",

  // ── ProjectTerminalDock.tsx ───────────────────────────────────────────
  "Dock Bottom": "Dock Bottom",
  "Dock Top": "Dock Top",
  "Dock Left": "Dock Left",
  "Dock Right": "Dock Right",
  "Move Terminal": "Move Terminal",
  "Hide Terminal ({0}J)": "Hide Terminal ({0}J)",
  "Resize terminal": "Resize terminal",
  "Move terminal": "Move terminal",

  // ── ReleaseNotesSurface.tsx ───────────────────────────────────────────
  "Release notes": "Release notes",

  // ── SearchView.tsx ────────────────────────────────────────────────────
  "Search everything...": "Search everything...",
  "Find files, conversations, messages, and projects.": "Find files, conversations, messages, and projects.",
  "Search results": "Search results",
  "All": "All",
  "Conversations": "Conversations",

  // ── SessionChangesDiff.tsx ────────────────────────────────────────────
  "No session changes": "No session changes",
  "Couldn't load session changes": "Couldn't load session changes",

  // ── SessionPane.tsx ───────────────────────────────────────────────────
  "Close Pane ({0}W)": "Close Pane ({0}W)",
  "Close pane": "Close pane",
  "Jump to latest": "Jump to latest",

  // ── SettingsView.tsx ──────────────────────────────────────────────────
  "Restore defaults": "Restore defaults",
  "General": "General",
  "Appearance": "Appearance",
  "Keybindings": "Keybindings",
  "Transcript layout": "Transcript layout",
  "Full width keeps user prompts as a spanning card. Chat aligns them to the right with a max width, like a messaging app.": "Full width keeps user prompts as a spanning card. Chat aligns them to the right with a max width, like a messaging app.",
  "Full width": "Full width",
  "Chat": "Chat",
  "Diff view": "Diff view",
  "Editor keeps working-tree changes in the file. Unified stacks every changed file in one review, with sticky headers and collapsed unchanged lines.": "Editor keeps working-tree changes in the file. Unified stacks every changed file in one review, with sticky headers and collapsed unchanged lines.",
  "Editor": "Editor",
  "Unified": "Unified",
  "Follow-up behavior": "Follow-up behavior",
  "Queue follow-ups until the active turn finishes, or steer the active turn immediately.": "Queue follow-ups until the active turn finishes, or steer the active turn immediately.",
  "Queue": "Queue",
  "Anchor prompts to top": "Anchor prompts to top",
  "When you send, the new prompt sits at the top of the transcript and the reply grows into the space below. Turn this off to keep the classic layout, with the latest message resting on the composer.": "When you send, the new prompt sits at the top of the transcript and the reply grows into the space below. Turn this off to keep the classic layout, with the latest message resting on the composer.",
  "Composer mascot": "Composer mascot",
  "When a turn is running, the project mascot runs along the composer, bonks the scroll-to-latest button the first time, then jumps it, and sometimes grabs a coin.": "When a turn is running, the project mascot runs along the composer, bonks the scroll-to-latest button the first time, then jumps it, and sometimes grabs a coin.",
  "Empty session games": "Empty session games",
  "Pac-man and snake idle on the empty-session grid. Hover the band to take control of whichever is on screen. Turn this off to keep the pane still.": "Pac-man and snake idle on the empty-session grid. Hover the band to take control of whichever is on screen. Turn this off to keep the pane still.",
  "A global markdown notebook on the project rail. Save a finished turn from the transcript, then mention it later with @note or add it to chat. Turn this off to hide Notes from the UI.": "A global markdown notebook on the project rail. Save a finished turn from the transcript, then mention it later with @note or add it to chat. Turn this off to hide Notes from the UI.",
  "When two or more chats are in flight, a card on the project rail lists them so you can jump across projects. Finished turns stay until you open that session. Turn this off to hide the card.": "When two or more chats are in flight, a card on the project rail lists them so you can jump across projects. Finished turns stay until you open that session. Turn this off to hide the card.",
  "Sounds": "Sounds",
  "Short cues when a turn finishes, a new inbox item appears on the project rail, or an update is available. Switches and Copy on a finished turn also play.": "Short cues when a turn finishes, a new inbox item appears on the project rail, or an update is available. Switches and Copy on a finished turn also play.",
  "Claude Code hooks": "Claude Code hooks",
  "Run the hooks configured in your settings.json files — PreToolUse command rewrites, blocks, notifications, and the rest — just as the Claude Code CLI would. Turn this off if a hook is misbehaving and you need the session back. Takes effect on the next turn.": "Run the hooks configured in your settings.json files — PreToolUse command rewrites, blocks, notifications, and the rest — just as the Claude Code CLI would. Turn this off if a hook is misbehaving and you need the session back. Takes effect on the next turn.",
  "About": "About",
  "API key": "API key",
  "Create a personal API key in Linear → Settings → Security & Access. Disconnect deletes it.": "Create a personal API key in Linear → Settings → Security & Access. Disconnect deletes it.",
  "Disconnect": "Disconnect",
  "lin_api_…": "lin_api_…",
  "Linear API key": "Linear API key",
  "Saving": "Saving",
  "Connect": "Connect",
  "Linear Teams": "Linear Teams",
  "Unchecked teams stay out of the inbox.": "Unchecked teams stay out of the inbox.",
  "Version": "Version",
  "Version {0} is available.": "Version {0} is available.",
  "Checking for updates…": "Checking for updates…",
  "You're on the latest version.": "You're on the latest version.",
  "Update check failed.": "Update check failed.",
  "MonoCode updates itself from the release feed.": "MonoCode updates itself from the release feed.",
  "Theme": "Theme",
  "System follows the OS appearance. Dark and light share the same tint, so the hue below applies to both.": "System follows the OS appearance. Dark and light share the same tint, so the hue below applies to both.",
  "System": "System",
  "Dark": "Dark",
  "Light": "Light",
  "Sidebar opacity": "Sidebar opacity",
  "How much of the desktop shows through the sidebar and the project rail.": "How much of the desktop shows through the sidebar and the project rail.",
  "Blur radius": "Blur radius",
  "Background blur behind the window. Higher values cost more to composite.": "Background blur behind the window. Higher values cost more to composite.",
  "Base hue for accents and tinted surfaces.": "Base hue for accents and tinted surfaces.",
  "Saturation": "Saturation",
  "How strongly the hue tints the interface. Zero keeps it neutral.": "How strongly the hue tints the interface. Zero keeps it neutral.",
  "Main pane glass": "Main pane glass",
  "Extend the translucent treatment to the main pane behind sessions and editors.": "Extend the translucent treatment to the main pane behind sessions and editors.",
  "binding": "binding",
  "bindings": "bindings",
  "Filter keybindings": "Filter keybindings",
  "Command": "Command",
  "Keybinding": "Keybinding",
  "When": "When",
  "No matching bindings": "No matching bindings",
  "Bindings come from the app menu and the workspace key handler; they aren't customizable yet.": "Bindings come from the app menu and the workspace key handler; they aren't customizable yet.",
  "A provider is listed as installed once its CLI is found on your PATH. Uninstalled CLIs stay listed here but are omitted from the model picker. Turn off Show in picker to hide an installed provider from those tabs. The model beside each provider is what new conversations use when that provider is selected; Use by default picks the provider itself.": "A provider is listed as installed once its CLI is found on your PATH. Uninstalled CLIs stay listed here but are omitted from the model picker. Turn off Show in picker to hide an installed provider from those tabs. The model beside each provider is what new conversations use when that provider is selected; Use by default picks the provider itself.",
  "model": "model",
  "models": "models",
  "available.": "available.",
  "{0} {1} available.": "{0} {1} available.",
  "Use by default": "Use by default",
  "Show in picker": "Show in picker",
  "Show {0} in the model picker": "Show {0} in the model picker",
  "Archived projects": "Archived projects",
  "Archive a project from the rail to keep its chats without listing it in the sidebar.": "Archive a project from the rail to keep its chats without listing it in the sidebar.",
  "Show archived in the sidebar": "Show archived in the sidebar",
  "Keep archived conversations listed alongside the active ones.": "Keep archived conversations listed alongside the active ones.",
  "Archived in {0}": "Archived in {0}",
  "Archived conversations": "Archived conversations",
  "Open a project to see its archived conversations.": "Open a project to see its archived conversations.",
  "No archived conversations in this project.": "No archived conversations in this project.",

  // ── TerminalGridBackground.tsx ────────────────────────────────────────
  "score {0}": "score {0}",
  "game over": "game over",
  "release": "release",
  "take control": "take control",

  // ── TerminalView.tsx ──────────────────────────────────────────────────
  "[process exited{0}]": "[process exited{0}]",
  "[process exited]": "[process exited]",
  "[process exited ({0})]": "[process exited ({0})]",

  // ── TranscriptSelectionMenu.tsx ───────────────────────────────────────
  "Selected text actions": "Selected text actions",

  // ── UnifiedDiffView.tsx ───────────────────────────────────────────────
  "1 file": "1 file",
  "Expand all files": "Expand all files",
  "Collapse all files": "Collapse all files",
  "Diff is too large to display in full. File list is shown without patches.": "Diff is too large to display in full. File list is shown without patches.",
  "Binary file changed": "Binary file changed",
  "Diff is too large to display": "Diff is too large to display",
  "Discard file": "Discard file",
  "Stage file": "Stage file",
  "Expand upward": "Expand upward",
  "Expand unmodified lines upward": "Expand unmodified lines upward",
  "Expand downward": "Expand downward",
  "Expand unmodified lines downward": "Expand unmodified lines downward",
  "{0} unmodified {1}": "{0} unmodified {1}",
  "Comment on line {0}": "Comment on line {0}",
  "Stage hunk": "Stage hunk",
  "unmodified": "unmodified",
  "line": "line",
  "lines": "lines",

  // ── WorkingTreeDiff.tsx ───────────────────────────────────────────────
  "Staged — no unstaged changes": "Staged — no unstaged changes",
  "No unstaged changes": "No unstaged changes",
  "Couldn't load changes": "Couldn't load changes",

  // ══════════════════════════════════════════════════════════════════════
  // lib/
  // ══════════════════════════════════════════════════════════════════════

  // ── session.ts ────────────────────────────────────────────────────────
  "Supervised": "Supervised",
  "Auto-accept edits": "Auto-accept edits",
  "Auto": "Auto",
  "Full access": "Full access",
  "Ask before commands and file changes.": "Ask before commands and file changes.",
  "Auto-approve edits, ask before other actions.": "Auto-approve edits, ask before other actions.",
  "An AI reviewer approves routine actions; risky ones still ask.": "An AI reviewer approves routine actions; risky ones still ask.",
  "Allow commands and edits without prompts.": "Allow commands and edits without prompts.",
  "Claude Code": "Claude Code",
  "Codex": "Codex",
  "Cursor": "Cursor",
  "Grok Build": "Grok Build",
  "OpenCode": "OpenCode",
  "Pi": "Pi",

  // ── settings.ts ───────────────────────────────────────────────────────
  "App-wide behavior and the build you are running.": "App-wide behavior and the build you are running.",
  "Theme, translucency, and the tint applied to the chrome.": "Theme, translucency, and the tint applied to the chrome.",
  "Every shortcut the workspace handles, from the app menu and the key handler.": "Every shortcut the workspace handles, from the app menu and the key handler.",
  "Agent CLIs MonoCode can drive, and the model new sessions start with.": "Agent CLIs MonoCode can drive, and the model new sessions start with.",
  "Projects and conversations you have archived.": "Projects and conversations you have archived.",

  // ── plan.ts ───────────────────────────────────────────────────────────
  "Create a reviewable implementation plan before changing files.": "Create a reviewable implementation plan before changing files.",
  "You are in plan mode.": "You are in plan mode.",
  "Investigate the request and the repository, but do not modify files, run destructive commands, or start implementing.": "Investigate the request and the repository, but do not modify files, run destructive commands, or start implementing.",
  "Resolve important implementation details and finish with one self-contained Markdown plan.": "Resolve important implementation details and finish with one self-contained Markdown plan.",
  "The plan must be specific enough to build after explicit user approval.": "The plan must be specific enough to build after explicit user approval.",
  "Structure the final plan with a Markdown heading and concrete implementation steps.": "Structure the final plan with a Markdown heading and concrete implementation steps.",
  "Do not ask the user to approve inside the response; the application provides a separate Build action.": "Do not ask the user to approve inside the response; the application provides a separate Build action.",
  "Request": "Request",
  "The user reviewed and explicitly approved the following implementation plan.": "The user reviewed and explicitly approved the following implementation plan.",
  "Implement it now, using this exact edited version as the source of truth.": "Implement it now, using this exact edited version as the source of truth.",
  "sections": "sections",
  "diagram": "diagram",
  "words": "words",

  // ── handoff.ts ────────────────────────────────────────────────────────
  "You are continuing an existing conversation handed off from": "You are continuing an existing conversation handed off from",
  "This is not a new session. Do not say you have no prior context.": "This is not a new session. Do not say you have no prior context.",
  "Continue from a": "Continue from a",
  "session. Do not invent prior work.": "session. Do not invent prior work.",
  "Prior conversation from": "Prior conversation from",
  "this is the thread you are joining, not optional background:": "this is the thread you are joining, not optional background:",
  "The user is switching to another coding agent.": "The user is switching to another coding agent.",
  "Their new message will be sent separately": "Their new message will be sent separately",
  "do not repeat it, and do not add a Goal heading.": "do not repeat it, and do not add a Goal heading.",
  "Write a short recap of this conversation so the next agent can continue.": "Write a short recap of this conversation so the next agent can continue.",
  "Under 120 words.": "Under 120 words.",
  "Plain markdown.": "Plain markdown.",
  "No title card.": "No title card.",
  "No greeting.": "No greeting.",
  "Do not paste the whole transcript.": "Do not paste the whole transcript.",
  "Use only this conversation.": "Use only this conversation.",
  "Do not run git, do not inspect the working tree, do not read files, do not call tools.": "Do not run git, do not inspect the working tree, do not read files, do not call tools.",
  "Mention files only if this chat edited them.": "Mention files only if this chat edited them.",
  "If the chat was a greeting or has no task yet, say that in one sentence.": "If the chat was a greeting or has no task yet, say that in one sentence.",
  "Do not invent work from uncommitted repo files.": "Do not invent work from uncommitted repo files.",
  "Session so far": "Session so far",
  "Files edited in this session": "Files edited in this session",
  "Suggested next step": "Suggested next step",
  "Current tasks": "Current tasks",
  "(no text)": "(no text)",
  "User:": "User:",
  "Assistant:": "Assistant:",
  "(omitted earlier messages)": "(omitted earlier messages)",
  "Goal": "Goal",

  // ── secondOpinion.ts ──────────────────────────────────────────────────
  "Give a second opinion on work": "Give a second opinion on work",
  "just finished in this same working copy. The files are already on disk.": "just finished in this same working copy. The files are already on disk.",
  "Review that work: what is wrong, what is missing, and what you would have done differently.": "Review that work: what is wrong, what is missing, and what you would have done differently.",
  "Fix anything you agree is broken or incomplete.": "Fix anything you agree is broken or incomplete.",
  "If you would leave it, say so and stop.": "If you would leave it, say so and stop.",
  "Do not redo the task from scratch unless the work is actually wrong.": "Do not redo the task from scratch unless the work is actually wrong.",
  "Read the listed files before changing anything.": "Read the listed files before changing anything.",
  "User request": "User request",
  "(no user message on this turn)": "(no user message on this turn)",
  "reported": "reported",
  "(no written summary — inspect the files)": "(no written summary — inspect the files)",
  "Files it edited": "Files it edited",
  "(none recorded on this turn)": "(none recorded on this turn)",

  // ── liveAgents.ts ─────────────────────────────────────────────────────

  // ── githubTasks.ts ────────────────────────────────────────────────────
  "Approved": "Approved",
  "Changes requested": "Changes requested",
  "Review required": "Review required",
  "Requested changes": "Requested changes",
  "Dismissed": "Dismissed",
  "Commented": "Commented",
  "Work on this Linear issue:": "Work on this Linear issue:",
  "Work on this GitHub": "Work on this GitHub",
  "Linear #": "Linear #",

  // ── inFlight.ts ───────────────────────────────────────────────────────
  "Turn interrupted when MonoCode quit.": "Turn interrupted when MonoCode quit.",
  "Continue from where you left off.": "Continue from where you left off.",
  "1 chat is still running. Quit anyway? It will resume when you reopen MonoCode.": "1 chat is still running. Quit anyway? It will resume when you reopen MonoCode.",
  "{0} chats are still running. Quit anyway? They will resume when you reopen MonoCode.": "{0} chats are still running. Quit anyway? They will resume when you reopen MonoCode.",

  // ── jsonText.ts ───────────────────────────────────────────────────────
  "[truncated]": "[truncated]",

  // ── contextUsage.ts ───────────────────────────────────────────────────
  "Context used": "Context used",
  "{0}% context used": "{0}% context used",
  "tokens": "tokens",

  // ── rateLimits.ts ─────────────────────────────────────────────────────
  "Resets now": "Resets now",
  "Resets in": "Resets in",
  "used": "used",
  "window": "window",
  "wk": "wk",
  "5h": "5h",
  "1h": "1h",
  "Claude usage response was not JSON": "Claude usage response was not JSON",
  "Claude usage response was empty": "Claude usage response was empty",

  // ── rateLimitsFetch.ts ────────────────────────────────────────────────
  "Claude not signed in": "Claude not signed in",
  "Claude usage unavailable": "Claude usage unavailable",
  "Codex CLI not found": "Codex CLI not found",
  "Codex usage probe exited": "Codex usage probe exited",
  "No Codex usage data": "No Codex usage data",
  "Codex not signed in": "Codex not signed in",
  "Codex usage probe timed out": "Codex usage probe timed out",

  // ── updater.ts ────────────────────────────────────────────────────────
  "Update available": "Update available",
  "is available (you have": "is available (you have",
  "Install now?": "Install now?",
  "Automatic updates aren't configured for this build.": "Automatic updates aren't configured for this build.",
  "Download releases at https://github.com/hardbeat920/monocode/releases/latest": "Download releases at https://github.com/hardbeat920/monocode/releases/latest",
  "Couldn't check for updates.": "Couldn't check for updates.",
  "Couldn't install the update.": "Couldn't install the update.",

  // ── terminalClose.ts ──────────────────────────────────────────────────
  "is still running in": "is still running in",
  "Close this terminal anyway?": "Close this terminal anyway?",
  "These terminals are still running:": "These terminals are still running:",
  "Close them anyway?": "Close them anyway?",

  // ── terminalTab.ts ────────────────────────────────────────────────────

  // ── sessionFolders.ts ─────────────────────────────────────────────────
  // (New folder already covered above)

  // ── tabGroups.ts ──────────────────────────────────────────────────────
  "Group": "Group",

  // ── taskList.ts ───────────────────────────────────────────────────────
  "Complete": "Complete",
  "of": "of",

  // ── projectLogos.ts ───────────────────────────────────────────────────
  "Choose project logo": "Choose project logo",
  "Images": "Images",

  // ── compact.ts ────────────────────────────────────────────────────────
  "Summarize older conversation context to free space.": "Summarize older conversation context to free space.",

  // ── createSkill.ts ────────────────────────────────────────────────────
  "Create a MonoCode skill as a SKILL.md in .agents/skills. Use when the user wants to author, write, save, or scaffold a skill, or asks about skill format.": "Create a MonoCode skill as a SKILL.md in .agents/skills. Use when the user wants to author, write, save, or scaffold a skill, or asks about skill format.",
  "Create a MonoCode skill": "Create a MonoCode skill",
  "Write a portable Agent Skill so every harness (Claude, Cursor, Codex, Grok Build, OpenCode, Pi, omp, fx) can load it.": "Write a portable Agent Skill so every harness (Claude, Cursor, Codex, Grok Build, OpenCode, Pi, omp, fx) can load it.",
  "Storage (required)": "Storage (required)",
  "Gather before writing": "Gather before writing",
  "Purpose": "Purpose",
  "Scope": "Scope",
  "Triggers": "Triggers",
  "Domain knowledge": "Domain knowledge",
  "Output format": "Output format",
  "Verbatim text": "Verbatim text",
  "Ask where it should live if the user did not say. Default to the project when a project folder is open.": "Ask where it should live if the user did not say. Default to the project when a project folder is open.",
  "File layout": "File layout",
  "SKILL.md format": "SKILL.md format",
  "Description": "Description",
  "Authoring rules": "Authoring rules",
  "After writing": "After writing",
  "Old patterns": "Old patterns",
  "Confirm the file path": "Confirm the file path",
  "Confirm name + description": "Confirm name + description",
  "Do not copy the skill into harness-specific folders": "Do not copy the skill into harness-specific folders",

  // ── skills.ts ─────────────────────────────────────────────────────────
  "The user invoked skill(s) with /name. Follow every instruction in each skill body.": "The user invoked skill(s) with /name. Follow every instruction in each skill body.",
  "Use a lowercase name with letters, numbers, and hyphens.": "Use a lowercase name with letters, numbers, and hyphens.",

  // ── diffComment.ts ────────────────────────────────────────────────────
  "Diff comment on": "Diff comment on",
  " (deleted line)": " (deleted line)",

  // ── fs.ts ─────────────────────────────────────────────────────────────
  "Attach files": "Attach files",

  // ── filePreview.ts ────────────────────────────────────────────────────
  "KB": "KB",
  "MB": "MB",
  "GB": "GB",

  // ── notes.ts ──────────────────────────────────────────────────────────
  "Note:": "Note:",
  "Referenced note": "Referenced note",
  "Use this note.": "Use this note.",

  // ── userQuestion.ts ──────────────────────────────────────────────────
  "Question 1": "Question 1",

  // ── appLifecycle.ts ───────────────────────────────────────────────────
  "Quit": "Quit",

  // ── releaseNotes.ts ───────────────────────────────────────────────────
  "What's new in MonoCode": "What's new in MonoCode",
  "Jan": "Jan",
  "Feb": "Feb",
  "Mar": "Mar",
  "Apr": "Apr",
  "May": "May",
  "Jun": "Jun",
  "Jul": "Jul",
  "Aug": "Aug",
  "Sep": "Sep",
  "Oct": "Oct",
  "Nov": "Nov",
  "Dec": "Dec",

  // ── Missing keys added during audit ────────────────────────────────────

  // App.tsx
  "Close this tab with unsaved files?": "Close this tab with unsaved files?",
  "Close other tabs with unsaved files?": "Close other tabs with unsaved files?",
  "Close {0} without saving?": "Close {0} without saving?",
  "Close this conversation with unsaved files?": "Close this conversation with unsaved files?",
  "this session": "this session",
  "Archive this conversation with unsaved files?": "Archive this conversation with unsaved files?",
  "Delete this conversation with unsaved files?": "Delete this conversation with unsaved files?",
  "Build approved plan": "Build approved plan",
  "Compacting context…": "Compacting context…",
  "Compacted context": "Compacted context",
  "{0} could not compact this context": "{0} could not compact this context",

  // Sidebar.tsx
  "Pin": "Pin",
  "Unpin": "Unpin",
  "now": "now",
  "{0}m": "{0}m",
  "{0}h {1}m": "{0}h {1}m",
  "{0}h": "{0}h",
  "{0}d": "{0}d",

  // TitleBar.tsx
  "Back ({0})": "Back ({0})",
  "Forward ({0})": "Forward ({0})",
  "Toggle Sidebar ({0})": "Toggle Sidebar ({0})",
  "Go to File ({0})": "Go to File ({0})",
  "New session ({0})": "New session ({0})",
  "New Terminal ({0})": "New Terminal ({0})",
  "Settings ({0})": "Settings ({0})",

  // FileTree.tsx
  "files": "files",
  "changed": "changed",
  "A file or folder": "A file or folder",
  "already exists at this location. Please choose a different name.": "already exists at this location. Please choose a different name.",
  "The name": "The name",
  "is not valid as a file or folder name. Please choose a different name.": "is not valid as a file or folder name. Please choose a different name.",

  // SurfaceTabs.tsx
  "(Working Tree)": "(Working Tree)",
  "problem": "problem",
  "problems": "problems",

  // ProjectRail.tsx
  "Search ({0})": "Search ({0})",
  "{0} {1} changed": "{0} {1} changed",

  // CwdPicker.tsx
  "Project picker": "Project picker",
  "More projects": "More projects",

  // SessionFiltersMenu.tsx
  "Provider": "Provider",

  // ModelPicker.tsx
  "{0} · {1} ({2}.)": "{0} · {1} ({2}.)",
  "{0} {1}": "{0} {1}",
  "Model picker": "Model picker",

  // AttachmentChip.tsx
  "Remove {0}": "Remove {0}",

  // BranchPicker.tsx
  "Branch picker": "Branch picker",

  // GitChangesPanel.tsx
  "Discard all unstaged changes in {0} files? This cannot be undone.": "Discard all unstaged changes in {0} files? This cannot be undone.",
  "Could not prepare pull request content": "Could not prepare pull request content",

  // RemoveProjectDialog.tsx
  "Delete {0}": "Delete {0}",

  // SecondOpinionButton.tsx
  "{0} models": "{0} models",

  // settings.ts keybinding labels
  "App: Search": "App: Search",
  "App: Go to File": "App: Go to File",
  "App: Find in Files": "App: Find in Files",
  "App: Open Project": "App: Open Project",
  "App: New Window": "App: New Window",
  "App: Toggle Sidebar": "App: Toggle Sidebar",
  "App: Switch Model": "App: Switch Model",
  "Tab: New": "Tab: New",
  "Tab: Close Others": "Tab: Close Others",
  "Tab: Next": "Tab: Next",
  "Tab: Previous": "Tab: Previous",
  "Tab: Cycle Next": "Tab: Cycle Next",
  "Tab: Cycle Previous": "Tab: Cycle Previous",
  "Tab: Back": "Tab: Back",
  "Tab: Forward": "Tab: Forward",
  "Tab: Activate 1–8": "Tab: Activate 1–8",
  "Tab: Activate Last": "Tab: Activate Last",
  "Session: Previous": "Session: Previous",
  "Session: Next": "Session: Next",
  "Project: Previous": "Project: Previous",
  "Project: Next": "Project: Next",
  "Pane: Close": "Pane: Close",
  "Pane: Split Right": "Pane: Split Right",
  "Pane: Split Down": "Pane: Split Down",
  "Pane: Focus Left": "Pane: Focus Left",
  "Pane: Focus Right": "Pane: Focus Right",
  "Pane: Focus Up": "Pane: Focus Up",
  "Pane: Focus Down": "Pane: Focus Down",
  "Terminal: New": "Terminal: New",
  "Terminal: New Tab": "Terminal: New Tab",
  "Terminal: Toggle Dock": "Terminal: Toggle Dock",
  "Editor: Find": "Editor: Find",
  "Editor: Replace": "Editor: Replace",
  "Always": "Always",

  // userQuestion.ts
  "Question {0}": "Question {0}",

  // githubTasks.ts
  "Linear #{0}": "Linear #{0}",
  "pull request": "pull request",
  "issue": "issue",
  "GitHub {0} #{1}": "GitHub {0} #{1}",
  "Work on this GitHub {0}:": "Work on this GitHub {0}:",

  // contextUsage.ts
  "{0} / {1} tokens": "{0} / {1} tokens",
  "{0} tokens": "{0} tokens",

  // session.ts
  "omp": "omp",
  "fx": "fx",

  // notes.ts
  "Note: {0}": "Note: {0}",

  // updater.ts
  "MonoCode {0} is available (you have {1}).": "MonoCode {0} is available (you have {1}).",
  "Download releases at {0}": "Download releases at {0}",

  // rateLimits.ts
  "Resets in {0}": "Resets in {0}",
  "{0} used": "{0} used",
  "{0} window": "{0} window",

  // diffComment.ts
  "deleted line": "deleted line",

  // handoff.ts
  "({0} earlier messages omitted)": "({0} earlier messages omitted)",

  // models.ts
  "Reasoning": "Reasoning",
  "Extra High": "Extra High",
  "High": "High",
  "Medium": "Medium",
  "Low": "Low",

  // plan.ts
  "{0} sections": "{0} sections",
  "{0} words": "{0} words",

  // InboxComments.tsx
  "Replying to {0}": "Replying to {0}",
  "comment": "comment",
  "Write a reply ({0})": "Write a reply ({0})",
  "Leave a comment ({0})": "Leave a comment ({0})",

  // BinaryFileView.tsx
  "Copy path": "Copy path",

  // UnifiedDiffView.tsx
  "{0} unmodified line": "{0} unmodified line",
  "{0} unmodified lines": "{0} unmodified lines",

  // AgentTranscript.tsx
  "Thinking": "Thinking",
  "Thinking: {0}": "Thinking: {0}",

  // SettingsView.tsx
  "Filter": "Filter",
  "{0} model": "{0} model",

  // NotesView.tsx
  "Filter notes": "Filter notes",

  // ProjectTerminalDock.tsx
  "Hide Terminal ({0})": "Hide Terminal ({0})",

  // AccessPicker.tsx
  "Access": "Access",

  // MenuBar.tsx
  "menu": "menu",

  // harness protocol fallbacks
  "Approve file changes": "Approve file changes",
  "Choose an option": "Choose an option",
  "Confirm": "Confirm",

};
