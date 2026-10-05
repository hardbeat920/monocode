//! Port of src/features/automations/model/automationTemplates.ts: the
//! template gallery the automations view offers for a new automation.

use super::model::{AutomationScheduleKind, AutomationTriggerKind, TemplateTrigger};

/// `AutomationTemplateCategoryId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateCategory {
    Popular,
    Review,
    Security,
    Incidents,
    Research,
    Environment,
}

impl TemplateCategory {
    pub const fn id(self) -> &'static str {
        match self {
            TemplateCategory::Popular => "popular",
            TemplateCategory::Review => "review",
            TemplateCategory::Security => "security",
            TemplateCategory::Incidents => "incidents",
            TemplateCategory::Research => "research",
            TemplateCategory::Environment => "environment",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            TemplateCategory::Popular => "Popular",
            TemplateCategory::Review => "Code Review",
            TemplateCategory::Security => "Security",
            TemplateCategory::Incidents => "Incidents & Triage",
            TemplateCategory::Research => "Data & Research",
            TemplateCategory::Environment => "Environment",
        }
    }
}

/// `AUTOMATION_TEMPLATE_CATEGORIES`, in gallery order.
pub const AUTOMATION_TEMPLATE_CATEGORIES: [TemplateCategory; 6] = [
    TemplateCategory::Popular,
    TemplateCategory::Review,
    TemplateCategory::Security,
    TemplateCategory::Incidents,
    TemplateCategory::Research,
    TemplateCategory::Environment,
];

/// `AutomationTemplateIcon`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemplateIcon {
    Search,
    Alert,
    File,
    Check,
    Lock,
    Pr,
    Inbox,
    Gauge,
    Terminal,
    Note,
}

/// `AutomationTemplate`. `category` is never `Popular`; `popular` marks the
/// templates that also show there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationTemplate {
    pub id: &'static str,
    pub category: TemplateCategory,
    pub popular: bool,
    pub icon: TemplateIcon,
    pub name: &'static str,
    pub description: &'static str,
    pub prompt: &'static str,
    pub trigger: TemplateTrigger,
    pub trigger_label: &'static str,
}

/// `templatesForCategory`.
pub fn templates_for_category(category: TemplateCategory) -> Vec<&'static AutomationTemplate> {
    if category == TemplateCategory::Popular {
        return AUTOMATION_TEMPLATES
            .iter()
            .filter(|template| template.popular)
            .collect();
    }
    AUTOMATION_TEMPLATES
        .iter()
        .filter(|template| template.category == category)
        .collect()
}

/// `AUTOMATION_TEMPLATES`.
pub const AUTOMATION_TEMPLATES: [AutomationTemplate; 14] = [
    AutomationTemplate {
        id: "find-critical-bugs",
        category: TemplateCategory::Review,
        popular: true,
        icon: TemplateIcon::Alert,
        name: "Find critical bugs",
        description: "Analyze recent commits for high-severity correctness bugs and submit safe fixes",
        trigger: TemplateTrigger {
            kind: AutomationTriggerKind::Time,
            event: "weekdays",
            schedule_kind: Some(AutomationScheduleKind::Weekdays),
            time: Some("09:00"),
            day_of_week: None,
            minute: None,
        },
        trigger_label: "Weekdays at 09:00",
        prompt: "Review recent git history in this repo for high-severity correctness bugs.

Focus on:
- Logic errors, race conditions, and data loss
- Broken error handling that can fail silently in production
- Regressions introduced in the last few days of commits

Only report issues you can validate from the current code. If a fix is clearly safe and local, implement it. Skip style nits and speculative issues.

At the end, summarize what you found, what you changed, and anything that still needs a human.",
    },
    AutomationTemplate {
        id: "scan-vulnerabilities",
        category: TemplateCategory::Security,
        popular: true,
        icon: TemplateIcon::Search,
        name: "Scan codebase for vulnerabilities",
        description: "Review the full repository on a schedule and alert on validated high-impact security issues",
        trigger: TemplateTrigger {
            kind: AutomationTriggerKind::Time,
            event: "weekly",
            schedule_kind: Some(AutomationScheduleKind::Weekly),
            time: Some("10:00"),
            day_of_week: Some(1),
            minute: None,
        },
        trigger_label: "Monday at 10:00",
        prompt: "Perform a security review of this repository.

Look for:
- Injection, XSS, SSRF, and auth/authz bypasses
- Secrets, tokens, or credentials committed to the repo
- Unsafe deserialization, path traversal, and command injection
- Dependency or config issues that meaningfully increase risk

Only report issues you can validate with concrete evidence. Do not invent CVEs. Rank findings by impact and include the file path, why it is exploitable, and a recommended fix. Implement safe, local remediations when the change is clearly correct.",
    },
    AutomationTemplate {
        id: "generate-docs",
        category: TemplateCategory::Research,
        popular: true,
        icon: TemplateIcon::File,
        name: "Generate docs",
        description: "Create and update developer documentation for recently changed or under-documented code",
        trigger: TemplateTrigger {
            kind: AutomationTriggerKind::Time,
            event: "weekly",
            schedule_kind: Some(AutomationScheduleKind::Weekly),
            time: Some("09:00"),
            day_of_week: Some(1),
            minute: None,
        },
        trigger_label: "Monday at 09:00",
        prompt: "Update developer documentation for this repo based on recent changes.

- Find APIs, modules, and workflows that are new, renamed, or under-documented
- Prefer editing existing docs over creating new files
- Keep the writing concise and accurate; do not invent behavior
- Include setup, how to run, and the main entry points if those are missing

Open a concise summary of what docs you changed and why.",
    },
    AutomationTemplate {
        id: "add-test-coverage",
        category: TemplateCategory::Review,
        popular: true,
        icon: TemplateIcon::Check,
        name: "Add test coverage",
        description: "Review recent changes and add tests for high-risk logic that lacks adequate coverage",
        trigger: TemplateTrigger {
            kind: AutomationTriggerKind::Time,
            event: "weekdays",
            schedule_kind: Some(AutomationScheduleKind::Weekdays),
            time: Some("11:00"),
            day_of_week: None,
            minute: None,
        },
        trigger_label: "Weekdays at 11:00",
        prompt: "Look at recent commits and add tests for high-risk logic that is missing coverage.

- Prefer the project's existing test runner and style
- Target correctness, edge cases, and regressions — not coverage for its own sake
- Do not rewrite production code unless a test reveals a clear bug
- Run the relevant tests and fix anything you break

Summarize which tests you added and which gaps remain.",
    },
    AutomationTemplate {
        id: "review-pull-requests",
        category: TemplateCategory::Review,
        popular: false,
        icon: TemplateIcon::Pr,
        name: "Review pull requests",
        description: "When a pull request is opened, review the diff for bugs, regressions, and missing tests",
        trigger: TemplateTrigger {
            kind: AutomationTriggerKind::Github,
            event: "pull_request_opened",
            schedule_kind: None,
            time: None,
            day_of_week: None,
            minute: None,
        },
        trigger_label: "Pull request opened",
        prompt: "Review the newly opened pull request.

Check for:
- Correctness bugs and regressions
- Missing tests for the changed behavior
- Security or data-loss risks
- API / contract breakage

Leave a structured review: blockers first, then suggestions. Do not nitpick formatting. If the change is good, say so briefly and note residual risk.",
    },
    AutomationTemplate {
        id: "review-draft-prs",
        category: TemplateCategory::Review,
        popular: false,
        icon: TemplateIcon::Pr,
        name: "Review draft PRs",
        description: "Give early feedback when a draft pull request is opened so issues are caught before review",
        trigger: TemplateTrigger {
            kind: AutomationTriggerKind::Github,
            event: "draft_opened",
            schedule_kind: None,
            time: None,
            day_of_week: None,
            minute: None,
        },
        trigger_label: "Draft opened",
        prompt: "A draft pull request was opened. Give early, high-signal feedback.

Focus on direction, missing tests, and likely bugs — not polish. Call out anything that will be expensive to change later. Keep the review short and specific to the diff.",
    },
    AutomationTemplate {
        id: "dependency-audit",
        category: TemplateCategory::Security,
        popular: false,
        icon: TemplateIcon::Lock,
        name: "Audit dependencies",
        description: "Check lockfiles and manifests for vulnerable, abandoned, or unexpectedly upgraded packages",
        trigger: TemplateTrigger {
            kind: AutomationTriggerKind::Time,
            event: "weekly",
            schedule_kind: Some(AutomationScheduleKind::Weekly),
            time: Some("09:30"),
            day_of_week: Some(1),
            minute: None,
        },
        trigger_label: "Monday at 09:30",
        prompt: "Audit this repo's dependencies.

- Inspect lockfiles and package manifests for vulnerable, unused, or unexpectedly upgraded packages
- Confirm findings against the project's current tooling (npm, cargo, etc.)
- Only propose upgrades or removals you can justify
- Do not bump majors unless the current version is unsafe and the upgrade is clearly required

Report what is risky, what you changed, and what still needs a human.",
    },
    AutomationTemplate {
        id: "secret-scan",
        category: TemplateCategory::Security,
        popular: false,
        icon: TemplateIcon::Lock,
        name: "Scan for secrets",
        description: "Search the working tree and recent history for committed credentials, tokens, and keys",
        trigger: TemplateTrigger {
            kind: AutomationTriggerKind::Time,
            event: "weekly",
            schedule_kind: Some(AutomationScheduleKind::Weekly),
            time: Some("09:30"),
            day_of_week: Some(1),
            minute: None,
        },
        trigger_label: "Monday at 09:30",
        prompt: "Scan the working tree and recent git history for secrets.

Look for API keys, tokens, private keys, .env files, and credentials in config. If you find a real secret, do not echo the full value. Report the file and a redacted snippet, explain why it is sensitive, and recommend rotation plus a git-history cleanup if it was committed.",
    },
    AutomationTemplate {
        id: "triage-github-issues",
        category: TemplateCategory::Incidents,
        popular: false,
        icon: TemplateIcon::Inbox,
        name: "Triage GitHub issues",
        description: "When a GitHub issue is opened, inspect the repo and add a concrete reproduction or next step",
        trigger: TemplateTrigger {
            kind: AutomationTriggerKind::Github,
            event: "issue_opened",
            schedule_kind: None,
            time: None,
            day_of_week: None,
            minute: None,
        },
        trigger_label: "Issue opened",
        prompt: "A new GitHub issue was opened. Triage it against this repo.

- Reproduce or locate the relevant code if the report is specific enough
- Label the severity in your summary (blocker / bug / request / unclear)
- Add a concrete next step: file paths, likely cause, or the missing information
- Do not implement a large fix unless the issue is clearly a small, validated bug",
    },
    AutomationTemplate {
        id: "triage-new-issues",
        category: TemplateCategory::Incidents,
        popular: false,
        icon: TemplateIcon::Inbox,
        name: "Triage new issues",
        description: "When a Linear issue is created, inspect the repo and add a concrete reproduction or next step",
        trigger: TemplateTrigger {
            kind: AutomationTriggerKind::Linear,
            event: "issue_created",
            schedule_kind: None,
            time: None,
            day_of_week: None,
            minute: None,
        },
        trigger_label: "Issue created",
        prompt: "A new Linear issue was created. Triage it against this repo.

- Reproduce or locate the relevant code if the report is specific enough
- Label the severity in your summary (blocker / bug / request / unclear)
- Add a concrete next step: file paths, likely cause, or the missing information
- Do not implement a large fix unless the issue is clearly a small, validated bug",
    },
    AutomationTemplate {
        id: "failing-ci-watch",
        category: TemplateCategory::Incidents,
        popular: false,
        icon: TemplateIcon::Alert,
        name: "Watch failing checks",
        description: "On a weekday morning, run the project's tests and diagnose anything that is already red",
        trigger: TemplateTrigger {
            kind: AutomationTriggerKind::Time,
            event: "weekdays",
            schedule_kind: Some(AutomationScheduleKind::Weekdays),
            time: Some("08:30"),
            day_of_week: None,
            minute: None,
        },
        trigger_label: "Weekdays at 08:30",
        prompt: "Run the project's existing test / lint / typecheck commands.

If something fails:
- Identify the first real failure, not the cascade
- Fix it if the cause is local and obvious
- Otherwise write a short diagnosis with the command, the error, and the suspected file

Do not add new test infrastructure. Do not \"fix\" flakes by weakening assertions.",
    },
    AutomationTemplate {
        id: "weekly-changelog",
        category: TemplateCategory::Research,
        popular: false,
        icon: TemplateIcon::Note,
        name: "Weekly changelog",
        description: "Summarize the week's commits into a changelog humans can actually read",
        trigger: TemplateTrigger {
            kind: AutomationTriggerKind::Time,
            event: "weekly",
            schedule_kind: Some(AutomationScheduleKind::Weekly),
            time: Some("16:00"),
            day_of_week: Some(5),
            minute: None,
        },
        trigger_label: "Friday at 16:00",
        prompt: "Write a concise changelog for this repo covering the last 7 days of commits.

Group by user-facing changes, fixes, and internal work. Skip noise (formatting, lockfile-only, merge commits). Use the project's existing changelog or docs style if one exists; otherwise write a short markdown summary. Do not invent features that are not in the commits.",
    },
    AutomationTemplate {
        id: "repo-health",
        category: TemplateCategory::Environment,
        popular: false,
        icon: TemplateIcon::Gauge,
        name: "Repo health check",
        description: "Inspect the working tree, stale branches, and obvious project-setup drift on a schedule",
        trigger: TemplateTrigger {
            kind: AutomationTriggerKind::Time,
            event: "weekly",
            schedule_kind: Some(AutomationScheduleKind::Weekly),
            time: Some("09:00"),
            day_of_week: Some(1),
            minute: None,
        },
        trigger_label: "Monday at 09:00",
        prompt: "Do a repo health check.

- Working tree cleanliness and leftover build artifacts that should be gitignored
- README / setup instructions that no longer match the project
- Obvious CI, lint, or typecheck config drift
- Stale or broken scripts in package.json / Makefile / justfile

Fix the small, clearly correct issues. Report the rest with file paths. Do not do a broad refactor.",
    },
    AutomationTemplate {
        id: "install-doctor",
        category: TemplateCategory::Environment,
        popular: false,
        icon: TemplateIcon::Terminal,
        name: "Environment doctor",
        description: "Verify the project still installs and boots from a clean working copy",
        trigger: TemplateTrigger {
            kind: AutomationTriggerKind::Time,
            event: "weekly",
            schedule_kind: Some(AutomationScheduleKind::Weekly),
            time: Some("10:00"),
            day_of_week: Some(1),
            minute: None,
        },
        trigger_label: "Monday at 10:00",
        prompt: "Verify this project still sets up cleanly.

Follow the README / documented install steps as closely as possible. Note any missing prerequisites, broken scripts, or docs that don't match reality. Fix small doc or script issues. Do not change application architecture.

End with a pass/fail and the exact commands you ran.",
    },
];
