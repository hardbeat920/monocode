# MonoCode 待汉化清单（按文案类型分类）

更新于 2026-09-28，分支 `feature/i18n-chinese`。前端界面可见文案共 900 条，另加第八类原生菜单与数据表。只给读屏软件用的文字不在本清单内。

## 已确定的翻译约定

| 类别 | 约定 |
|---|---|
| 模式标签 | Operator → 操作员，Orchestrator → 编排器，Plan → 计划，Draft → 草稿 |
| 推理强度 | Reasoning / Effort → 推理强度；Low → 低，Medium → 中，High → 高，Extra High → 超高，Max → 最高；Thinking → 思考；Fast → 快速；On / Off → 开 / 关；Standard → 标准；Service Tier → 服务等级 |
| 沿用术语 | Worktree → 工作树，Inbox → 收件箱，Skills → 技能，Session → 会话，Agent → 智能体，Provider → 服务商 |
| 保留英文 | MonoCode、GitHub、GitLab、Jira、Linear、Azure DevOps、Claude Code、Codex、OpenCode、Pi、Ultrathink、Ultracode；缩写 CLI / API / PR / CI / MCP / PAT；Token 占位符；快捷键命令 ID（只加中文显示名，不改值）；内部协议报错 |

## 分类总览

| 类别 | 条数 |
|---|---|
| 一、模型参数档位（推理强度、思考、快速、开关） | 70 |
| 二、菜单项与按钮文字 | 209 |
| 三、标题、字段名与状态标签 | 285 |
| 四、鼠标悬停提示 | 119 |
| 五、输入框占位提示 | 21 |
| 六、说明文字、引导语与空状态 | 118 |
| 七、报错、警告与运行状态消息 | 78 |
| 八、原生菜单（Rust）与界面直显的英文数据表 | 约 105 |

## 一、模型参数档位（推理强度、思考、快速、开关）（70）

### `src/features/sessions/model/models.ts`

- L122 Reasoning
- L126 Extra High
- L127 High
- L128 Medium
- L129 Low
- L143 Reasoning（同上，另一处）
- L147 High（同上，另一处）
- L148 Medium（同上，另一处）
- L149 Low（同上，另一处）

### `src/integrations/harness/providers/antigravity/antigravityProtocol.ts`

- L261 Effort

### `src/integrations/harness/providers/claude/claudeCatalog.ts`

- L37 Reasoning
- L41 Low
- L42 Medium
- L43 High
- L44 Max
- L51 Reasoning（同上，另一处）
- L55 Low（同上，另一处）
- L56 Medium（同上，另一处）
- L57 High（同上，另一处）
- L58 Extra High
- L59 Max（同上，另一处）
- L70 Reasoning（同上，另一处）
- L74 Low（同上，另一处）
- L75 Medium（同上，另一处）
- L76 High（同上，另一处）
- L77 Extra High（同上，另一处）
- L78 Max（同上，另一处）
- L85 Fast
- L89 On
- L90 Off
- L96 Thinking
- L100 On（同上，另一处）
- L101 Off（同上，另一处）
- L108 Context
- L184 Reasoning（同上，另一处）
- L188 Low（同上，另一处）
- L189 Medium（同上，另一处）
- L190 High（同上，另一处）
- L191 Max（同上，另一处）
- L419 Reasoning（同上，另一处）

### `src/integrations/harness/providers/codex/codexCatalog.ts`

- L198 Reasoning
- L212 Standard
- L236 Service Tier

### `src/integrations/harness/providers/cursor/cursorCatalog.ts`

- L257 Effort
- L269 Thinking
- L273 Off
- L274 On
- L281 Fast
- L285 Off（同上，另一处）
- L286 Fast（同上，另一处）
- L363 Off（同上，另一处）
- L364 On（同上，另一处）

### `src/integrations/harness/providers/fx/fxProtocol.ts`

- L440 Effort
- L449 Fast
- L453 On
- L454 Off

### `src/integrations/harness/providers/grok/grokProtocol.ts`

- L487 Extra High
- L488 High
- L489 Medium
- L490 Low
- L496 High（同上，另一处）
- L497 Medium（同上，另一处）
- L498 Low（同上，另一处）
- L569 Reasoning

### `src/integrations/harness/providers/opencode/opencodeCatalog.ts`

- L241 Variant
- L251 Agent

### `src/integrations/harness/providers/pi/piProtocol.ts`

- L755 Thinking
- L768 Fast
- L773 On
- L774 Off

## 二、菜单项与按钮文字（209）

### `src/app/App.tsx`

- L9403 Fix CI #{…}: {…}

### `src/app/shell/MenuBar.tsx`

- L370 File
- L371 View
- L372 Terminal

### `src/app/shell/Sidebar.tsx`

- L877 Rename
- L879 Ungroup
- L887 Cancel reminder
- L901 Unpin
- L901 Pin
- L910 Rename（同上，另一处）
- L929 Remind me
- L934 New folder
- L939 Add to {…}
- L952 Remove from folders
- L953 Remove from folder
- L968 Unarchive
- L968 Archive
- L977 Delete

### `src/app/shell/TitleBar.tsx`

- L550 Back ({…}[)
- L557 Forward ({…}])
- L588 Back ({…}[)（同上，另一处）

### `src/app/shell/UsageProviderChip.tsx`

- L407 Switch
- L504 Add account
- L514 Manage accounts…
- L564 Add
- L883 Applying…
- L903 Use reset
- L918 Cancel
- L925 Confirm
- L945 Loading usage…

### `src/features/files/ui/BinaryFileView.tsx`

- L99 Opening
- L237 Copy original file
- L280 Copy Original File
- L344 Retry
- L349 Reveal
- L352 Copy path

### `src/features/files/ui/EditorSelectionMenu.tsx`

- L64 Add to chat

### `src/features/files/ui/FileActionError.tsx`

- L23 Dismiss

### `src/features/files/ui/FileEditor.tsx`

- L415 Opening
- L437 Retry
- L525 Saved

### `src/features/files/ui/FilePicker.tsx`

- L38 Reload MonoCode

### `src/features/files/ui/FileTree.tsx`

- L173 New File
- L174 New Folder
- L179 Cut
- L186 Copy
- L193 Paste
- L200 Duplicate
- L204 Copy Path
- L205 Copy Relative Path
- L210 Rename
- L217 Delete
- L228 Open in Terminal

### `src/features/inbox/ui/CheckEvidence.tsx`

- L45 Show

### `src/features/inbox/ui/CheckRepairForm.tsx`

- L169 Fix with AI
- L304 Start fix

### `src/features/inbox/ui/CheckRepairProgress.tsx`

- L378 Show check
- L388 Open conversation

### `src/features/inbox/ui/InboxConnectMenu.tsx`

- L53 Connect

### `src/features/inbox/ui/InboxFiltersMenu.tsx`

- L60 Issues
- L65 Pull requests

### `src/features/inbox/ui/InboxNotificationMenu.tsx`

- L57 Mark all as read
- L64 Mute all projects
- L71 Resume muted projects
- L77 Notification settings…
- L92 Mute all projects（同上，另一处）

### `src/features/inbox/ui/InboxPrChecks.tsx`

- L113 Checks
- L414 Loading steps…
- L426 Retry details
- L444 View run steps
- L570 Retry

### `src/features/inbox/ui/InboxView.tsx`

- L1307 Close {…} panel
- L1341 Open on GitHub
- L1759 Merge pull request
- L1784 Ready for review
- L1795 Convert to draft
- L1806 Close pull request
- L1817 Reopen pull request
- L1909 Cancel
- L2552 Merge request
- L2660 Created
- L3123 Choose project

### `src/features/inbox/ui/LinkedWorkItemUpdateNotice.tsx`

- L255 Archive session
- L271 Delete…

### `src/features/notifications/ui/NotificationMuteControl.tsx`

- L67 Resume notifications
- L83 Muted
- L83 Mute
- L99 Mute all notifications for

### `src/features/notifications/ui/NotificationMuteDatePicker.tsx`

- L66 Mute all notifications until
- L88 Cancel
- L95 Mute until then

### `src/features/notifications/ui/notificationMuteActions.ts`

- L28 Until resumed
- L31 Choose date and time

### `src/features/orchestration/ui/OrchestrationPreview.tsx`

- L549 Try again
- L561 Starting…
- L561 Confirm & start
- L578 View agents
- L721 Show fewer tasks

### `src/features/orchestration/ui/OrchestrationSidebarAgents.tsx`

- L217 See details
- L231 Cancel task
- L271 Open blocker
- L297 Resume

### `src/features/projects/ui/CwdPicker.tsx`

- L283 Move to project
- L367 New terminal

### `src/features/projects/ui/SearchableProjectPicker.tsx`

- L378 New project

### `src/features/sessions/ui/AgentMarkdown.tsx`

- L140 Open in MonoCode
- L143 Open in Default App
- L146 Copy Path
- L152 Copy Relative Path

### `src/features/sessions/ui/AgentTranscript.tsx`

- L642 Load earlier messages
- L1741 Show less
- L1741 Show more
- L1776 Remove
- L1785 Send
- L3543 Allow
- L3550 Deny
- L3686 Show less（同上，另一处）
- L3686 Show more（同上，另一处）

### `src/features/sessions/ui/ApprovalToasts.tsx`

- L128 Allow
- L135 Deny

### `src/features/sessions/ui/BtwSheet.tsx`

- L817 Retry

### `src/features/sessions/ui/Composer.tsx`

- L363 Resume
- L433 Steer
- L1831 Drop files to attach
- L2028 Add to message

### `src/features/sessions/ui/ContextMeter.tsx`

- L92 Compact now

### `src/features/sessions/ui/ModelPicker.tsx`

- L1388 Loading Codex models…

### `src/features/sessions/ui/PlanPreview.tsx`

- L93 Open

### `src/features/sessions/ui/QuestionForm.tsx`

- L128 Skip
- L184 Continue
- L359 Other

### `src/features/sessions/ui/ReminderNotices.tsx`

- L93 Retry
- L120 Open session
- L133 Snooze
- L141 Dismiss

### `src/features/sessions/ui/SessionFiltersMenu.tsx`

- L25 All time
- L26 Today
- L27 Last 7 days
- L28 Last 30 days
- L136 Clear filters

### `src/features/sessions/ui/SessionReview.tsx`

- L138 Changed
- L163 Undo
- L172 Keep
- L180 Review
- L214 Show fewer files

### `src/features/sessions/ui/UsageLimitNotice.tsx`

- L44 Resume
- L64 Resume at reset

### `src/features/sessions/ui/sessionReminderPresets.ts`

- L20 This evening (18:00)
- L23 Tomorrow (9:00)
- L24 Next week (Mon 9:00)

### `src/features/settings/ui/JiraSettings.tsx`

- L99 Checking Jira connection…
- L107 Disconnecting
- L107 Disconnect
- L171 Connecting
- L171 Connect
- L182 Create API token
- L202 Refresh projects

### `src/features/settings/ui/SettingsView.tsx`

- L1612 Disconnect
- L1648 Connect
- L1757 Disconnect（同上，另一处）
- L1780 Connect（同上，另一处）
- L1900 Download
- L1900 Check for updates
- L3175 Cancel
- L3182 Use auto-detected path
- L3186 Save path
- L3201 Checking version…
- L3233 Retry configured path
- L3233 Retry auto-detect
- L3252 Open location
- L3260 Edit path
- L3538 Remove {…} account
- L3609 Add account
- L3764 Cancel（同上，另一处）
- L3775 Sign in and add
- L3776 Save
- L3981 Restore
- L3986 Delete
- L4052 Unarchive
- L4058 Delete（同上，另一处）
- L4366 Open System Settings

### `src/features/source-control/ui/GitChangesPanel.tsx`

- L725 Commit & Push
- L1054 Create PR

### `src/features/source-control/ui/WorktreePicker.tsx`

- L162 Loading…

### `src/features/terminal/ui/ProjectTerminalDock.tsx`

- L50 Dock Bottom
- L51 Dock Top
- L52 Dock Left
- L53 Dock Right
- L218 New Terminal ({…}`)

### `src/features/workspace/ui/SurfaceTabs.tsx`

- L73 Close
- L78 Close Others
- L86 Open in Default App
- L89 Copy Path
- L93 Copy Relative Path
- L95 Copy File Name
- L118 Changes
- L127 Session Changes

### `src/features/workspace/ui/TabGroupMenu.tsx`

- L96 New tab in group
- L102 Move group to new window
- L107 Close group
- L113 Ungroup
- L118 Delete group

### `src/features/workspace/ui/WorkspacePicker.tsx`

- L427 Loading worktrees…

### `src/integrations/harness/providers/codex/codexProtocol.ts`

- L1113 Edit
- L1155 Edit（同上，另一处）
- L1181 Edit（同上，另一处）

### `src/integrations/harness/providers/opencode/opencode.ts`

- L1191 OpenCode question

### `src/integrations/harness/providers/pi/piProtocol.ts`

- L277 Choose an option
- L285 Confirm

## 三、标题、字段名与状态标签（285）

### `src/app/App.tsx`

- L3796 Ask · {…}

### `src/app/shell/Sidebar.tsx`

- L891 Multiple reminder times
- L1401 Workspace
- L1494 No project folder
- L1526 No project folder（同上，另一处）

### `src/app/shell/TitleBar.tsx`

- L516 Development
- L594 Toggle Sidebar ({…}B)
- L891 Settings ({…},)

### `src/app/shell/UpdateRailCard.tsx`

- L33 Updated to
- L36 What's new

### `src/app/shell/UsageFooter.tsx`

- L460 Terminal
- L545 sign in

### `src/app/shell/UsageProviderChip.tsx`

- L335 Updating
- L570 Account name
- L590 Waiting for browser…
- L654 % remaining
- L715 Banked resets
- L871 Expiry not provided
- L873 Expires now

### `src/features/automations/model/automationTemplates.ts`

- L4 Popular
- L5 Code Review
- L6 Security
- L7 Incidents & Triage
- L8 Data & Research
- L9 Environment

### `src/features/automations/ui/AutomationsView.tsx`

- L826 Test run
- L856 Scheduled
- L1343 Search triggers
- L1414 No matching triggers
- L1460 Instructions
- L1512 Session
- L1515 Working copy
- L1537 Conversation
- L1551 Session folder
- L1571 Advanced
- L1581 Missed-run grace
- L1598 Run history
- L1602 Trigger
- L1603 Triggered
- L1604 Status
- L1605 Duration
- L1791 Scheduled（同上，另一处）
- L1806 Hourly
- L1807 Daily
- L1808 Weekdays
- L1809 Weekly
- L1812 Draft opened
- L1813 Pull request opened
- L1814 Issue opened
- L1816 Issue created
- L1817 Issue appeared
- L1819 Merge request opened
- L1820 Issue opened（同上，另一处）
- L1823 Pull request appeared
- L1824 Work item appeared

### `src/features/files/ui/BinaryFileView.tsx`

- L237 Copied
- L248 Zoom out
- L261 Fit
- L264 Zoom in

### `src/features/files/ui/FileEditor.tsx`

- L451 Staged
- L451 Unstaged
- L523 Saving…

### `src/features/files/ui/FilePreview.tsx`

- L146 Empty file

### `src/features/files/ui/FilePreviewSearch.tsx`

- L406 Previous Match
- L414 Next Match
- L422 Close

### `src/features/files/ui/FileTree.tsx`

- L874 New File
- L877 New Folder
- L881 Collapse All
- L894 Search in files ({…}Shift+F)

### `src/features/inbox/model/githubPrChecks.ts`

- L163 No checks reported
- L169 No checks reported（同上，另一处）
- L175 No checks reported（同上，另一处）
- L181 No checks reported（同上，另一处）
- L187 No checks reported（同上，另一处）

### `src/features/inbox/ui/CheckEvidence.tsx`

- L45 more annotations

### `src/features/inbox/ui/CheckRepairForm.tsx`

- L178 · PR #
- L255 Untitled chat
- L268 No matching chats
- L290 CI details included
- L304 Preparing...

### `src/features/inbox/ui/CheckRepairProgress.tsx`

- L174 Repairing
- L180 Awaiting CI
- L186 Refreshing
- L192 Out of date
- L198 CI passed
- L210 CI running
- L216 CI cancelled
- L222 CI skipped
- L228 Unknown
- L234 Stopped
- L246 Agent stopped
- L397 Included checks
- L410 Latest PR commit:

### `src/features/inbox/ui/InboxDiscussionPanel.tsx`

- L77 Ask ·
- L80 Restart conversation
- L95 Close panel

### `src/features/inbox/ui/InboxNotificationMenu.tsx`

- L113 Inbox

### `src/features/inbox/ui/InboxPrChecks.tsx`

- L741 No checks reported

### `src/features/inbox/ui/InboxView.tsx`

- L1025 Nothing needs your attention
- L1585 Create a merge commit
- L1590 Squash and merge
- L1595 Rebase and merge
- L2553 Pull request
- L2554 Issue
- L2667 Updated

### `src/features/inbox/ui/LinkedWorkItemUpdateNotice.tsx`

- L149 GitHub activity
- L217 No message
- L239 Clean up this session

### `src/features/notifications/model/approvalToast.ts`

- L24 Question

### `src/features/notifications/model/notificationPreferences.ts`

- L2 Pull requests / Merge requests
- L3 Issues and Linear tasks
- L4 Agent finished
- L5 Agent approvals and questions
- L6 Reminders

### `src/features/notifications/ui/ProjectNotificationSettings.tsx`

- L252 Select {…}
- L308 Local project ·
- L311 All notifications paused
- L313 All categories enabled

### `src/features/orchestration/ui/OrchestrationPreview.tsx`

- L310 No matching models
- L521 Planning assignments…
- L532 Lead ·
- L664 Task
- L675 Instructions
- L677 Instructions for task {…}
- L691 After ·
- L735 Parallel workers
- L775 Approved
- L777 Awaiting confirmation
- L779 · Shared project folder

### `src/features/orchestration/ui/OrchestrationSidebarAgents.tsx`

- L259 Another conversation

### `src/features/projects/ui/CwdPicker.tsx`

- L268 Current project
- L283 Recent projects
- L287 No other projects
- L342 More Projects

### `src/features/projects/ui/ProjectSearch.tsx`

- L144 No project folder
- L161 Search in files
- L220 Searching…
- L227 No results
- L229 (limited)

### `src/features/projects/ui/SearchableProjectPicker.tsx`

- L274 Search projects
- L363 No projects found

### `src/features/search/ui/SearchView.tsx`

- L47 All
- L48 Conversations
- L49 Files
- L50 Projects

### `src/features/sessions/model/monocodeToolCall.ts`

- L99 View CLI commands

### `src/features/sessions/ui/AgentTranscript.tsx`

- L701 Waiting for orchestrator
- L703 Waiting for answers
- L987 Thinking…
- L987 Thinking…（同上，另一处）
- L1751 CI context
- L1765 Draft

### `src/features/sessions/ui/ApprovalToasts.tsx`

- L113 Question
- L113 Approval

### `src/features/sessions/ui/Composer.tsx`

- L2011 Add files or choose a mode
- L2175 Operator
- L2192 Orchestrator
- L2208 Plan
- L2224 Draft

### `src/features/sessions/ui/FileMentionPicker.tsx`

- L136 Note

### `src/features/sessions/ui/MarkdownModeToggle.tsx`

- L40 Preview
- L45 Source

### `src/features/sessions/ui/ModelPicker.tsx`

- L735 Model
- L1384 No favorite models
- L1389 No matching models

### `src/features/sessions/ui/QuestionForm.tsx`

- L175 Optional question
- L263 Select all that apply

### `src/features/sessions/ui/ReminderNotices.tsx`

- L83 Due reminders

### `src/features/sessions/ui/SecondOpinionCard.tsx`

- L18 Handoff
- L18 Second opinion

### `src/features/sessions/ui/SessionFiltersMenu.tsx`

- L77 Archived
- L82 Status
- L84 Working
- L89 Needs approval
- L94 Done
- L99 Time
- L111 Provider

### `src/features/sessions/ui/SessionReview.tsx`

- L259 Mixed changes

### `src/features/sessions/ui/TaskListPreview.tsx`

- L24 Tasks

### `src/features/sessions/ui/TranscriptFind.tsx`

- L146 No results
- L150 Previous match
- L157 Next match
- L163 Close find

### `src/features/sessions/ui/UsageLimitNotice.tsx`

- L39 Limit has reset
- L54 Resuming at reset

### `src/features/sessions/ui/UserLinkPreview.tsx`

- L370 Pull request
- L370 Issue
- L394 Updated
- L529 Pull request（同上，另一处）
- L529 Issue（同上，另一处）
- L536 Draft
- L543 Merged
- L550 Closed
- L556 Open

### `src/features/sessions/ui/sessionReminderPresets.ts`

- L11 In 1 hour ({…})
- L15 In 3 hours ({…})

### `src/features/settings/model/settings.ts`

- L152 Project worktrees
- L158 Version
- L164 Sounds
- L170 Notifications
- L176 Notes
- L184 Quick composer
- L192 Working agents
- L198 File tabs
- L204 Tab animations
- L212 Close to tray
- L220 Theme
- L226 Accent color
- L232 Hue
- L238 Saturation
- L244 Dark-mode lightness
- L250 Sidebar opacity
- L256 Blur radius
- L262 Main pane glass
- L268 Interface scale
- L274 Collapsed project rail
- L280 Show excluded files
- L286 Chat background
- L292 Transcript layout
- L298 Anchor prompts to top
- L304 Follow-up behavior
- L310 Model controls
- L317 Composer mascot
- L323 Format on save
- L329 Diff view
- L335 Empty session games
- L341 Agent CLIs
- L347 Provider accounts
- L353 Claude Code hooks
- L359 Project notifications
- L395 Show archived in the sidebar

### `src/features/settings/ui/JiraSettings.tsx`

- L125 Jira site
- L132 Atlassian email
- L139 Jira API token
- L196 Projects

### `src/features/settings/ui/SettingsView.tsx`

- L676 No matching settings
- L696 Page
- L920 Notes
- L928 Quick composer
- L960 Close to tray
- L1600 Connection
- L1648 Saving
- L1780 Saving（同上，另一处）
- L1792 Teams
- L1877 Version
- L1890 What's new
- L2172 Theme
- L2284 Sidebar opacity
- L2452 Empty chat preview at
- L3122 Global path
- L3126 Needs attention
- L3130 Configured
- L3131 Auto-detected
- L3141 CLI path
- L3652 Provider CLI profile
- L3653 Isolated profile
- L3659 Default
- L3745 Account name
- L3774 Waiting for browser…
- L3851 Default（同上，另一处）
- L3864 {…} model

### `src/features/source-control/ui/GitChangesPanel.tsx`

- L783 No uncommitted changes

### `src/features/source-control/ui/WorktreePicker.tsx`

- L161 No repo

### `src/features/terminal/ui/TerminalGridBackground.tsx`

- L536 game over
- L585 take control

### `src/features/workspace/ui/SurfaceTabs.tsx`

- L139 {…} — orchestration agent

### `src/features/workspace/ui/TabGroupMenu.tsx`

- L268 Project logo
- L312 Mascot

### `src/features/workspace/ui/WorkspacePicker.tsx`

- L312 Workspace
- L374 Existing worktree…
- L396 Worktree settings
- L459 No existing worktrees
- L550 From {…}
- L641 No matching branches

### `src/integrations/harness/providers/claude/claudeProtocol.ts`

- L622 Subagent
- L652 Subagent（同上，另一处）
- L749 Subagent（同上，另一处）

### `src/integrations/harness/providers/codex/codexCatalog.ts`

- L219 Fast

### `src/integrations/harness/providers/codex/codexProtocol.ts`

- L767 Find {…}
- L773 Read {…}
- L779 List

### `src/integrations/harness/providers/fx/fxTool.ts`

- L114 List {…}

### `src/shared/ui/DateTimePicker.tsx`

- L243 Time
- L246 Local time, 24-hour

## 四、鼠标悬停提示（119）

### `src/app/shell/ProjectRail.tsx`

- L789 Group options
- L992 Unpin project
- L992 Pin project
- L1036 {…} uncommitted

### `src/app/shell/Sidebar.tsx`

- L2861 Linked {…} updated since this session
- L2871 Open {…} #{…} beside this session ({…}-click for GitHub)
- L3200 Started by an automation
- L3398 {…} uncommitted

### `src/app/shell/TitleBar.tsx`

- L378 Unsaved changes
- L393 Close Tab
- L513 Development build

### `src/app/shell/UsageFooter.tsx`

- L424 Refresh usage
- L538 {…} sign-in required

### `src/app/shell/UsageProviderChip.tsx`

- L193 Not connected
- L195 Loading usage…
- L196 Usage details

### `src/app/shell/WindowControls.tsx`

- L61 Minimize
- L71 Restore
- L71 Maximize
- L85 Close

### `src/features/automations/ui/AutomationsView.tsx`

- L743 Open session
- L744 This run has no session yet

### `src/features/files/ui/BinaryFileView.tsx`

- L109 Couldn’t open {…}
- L257 Fit to window

### `src/features/files/ui/FileEditor.tsx`

- L1047 Previous change
- L1061 Next change

### `src/features/files/ui/FilePreviewSearch.tsx`

- L389 Match Case ({…}C)
- L395 Match Whole Word ({…}W)
- L401 Use Regular Expression ({…}R)
- L407 Previous Match ({…}{…}G)
- L415 Next Match ({…}G)
- L423 Close (Escape)

### `src/features/inbox/ui/CheckEvidence.tsx`

- L131 View source at the checked commit

### `src/features/inbox/ui/CheckRepairForm.tsx`

- L253 Untitled chat

### `src/features/inbox/ui/InboxComments.tsx`

- L294 Open in Linear
- L296 Open in Jira
- L298 Open on GitLab
- L300 Open on ADO
- L301 Open on GitHub

### `src/features/inbox/ui/InboxMiniCard.tsx`

- L35 Open in {…}
- L79 Remove

### `src/features/inbox/ui/InboxPrChecks.tsx`

- L361 Fix with AI
- L389 View full log on GitHub
- L564 Retry loading checks
- L652 Refresh checks

### `src/features/inbox/ui/InboxView.tsx`

- L1518 {…} related {…}
- L1764 Merge options
- L2570 No link available
- L2707 Open thread: {…}
- L2789 No link available（同上，另一处）
- L2999 Copied
- L2999 Copy branch name

### `src/features/inbox/ui/LinkedWorkItemUpdateNotice.tsx`

- L154 Dismiss
- L244 Archive session
- L260 Delete session

### `src/features/notifications/ui/NotificationMuteControl.tsx`

- L78 Mute pauses all project notifications without changing your category choices.

### `src/features/orchestration/ui/OrchestrationSidebarAgents.tsx`

- L206 Open this agent beside the orchestrator
- L280 Wait for interrupted agents to stop
- L282 Wait for the lead's interrupted turn to finish
- L285 Continue interrupted and queued work

### `src/features/projects/ui/ProjectSearch.tsx`

- L154 Back to files

### `src/features/sessions/ui/AgentMarkdown.tsx`

- L371 Copied
- L371 Copy code

### `src/features/sessions/ui/AgentTranscript.tsx`

- L1770 Remove draft
- L1780 Send draft
- L2501 Model: {…}

### `src/features/sessions/ui/AttachmentChip.tsx`

- L30 Open {…} full screen
- L57 Remove

### `src/features/sessions/ui/BtwSheet.tsx`

- L750 Delete
- L750 Discard
- L766 New side question
- L776 Back to the conversation (Esc)

### `src/features/sessions/ui/Composer.tsx`

- L402 Save queued message
- L414 Cancel queued message edit
- L437 Edit queued message
- L446 Remove queued message
- L2284 Stop editing last message
- L2432 Stop

### `src/features/sessions/ui/ContextMeter.tsx`

- L56 Context usage
- L83 Wait for the current operation to finish
- L84 Compact this conversation's context

### `src/features/sessions/ui/ModelPicker.tsx`

- L637 {…} · Recent models: right-click or {…}.
- L1337 Favorites
- L1453 Remove from favorites
- L1454 Add to favorites

### `src/features/sessions/ui/PlanPreview.tsx`

- L87 Open in pane
- L100 Build this plan

### `src/features/sessions/ui/QuestionForm.tsx`

- L172 Interact to keep this question open.

### `src/features/sessions/ui/SessionFolderPicker.tsx`

- L95 Cancel

### `src/features/sessions/ui/SessionPane.tsx`

- L696 Close Pane ({…}W)
- L893 Jump to latest

### `src/features/sessions/ui/SessionReview.tsx`

- L154 Undo all session changes
- L156 Undo is unavailable while another session is running in this project
- L157 Undo is unavailable because a file changed outside this session
- L167 Keep all session changes and dismiss this card
- L176 Review changes

### `src/features/sessions/ui/UsageLimitNotice.tsx`

- L49 Cancel the automatic resume
- L59 Continue this session once the limit resets
- L69 Dismiss

### `src/features/settings/ui/SettingsView.tsx`

- L3070 {…} CLI path{…}
- L3395 Agent CLIs
- L3454 Advanced
- L3576 Accounts
- L3666 Rename account
- L3677 Remove account
- L3998 Archived conversations

### `src/features/workspace/ui/SurfaceTabs.tsx`

- L120 Working tree changes
- L129 Changes captured for this session only
- L270 Drag to reorder pane
- L391 Unsaved changes
- L398 Close {…}

### `src/features/workspace/ui/TabGroupMenu.tsx`

- L242 Change project logo
- L242 Add project logo
- L276 Remove project logo

### `src/features/workspace/ui/WorkspacePicker.tsx`

- L143 Workspace: {…}
- L382 Open worktree settings
- L544 Create from {…}

### `src/shared/ui/ColorPickerPopover.tsx`

- L68 Custom color

### `src/shared/ui/ImageLightbox.tsx`

- L58 Close

## 五、输入框占位提示（21）

### `src/app/shell/UsageProviderChip.tsx`

- L577 Work or Personal

### `src/features/automations/ui/AutomationsView.tsx`

- L1348 Search triggers

### `src/features/files/ui/FilePicker.tsx`

- L225 Go to File (type > for commands)

### `src/features/files/ui/FilePreviewSearch.tsx`

- L368 Find

### `src/features/inbox/ui/CheckRepairForm.tsx`

- L202 Search chats...

### `src/features/orchestration/ui/OrchestrationPreview.tsx`

- L235 Search models or harnesses…

### `src/features/projects/ui/ProjectSearch.tsx`

- L171 Search
- L201 files to include
- L209 files to exclude

### `src/features/projects/ui/SearchableProjectPicker.tsx`

- L282 Search projects...

### `src/features/sessions/ui/BtwSheet.tsx`

- L842 Ask a side question…

### `src/features/sessions/ui/ModelPicker.tsx`

- L1366 Search models

### `src/features/sessions/ui/QuestionForm.tsx`

- L269 Type your answer
- L337 Type your answer（同上，另一处）

### `src/features/sessions/ui/SessionFolderPicker.tsx`

- L66 Choose or name a session folder…

### `src/features/sessions/ui/TranscriptFind.tsx`

- L122 Find in conversation

### `src/features/settings/ui/JiraSettings.tsx`

- L143 API token

### `src/features/settings/ui/SettingsView.tsx`

- L635 Search settings
- L3148 Auto-detected path
- L3752 Work or Personal

### `src/features/workspace/ui/WorkspacePicker.tsx`

- L570 Search base branches…

## 六、说明文字、引导语与空状态（118）

### `src/app/App.tsx`

- L6018 Orchestration plan

### `src/app/model/updater.ts`

- L81 Update available

### `src/app/shell/Sidebar.tsx`

- L921 Edit GitHub issue or PR link…
- L922 Link GitHub issue or PR…

### `src/app/shell/UsageProviderChip.tsx`

- L451 Each conversation stays pinned to the account that started it.
- L567 Give this account a local name, then finish sign-in in your browser.
- L590 Sign in and add account
- L910 Spend this reset now?

### `src/app/ui/ReleaseNotesSurface.tsx`

- L29 Release notes for this version are not available in this build.

### `src/features/automations/model/automationTemplates.ts`

- L54 Analyze recent commits for high-severity correctness bugs and submit safe fixes
- L80 Review the full repository on a schedule and alert on validated high-impact security issues
- L106 Create and update developer documentation for recently changed or under-documented code
- L131 Review recent changes and add tests for high-risk logic that lacks adequate coverage
- L154 When a pull request is opened, review the diff for bugs, regressions, and missing tests
- L176 Give early feedback when a draft pull request is opened so issues are caught before review
- L192 Check lockfiles and manifests for vulnerable, abandoned, or unexpectedly upgraded packages
- L216 Search the working tree and recent history for committed credentials, tokens, and keys
- L235 When a GitHub issue is opened, inspect the repo and add a concrete reproduction or next step
- L254 When a Linear issue is created, inspect the repo and add a concrete reproduction or next step
- L273 On a weekday morning, run the project's tests and diagnose anything that is already red
- L296 Summarize the week's commits into a changelog humans can actually read
- L315 Inspect the working tree, stale branches, and obvious project-setup drift on a schedule
- L339 Verify the project still installs and boots from a clean working copy

### `src/features/automations/ui/AutomationsView.tsx`

- L1507 Skills, @file references, and built-in commands work here.
- L1516 This repo, or a fresh worktree
- L1538 New chat, or continue the last run
- L1552 Where runs appear in the sidebar
- L1574 Catch-up window for missed runs
- L1582 Catch up if a scheduled run was missed
- L1620 This automation has not run yet.

### `src/features/files/ui/BinaryFileView.tsx`

- L123 {…} · not a readable image

### `src/features/files/ui/FileEditor.tsx`

- L451 line-ending changes. Line breaks are normalized in this view.

### `src/features/inbox/model/githubPrChecks.ts`

- L145 Loading checks

### `src/features/inbox/ui/CheckRepairForm.tsx`

- L60 New project chat

### `src/features/inbox/ui/CheckRepairProgress.tsx`

- L175 Repair in progress
- L181 Awaiting new GitHub checks
- L187 Refreshing GitHub checks
- L193 GitHub results are out of date
- L235 Repair stopped

### `src/features/inbox/ui/InboxComments.tsx`

- L99 Latest comments · more on

### `src/features/inbox/ui/InboxNotificationMenu.tsx`

- L116 {…} {…} · {…} muted

### `src/features/inbox/ui/InboxPrChecks.tsx`

- L477 No steps reported for this job.
- L689 Saved results may be out of date.

### `src/features/inbox/ui/InboxView.tsx`

- L1019 No matching issues or merge requests
- L1020 No matching issues or pull requests
- L1027 No GitLab items match these filters
- L1028 No ADO items match these filters
- L1029 No issues or pull requests match these filters
- L1034 Open a project to fill the inbox
- L1035 No matching issues or merge requests（同上，另一处）
- L1037 Open a project to fill the inbox（同上，另一处）
- L1038 No matching issues or pull requests（同上，另一处）
- L1586 Add every commit to the base branch.
- L1591 Combine the commits into one.
- L1596 Add the commits without a merge commit.

### `src/features/notes/ui/NotesView.tsx`

- L193 Untitled

### `src/features/notifications/hooks/useSessionReminders.ts`

- L96 Reminder

### `src/features/notifications/ui/ProjectNotificationSettings.tsx`

- L345 Your category choices apply when notifications resume. You can edit them while muted.

### `src/features/orchestration/ui/OrchestrationPreview.tsx`

- L399 How many workers run at once
- L402 The rest of the tasks wait their turn, and a task that depends on another waits for it either way. Every worker edits this same project fold
- L586 Your lead is choosing tasks and worker models. Review the assignments here before starting.
- L587 Checking available harnesses and models…

### `src/features/orchestration/ui/OrchestrationSidebarAgents.tsx`

- L259 is still running in this project.

### `src/features/projects/model/chatBackground.ts`

- L8 Choose chat background
- L30 Choose project chat background

### `src/features/projects/model/projectLogos.ts`

- L16 Choose project logo

### `src/features/projects/ui/ProjectSearch.tsx`

- L232 Type to search across the project

### `src/features/sessions/model/btw.ts`

- L177 Ask a read-only side question about the current turn.

### `src/features/sessions/model/compact.ts`

- L7 Summarize older conversation context to free space.

### `src/features/sessions/model/operatorCommand.ts`

- L9 Give this thread access to MonoCode sessions, folders, and notes.

### `src/features/sessions/model/plan.ts`

- L7 Create a reviewable implementation plan before changing files.

### `src/features/sessions/model/sessionFolderCommand.ts`

- L7 Place this session in an existing or new sidebar folder.

### `src/features/sessions/ui/AccessPicker.tsx`

- L172 Access changes apply to the next turn. Stop and resend to apply them now.

### `src/features/sessions/ui/AgentTabView.tsx`

- L144 Run by the orchestrator · read-only

### `src/features/sessions/ui/ReminderNotices.tsx`

- L152 Desktop alerts are off. Enable in Settings.

### `src/features/sessions/ui/SessionFolderPicker.tsx`

- L111 Type a name to create the first folder

### `src/features/sessions/ui/SessionPane.tsx`

- L746 Explore this item with your agent.

### `src/features/sessions/ui/UserLinkPreview.tsx`

- L378 Details aren&apos;t available here, but the link can still be opened on GitHub.
- L449 Click the chip to open on GitHub


### `src/features/settings/ui/JiraSettings.tsx`

- L119 Connect your Jira Cloud site using your Atlassian email and an API token without scopes. Disconnect deletes the saved credentials.
- L206 Unchecked projects stay out of the inbox.

### `src/features/settings/ui/SettingsView.tsx`

- L862 Not available on this platform
- L921 A global markdown notebook on the project rail. Save a finished turn from the transcript, then mention it later with @note or add it to chat
- L929 Press {…} in any app to float a prompt over it and start a session without switching to MonoCode. Change the shortcut in Keybindings. Return
- L961 Closing a window hides it to the system tray instead of quitting, so running agents keep going. Reopen from the tray icon, and quit for real
- L1276 Pull requests, reviews, and issues, read through the GitHub CLI.
- L1289 Merge requests from GitLab.com or a self-managed instance.
- L1305 Pull requests and Boards work items from your ADO organization.
- L1318 Jira Cloud issues from the projects you pick.
- L1331 Issues assigned to you, from the teams you pick.
- L1476 Connect GitLab.com or a self-managed GitLab instance. Use a personal access token with API access; the token is stored locally and Disconnec
- L1601 Connect your ADO organization with a personal access token (Boards + Repos read & write for comments). The token is stored locally and Disco
- L1753 Create a personal API key in Linear → Settings → Security & Access. Disconnect deletes it.
- L1794 Unchecked teams stay out of the inbox.
- L2173 System follows the OS appearance.
- L2192 Used for the composer send button and your message bubbles.
- L2252 This only affects dark mode. Your dark-mode value is preserved.
- L2275 Light mode always uses an opaque window, so these are off. Your dark-mode values are preserved.
- L2285 Applies to the project rail and the other glass panes.
- L2525 Empty sessions only, or every conversation.
- L2551 Background strength before a chat has messages.
- L2567 Background strength once the conversation has messages.
- L2751 Del disables · Esc cancels
- L3155 Enter the absolute path to the CLI executable. Changes apply after restarting MonoCode.
- L3197 Checking the selected CLI…
- L3201 Retry to check this CLI
- L3410 A provider is listed as installed once its CLI is found on your PATH. Uninstalled CLIs stay listed but are left out of the model picker, as 
- L3541 Cancel
- L3577 Create isolated sign-ins for providers that support account profiles. Account switching stays available from the usage control in the footer
- L4020 Open a project to see its archived conversations.

### `src/features/source-control/ui/GitChangesPanel.tsx`

- L734 Commit, Push & Create PR

### `src/features/source-control/ui/UnifiedDiffView.tsx`

- L236 Diff is too large to display in full. File list is shown without patches.

### `src/features/source-control/ui/WorktreesPage.tsx`

- L230 New worktrees are created in

### `src/features/workspace/ui/TabGroupMenu.tsx`

- L270 Shown in tabs and composer
- L270 Optional — replaces folder icon

### `src/integrations/harness/providers/antigravity/antigravity.ts`

- L709 Antigravity ended the turn ({…}).

### `src/integrations/harness/providers/pi/piProtocol.ts`

- L769 Use priority processing when the current model supports it

### `src/integrations/harness/providers/pi/piSubagents.ts`

- L35 Delegate subagents

## 七、报错、警告与运行状态消息（78）

### `src/app/App.tsx`

- L5660 Session is unavailable or already running
- L5781 This conversation uses a removed provider account. Switch accounts from the usage control to start a new conversation.
- L5797 Use /operator from a regular session turn, outside an orchestration run.
- L5885 {…} cannot take a follow-up mid-turn — wait for this turn to finish, or stop it first.
- L5975 The chat became unavailable before the request could start. Try again when it is ready.
- L6139 {…} is not connected yet — install and sign in to that provider, then retry.
- L6238 Harness is not connected
- L6269 Turn did not complete
- L7248 Orchestration planning will start after the current turn finishes.
- L7576 A project working directory is required for this question.
- L7593 The selected Codex model is unavailable.
- L7621 The completed turn is no longer available.
- L8139 {…} does not support manual context compaction.
- L8156 Compacting context…
- L8185 Compacted context

### `src/app/shell/UsageProviderChip.tsx`

- L204 not connected
- L342 Couldn’t refresh. Showing the last available snapshot.
- L947 Not connected
- L948 Usage unavailable

### `src/features/automations/ui/AutomationsView.tsx`

- L1406 Not connected

### `src/features/files/ui/FileEditor.tsx`

- L426 Couldn’t open
- L531 Save failed:

### `src/features/files/ui/FilePane.tsx`

- L290 This plan is no longer in the session.

### `src/features/inbox/model/githubPrChecks.ts`

- L154 Checks failed to load

### `src/features/inbox/ui/CheckEvidence.tsx`

- L177 Source preview unavailable for this commit.

### `src/features/inbox/ui/CheckRepairForm.tsx`

- L282 Wait for the latest checks before starting a fix.

### `src/features/inbox/ui/CheckRepairProgress.tsx`

- L204 Still failing
- L205 still failing
- L240 Interrupted
- L241 Tracking interrupted
- L247 Agent could not finish
- L412 Unavailable

### `src/features/inbox/ui/InboxNotificationMenu.tsx`

- L129 Could not save read status. Please try again.
- L155 Could not save notification preferences. Please try again.

### `src/features/inbox/ui/InboxPrChecks.tsx`

- L419 Could not load job details.
- L484 No error annotations reported. View the full log on GitHub.
- L641 Fix all failed
- L736 Failures first

### `src/features/notifications/ui/NotificationMuteControl.tsx`

- L50 Could not save notification preferences. Please try again.

### `src/features/notifications/ui/NotificationMuteDatePicker.tsx`

- L45 Choose a valid date and time.
- L49 Choose a date and time in the future.
- L60 Could not save notification preferences. Please try again.

### `src/features/notifications/ui/ProjectNotificationSettings.tsx`

- L107 Could not save notification preferences. Please try again.

### `src/features/orchestration/model/orchestration.ts`

- L361 Could not save run: {…}
- L396 Stopped because run history could not be saved. Resume will continue from the retained worker checkout.
- L411 Stopped because run history could not be saved.
- L529 Interrupted while MonoCode was not running. Resume will continue from the retained worker checkout.
- L544 Interrupted while MonoCode was not running.
- L550 Run interrupted while MonoCode was not running. Worker checkouts were retained; Resume will continue them.

### `src/features/orchestration/model/orchestrationPlan.ts`

- L375 Planning was interrupted. Generate the assignments again.

### `src/features/orchestration/ui/OrchestrationSidebarAgents.tsx`

- L252 Stopping interrupted work before this run can resume.
- L254 Waiting for the lead's interrupted turn to finish before this run can resume.
- L255 Resume continues interrupted workers from their retained checkouts and starts queued work. Policy-blocked tasks stay stopped for review.

### `src/features/sessions/data/sessionStore.ts`

- L759 This by-the-way request was interrupted before reload.

### `src/features/sessions/ui/AgentTabView.tsx`

- L108 This agent is no longer running. Its work is summarised in the orchestrator's conversation.

### `src/features/sessions/ui/AgentTranscript.tsx`

- L1754 CI instructions and failure details included with this request.

### `src/features/sessions/ui/BtwSheet.tsx`

- L804 Couldn’t finish

### `src/features/sessions/ui/Composer.tsx`

- L355 Queue paused because you interrupted

### `src/features/sessions/ui/ReminderNotices.tsx`

- L91 Couldn’t load reminders.

### `src/features/sessions/ui/UsageLimitNotice.tsx`

- L33 Usage limit reached

### `src/features/settings/ui/SettingsView.tsx`

- L3033 Could not save the binary path.
- L3046 Could not save the binary path.（同上，另一处）
- L3128 Restart required
- L3196 CLI could not be resolved
- L3218 Could not open the CLI location:
- L4357 Permission needed

### `src/features/source-control/ui/WorktreePicker.tsx`

- L160 Worktree unavailable

### `src/integrations/harness/core/apply.ts`

- L601 Planning was interrupted. Generate the assignments again.

### `src/integrations/harness/providers/antigravity/antigravity.ts`

- L329 Antigravity has been quiet for two minutes — its post-turn work may be stuck. Stop and resend to recover.

### `src/integrations/harness/providers/claude/claudeProtocol.ts`

- L490 Claude turn failed.

### `src/integrations/harness/providers/codex/codex.ts`

- L1074 This MCP server requested a form or browser sign-in that MonoCode does not support yet. Complete it in the server's own interface.

### `src/integrations/harness/providers/codex/codexProtocol.ts`

- L515 Codex turn failed.
- L839 Subagent interrupted.

### `src/integrations/harness/providers/cursor/cursor.ts`

- L1058 Subagent failed.

### `src/integrations/harness/providers/opencode/opencode.ts`

- L963 Subagent failed.
- L964 Tool failed.

### `src/integrations/harness/providers/pi/piFamily.ts`

- L770 Advisor reviewed this turn
- L1172 Fast mode is unavailable for the current model.

## 八、原生菜单（Rust）与界面直显的英文数据表

### 原生层（Rust，前端翻译系统管不到）

#### `src-tauri/src/menu.rs`（macOS 顶部菜单栏）

- 子菜单：File / View / Edit / Window（`MonoCode` 应用菜单名保留）
- 菜单项：Settings… / Check for Updates… / New Window / Open Project… / Go to File… / Command Palette… / Search… / Inbox / Notes / New Tab / New Terminal / New Terminal Tab / Toggle Terminal / Split Pane Right / Split Pane Down / Close Pane / Close Other Tabs / Close All Tabs / Next Tab / Previous Tab / Go Back / Go Forward / Focus Pane Left·Right·Up·Down / Toggle Sidebar / Toggle Session Sidebar / Switch Model… / Sidebar Appearance… / Zoom In / Zoom Out / Reset Zoom / Reload / Find / Find in Files… / Quit MonoCode
- 注意：第 4 列 "App: Settings" 这类是快捷键命令 ID，必须保持英文不动（与前端 keybinding 表对齐）

#### `src-tauri/src/tray.rs`（托盘菜单）

- Show MonoCode / Quit MonoCode

#### `src-tauri/src/quick_composer/git_popup.rs`

- L290 窗口标题 Choose workspace

### 界面直接显示的英文数据表

- `src/features/settings/model/settings.ts` L148 起 `SETTINGS_INDEX`：设置搜索结果显示的条目名（Project worktrees / Version / Sounds …），当前直接渲染英文；结果右侧的 "Page" 也未翻译
- `src/features/settings/model/settings.ts` L900 起快捷键表 46 条 `command`（"Session: Previous" 等）：设置 → 快捷键页面直接显示。它同时是覆盖配置的存储键，不能改值，只能渲染时加显示名映射
- `src/features/workspace/model/tabKeys.ts`：同上，命令 ID 映射表
- `src/features/files/ui/FilePicker.tsx` L38 命令面板动作 "Reload MonoCode"
