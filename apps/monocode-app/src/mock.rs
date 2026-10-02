//! Mock data for the shell, shaped after what the React shell reads. These are
//! plain structs on purpose: the engine will replace them with real sessions,
//! projects, tabs, and usage.

use monocode_ui::ProviderLogo;

#[derive(Clone, Debug)]
pub struct Project {
    pub name: &'static str,
    pub path: &'static str,
    pub additions: i64,
    pub deletions: i64,
    pub busy: bool,
    /// The mascot tint (`resolveTabGroupColor`), as `0xrrggbb`.
    pub color: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionStatus {
    Idle,
    Busy,
    Done,
    NeedsApproval,
    Draft,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub provider: ProviderLogo,
    pub model: &'static str,
    pub title: &'static str,
    pub repo: &'static str,
    pub branch: &'static str,
    pub additions: i64,
    pub deletions: i64,
    /// Minutes since the last update, for `formatRelative`.
    pub updated_minutes_ago: i64,
    pub status: SessionStatus,
    pub pinned: bool,
}

#[derive(Clone, Debug)]
pub enum TabKind {
    /// A session tab shows its harness icons.
    Session(Vec<ProviderLogo>),
    /// A file tab shows its file-type icon.
    File(&'static str),
    Terminal,
}

#[derive(Clone, Debug)]
pub struct WorkspaceTab {
    pub kind: TabKind,
    pub headline: &'static str,
    pub meta: Option<&'static str>,
    pub dirty: bool,
}

#[derive(Clone, Debug)]
pub struct UsageWindow {
    pub used_percent: f32,
    pub label: &'static str,
}

#[derive(Clone, Debug)]
pub struct UsageChip {
    pub provider: ProviderLogo,
    pub windows: Vec<UsageWindow>,
}

#[derive(Clone, Debug)]
pub struct ShellData {
    pub projects: Vec<Project>,
    pub active_project: usize,
    pub sessions: Vec<Session>,
    pub tabs: Vec<WorkspaceTab>,
    pub usage: Vec<UsageChip>,
    pub inbox_unseen: bool,
    pub cwd: &'static str,
    pub branch: &'static str,
}

impl ShellData {
    /// The same project and sessions as docs/screenshot.jpg.
    pub fn sample() -> Self {
        Self {
            projects: vec![
                Project {
                    name: "monocode",
                    path: "~/code/monocode",
                    additions: 949,
                    deletions: 10,
                    busy: false,
                    color: 0x7dd3fc,
                },
                Project {
                    name: "agent-terminal",
                    path: "~/code/agent-terminal",
                    additions: 0,
                    deletions: 0,
                    busy: true,
                    color: 0xf9a8c9,
                },
                Project {
                    name: "comet",
                    path: "~/src/comet",
                    additions: 12,
                    deletions: 3,
                    busy: false,
                    color: 0xe8c547,
                },
                Project {
                    name: "zeron",
                    path: "~/src/zeron",
                    additions: 0,
                    deletions: 0,
                    busy: false,
                    color: 0x86efac,
                },
            ],
            active_project: 0,
            sessions: vec![
                Session {
                    provider: ProviderLogo::Cursor,
                    model: "Composer 2.5",
                    title: "Replace public README screenshot",
                    repo: "monocode",
                    branch: "main",
                    additions: 0,
                    deletions: 0,
                    updated_minutes_ago: 3,
                    status: SessionStatus::Idle,
                    pinned: false,
                },
                Session {
                    provider: ProviderLogo::Claude,
                    model: "Claude Opus 5",
                    title: "Benchmark arcade games",
                    repo: "monocode",
                    branch: "main",
                    additions: 478,
                    deletions: 2,
                    updated_minutes_ago: 22,
                    status: SessionStatus::Idle,
                    pinned: false,
                },
                Session {
                    provider: ProviderLogo::Cursor,
                    model: "Cursor Grok 4.6",
                    title: "Empty chat dots spell MONOCODE",
                    repo: "monocode",
                    branch: "main",
                    additions: 471,
                    deletions: 8,
                    updated_minutes_ago: 7 * 60 + 59,
                    status: SessionStatus::Idle,
                    pinned: false,
                },
                Session {
                    provider: ProviderLogo::Cursor,
                    model: "Cursor Grok 4.6",
                    title: "Repeated folder permission prompts",
                    repo: "monocode",
                    branch: "main",
                    additions: 0,
                    deletions: 0,
                    updated_minutes_ago: 10 * 60 + 58,
                    status: SessionStatus::Idle,
                    pinned: false,
                },
                Session {
                    provider: ProviderLogo::Codex,
                    model: "GPT-5.5 Codex",
                    title: "Port the theme tokens to GPUI",
                    repo: "monocode",
                    branch: "gpui-native",
                    additions: 1204,
                    deletions: 37,
                    updated_minutes_ago: 0,
                    status: SessionStatus::Busy,
                    pinned: false,
                },
                Session {
                    provider: ProviderLogo::Opencode,
                    model: "Kimi K3",
                    title: "Sort imports in the harness crate",
                    repo: "monocode",
                    branch: "tidy",
                    additions: 18,
                    deletions: 18,
                    updated_minutes_ago: 40,
                    status: SessionStatus::Done,
                    pinned: true,
                },
                Session {
                    provider: ProviderLogo::Claude,
                    model: "Claude Sonnet 5",
                    title: "Allow writes outside the worktree?",
                    repo: "monocode",
                    branch: "connect",
                    additions: 0,
                    deletions: 0,
                    updated_minutes_ago: 2 * 24 * 60,
                    status: SessionStatus::NeedsApproval,
                    pinned: false,
                },
            ],
            tabs: vec![
                WorkspaceTab {
                    kind: TabKind::Session(vec![ProviderLogo::Cursor]),
                    headline: "gridArcade.ts",
                    meta: Some("Empty chat dots spell MONOCODE"),
                    dirty: false,
                },
                WorkspaceTab {
                    kind: TabKind::Session(vec![ProviderLogo::Claude]),
                    headline: "Benchmark arcade games",
                    meta: Some("README.md"),
                    dirty: false,
                },
                WorkspaceTab {
                    kind: TabKind::File("screenshot.jpg"),
                    headline: "screenshot.jpg",
                    meta: Some("Replace public README screenshot"),
                    dirty: false,
                },
                WorkspaceTab {
                    kind: TabKind::Terminal,
                    headline: "agent-terminal",
                    meta: None,
                    dirty: false,
                },
            ],
            usage: vec![
                UsageChip {
                    provider: ProviderLogo::Claude,
                    windows: vec![
                        UsageWindow {
                            used_percent: 23.0,
                            label: "3h 12m",
                        },
                        UsageWindow {
                            used_percent: 41.0,
                            label: "4d",
                        },
                    ],
                },
                UsageChip {
                    provider: ProviderLogo::Codex,
                    windows: vec![
                        UsageWindow {
                            used_percent: 84.0,
                            label: "1h 40m",
                        },
                        UsageWindow {
                            used_percent: 12.0,
                            label: "6d",
                        },
                    ],
                },
            ],
            inbox_unseen: true,
            cwd: "~/code/monocode",
            branch: "main",
        }
    }
}

/// `formatRelative` from Sidebar.tsx, for minute offsets.
pub fn format_relative(minutes: i64) -> String {
    if minutes < 1 {
        return "now".into();
    }
    if minutes < 60 {
        return format!("{minutes}m");
    }
    let hours = minutes / 60;
    if hours < 24 {
        let rest = minutes % 60;
        return if rest > 0 {
            format!("{hours}h {rest}m")
        } else {
            format!("{hours}h")
        };
    }
    let days = hours / 24;
    if days < 7 {
        return format!("{days}d");
    }
    // TODO(port): the React version prints a short month and day here.
    format!("{days}d")
}

/// `formatUsagePercent`: whole percent.
pub fn format_percent(value: f32) -> String {
    format!("{}%", value.round() as i64)
}

#[cfg(test)]
mod tests {
    use super::format_relative;

    #[test]
    fn relative_times_match_the_sidebar() {
        assert_eq!(format_relative(0), "now");
        assert_eq!(format_relative(3), "3m");
        assert_eq!(format_relative(60), "1h");
        assert_eq!(format_relative(7 * 60 + 59), "7h 59m");
        assert_eq!(format_relative(3 * 24 * 60), "3d");
    }
}
