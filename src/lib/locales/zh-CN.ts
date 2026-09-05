/**
 * Chinese (Simplified) locale for MonoCode.
 *
 * Keys are the original English strings. Every user-visible string in the app
 * should appear here exactly once.
 */

export const zhCN: Record<string, string> = {
  // ── General / Common ──────────────────────────────────────────────────
  "MonoCode": "MonoCode",
  "Close": "关闭",
  "Cancel": "取消",
  "Delete": "删除",
  "Remove": "移除",
  "Copy": "复制",
  "Cut": "剪切",
  "Paste": "粘贴",
  "Rename": "重命名",
  "Duplicate": "创建副本",
  "Save": "保存",
  "Back": "返回",
  "Search": "搜索",
  "Settings": "设置",
  "Create": "创建",
  "Retry": "重试",
  "Allow": "允许",
  "Deny": "拒绝",
  "Continue": "继续",
  "Skip": "跳过",
  "Loading…": "加载中…",
  "Loading": "加载中",
  "No results": "无结果",
  "Untitled": "无标题",
  "Question": "问题",
  "Approval": "审批",
  "Send": "发送",
  "Stop": "停止",
  "Plan": "计划",
  "Resume": "恢复",
  "Done": "完成",
  "Working": "工作中",
  "Open": "打开",
  "Closed": "已关闭",
  "Draft": "草稿",
  "Merged": "已合并",
  "New": "新建",
  "Default": "默认",
  "Refresh": "刷新",
  "Archive": "归档",
  "Unarchive": "取消归档",
  "Restore": "恢复",
  "Download": "下载",

  // ── TitleBar.tsx ──────────────────────────────────────────────────────
  "New session": "新建会话",
  "{0} sessions": "{0} 个会话",
  "Unsaved changes": "未保存的更改",
  "{0} — MonoCode": "{0} — MonoCode",
  "{0} — {1} — MonoCode": "{0} — {1} — MonoCode",
  "Development build": "开发版本",
  "Development": "开发",
  "Back ({0}[)": "后退 ({0}[)",
  "Forward ({0}])": "前进 ({0}])",
  "Toggle Projects": "切换项目",
  "Toggle Sidebar ({0}B)": "切换侧边栏 ({0}B)",
  "Inbox": "收件箱",
  "Notes": "笔记",
  "Go to File ({0}P)": "跳转到文件 ({0}P)",
  "New session ({0}T)": "新建会话 ({0}T)",
  "New Terminal": "新建终端",
  "New Terminal ({0}`)": "新建终端 ({0}`)",
  "Terminal": "终端",
  "Settings ({0},)": "设置 ({0},)",
  "No project": "无项目",
  "Scroll tabs left": "向左滚动标签页",
  "Scroll tabs right": "向右滚动标签页",
  "Close Tab": "关闭标签页",
  "Close {0}": "关闭 {0}",

  // ── MenuBar.tsx ───────────────────────────────────────────────────────
  "New Tab": "新建标签页",
  "New Window": "新建窗口",
  "Open Project…": "打开项目…",
  "Search…": "搜索…",
  "Go to File…": "跳转到文件…",
  "Find in Files…": "在文件中查找…",
  "Close Pane": "关闭面板",
  "Close Other Tabs": "关闭其他标签页",
  "Check for Updates…": "检查更新…",
  "Toggle Sidebar": "切换侧边栏",
  "Switch Model…": "切换模型…",
  "Toggle Changes": "切换更改",
  "Toggle Terminal": "切换终端",
  "File": "文件",
  "View": "视图",

  // ── Sidebar.tsx ───────────────────────────────────────────────────────
  "Sessions": "会话",
  "Explorer": "资源管理器",
  "Changes": "更改",
  "Search conversations...": "搜索会话…",
  "Search conversations": "搜索会话",
  "Filter sessions": "筛选会话",
  "No project folder": "无项目文件夹",
  "Couldn't load sessions": "无法加载会话",
  "No matching sessions": "没有匹配的会话",
  "No sessions match these filters": "没有符合筛选条件的会话",
  "Sessions you start will show up here": "你创建的会话将显示在这里",
  "Session actions": "会话操作",
  "Folder actions": "文件夹操作",
  "Workspace": "工作区",
  "Go to File": "跳转到文件",
  "{0} files changed": "{0} 个文件已更改",
  "1 file changed": "1 个文件已更改",
  "{0} uncommitted": "{0} 未提交",
  "Resize sidebar": "调整侧边栏大小",
  "Working...": "工作中…",
  "Need approval": "需要审批",
  "Ungroup": "取消分组",
  "New folder": "新建文件夹",
  "Add to {0}": "添加到 {0}",
  "Remove from folder": "从文件夹移除",

  // ── Composer.tsx ──────────────────────────────────────────────────────
  "Queue paused because you interrupted": "队列已暂停，因为你中断了操作",
  "Steer": "引导",
  "Edit queued message": "编辑排队消息",
  "Save queued message": "保存排队消息",
  "Cancel queued message edit": "取消编辑排队消息",
  "Remove queued message": "移除排队消息",
  "Add a note, or send to start…": "添加备注，或发送以开始…",
  "Add a message, or send…": "添加消息，或发送…",
  "Add context, or send to continue…": "添加上下文，或发送以继续…",
  "Ask, build, / for commands, @ for references... ": "提问、构建、/ 用于命令、@ 用于引用… ",
  "Add files or choose a mode": "添加文件或选择模式",
  "Add to message": "添加到消息",
  "Upload file": "上传文件",
  "Attach files or images to this message": "将文件或图片附加到此消息",
  "{0} does not support attachments": "{0} 不支持附件",
  "Plan mode": "计划模式",
  "Create a plan to review before building": "在构建前创建一个可供审查的计划",
  "Turn off Plan mode": "关闭计划模式",
  "Drop files to attach": "拖放文件以附加",
  "{0} attachments": "{0} 个附件",
  "1 attachment": "1 个附件",

  // ── ContextMeter.tsx ──────────────────────────────────────────────────
  "Context usage": "上下文使用量",
  "{0}, {1}. Open context actions": "{0}, {1}。打开上下文操作",
  "Compact now": "立即压缩",
  "Wait for the current operation to finish": "等待当前操作完成",
  "Compact this conversation's context": "压缩此对话的上下文",

  // ── CwdPicker.tsx ─────────────────────────────────────────────────────
  "Project {0}": "项目 {0}",
  "Current project": "当前项目",
  "Recent projects": "最近的项目",
  "More Projects": "更多项目",
  "New terminal": "新建终端",

  // ── FileTree.tsx ──────────────────────────────────────────────────────
  "Reveal in Finder": "在 Finder 中显示",
  "Reveal in File Explorer": "在文件资源管理器中显示",
  "Open Containing Folder": "打开所在文件夹",
  "New File": "新建文件",
  "New Folder": "新建文件夹",
  "Copy Path": "复制路径",
  "Copy Relative Path": "复制相对路径",
  "Open in Terminal": "在终端中打开",
  "Delete folder \"{0}\" and everything inside it?": "删除文件夹「{0}」及其中的所有内容？",
  "Delete \"{0}\"?": "删除「{0}」？",
  "Cannot paste a folder into itself.": "无法将文件夹粘贴到自身。",
  "Collapse All": "全部折叠",
  "Search in files ({0}Shift+F)": "在文件中搜索 ({0}Shift+F)",
  "{0} files": "{0} 个文件",
  "Hide changes": "隐藏更改",
  "Show changes": "显示更改",
  "{0} file changed": "{0} 个文件已更改",
  "A file or folder name must be provided.": "必须提供文件或文件夹名称。",
  "A file or folder name cannot start with a slash.": "文件或文件夹名称不能以斜杠开头。",
  "A file or folder {0} already exists at this location. Please choose a different name.": "文件或文件夹 {0} 已存在于此位置。请选择其他名称。",
  "The name {0} is not valid as a file or folder name. Please choose a different name.": "名称 {0} 不是有效的文件或文件夹名称。请选择其他名称。",
  "Leading or trailing whitespace detected in file or folder name.": "检测到文件或文件夹名称中有前导或尾随空格。",
  "Type file name. Press Enter to confirm or Escape to cancel.": "输入文件名。按 Enter 确认或 Escape 取消。",

  // ── FilePicker.tsx ────────────────────────────────────────────────────
  "Open a project to search files": "打开项目以搜索文件",
  "Indexing files…": "正在索引文件…",
  "No files found": "未找到文件",
  "No matching files": "没有匹配的文件",
  "Type a file name to search": "输入文件名以搜索",
  "Files": "文件",

  // ── FileMentionPicker.tsx ─────────────────────────────────────────────
  "No matching files or notes": "没有匹配的文件或笔记",
  "No matching files or folders": "没有匹配的文件或文件夹",
  "No files or notes found": "未找到文件或笔记",
  "No files or folders found": "未找到文件或文件夹",
  "Files and notes": "文件和笔记",
  "Files and folders": "文件和文件夹",
  "Note": "笔记",

  // ── FilePreview.tsx ───────────────────────────────────────────────────
  "+{0}": "+{0}",
  "-{0}": "-{0}",

  // ── ApprovalToasts.tsx ────────────────────────────────────────────────
  // (Question, Approval, Allow, Deny already covered above)

  // ── AttachmentChip.tsx ────────────────────────────────────────────────
  // (Remove already covered above)

  // ── BranchPicker.tsx ──────────────────────────────────────────────────
  "detached {0}": "分离 {0}",
  "No repo": "无仓库",
  "Loading branch…": "加载分支中…",
  "No git repository": "无 Git 仓库",
  "Loading branch": "加载分支中",
  "Branch {0}": "分支 {0}",
  "Search or create a branch...": "搜索或创建分支…",
  "Search or create a branch": "搜索或创建分支",
  "No matching branches": "没有匹配的分支",
  "No branches": "无分支",
  "Branches": "分支",
  "Create and checkout {0}": "创建并切换到 {0}",
  "WIP before switching to {0}": "切换到 {0} 前的工作副本",

  // ── ColorPickerPopover.tsx ────────────────────────────────────────────
  "Color {0}": "颜色 {0}",
  "Custom color": "自定义颜色",
  "Saturation and brightness": "饱和度和亮度",
  "Hue": "色相",
  "Hex color": "十六进制颜色",

  // ── ExplorerMenu.tsx ──────────────────────────────────────────────────
  "File actions": "文件操作",

  // ── GitChangesPanel.tsx ───────────────────────────────────────────────
  "Message ({0}↩ to commit)": "提交信息 ({0}↩ 提交)",
  "Generate commit message": "生成提交信息",
  "Commit": "提交",
  "Commit options": "提交选项",
  "Commit & Push": "提交并推送",
  "Commit, Push & Create PR": "提交、推送并创建 PR",
  "Publish Branch": "发布分支",
  "Sync Changes": "同步更改",
  "Create PR": "创建 PR",
  "View PR": "查看 PR",
  "View PR #{0}": "查看 PR #{0}",
  "Synchronizing Changes...": "正在同步更改…",
  "Publish Branch \"{0}\"": "发布分支「{0}」",
  "Pull {0} and push {1} commits between {2}": "从 {2} 拉取 {0} 并推送 {1} 个提交",
  "Pull {0} commit from {1}": "从 {1} 拉取 {0} 个提交",
  "Pull {0} commits from {1}": "从 {1} 拉取 {0} 个提交",
  "Push {0} commit to {1}": "向 {1} 推送 {0} 个提交",
  "Push {0} commits to {1}": "向 {1} 推送 {0} 个提交",
  "Create a pull request into {0}": "创建合并到 {0} 的拉取请求",
  "Create pull request": "创建拉取请求",
  "View PR #{0}: {1}": "查看 PR #{0}: {1}",
  "View pull request": "查看拉取请求",
  "Create a pull request from default branch \"{0}\"?": "从默认分支「{0}」创建拉取请求？",
  "Push to default branch \"{0}\"?": "推送到默认分支「{0}」？",
  "Delete untracked file {0}?": "删除未跟踪的文件 {0}？",
  "Discard changes in {0}? This cannot be undone.": "丢弃 {0} 中的更改？此操作无法撤销。",
  "Discard": "丢弃",
  "No uncommitted changes": "没有未提交的更改",
  "Loading changes…": "加载更改中…",
  "{0} unpushed commit": "{0} 个未推送的提交",
  "{0} unpushed commits": "{0} 个未推送的提交",
  "{0} incoming commit": "{0} 个传入的提交",
  "{0} incoming commits": "{0} 个传入的提交",
  "No files": "无文件",
  "Staged Changes": "已暂存的更改",
  "Unstage All Changes": "取消暂存所有更改",
  "Discard All Changes": "丢弃所有更改",
  "Stage All Changes": "暂存所有更改",
  "Discard Changes": "丢弃更改",
  "Unstage Changes": "取消暂存",
  "Stage Changes": "暂存更改",

  // ── GitHistoryGraph.tsx ───────────────────────────────────────────────
  "Collapse graph": "折叠图表",
  "Expand graph": "展开图表",
  "Graph": "图表",
  "No commits yet": "暂无提交",
  "Resize graph": "调整图表大小",

  // ── HandoffMiniCard.tsx ───────────────────────────────────────────────
  "Handoff": "交接",
  "{0} file": "{0} 个文件",
  "Remove handoff": "移除交接",

  // ── InboxFiltersMenu.tsx ──────────────────────────────────────────────
  "All time": "全部时间",
  "Today": "今天",
  "Last 7 days": "最近 7 天",
  "Last 30 days": "最近 30 天",
  "Issues": "议题",
  "Pull requests": "拉取请求",
  "Assigned to me": "分配给我",
  "Status": "状态",
  "Time": "时间",
  "Type": "类型",
  "Projects": "项目",
  "Clear filters": "清除筛选",
  "Filter inbox": "筛选收件箱",

  // ── InboxMiniCard.tsx ─────────────────────────────────────────────────
  "Pull request": "拉取请求",
  "Issue": "议题",
  "Open in {0}": "在 {0} 中打开",
  "Open {0} {1} in {2}": "在 {2} 中打开 {0} {1}",
  "Remove {0} {1}": "移除 {0} {1}",

  // ── MarkdownModeToggle.tsx ────────────────────────────────────────────
  "Markdown view": "Markdown 视图",
  "Preview": "预览",
  "Source": "源码",

  // ── Modal.tsx ─────────────────────────────────────────────────────────
  // (Close already covered above)

  // ── ModelPicker.tsx ───────────────────────────────────────────────────
  "Search models...": "搜索模型…",
  "Search models": "搜索模型",
  "Providers": "提供商",
  "Favorites": "收藏",
  "No favorite models": "没有收藏的模型",
  "Loading Codex models…": "加载 Codex 模型中…",
  "No matching models": "没有匹配的模型",
  "Remove from favorites": "从收藏中移除",
  "Add to favorites": "添加到收藏",
  "Models": "模型",

  // ── NoteMiniCard.tsx ──────────────────────────────────────────────────
  "Remove note {0}": "移除笔记 {0}",

  // ── PlanPreview.tsx ───────────────────────────────────────────────────
  "Building…": "构建中…",
  "Built": "已构建",
  "Build": "构建",
  "Open in pane": "在面板中打开",
  "Open plan in pane": "在面板中打开计划",
  "Build this plan": "构建此计划",

  // ── ProjectRail.tsx ───────────────────────────────────────────────────
  "Unpin project": "取消固定项目",
  "Pin project": "固定项目",
  "Inbox, new items": "收件箱，有新项目",
  "Pinned": "已固定",
  "No projects yet": "暂无项目",
  "Working agents": "工作代理",
  "Show less": "收起",
  "{0} more": "还有 {0} 个",
  "Open project": "打开项目",
  "Project options": "项目选项",
  "Resize project sidebar": "调整项目侧边栏大小",

  // ── ProjectSearch.tsx ─────────────────────────────────────────────────
  "Back to files": "返回文件",
  "Search in files": "在文件中搜索",
  "Match case": "区分大小写",
  "Match whole word": "匹配整个单词",
  "Use regular expression": "使用正则表达式",
  "files to include": "包含的文件",
  "files to exclude": "排除的文件",
  "Searching…": "搜索中…",
  "{0} result in {1} file": "{1} 个文件中有 {0} 个结果",
  "{0} results in {1} file": "{1} 个文件中有 {0} 个结果",
  "{0} results in {1} files": "{1} 个文件中有 {0} 个结果",
  " (limited)": " （已限制）",
  "Type to search across the project": "输入以在项目中搜索",

  // ── QuestionForm.tsx ──────────────────────────────────────────────────
  "{0} of {1}": "{0} / {1}",
  "Select all that apply": "选择所有适用项",
  "Type your answer": "输入你的答案",
  "Other": "其他",

  // ── RemoveProjectDialog.tsx ───────────────────────────────────────────
  "All conversations for this project will be deleted. It also leaves the sidebar. The folder on disk stays put, and opening it again brings the project back empty.": "此项目的所有对话将被删除，同时从侧边栏移除。磁盘上的文件夹将保留，再次打开会以空白项目恢复。",
  "1 saved conversation will be removed.": "1 个已保存的对话将被移除。",
  "{0} saved conversations will be removed.": "{0} 个已保存的对话将被移除。",

  // ── SecondOpinionButton.tsx ───────────────────────────────────────────
  "Install another provider to hand off": "安装另一个提供商以进行交接",
  "Hand this session to another agent to continue the work.": "将此会话交给另一个代理以继续工作。",
  "Hand this session to another agent": "将此会话交给另一个代理",
  "Build with another model": "使用另一个模型构建",
  "No build providers are available": "没有可用的构建提供商",
  "Choose the model and provider that should build this plan.": "选择应构建此计划的模型和提供商。",
  "Build this plan with another model or provider": "使用另一个模型或提供商构建此计划",
  "Second opinion": "第二意见",
  "Install another provider for a second opinion": "安装另一个提供商以获取第二意见",
  "Send this turn to another agent to review the work.": "将此轮对话发送给另一个代理以审查工作。",
  "Send this turn to another agent": "将此轮对话发送给另一个代理",

  // ── SecondOpinionCard.tsx ─────────────────────────────────────────────
  // (Handoff, Second opinion, file/files already covered above)

  // ── SessionFiltersMenu.tsx ────────────────────────────────────────────
  "Archived": "已归档",
  "Needs approval": "需要审批",

  // ── SessionReview.tsx ─────────────────────────────────────────────────
  "Collapse files": "折叠文件",
  "Expand files": "展开文件",
  "{0} Files": "{0} 个文件",
  "Undo all session changes": "撤销所有会话更改",
  "Undo is unavailable while another session is running in this project": "当此项目中有其他会话正在运行时，撤销不可用",
  "Undo is unavailable because a file changed outside this session": "因为文件在此会话外部被修改，撤销不可用",
  "Undo All": "全部撤销",
  "Keep all session changes": "保留所有会话更改",
  "Keep All": "全部保留",
  "Review changes": "审查更改",
  "Review": "审查",
  "Shared file": "共享文件",

  // ── SettingsRail.tsx ──────────────────────────────────────────────────
  // (Settings, Back already covered above)

  // ── SkillPicker.tsx ───────────────────────────────────────────────────
  "No matching commands or skills": "没有匹配的命令或技能",
  "No commands yet": "暂无命令",
  "Commands and skills": "命令和技能",
  "New skill": "新建技能",
  "Writes a starter SKILL.md you can edit.": "写入一个可编辑的 SKILL.md 模板。",
  "skill-name": "技能名称",
  "Skill name": "技能名称",
  "Project": "项目",
  ".agents/skills": ".agents/skills",
  "Personal": "个人",
  "~/.agents/skills": "~/.agents/skills",
  "Use lowercase letters, numbers, and hyphens.": "使用小写字母、数字和连字符。",
  "Creating…": "创建中…",
  "monocode": "monocode",
  "personal": "个人",
  "project": "项目",

  // ── SurfaceTabs.tsx ───────────────────────────────────────────────────
  "Working tree changes": "工作树更改",
  "Session Changes": "会话更改",
  "Changes captured for this session only": "仅捕获此会话的更改",
  "{0} (Working Tree)": "{0}（工作树）",
  "{0} — {1}": "{0} — {1}",
  "{0} problem": "{0} 个问题",
  "{0} problems": "{0} 个问题",
  "Open files": "打开文件",
  "Drag to reorder pane": "拖动以重新排列面板",

  // ── SwitchBranchDialog.tsx ────────────────────────────────────────────
  "Create {0}": "创建 {0}",
  "Switch to {0}": "切换到 {0}",
  "Uncommitted changes": "未提交的更改",
  "Creating \"{0}\" would overwrite your local changes. Stash them for later, or commit them on this branch first.": "创建「{0}」将覆盖你的本地更改。请先储藏或在当前分支上提交。",
  "Switching to \"{0}\" would overwrite your local changes. Stash them for later, or commit them on this branch first.": "切换到「{0}」将覆盖你的本地更改。请先储藏或在当前分支上提交。",
  "Commit message": "提交信息",
  "Commit & switch": "提交并切换",
  "Stash & switch": "储藏并切换",

  // ── TabGroupMenu.tsx ──────────────────────────────────────────────────
  "New tab in group": "在组中新建标签页",
  "Move group to new window": "将组移到新窗口",
  "Close group": "关闭组",
  "Delete group": "删除组",
  "Tab group actions": "标签组操作",
  "Group name": "组名",
  "Change project logo": "更改项目标志",
  "Add project logo": "添加项目标志",
  "Project logo": "项目标志",
  "Shown in tabs and composer": "显示在标签页和编写器中",
  "Optional — replaces folder icon": "可选 — 替换文件夹图标",
  "Remove project logo": "移除项目标志",
  "Mascot": "吉祥物",
  "Mascot {0}": "吉祥物 {0}",

  // ── TaskListPreview.tsx ───────────────────────────────────────────────
  "Task progress": "任务进度",
  "Tasks": "任务",
  "Completed": "已完成",
  "In progress": "进行中",
  "Cancelled": "已取消",
  "Pending": "待处理",

  // ── UpdateRailCard.tsx ────────────────────────────────────────────────
  "Updated to {0}": "已更新到 {0}",
  "What's new": "更新说明",
  "Dismiss update notification": "关闭更新通知",

  // ── UsageFooter.tsx ───────────────────────────────────────────────────
  "Provider usage": "提供商用量",
  "Terminals": "终端",
  "Session": "会话",
  "Refresh usage": "刷新用量",
  "Hide {0}": "隐藏 {0}",
  "Show {0}": "显示 {0}",
  "Hide running terminals": "隐藏运行中的终端",
  "{0} terminals are running processes": "{0} 个终端正在运行进程",
  "Running terminals": "运行中的终端",
  "not connected": "未连接",
  "Loading usage…": "加载用量中…",
  "Not connected": "未连接",
  "expired": "已过期",

  // ── WhatsNewDialog.tsx ────────────────────────────────────────────────
  "MonoCode {0}": "MonoCode {0}",
  "Release notes for this version are not available in this build.": "此版本的更新说明在此构建中不可用。",

  // ── WindowControls.tsx ────────────────────────────────────────────────
  "Minimize": "最小化",
  "Minimize window": "最小化窗口",
  "Maximize": "最大化",
  "Restore window": "还原窗口",
  "Maximize window": "最大化窗口",
  "Close window": "关闭窗口",

  // ── SidebarUpdate.tsx ─────────────────────────────────────────────────
  "Update to {0}": "更新到 {0}",
  "Downloading{0}": "下载中{0}",
  "Checking…": "检查中…",
  "Check for updates": "检查更新",

  // ══════════════════════════════════════════════════════════════════════
  // surfaces/
  // ══════════════════════════════════════════════════════════════════════

  // ── AgentMarkdown.tsx ─────────────────────────────────────────────────
  "Copied": "已复制",
  "Copy code": "复制代码",

  // ── AgentTranscript.tsx ───────────────────────────────────────────────
  "Load earlier messages": "加载更早的消息",
  "Waiting for answers": "等待回复",
  "Thinking…": "思考中…",
  "Waiting for approval": "等待审批",
  "Subagent is running": "子代理正在运行",
  "Agent is working": "代理正在工作",
  "Copy response": "复制回复",
  "Saved to Notes": "已保存到笔记",
  "Save as note": "保存为笔记",
  "Preparing a handoff": "正在准备交接",
  "Preparing a handoff to {0}": "正在准备交接给 {0}",
  "Continued with {0}": "已使用 {0} 继续",
  "Accepted": "已接受",
  "Rejected": "已拒绝",
  "Hide the steps for {0}": "隐藏 {0} 的步骤",
  "Show the steps for {0}": "显示 {0} 的步骤",
  "Hide thinking": "隐藏思考过程",
  "Show thinking: {0}": "显示思考过程：{0}",
  "Hide the full note": "隐藏完整笔记",
  "Agent said: {0}": "代理说：{0}",
  "Tool call: {0}": "工具调用：{0}",
  "{0} tool call: {1}": "{0} 工具调用：{1}",
  "Read": "读取",
  "Find": "查找",
  "Skill": "技能",
  "List": "列表",
  "Edit": "编辑",
  "Write": "写入",
  "Worked": "已完成",
  "worked": "已完成",
  "Subagent running": "子代理运行中",
  "subagent running": "子代理运行中",
  "working": "工作中",
  "{0} subagent is running": "{0} 子代理正在运行",
  "{0} is working": "{0} 正在工作",
  "file": "文件",

  // ── BinaryFileView.tsx ────────────────────────────────────────────────
  "Opening {0}…": "正在打开 {0}…",
  "Couldn't open {0}": "无法打开 {0}",
  "{0} · not a readable image": "{0} · 不是可读的图片",
  "Zoom out": "缩小",
  "Fit to window": "适应窗口",
  "Fit": "适应",
  "Zoom in": "放大",
  "Reveal": "显示",
  "—": "—",

  // ── CommitDiff.tsx ────────────────────────────────────────────────────
  "Couldn't load diff: {0}": "无法加载差异：{0}",
  "No textual diff": "无文本差异",
  "Couldn't load commit": "无法加载提交",

  // ── DiffCommentComposer.tsx ───────────────────────────────────────────
  "Cancel comment": "取消评论",
  "Leave a comment…": "留下评论…",
  "{0}↩ to add": "{0}↩ 添加",
  "Add to chat": "添加到聊天",
  "Comment on {0}": "评论 {0}",

  // ── EmptySession.tsx ──────────────────────────────────────────────────
  "What should we work on in {0}?": "我们应该在 {0} 中做什么？",
  "What should we work on?": "我们应该做什么？",

  // ── FileEditor.tsx ────────────────────────────────────────────────────
  "Saving…": "保存中…",
  "Saved": "已保存",
  "Save failed: {0}": "保存失败：{0}",
  "Can't stage this file": "无法暂存此文件",
  "Jump between changes": "在更改间跳转",
  "Previous change": "上一处更改",
  "Next change": "下一处更改",
  "Plan markdown": "计划 Markdown",

  // ── FilePane.tsx ──────────────────────────────────────────────────────
  "This plan is no longer in the session.": "此计划已不在会话中。",

  // ── InboxComments.tsx ─────────────────────────────────────────────────
  "1 comment": "1 条评论",
  "{0} comments": "{0} 条评论",
  "Latest comments · more on {0}": "最新评论 · 更多在 {0}",
  "Cancel reply": "取消回复",
  "Write a reply ({0}↩)": "写回复 ({0}↩)",
  "Leave a comment ({0}↩)": "留言 ({0}↩)",
  "Posting...": "发布中…",
  "Reply": "回复",
  "Comment": "评论",
  "Loading comments": "加载评论中",
  "Resolved": "已解决",
  "Open in Linear": "在 Linear 中打开",
  "Open on GitHub": "在 GitHub 上打开",

  // ── InboxMedia.tsx ────────────────────────────────────────────────────
  "Image": "图片",

  // ── InboxPrDiff.tsx ───────────────────────────────────────────────────
  "Patch unavailable because this pull request is too large": "补丁不可用，因为此拉取请求太大",

  // ── InboxView.tsx ─────────────────────────────────────────────────────
  "Inbox source": "收件箱来源",
  "Mark all as read": "全部标为已读",
  "No matching Linear issues": "没有匹配的 Linear 议题",
  "No matching issues or pull requests": "没有匹配的议题或拉取请求",
  "No Linear issues match these filters": "没有符合筛选条件的 Linear 议题",
  "No issues or pull requests match these filters": "没有符合筛选条件的议题或拉取请求",
  "No Linear issues": "没有 Linear 议题",
  "Open a project to fill the inbox": "打开项目以填充收件箱",
  "Select an issue or pull request": "选择一个议题或拉取请求",
  "Pull request sections": "拉取请求部分",
  "Summary": "摘要",
  "Code": "代码",
  "No file changes": "无文件更改",
  "No description": "无描述",
  "Unassigned": "未分配",
  "Updated {0}": "更新于 {0}",
  "Send to agent": "发送给代理",
  "Sending...": "发送中…",
  "Review on GitHub": "在 GitHub 上审查",
  "Choose project": "选择项目",
  "Linear": "Linear",
  "GitHub": "GitHub",
  "new": "新建",
  "Resize inbox list": "调整收件箱列表大小",

  // ── NotesView.tsx ─────────────────────────────────────────────────────
  "New note": "新建笔记",
  "No matching notes": "没有匹配的笔记",
  "No notes yet. Save a turn from the transcript, or create one here.": "暂无笔记。从对话记录中保存一轮，或在此处创建。",
  "Select a note": "选择一个笔记",
  "Note sections": "笔记部分",
  "Write markdown…": "编写 Markdown…",
  "Note title": "笔记标题",
  "Resize notes list": "调整笔记列表大小",

  // ── ProjectTerminalDock.tsx ───────────────────────────────────────────
  "Dock Bottom": "停靠底部",
  "Dock Top": "停靠顶部",
  "Dock Left": "停靠左侧",
  "Dock Right": "停靠右侧",
  "Move Terminal": "移动终端",
  "Hide Terminal ({0}J)": "隐藏终端 ({0}J)",
  "Resize terminal": "调整终端大小",
  "Move terminal": "移动终端",

  // ── ReleaseNotesSurface.tsx ───────────────────────────────────────────
  "Release notes": "更新说明",

  // ── SearchView.tsx ────────────────────────────────────────────────────
  "Search everything...": "搜索全部…",
  "Find files, conversations, messages, and projects.": "查找文件、对话、消息和项目。",
  "Search results": "搜索结果",
  "All": "全部",
  "Conversations": "对话",

  // ── SessionChangesDiff.tsx ────────────────────────────────────────────
  "No session changes": "无会话更改",
  "Couldn't load session changes": "无法加载会话更改",

  // ── SessionPane.tsx ───────────────────────────────────────────────────
  "Close Pane ({0}W)": "关闭面板 ({0}W)",
  "Close pane": "关闭面板",
  "Jump to latest": "跳转到最新",

  // ── SettingsView.tsx ──────────────────────────────────────────────────
  "Restore defaults": "恢复默认",
  "General": "通用",
  "Appearance": "外观",
  "Keybindings": "快捷键",
  "Transcript layout": "对话记录布局",
  "Full width keeps user prompts as a spanning card. Chat aligns them to the right with a max width, like a messaging app.": "全宽将用户提示显示为跨列卡片。聊天模式将其右对齐并限制最大宽度，类似即时通讯应用。",
  "Full width": "全宽",
  "Chat": "聊天",
  "Diff view": "差异视图",
  "Editor keeps working-tree changes in the file. Unified stacks every changed file in one review, with sticky headers and collapsed unchanged lines.": "编辑器在文件中保留工作树更改。统一视图将所有更改的文件堆叠在一个审查中，带有粘性标题和折叠的未更改行。",
  "Editor": "编辑器",
  "Unified": "统一",
  "Follow-up behavior": "后续行为",
  "Queue follow-ups until the active turn finishes, or steer the active turn immediately.": "将后续消息排队等待当前轮次完成，或立即引导当前轮次。",
  "Queue": "排队",
  "Anchor prompts to top": "将提示固定在顶部",
  "When you send, the new prompt sits at the top of the transcript and the reply grows into the space below. Turn this off to keep the classic layout, with the latest message resting on the composer.": "发送时，新提示位于对话记录顶部，回复在下方展开。关闭此选项可保留经典布局，最新消息位于编写器上方。",
  "Composer mascot": "编写器吉祥物",
  "When a turn is running, the project mascot runs along the composer, bonks the scroll-to-latest button the first time, then jumps it, and sometimes grabs a coin.": "运行轮次时，项目吉祥物沿编写器奔跑，首次碰到滚动到最新按钮后跳跃，有时还会抓取金币。",
  "Empty session games": "空会话游戏",
  "Pac-man and snake idle on the empty-session grid. Hover the band to take control of whichever is on screen. Turn this off to keep the pane still.": "吃豆人和贪吃蛇在空会话网格上待机。悬停控制栏以操控屏幕上的游戏。关闭此选项可保持面板静止。",
  "A global markdown notebook on the project rail. Save a finished turn from the transcript, then mention it later with @note or add it to chat. Turn this off to hide Notes from the UI.": "项目栏上的全局 Markdown 笔记本。从对话记录中保存已完成的轮次，之后可通过 @note 引用或添加到聊天。关闭此选项可隐藏笔记功能。",
  "When two or more chats are in flight, a card on the project rail lists them so you can jump across projects. Finished turns stay until you open that session. Turn this off to hide the card.": "当两个或更多聊天正在进行时，项目栏上会显示卡片以便跨项目跳转。已完成的轮次会保留直到你打开该会话。关闭此选项可隐藏卡片。",
  "Sounds": "声音",
  "Short cues when a turn finishes, a new inbox item appears on the project rail, or an update is available. Switches and Copy on a finished turn also play.": "轮次完成、收件箱出现新项目或有可用更新时的简短提示音。切换和复制已完成轮次时也会播放。",
  "Claude Code hooks": "Claude Code 钩子",
  "Run the hooks configured in your settings.json files — PreToolUse command rewrites, blocks, notifications, and the rest — just as the Claude Code CLI would. Turn this off if a hook is misbehaving and you need the session back. Takes effect on the next turn.": "运行在 settings.json 文件中配置的钩子 — PreToolUse 命令重写、阻止、通知等 — 就像 Claude Code CLI 一样。如果钩子行为异常且需要恢复会话，请关闭此选项。在下一轮次生效。",
  "About": "关于",
  "API key": "API 密钥",
  "Create a personal API key in Linear → Settings → Security & Access. Disconnect deletes it.": "在 Linear → 设置 → 安全与访问 中创建个人 API 密钥。断开连接将删除它。",
  "Disconnect": "断开连接",
  "lin_api_…": "lin_api_…",
  "Linear API key": "Linear API 密钥",
  "Saving": "保存中",
  "Connect": "连接",
  "Linear Teams": "Linear 团队",
  "Unchecked teams stay out of the inbox.": "未勾选的团队不会出现在收件箱中。",
  "Version": "版本",
  "Version {0} is available.": "版本 {0} 可用。",
  "Checking for updates…": "检查更新中…",
  "You're on the latest version.": "你已是最新版本。",
  "Update check failed.": "更新检查失败。",
  "MonoCode updates itself from the release feed.": "MonoCode 通过发布源自动更新。",
  "Theme": "主题",
  "System follows the OS appearance. Dark and light share the same tint, so the hue below applies to both.": "系统跟随操作系统外观。深色和浅色共享相同色调，下方色相同时应用于两者。",
  "System": "系统",
  "Dark": "深色",
  "Light": "浅色",
  "Sidebar opacity": "侧边栏不透明度",
  "How much of the desktop shows through the sidebar and the project rail.": "桌面透过侧边栏和项目栏的可见程度。",
  "Blur radius": "模糊半径",
  "Background blur behind the window. Higher values cost more to composite.": "窗口背景模糊。较高的值会增加合成开销。",
  "Base hue for accents and tinted surfaces.": "强调色和着色表面的基础色相。",
  "Saturation": "饱和度",
  "How strongly the hue tints the interface. Zero keeps it neutral.": "色相对界面的着色强度。零保持中性。",
  "Main pane glass": "主面板玻璃效果",
  "Extend the translucent treatment to the main pane behind sessions and editors.": "将半透明效果扩展到会话和编辑器后面的主面板。",
  "binding": "绑定",
  "bindings": "绑定",
  "Filter keybindings": "筛选快捷键",
  "Command": "命令",
  "Keybinding": "快捷键",
  "When": "触发条件",
  "No matching bindings": "没有匹配的绑定",
  "Bindings come from the app menu and the workspace key handler; they aren't customizable yet.": "绑定来自应用菜单和工作区按键处理程序；目前尚不可自定义。",
  "A provider is listed as installed once its CLI is found on your PATH. Uninstalled CLIs stay listed here but are omitted from the model picker. Turn off Show in picker to hide an installed provider from those tabs. The model beside each provider is what new conversations use when that provider is selected; Use by default picks the provider itself.": "提供商在其 CLI 被找到时列为已安装。未安装的 CLI 仍会列在此处，但会从模型选择器中省略。关闭「在选择器中显示」可从标签页中隐藏已安装的提供商。每个提供商旁边的模型是选择该提供商时新对话使用的模型；「默认使用」选择提供商本身。",
  "model": "模型",
  "models": "模型",
  "available.": "可用。",
  "{0} {1} available.": "{0} 个{1}可用。",
  "Use by default": "默认使用",
  "Show in picker": "在选择器中显示",
  "Show {0} in the model picker": "在模型选择器中显示 {0}",
  "Archived projects": "已归档项目",
  "Archive a project from the rail to keep its chats without listing it in the sidebar.": "从项目栏归档项目以保留其对话，同时不在侧边栏中列出。",
  "Show archived in the sidebar": "在侧边栏中显示已归档",
  "Keep archived conversations listed alongside the active ones.": "将已归档的对话与活跃对话一起列出。",
  "Archived in {0}": "归档于 {0}",
  "Archived conversations": "已归档对话",
  "Open a project to see its archived conversations.": "打开项目以查看其已归档对话。",
  "No archived conversations in this project.": "此项目中没有已归档对话。",

  // ── TerminalGridBackground.tsx ────────────────────────────────────────
  "score {0}": "得分 {0}",
  "game over": "游戏结束",
  "release": "松开",
  "take control": "接管控制",

  // ── TerminalView.tsx ──────────────────────────────────────────────────
  "[process exited{0}]": "[进程已退出{0}]",
  "[process exited]": "[进程已退出]",
  "[process exited ({0})]": "[进程已退出 ({0})]",

  // ── TranscriptSelectionMenu.tsx ───────────────────────────────────────
  "Selected text actions": "选中文本操作",

  // ── UnifiedDiffView.tsx ───────────────────────────────────────────────
  "1 file": "1 个文件",
  "Expand all files": "展开所有文件",
  "Collapse all files": "折叠所有文件",
  "Diff is too large to display in full. File list is shown without patches.": "差异过大，无法完整显示。文件列表将在不显示补丁的情况下展示。",
  "Binary file changed": "二进制文件已更改",
  "Diff is too large to display": "差异过大，无法显示",
  "Discard file": "丢弃文件",
  "Stage file": "暂存文件",
  "Expand upward": "向上展开",
  "Expand unmodified lines upward": "向上展开未修改的行",
  "Expand downward": "向下展开",
  "Expand unmodified lines downward": "向下展开未修改的行",
  "{0} unmodified {1}": "{0} 行未修改 {1}",
  "Comment on line {0}": "评论第 {0} 行",
  "Stage hunk": "暂存代码块",
  "unmodified": "未修改",
  "line": "行",
  "lines": "行",

  // ── WorkingTreeDiff.tsx ───────────────────────────────────────────────
  "Staged — no unstaged changes": "已暂存 — 无未暂存更改",
  "No unstaged changes": "无未暂存更改",
  "Couldn't load changes": "无法加载更改",

  // ══════════════════════════════════════════════════════════════════════
  // lib/
  // ══════════════════════════════════════════════════════════════════════

  // ── session.ts ────────────────────────────────────────────────────────
  "Supervised": "监督模式",
  "Auto-accept edits": "自动接受编辑",
  "Auto": "自动",
  "Full access": "完全访问",
  "Ask before commands and file changes.": "在执行命令和文件更改前询问。",
  "Auto-approve edits, ask before other actions.": "自动批准编辑，其他操作前询问。",
  "An AI reviewer approves routine actions; risky ones still ask.": "AI 审查者批准常规操作；风险操作仍会询问。",
  "Allow commands and edits without prompts.": "允许命令和编辑，无需提示。",
  "Claude Code": "Claude Code",
  "Codex": "Codex",
  "Cursor": "Cursor",
  "Grok Build": "Grok Build",
  "OpenCode": "OpenCode",
  "Pi": "Pi",

  // ── settings.ts ───────────────────────────────────────────────────────
  "App-wide behavior and the build you are running.": "应用全局行为和你正在运行的版本。",
  "Theme, translucency, and the tint applied to the chrome.": "主题、半透明度和应用于界面的色调。",
  "Every shortcut the workspace handles, from the app menu and the key handler.": "工作区处理的每个快捷键，来自应用菜单和按键处理程序。",
  "Agent CLIs MonoCode can drive, and the model new sessions start with.": "MonoCode 可驱动的代理 CLI，以及新会话使用的默认模型。",
  "Projects and conversations you have archived.": "你已归档的项目和对话。",

  // ── plan.ts ───────────────────────────────────────────────────────────
  "Create a reviewable implementation plan before changing files.": "在更改文件前创建可审查的实现计划。",
  "You are in plan mode.": "你处于计划模式。",
  "Investigate the request and the repository, but do not modify files, run destructive commands, or start implementing.": "调查请求和仓库，但不要修改文件、运行破坏性命令或开始实现。",
  "Resolve important implementation details and finish with one self-contained Markdown plan.": "解决重要的实现细节，并以一个独立的 Markdown 计划结束。",
  "The plan must be specific enough to build after explicit user approval.": "计划必须足够具体，以便在用户明确批准后进行构建。",
  "Structure the final plan with a Markdown heading and concrete implementation steps.": "使用 Markdown 标题和具体实现步骤来组织最终计划。",
  "Do not ask the user to approve inside the response; the application provides a separate Build action.": "不要在回复中要求用户批准；应用程序提供了单独的构建操作。",
  "Request": "请求",
  "The user reviewed and explicitly approved the following implementation plan.": "用户已审查并明确批准了以下实现计划。",
  "Implement it now, using this exact edited version as the source of truth.": "现在实现它，使用这个确切的编辑版本作为事实来源。",
  "sections": "章节",
  "diagram": "图表",
  "words": "字",

  // ── handoff.ts ────────────────────────────────────────────────────────
  "You are continuing an existing conversation handed off from": "你正在继续从以下位置交接的现有对话",
  "This is not a new session. Do not say you have no prior context.": "这不是一个新会话。不要说你没有先前的上下文。",
  "Continue from a": "继续来自",
  "session. Do not invent prior work.": "会话。不要编造先前的工作。",
  "Prior conversation from": "来自以下的先前对话",
  "this is the thread you are joining, not optional background:": "这是你正在加入的线程，不是可选的背景：",
  "The user is switching to another coding agent.": "用户正在切换到另一个编码代理。",
  "Their new message will be sent separately": "他们的新消息将单独发送",
  "do not repeat it, and do not add a Goal heading.": "不要重复它，也不要添加目标标题。",
  "Write a short recap of this conversation so the next agent can continue.": "写一段此对话的简短摘要，以便下一个代理可以继续。",
  "Under 120 words.": "不超过 120 字。",
  "Plain markdown.": "纯 Markdown。",
  "No title card.": "无标题卡片。",
  "No greeting.": "无问候语。",
  "Do not paste the whole transcript.": "不要粘贴整个对话记录。",
  "Use only this conversation.": "仅使用此对话。",
  "Do not run git, do not inspect the working tree, do not read files, do not call tools.": "不要运行 git，不要检查工作树，不要读取文件，不要调用工具。",
  "Mention files only if this chat edited them.": "仅在此聊天编辑了文件时才提及文件。",
  "If the chat was a greeting or has no task yet, say that in one sentence.": "如果聊天是问候或还没有任务，用一句话说明。",
  "Do not invent work from uncommitted repo files.": "不要从未提交的仓库文件中编造工作。",
  "Session so far": "到目前为止的会话",
  "Files edited in this session": "此会话中编辑的文件",
  "Suggested next step": "建议的下一步",
  "Current tasks": "当前任务",
  "(no text)": "（无文本）",
  "User:": "用户：",
  "Assistant:": "助手：",
  "(omitted earlier messages)": "（省略了早期消息）",
  "Goal": "目标",

  // ── secondOpinion.ts ──────────────────────────────────────────────────
  "Give a second opinion on work": "对工作给出第二意见",
  "just finished in this same working copy. The files are already on disk.": "刚在此同一工作副本中完成。文件已在磁盘上。",
  "Review that work: what is wrong, what is missing, and what you would have done differently.": "审查该工作：有什么问题，缺少什么，以及你会怎么做。",
  "Fix anything you agree is broken or incomplete.": "修复你认为有缺陷或不完整的部分。",
  "If you would leave it, say so and stop.": "如果你会保留它，说明并停止。",
  "Do not redo the task from scratch unless the work is actually wrong.": "除非工作确实有误，否则不要从头重做任务。",
  "Read the listed files before changing anything.": "在更改任何内容之前，先读取列出的文件。",
  "User request": "用户请求",
  "(no user message on this turn)": "（本轮无用户消息）",
  "reported": "报告",
  "(no written summary — inspect the files)": "（无书面摘要 — 检查文件）",
  "Files it edited": "它编辑的文件",
  "(none recorded on this turn)": "（本轮无记录）",

  // ── liveAgents.ts ─────────────────────────────────────────────────────

  // ── githubTasks.ts ────────────────────────────────────────────────────
  "Approved": "已批准",
  "Changes requested": "已请求更改",
  "Review required": "需要审查",
  "Requested changes": "已请求更改",
  "Dismissed": "已驳回",
  "Commented": "已评论",
  "Work on this Linear issue:": "处理此 Linear 议题：",
  "Work on this GitHub": "处理此 GitHub",
  "Linear #": "Linear #",

  // ── inFlight.ts ───────────────────────────────────────────────────────
  "Turn interrupted when MonoCode quit.": "MonoCode 退出时轮次被中断。",
  "Continue from where you left off.": "从上次中断的地方继续。",
  "1 chat is still running. Quit anyway? It will resume when you reopen MonoCode.": "1 个聊天仍在运行。确定退出吗？重新打开 MonoCode 时将恢复。",
  "{0} chats are still running. Quit anyway? They will resume when you reopen MonoCode.": "{0} 个聊天仍在运行。确定退出吗？重新打开 MonoCode 时将恢复。",

  // ── jsonText.ts ───────────────────────────────────────────────────────
  "[truncated]": "[已截断]",

  // ── contextUsage.ts ───────────────────────────────────────────────────
  "Context used": "已用上下文",
  "{0}% context used": "已用 {0}% 上下文",
  "tokens": "token",

  // ── rateLimits.ts ─────────────────────────────────────────────────────
  "Resets now": "立即重置",
  "Resets in": "重置于",
  "used": "已使用",
  "window": "窗口",
  "wk": "周",
  "5h": "5小时",
  "1h": "1小时",
  "Claude usage response was not JSON": "Claude 用量响应不是 JSON",
  "Claude usage response was empty": "Claude 用量响应为空",

  // ── rateLimitsFetch.ts ────────────────────────────────────────────────
  "Claude not signed in": "Claude 未登录",
  "Claude usage unavailable": "Claude 用量不可用",
  "Codex CLI not found": "未找到 Codex CLI",
  "Codex usage probe exited": "Codex 用量探测已退出",
  "No Codex usage data": "无 Codex 用量数据",
  "Codex not signed in": "Codex 未登录",
  "Codex usage probe timed out": "Codex 用量探测超时",

  // ── updater.ts ────────────────────────────────────────────────────────
  "Update available": "有可用更新",
  "is available (you have": "可用（你当前版本为",
  "Install now?": "立即安装？",
  "Automatic updates aren't configured for this build.": "此构建未配置自动更新。",
  "Download releases at https://github.com/hardbeat920/monocode/releases/latest": "在 https://github.com/hardbeat920/monocode/releases/latest 下载发布版",
  "Couldn't check for updates.": "无法检查更新。",
  "Couldn't install the update.": "无法安装更新。",

  // ── terminalClose.ts ──────────────────────────────────────────────────
  "is still running in": "仍在以下位置运行",
  "Close this terminal anyway?": "仍然关闭此终端？",
  "These terminals are still running:": "以下终端仍在运行：",
  "Close them anyway?": "仍然关闭它们？",

  // ── terminalTab.ts ────────────────────────────────────────────────────

  // ── sessionFolders.ts ─────────────────────────────────────────────────
  // (New folder already covered above)

  // ── tabGroups.ts ──────────────────────────────────────────────────────
  "Group": "分组",

  // ── taskList.ts ───────────────────────────────────────────────────────
  "Complete": "完成",
  "of": "/",

  // ── projectLogos.ts ───────────────────────────────────────────────────
  "Choose project logo": "选择项目标志",
  "Images": "图片",

  // ── compact.ts ────────────────────────────────────────────────────────
  "Summarize older conversation context to free space.": "压缩较旧的对话上下文以释放空间。",

  // ── createSkill.ts ────────────────────────────────────────────────────
  "Create a MonoCode skill as a SKILL.md in .agents/skills. Use when the user wants to author, write, save, or scaffold a skill, or asks about skill format.": "在 .agents/skills 中创建 MonoCode 技能（SKILL.md）。当用户想要编写、保存或搭建技能，或询问技能格式时使用。",
  "Create a MonoCode skill": "创建 MonoCode 技能",
  "Write a portable Agent Skill so every harness (Claude, Cursor, Codex, Grok Build, OpenCode, Pi, omp, fx) can load it.": "编写一个可移植的代理技能，使每个运行器（Claude、Cursor、Codex、Grok Build、OpenCode、Pi、omp、fx）都能加载它。",
  "Storage (required)": "存储位置（必填）",
  "Gather before writing": "写前收集",
  "Purpose": "目的",
  "Scope": "范围",
  "Triggers": "触发条件",
  "Domain knowledge": "领域知识",
  "Output format": "输出格式",
  "Verbatim text": "逐字文本",
  "Ask where it should live if the user did not say. Default to the project when a project folder is open.": "如果用户未指定，询问存储位置。当项目文件夹打开时默认为项目。",
  "File layout": "文件布局",
  "SKILL.md format": "SKILL.md 格式",
  "Description": "描述",
  "Authoring rules": "编写规则",
  "After writing": "编写后",
  "Old patterns": "旧模式",
  "Confirm the file path": "确认文件路径",
  "Confirm name + description": "确认名称和描述",
  "Do not copy the skill into harness-specific folders": "不要将技能复制到运行器特定文件夹中",

  // ── skills.ts ─────────────────────────────────────────────────────────
  "The user invoked skill(s) with /name. Follow every instruction in each skill body.": "用户通过 /name 调用了技能。请遵循每个技能体中的所有指令。",
  "Use a lowercase name with letters, numbers, and hyphens.": "使用小写字母、数字和连字符的名称。",

  // ── diffComment.ts ────────────────────────────────────────────────────
  "Diff comment on": "差异评论于",
  " (deleted line)": "（已删除行）",

  // ── fs.ts ─────────────────────────────────────────────────────────────
  "Attach files": "附加文件",

  // ── filePreview.ts ────────────────────────────────────────────────────
  "KB": "KB",
  "MB": "MB",
  "GB": "GB",

  // ── notes.ts ──────────────────────────────────────────────────────────
  "Note:": "笔记：",
  "Referenced note": "引用的笔记",
  "Use this note.": "使用此笔记。",

  // ── userQuestion.ts ──────────────────────────────────────────────────
  "Question 1": "问题 1",

  // ── appLifecycle.ts ───────────────────────────────────────────────────
  "Quit": "退出",

  // ── releaseNotes.ts ───────────────────────────────────────────────────
  "What's new in MonoCode": "MonoCode 更新说明",
  "Jan": "1月",
  "Feb": "2月",
  "Mar": "3月",
  "Apr": "4月",
  "May": "5月",
  "Jun": "6月",
  "Jul": "7月",
  "Aug": "8月",
  "Sep": "9月",
  "Oct": "10月",
  "Nov": "11月",
  "Dec": "12月",

  // ── Missing keys added during audit ────────────────────────────────────

  // App.tsx
  "Close this tab with unsaved files?": "关闭此标签页（有未保存的文件）？",
  "Close other tabs with unsaved files?": "关闭其他标签页（有未保存的文件）？",
  "Close {0} without saving?": "关闭 {0} 而不保存？",
  "Close this conversation with unsaved files?": "关闭此对话（有未保存的文件）？",
  "this session": "此会话",
  "Archive this conversation with unsaved files?": "归档此对话（有未保存的文件）？",
  "Delete this conversation with unsaved files?": "删除此对话（有未保存的文件）？",
  "Build approved plan": "构建已批准的计划",
  "Compacting context…": "正在压缩上下文…",
  "Compacted context": "已压缩上下文",
  "{0} could not compact this context": "{0} 无法压缩此上下文",

  // Sidebar.tsx
  "Pin": "固定",
  "Unpin": "取消固定",
  "now": "刚刚",
  "{0}m": "{0}分钟",
  "{0}h {1}m": "{0}小时{1}分钟",
  "{0}h": "{0}小时",
  "{0}d": "{0}天",

  // TitleBar.tsx
  "Back ({0})": "后退 ({0})",
  "Forward ({0})": "前进 ({0})",
  "Toggle Sidebar ({0})": "切换侧边栏 ({0})",
  "Go to File ({0})": "跳转到文件 ({0})",
  "New session ({0})": "新建会话 ({0})",
  "New Terminal ({0})": "新建终端 ({0})",
  "Settings ({0})": "设置 ({0})",

  // FileTree.tsx
  "files": "文件",
  "changed": "已更改",
  "A file or folder": "文件或文件夹",
  "already exists at this location. Please choose a different name.": "已存在于此位置。请选择其他名称。",
  "The name": "名称",
  "is not valid as a file or folder name. Please choose a different name.": "不是有效的文件或文件夹名称。请选择其他名称。",

  // SurfaceTabs.tsx
  "(Working Tree)": "（工作树）",
  "problem": "个问题",
  "problems": "个问题",

  // ProjectRail.tsx
  "Search ({0})": "搜索 ({0})",
  "{0} {1} changed": "{0} {1} 已更改",

  // CwdPicker.tsx
  "Project picker": "项目选择器",
  "More projects": "更多项目",

  // SessionFiltersMenu.tsx
  "Provider": "提供商",

  // ModelPicker.tsx
  "{0} · {1} ({2}.)": "{0} · {1} ({2}.)",
  "{0} {1}": "{0} {1}",
  "Model picker": "模型选择器",

  // AttachmentChip.tsx
  "Remove {0}": "移除 {0}",

  // BranchPicker.tsx
  "Branch picker": "分支选择器",

  // GitChangesPanel.tsx
  "Discard all unstaged changes in {0} files? This cannot be undone.": "丢弃 {0} 个文件中所有未暂存的更改？此操作无法撤销。",
  "Could not prepare pull request content": "无法准备拉取请求内容",

  // RemoveProjectDialog.tsx
  "Delete {0}": "删除 {0}",

  // SecondOpinionButton.tsx
  "{0} models": "{0} 个模型",

  // settings.ts keybinding labels
  "App: Search": "应用：搜索",
  "App: Go to File": "应用：跳转到文件",
  "App: Find in Files": "应用：在文件中查找",
  "App: Open Project": "应用：打开项目",
  "App: New Window": "应用：新建窗口",
  "App: Toggle Sidebar": "应用：切换侧边栏",
  "App: Switch Model": "应用：切换模型",
  "Tab: New": "标签页：新建",
  "Tab: Close Others": "标签页：关闭其他",
  "Tab: Next": "标签页：下一个",
  "Tab: Previous": "标签页：上一个",
  "Tab: Cycle Next": "标签页：循环下一个",
  "Tab: Cycle Previous": "标签页：循环上一个",
  "Tab: Back": "标签页：后退",
  "Tab: Forward": "标签页：前进",
  "Tab: Activate 1–8": "标签页：激活 1–8",
  "Tab: Activate Last": "标签页：激活最后",
  "Session: Previous": "会话：上一个",
  "Session: Next": "会话：下一个",
  "Project: Previous": "项目：上一个",
  "Project: Next": "项目：下一个",
  "Pane: Close": "面板：关闭",
  "Pane: Split Right": "面板：向右拆分",
  "Pane: Split Down": "面板：向下拆分",
  "Pane: Focus Left": "面板：聚焦左侧",
  "Pane: Focus Right": "面板：聚焦右侧",
  "Pane: Focus Up": "面板：聚焦上方",
  "Pane: Focus Down": "面板：聚焦下方",
  "Terminal: New": "终端：新建",
  "Terminal: New Tab": "终端：新建标签页",
  "Terminal: Toggle Dock": "终端：切换停靠",
  "Editor: Find": "编辑器：查找",
  "Editor: Replace": "编辑器：替换",
  "Always": "始终",

  // userQuestion.ts
  "Question {0}": "问题 {0}",

  // githubTasks.ts
  "Linear #{0}": "Linear #{0}",
  "pull request": "拉取请求",
  "issue": "议题",
  "GitHub {0} #{1}": "GitHub {0} #{1}",
  "Work on this GitHub {0}:": "处理此 GitHub {0}：",

  // contextUsage.ts
  "{0} / {1} tokens": "{0} / {1} token",
  "{0} tokens": "{0} token",

  // session.ts
  "omp": "omp",
  "fx": "fx",

  // notes.ts
  "Note: {0}": "笔记：{0}",

  // updater.ts
  "MonoCode {0} is available (you have {1}).": "MonoCode {0} 可用（你当前版本为 {1}）。",
  "Download releases at {0}": "在 {0} 下载发布版",

  // rateLimits.ts
  "Resets in {0}": "重置于 {0}",
  "{0} used": "已使用 {0}",
  "{0} window": "{0} 窗口",

  // diffComment.ts
  "deleted line": "已删除行",

  // handoff.ts
  "({0} earlier messages omitted)": "（省略了 {0} 条早期消息）",

  // models.ts
  "Reasoning": "推理",
  "Extra High": "极高",
  "High": "高",
  "Medium": "中",
  "Low": "低",

  // plan.ts
  "{0} sections": "{0} 个章节",
  "{0} words": "{0} 字",

  // InboxComments.tsx
  "Replying to {0}": "回复 {0}",
  "comment": "评论",
  "Write a reply ({0})": "写回复 ({0})",
  "Leave a comment ({0})": "留言 ({0})",

  // BinaryFileView.tsx
  "Copy path": "复制路径",

  // UnifiedDiffView.tsx
  "{0} unmodified line": "{0} 行未修改",
  "{0} unmodified lines": "{0} 行未修改",

  // AgentTranscript.tsx
  "Thinking": "思考中",
  "Thinking: {0}": "思考中：{0}",

  // SettingsView.tsx
  "Filter": "筛选",
  "{0} model": "{0} 个模型",

  // NotesView.tsx
  "Filter notes": "筛选笔记",

  // ProjectTerminalDock.tsx
  "Hide Terminal ({0})": "隐藏终端 ({0})",

  // AccessPicker.tsx
  "Access": "访问权限",

  // MenuBar.tsx
  "menu": "菜单",

  // harness protocol fallbacks
  "Approve file changes": "批准文件更改",
  "Choose an option": "选择一个选项",
  "Confirm": "确认",

};
