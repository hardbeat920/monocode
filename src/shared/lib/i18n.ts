const LANGUAGE_KEY = "monocode.language";

/** Stored preference; "auto" follows the OS language. */
export type Language = "auto" | "en" | "zh";

/** The language actually rendered, after resolving "auto". */
export type ResolvedLanguage = "en" | "zh";

export const LANGUAGE_DEFAULT: Language = "auto";

/** Fired on `window` when the interface language preference flips. */
export const LANGUAGE_CHANGE_EVENT = "monocode:language-change";

/**
 * Gettext-style table: English source strings are the keys, so `t()` falls
 * back to the key itself and untranslatable surfaces keep working. Keys must
 * match the source literals exactly — watch for curly apostrophes (aren’t).
 */
const ZH: Record<string, string> = {
  // Title bar
  "New session": "新会话",
  "{count} sessions": "{count} 个会话",
  "Unsaved changes": "未保存的更改",
  "Scroll tabs left": "向左滚动标签页",
  "Scroll tabs right": "向右滚动标签页",
  Development: "开发版",
  "Development build": "开发构建",
  Back: "后退",
  Forward: "前进",
  "Toggle Projects": "切换项目栏",
  "Toggle Sidebar": "切换侧边栏",
  "Toggle Session Sidebar": "切换会话侧边栏",
  "Close Tab": "关闭标签页",
  "Close {name}": "关闭 {name}",
  "Close Other Tabs": "关闭其他标签页",
  "Close All Tabs": "关闭所有标签页",
  "Close Tabs to the Right": "关闭右侧标签页",
  "Close Tabs to the Left": "关闭左侧标签页",
  "Tab actions for {name}": "标签页操作：{name}",
  Inbox: "收件箱",
  Notes: "笔记",
  "Go to File": "前往文件",
  Settings: "设置",
  "No project": "无项目",

  "Response complete": "回复已完成",

  // Composer
  "Queue paused because you interrupted": "队列已暂停，因为你中断了运行",
  Resume: "继续",
  "1 attachment": "1 个附件",
  "{count} attachments": "{count} 个附件",
  "Edit queued message": "编辑排队消息",
  "Save queued message": "保存排队消息",
  "Cancel queued message edit": "取消编辑排队消息",
  "Remove queued message": "移除排队消息",
  "Drop files to attach": "拖放文件以添加附件",
  "Add a note, or send to start…": "添加备注，或直接发送以开始…",
  "Add a message, or send…": "添加消息，或直接发送…",
  "Add context, or send to continue…": "添加背景信息，或直接发送以继续…",
  "Select a branch or worktree to continue…": "选择一个分支或工作树以继续…",
  "Ask, build, / for commands, @ for references... ":
    "提问、构建，/ 呼出命令，@ 引用文件…",
  "Add to message": "添加到消息",
  "Upload file": "上传文件",
  "Attach files or images": "添加文件或图片",
  "{name} does not support attachments": "{name} 不支持附件",
  "Plan mode": "计划模式",
  "Review a plan before building": "先审查计划再开始构建",
  "Plan and coordinate agent work": "规划并协调智能体工作",
  "Turn off Orchestrator mode": "关闭编排器模式",
  Plan: "计划",
  "Turn off Plan mode": "关闭计划模式",
  Send: "发送",
  "Save draft": "保存草稿",
  Stop: "停止",

  // Menus (in-app menu bar on Windows/Linux)
  File: "文件",
  View: "视图",
  Terminal: "终端",
  "New Tab": "新建标签页",
  "New Window": "新建窗口",
  "Open Project…": "打开项目…",
  "Search…": "搜索…",
  "Go to File…": "前往文件…",
  "Find in Files…": "在文件中查找…",
  "Close Pane": "关闭面板",
  "Check for Updates…": "检查更新…",
  "Switch Model…": "切换模型…",
  "Toggle Changes": "显示/隐藏更改",
  "Zoom In": "放大",
  "Zoom Out": "缩小",
  "Reset Zoom": "重置缩放",
  "Toggle Terminal": "显示/隐藏终端",
  "New Terminal": "新建终端",

  // Chat background effects
  "Background effect": "背景效果",
  "Shows the original artwork.": "显示原始图像。",
  "Rebuilds the artwork with a dithered color palette.":
    "使用抖动调色板重新绘制图像。",
  "Recreates the artwork with colored characters on black.":
    "在黑色背景上用彩色字符重现图像。",
  "Recreates the artwork with colored print dots on black.":
    "在黑色背景上用彩色印刷圆点重现图像。",
  "Adds a pronounced horizontal display-line texture.":
    "添加明显的水平显示扫描线纹理。",
  None: "无",
  Dither: "抖动",
  ASCII: "ASCII",
  Halftone: "半色调",
  Scanlines: "扫描线",

  // Sidebar
  Sessions: "会话",
  Explorer: "资源管理器",
  Changes: "更改",
  Rename: "重命名",
  Ungroup: "取消分组",
  "Cancel reminder": "取消提醒",
  "Multiple reminder times": "多个提醒时间",
  Pin: "置顶",
  Unpin: "取消置顶",
  "Remind me": "提醒我",
  "New folder": "新建文件夹",
  "Add to {name}": "添加到 {name}",
  "Remove from folder": "从文件夹移除",
  "Remove from folders": "从文件夹移除",
  "Search conversations...": "搜索对话…",
  "Search conversations": "搜索对话",
  "No project folder": "没有项目文件夹",
  "Couldn’t load sessions": "无法加载会话",
  "No matching sessions": "没有匹配的会话",
  "No sessions match these filters": "没有会话符合这些筛选条件",
  "Sessions you start will show up here": "你发起的会话会显示在这里",
  Reminders: "提醒",
  Pinned: "已置顶",
  "Filter sessions": "筛选会话",
  "Search projects": "搜索项目",
  "Search projects...": "搜索项目…",
  "New project": "新建项目",
  "Project picker": "项目选择器",
  "Choose project": "选择项目",
  "Choose project for note": "为笔记选择项目",
  "Switch project": "切换项目",
  "Move note to project": "移动笔记到项目",
  "{action}, current project {project}": "{action}，当前项目 {project}",
  "No projects found": "未找到项目",
  "New tab": "新建标签页",
  Search: "搜索",
  "Inbox, new items": "收件箱，有新条目",
  "Session actions": "会话操作",
  "{count} selected session actions": "已选 {count} 个会话的操作",
  "Folder actions": "文件夹操作",
  "Needs input": "需要输入",
  "Need approval": "需要批准",
  "Working...": "运行中…",
  Done: "完成",
  "Linked work item updated": "关联的工作项已更新",
  "Linked {kind} updated since this session": "关联的{kind}自本会话后有更新",
  issue: "议题",
  Subagents: "子智能体",
  Orchestrator: "编排器",
  "Orchestrator, {count} {unit}, {done} done":
    "编排器，{count} 个{unit}，已完成 {done} 个",
  subagent: "子智能体",
  subagents: "子智能体",
  "{done}/{total} done": "已完成 {done}/{total}",

  // Settings navigation and search
  App: "应用",
  Agents: "智能体",
  Workspace: "工作区",
  "How project navigation and workspace tabs behave.":
    "项目导航与工作区标签页的行为方式。",
  General: "通用",
  Appearance: "外观",
  Keybindings: "快捷键",
  Chat: "对话",
  Providers: "提供商",
  Skills: "技能",
  Archive: "归档",
  Worktrees: "工作树",
  "The build you are running, how MonoCode reaches you, and the panels it shows.":
    "你正在运行的版本、MonoCode 联系你的方式，以及它显示的面板。",
  "Theme, tint, translucency, workspace layout, and conversation backgrounds.":
    "主题、色调、半透明效果、工作区布局，以及对话背景。",
  "Every shortcut the workspace handles, from the app menu and the key handler.":
    "工作区处理的全部快捷键，来自应用菜单和按键处理程序。",
  "How transcripts read, what the composer does with a follow-up, and how diffs open.":
    "会话记录的阅读方式、输入框对后续消息的处理，以及差异视图的打开方式。",
  "Provider accounts, agent CLIs MonoCode can drive, and the model new sessions start with.":
    "提供方账号、MonoCode 可以驱动的智能体 CLI，以及新会话使用的默认模型。",
  "Discover and manage file skills from project, personal, and harness folders.":
    "发现和管理来自项目、个人与智能体目录的文件技能。",
  "Manage Inbox services and notification preferences for each project.":
    "管理收件箱服务以及每个项目的通知偏好。",
  "Projects and conversations you have archived.": "你已归档的项目和会话。",
  "Manage additional worktrees for each project.": "管理每个项目的额外工作树。",
  "Search settings": "搜索设置",
  "Clear settings search": "清除设置搜索",
  "Settings search results": "设置搜索结果",
  "No matching settings": "没有匹配的设置",
  Page: "页面",
  "Restore defaults": "恢复默认设置",

  // General page
  Language: "语言",
  "Quick composer": "快速输入框",
  "Press {shortcut} in any app to float a prompt over it and start a session without switching to MonoCode. Change the shortcut in Keybindings. Return starts it in the background; ⌘Return starts it and brings the session forward.":
    "在任意应用中按 {shortcut} 可悬浮打开提示框并启动会话，无需切换到 MonoCode。可在快捷键设置中更改快捷键。按 Return 在后台启动；按 ⌘Return 启动并切换到会话。",
  "Display language for the interface. Takes effect immediately.":
    "界面显示语言，立即生效。",
  "Auto (system)": "自动（跟随系统）",
  Alerts: "提醒",
  "How MonoCode reaches you while you are looking somewhere else.":
    "当你聚焦别处时 MonoCode 联系你的方式。",
  Sounds: "提示音",
  "Short cues for project activity, finished turns, and available updates. Choose project notification categories in Inbox settings. Switches and Copy on a finished turn also play.":
    "项目活动、完成一轮对话以及有可用更新时播放短促提示音。可在收件箱设置中选择项目通知类别。切换开关与完成一轮后的复制操作也会播放。",
  Notifications: "通知",
  "Notify when a reminder is due, or when an agent finishes or needs input in another session or while MonoCode is in the background. Click the notification to open that session.":
    "在提醒到期、智能体完成或在其他会话中等待输入、或 MonoCode 处于后台时发送通知。点击通知可打开对应会话。",
  "Not available on this platform": "当前平台不可用",
  "A global markdown notebook on the project rail. Save a finished turn from the transcript, then mention it later with @note or add it to chat.":
    "项目栏上的全局 Markdown 笔记本。可从会话记录保存完成的一轮对话，之后用 @note 提及或加入聊天。",
  "Working agents": "运行中的智能体",
  "When two or more chats are in flight, a card on the project rail lists them so you can jump across projects. Finished turns stay until you open that session.":
    "当有两个或更多对话正在进行时，项目栏上的卡片会列出它们，方便跨项目跳转。完成的一轮会一直保留，直到你打开该会话。",
  About: "关于",
  Version: "版本",
  "Version {version} is available.": "新版本 {version} 可用。",
  "Downloading{progress}": "下载中{progress}",
  "Checking for updates…": "正在检查更新…",
  "You're on the latest version.": "已是最新版本。",
  "Update check failed.": "检查更新失败。",
  "MonoCode updates itself from the release feed.":
    "MonoCode 会从发布源自动更新。",
  "What's new": "新功能",
  Download: "下载",
  "Check for updates": "检查更新",
  "Permission needed": "需要权限",
  "Open System Settings": "打开系统设置",

  // Appearance page
  Theme: "主题",
  "Accent color": "强调色",
  "Chat background": "聊天背景",
  "Empty chat background visibility": "空聊天背景可见度",
  "Session background visibility": "会话背景可见度",
  "Dark and light share the same tint, so the color settings below apply to both.":
    "深色与浅色主题共享同一色调，因此下方的颜色设置对两者都生效。",
  "System follows the OS appearance.": "跟随系统外观。",
  System: "跟随系统",
  Dark: "深色",
  Light: "浅色",
  "Used for the composer send button and your message bubbles.":
    "用于输入框的发送按钮和你的消息气泡。",
  Default: "默认",
  Blue: "蓝色",
  Violet: "紫色",
  Pink: "粉色",
  Red: "红色",
  Orange: "橙色",
  Green: "绿色",
  Color: "颜色",
  "Hue and saturation tint every surface. Lightness only moves the dark theme.":
    "色相与饱和度为所有表面着色。亮度仅影响深色主题。",
  Hue: "色相",
  "Base hue for accents and tinted surfaces.": "强调色与着色表面的基础色相。",
  Saturation: "饱和度",
  "How strongly the hue tints the interface. Zero keeps it neutral.":
    "色相对界面的着色强度。为零时保持中性。",
  "Dark-mode lightness": "深色模式亮度",
  "Base brightness of the dark theme. Lower values are darker; zero is true black.":
    "深色主题的基础亮度。数值越低越暗，零为纯黑。",
  "This only affects dark mode. Your dark-mode value is preserved.":
    "仅影响深色模式。你的深色模式数值会保留。",
  Translucency: "半透明",
  "Light mode always uses an opaque window, so these are off. Your dark-mode values are preserved.":
    "浅色模式始终使用不透明窗口，因此这些选项不可用。你的深色模式数值会保留。",
  "How much of the desktop shows through MonoCode. Blur costs more to composite the higher it goes.":
    "桌面透过 MonoCode 显示的程度。模糊半径越大，合成的开销越高。",
  "Sidebar opacity": "侧边栏不透明度",
  "Applies to the project rail and the other glass panes.":
    "作用于项目栏和其他毛玻璃面板。",
  "Blur radius": "模糊半径",
  "Background blur behind the window.": "窗口背后的背景模糊。",
  "Main pane glass": "主面板毛玻璃",
  "Extend the translucent treatment to the main pane behind sessions and editors.":
    "将半透明效果扩展到会话和编辑器背后的主面板。",
  "An image behind your chat panes. It stays on this device.":
    "聊天面板背后的图片。仅保存在本设备上。",
  "Empty chat preview at {percent}%": "空聊天预览，{percent}%",
  "Choose an image": "选择图片",
  Change: "更改",
  Remove: "移除",
  "Show on": "显示范围",
  "Empty sessions only, or every conversation.": "仅空会话，或所有会话。",
  "Show background on": "背景显示范围",
  "Empty only": "仅空会话",
  "All sessions": "所有会话",
  "Empty chat visibility": "空聊天可见度",
  "Background strength before a chat has messages.":
    "聊天有消息之前的背景强度。",
  "Session visibility": "会话可见度",
  "Background strength once the conversation has messages.":
    "对话有消息之后的背景强度。",
  Layout: "布局",
  "Interface scale": "界面缩放",
  "Zoom the whole interface. You can also use Ctrl+=, Ctrl+-, and Ctrl+0 (Cmd on macOS).":
    "缩放整个界面。也可以使用 Ctrl+=、Ctrl+- 和 Ctrl+0（macOS 上为 Cmd）。",

  // Chat page
  Transcript: "会话记录",
  "How a conversation reads as it grows.": "对话随增长后的阅读方式。",
  "Transcript layout": "会话记录布局",
  "Full width keeps user prompts as a spanning card. Chat aligns them to the right with a max width, like a messaging app.":
    "全宽将用户提示显示为通栏卡片。聊天模式将其右对齐并限制最大宽度，类似聊天应用。",
  "Full width": "全宽",
  "Anchor prompts to top": "提示置顶",
  "When you send, the new prompt sits at the top of the transcript and the reply grows into the space below. Turn this off to keep the classic layout, with the latest message resting on the composer.":
    "发送后，新提示位于会话记录顶部，回复在下方空间中展开。关闭后保持经典布局，最新消息停留在输入框旁。",
  Composer: "输入框",
  "What the composer does with what you type.": "输入框如何处理你输入的内容。",
  "Follow-up behavior": "后续消息行为",
  "Queue follow-ups until the active turn finishes, or steer the active turn immediately.":
    "将后续消息排队等待当前回合完成，或立即介入当前回合。",
  Queue: "排队",
  Steer: "介入",
  "Model controls": "模型控制",
  "Show model options beside the picker instead of inside the model menu.":
    "在模型选择器旁显示模型选项，而不是放在模型菜单内。",
  Menu: "菜单",
  Beside: "选择器旁",
  "Code review": "代码审查",
  "Where a turn's changes open when you go to read them.":
    "查看一轮更改时打开的视图。",
  "What happens when you save a file in the workspace editor.":
    "在工作区编辑器中保存文件时执行的操作。",
  "Format on save": "保存时格式化",
  "Run Prettier on supported files before writing. Off keeps the text you typed, including quote style.":
    "写入前使用 Prettier 格式化支持的文件。关闭后会保留你输入的文本，包括引号样式。",
  "Diff view": "差异视图",
  "Editor keeps working-tree changes in the file. Unified stacks every changed file in one review, with sticky headers and collapsed unchanged lines.":
    "编辑器在文件内显示工作区更改。统一视图将所有更改的文件叠加在同一个审查界面中，带固定标题并折叠未更改的行。",
  Editor: "编辑器",
  Unified: "统一视图",
  Extras: "额外功能",
  "Idle animation, and nothing else. Turn both off for a still workspace.":
    "空闲动画，仅此而已。全部关闭可获得静止的工作区。",
  "Composer mascot": "输入框吉祥物",
  "When a turn is running, the project mascot runs along the composer, bonks the scroll-to-latest button the first time, then jumps it, and sometimes grabs a coin.":
    "对话进行时，项目吉祥物会沿输入框奔跑，第一次会撞上“滚动到最新”按钮，然后跳过去，偶尔还会捡起一枚金币。",
  "Empty session games": "空会话小游戏",
  "Pac-man and snake idle on the empty-session grid. Hover the band to take control of whichever is on screen. Turn this off to keep the pane still.":
    "吃豆人和贪吃蛇在空会话网格上待机。将鼠标悬停在其上即可操控当前显示的游戏。关闭可保持面板静止。",

  // Keybindings page
  Shortcuts: "快捷键",
  Keybinding: "按键",
  "Click a shortcut to record new keys. Press Delete while recording to disable it.":
    "点击快捷键即可录制新的组合键；录制时按 Delete 可禁用它。",
  binding: "个快捷键",
  bindings: "个快捷键",
  Filter: "筛选",
  "Filter keybindings": "筛选快捷键",
  Command: "命令",
  When: "条件",
  Always: "总是",
  "No matching bindings": "没有匹配的快捷键",

  // Providers page
  "Agent CLIs": "智能体 CLI",
  "Provider defaults scope": "提供方默认设置范围",
  Global: "全局",
  "These defaults apply to {name} only. A provider with Show in picker off is also kept out of new conversations started in this project. CLI paths remain global for MonoCode.":
    "这些默认设置仅应用于 {name}。关闭“在选择器中显示”的提供方也不会出现在此项目新建的会话中。CLI 路径对 MonoCode 全局生效。",
  "Hidden globally": "已在全局隐藏",
  "A provider is listed as installed once its CLI is found on your PATH. Uninstalled CLIs stay listed but are left out of the model picker, as are installed ones with Show in picker off. The model beside a provider is what its new conversations start with; Use by default picks the provider itself. CLI paths are global for MonoCode and apply to every project.":
    "提供方的 CLI 在 PATH 中找到后即显示为已安装。未安装的 CLI 仍会列出，但不会出现在模型选择器中；已安装但关闭“在选择器中显示”的也一样。提供方旁边的模型是其新对话的默认模型；“设为默认”会选中该提供方本身。CLI 路径对 MonoCode 全局生效，适用于每个项目。",
  Advanced: "高级",
  "Claude Code hooks": "Claude Code 钩子",
  "Run the hooks configured in your settings.json files — PreToolUse command rewrites, blocks, notifications, and the rest — just as the Claude Code CLI would. Turn this off if a hook is misbehaving and you need the session back. Takes effect on the next turn.":
    "运行你在 settings.json 中配置的钩子——PreToolUse 命令重写、拦截、通知等——与 Claude Code CLI 的行为一致。如果某个钩子行为异常，可关闭此项找回会话。下一回合生效。",
  "{count} {unit} available.": "有 {count} 个{unit}。",
  model: "模型",
  models: "模型",
  "{name} model": "{name} 模型",
  "Use by default": "设为默认",
  "Show in picker": "在选择器中显示",
  "Show {name} in the model picker": "在模型选择器中显示 {name}",
  "{name} not found{how}. Install it, or restart MonoCode if it is already installed.":
    "未找到 {name}{how}。请安装后重试；如果已安装，请重启 MonoCode。",

  // Inbox page
  "Pull requests, reviews, and issues, read through the GitHub CLI.":
    "通过 GitHub CLI 读取拉取请求、评审和议题。",
  "Merge requests from GitLab.com or a self-managed instance.":
    "来自 GitLab.com 或自托管实例的合并请求。",
  "Issues assigned to you, from the teams you pick.":
    "来自你选择的团队、分配给你的议题。",
  Connection: "连接",
  "GitHub CLI is installed and authenticated. MonoCode uses it for GitHub inbox items.":
    "GitHub CLI 已安装并完成认证。MonoCode 用它读取 GitHub 收件箱条目。",
  "Run gh auth login in a terminal, complete the sign-in flow, then check again.":
    "在终端运行 gh auth login 完成登录流程，然后重新检查。",
  "Install GitHub CLI from cli.github.com, run gh auth login in a terminal, then check again.":
    "从 cli.github.com 安装 GitHub CLI，在终端运行 gh auth login，然后重新检查。",
  Checking: "检查中",
  Connected: "已连接",
  "Sign in required": "需要登录",
  "Not installed": "未安装",
  "Installation guide": "安装指南",
  "Check again": "重新检查",
  "Connect GitLab.com or a self-managed GitLab instance. Use a personal access token with API access; the token is stored locally and Disconnect deletes it.":
    "连接 GitLab.com 或自托管的 GitLab 实例。请使用具有 API 权限的个人访问令牌；令牌仅保存在本地，断开连接时会将其删除。",
  Disconnect: "断开连接",
  "GitLab URL": "GitLab 地址",
  "GitLab access token": "GitLab 访问令牌",
  Saving: "保存中",
  Connect: "连接",
  "API key": "API 密钥",
  "Create a personal API key in Linear → Settings → Security & Access. Disconnect deletes it.":
    "在 Linear → 设置 → Security & Access 中创建个人 API 密钥。断开连接时会将其删除。",
  "Linear API key": "Linear API 密钥",
  Teams: "团队",
  "Unchecked teams stay out of the inbox.": "未勾选的团队不会进入收件箱。",

  // Archive page
  "Archived projects": "已归档项目",
  "Archive a project from the rail to keep its chats without listing it in the sidebar.":
    "从项目栏归档项目，保留其对话但不在侧边栏显示。",
  "No archived projects.": "没有已归档的项目。",
  Restore: "恢复",
  Delete: "删除",
  "Archived in {name}": "{name} 的归档",
  "Archived conversations": "已归档的对话",
  "Show archived in the sidebar": "在侧边栏显示已归档会话",
  "Keep archived conversations listed alongside the active ones.":
    "让已归档的会话与活跃会话一起显示。",
  "Open a project to see its archived conversations.":
    "打开一个项目以查看其归档的对话。",
  "No archived conversations in this project.": "该项目没有已归档的对话。",
  Unarchive: "取消归档",

  // Remove project dialog
  "Delete {name}": "删除 {name}",
  "Delete “{name}”?": "删除“{name}”吗？",
  "All conversations for this project will be deleted. It also leaves the sidebar. The folder on disk stays put, and opening it again brings the project back empty.":
    "该项目的所有对话都将被删除，项目也会从侧边栏移除。磁盘上的文件夹保持不变，重新打开它会以空项目形式恢复。",
  "1 saved conversation will be removed.": "将移除 1 条已保存的对话。",
  "{count} saved conversations will be removed.":
    "将移除 {count} 条已保存的对话。",
  Cancel: "取消",
};

const TABLES: Record<ResolvedLanguage, Record<string, string> | null> = {
  en: null,
  zh: ZH,
};

function isLanguage(value: unknown): value is Language {
  return value === "auto" || value === "en" || value === "zh";
}

export function loadLanguage(): Language {
  try {
    const raw = localStorage.getItem(LANGUAGE_KEY);
    return isLanguage(raw) ? raw : LANGUAGE_DEFAULT;
  } catch {
    return LANGUAGE_DEFAULT;
  }
}

export function saveLanguage(value: Language) {
  const next = isLanguage(value) ? value : LANGUAGE_DEFAULT;
  try {
    localStorage.setItem(LANGUAGE_KEY, next);
  } catch {
    // private mode / quota
  }
  applyDocumentLanguage(currentLanguage());
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<Language>(LANGUAGE_CHANGE_EVENT, {
      detail: next,
    }),
  );
}

function systemLanguage(): ResolvedLanguage {
  try {
    return navigator.language?.toLowerCase().startsWith("zh") ? "zh" : "en";
  } catch {
    return "en";
  }
}

export function resolveLanguage(value: Language): ResolvedLanguage {
  return value === "auto" ? systemLanguage() : value;
}

// Re-read on every change event but cache in between: `t()` runs hundreds of
// times per render, and comparing the raw stored string also self-heals when
// another app window (or a test) rewrites localStorage directly.
let cachedRaw: string | null | undefined;
let cachedResolved: ResolvedLanguage = "en";

export function currentLanguage(): ResolvedLanguage {
  let raw: string | null;
  try {
    raw = localStorage.getItem(LANGUAGE_KEY);
  } catch {
    raw = null;
  }
  if (raw !== cachedRaw) {
    cachedRaw = raw;
    cachedResolved = resolveLanguage(isLanguage(raw) ? raw : LANGUAGE_DEFAULT);
  }
  return cachedResolved;
}

export function subscribeLanguage(onStoreChange: () => void): () => void {
  if (typeof window === "undefined") return () => {};
  window.addEventListener(LANGUAGE_CHANGE_EVENT, onStoreChange);
  return () => window.removeEventListener(LANGUAGE_CHANGE_EVENT, onStoreChange);
}

function applyDocumentLanguage(language: ResolvedLanguage) {
  try {
    document.documentElement.lang = language === "zh" ? "zh-CN" : "en";
  } catch {
    // not in a DOM
  }
}

/** Applies the persisted language at startup; no re-render is needed yet. */
export function initLanguage() {
  applyDocumentLanguage(currentLanguage());
}

/** Looks up the current table, so calls inside render stay consistent. */
export function t(
  key: string,
  params?: Record<string, string | number>,
): string {
  const table = TABLES[currentLanguage()];
  let text = table?.[key] ?? key;
  if (params) {
    for (const [name, value] of Object.entries(params)) {
      text = text.split(`{${name}}`).join(String(value));
    }
  }
  return text;
}
