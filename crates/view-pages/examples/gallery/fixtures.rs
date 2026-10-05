//! Sample data and scenes for the pages gallery.

use std::rc::Rc;

use gpui::{AppContext as _, IntoElement, ParentElement as _, Render, Styled as _, div, px};
use monocode_core::notes::NoteCardMeta;
use monocode_ui::Theme;
use monocode_view_pages::date_time_picker::DateTimePicker;
use monocode_view_pages::format::now_ms;
use monocode_view_pages::notes::{LocalNotes, Note, NotesData, NotesView, note_mini_card};
use monocode_view_pages::widgets::{MarkdownMode, MarkdownModes};

use super::{Scene, padded, projects, scene, stage};

const MINUTE: i64 = 60_000;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

fn note(id: &str, title: &str, body: &str, tags: &[&str], cwd: Option<&str>, age: i64) -> Note {
    let at = now_ms() - age;
    Note {
        id: id.into(),
        slug: title.to_lowercase().replace(' ', "-"),
        title: title.into(),
        body: body.into(),
        tags: tags.iter().map(|tag| tag.to_string()).collect(),
        source_session_id: None,
        source_cwd: cwd.map(str::to_string),
        created_at: at,
        updated_at: at,
    }
}

pub fn sample_notes() -> Vec<Note> {
    vec![
        note(
            "release-plan",
            "Release plan for 0.7",
            "# Release plan for 0.7\n\nShip the native app behind a flag, then flip the default once the remote host passes the soak test.\n\n## Checklist\n\n- Port the **automations** page\n- Verify `monocode.db` migrations from v18\n- Sign and notarize the dmg\n\n```sh\ncargo build --release -p monocode-app\n```\n\n> Keep the Tauri build green until cutover.\n",
            &["release", "planning", "native", "macos"],
            Some("/Users/me/code/monocode"),
            12 * MINUTE,
        ),
        note(
            "flaky-tests",
            "Flaky transcript tests",
            "The reducer tests fail about once in fifty runs when two tool calls finish in the same batch. Suspect ordering in `apply.rs`.",
            &["bug"],
            Some("/Users/me/code/edefyn"),
            3 * HOUR,
        ),
        note(
            "api-ideas",
            "API ideas",
            "- Stream usage events over SSE\n- Let the operator pin a model per project\n- Expose reminders to agents",
            &[],
            None,
            2 * DAY,
        ),
        note(
            "design-review",
            "Design review notes",
            "Popovers need the 170ms open curve. Cards should keep the 13px titles and the 11px meta line.",
            &["design", "ui"],
            Some("/Users/me/code/website"),
            9 * DAY,
        ),
    ]
}

fn notes_view(
    window: &mut gpui::Window,
    cx: &mut gpui::App,
    notes: Vec<Note>,
    mode: MarkdownMode,
) -> (gpui::Entity<NotesView>, LocalNotes) {
    let data = LocalNotes::new(notes, cx);
    let shared: Rc<dyn NotesData> = Rc::new(data.clone());
    MarkdownModes::set("release-plan", mode, cx);
    let projects = projects();
    let view = cx.new(|cx| {
        NotesView::new(
            shared,
            projects,
            Some("/Users/me/code/monocode"),
            window,
            cx,
        )
    });
    (view, data)
}

pub fn notes_scenes() -> Vec<Scene> {
    vec![
        scene("notes-preview", 1280., 820., |window, cx| {
            let (view, _) = notes_view(window, cx, sample_notes(), MarkdownMode::Preview);
            stage(view)
        }),
        scene("notes-source", 1280., 820., |window, cx| {
            let (view, _) = notes_view(window, cx, sample_notes(), MarkdownMode::Source);
            stage(view)
        }),
        scene("notes-light", 1280., 820., |window, cx| {
            let (view, _) = notes_view(window, cx, sample_notes(), MarkdownMode::Preview);
            stage(view)
        })
        .light(),
        scene("notes-empty", 1000., 600., |window, cx| {
            let (view, _) = notes_view(window, cx, Vec::new(), MarkdownMode::Preview);
            stage(view)
        }),
        scene("notes-picker", 1280., 820., |window, cx| {
            let (view, _) = notes_view(window, cx, sample_notes(), MarkdownMode::Preview);
            let picker = view.read(cx).picker().clone();
            picker.update(cx, |picker, cx| picker.open_picker(window, cx));
            stage(view)
        }),
        scene("notes-save-error", 1280., 820., |window, cx| {
            let (view, data) = notes_view(window, cx, sample_notes(), MarkdownMode::Preview);
            data.fail_next_save("Disk full", cx);
            data.choose_project("/Users/me/code/portognjeeen", cx);
            data.set_image_busy(true, cx);
            stage(view)
        }),
        scene("note-mini-card", 420., 260., |window, cx| {
            let projects = projects();
            let view = cx.new(|_| MiniCards { projects });
            let _ = window;
            padded(view)
        }),
    ]
}

struct MiniCards {
    projects: Rc<monocode_view_pages::data::StaticProjects>,
}

impl Render for MiniCards {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        use monocode_view_pages::data::ProjectsData as _;
        let cwd = "/Users/me/code/monocode";
        let card = NoteCardMeta {
            id: "release-plan".into(),
            slug: "release-plan".into(),
            title: "Release plan for 0.7".into(),
            source_cwd: Some(cwd.into()),
            extra: Default::default(),
        };
        let mark = self.projects.mark(cwd, cx);
        let theme = Theme::of(cx);
        div()
            .w(px(360.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .text_color(theme.colors.content)
            .child(note_mini_card(card.clone(), Some(mark.clone())).on_dismiss(|_, _| {}))
            .child(note_mini_card(card, Some(mark)).embedded(true))
    }
}

struct DateStage {
    picker: gpui::Entity<DateTimePicker>,
}

impl Render for DateStage {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .w(px(268.))
            .p(px(12.))
            .rounded(px(12.))
            .border_1()
            .border_color(theme.colors.popover_border)
            .child(self.picker.clone())
    }
}

pub fn date_scenes() -> Vec<Scene> {
    vec![scene("date-time-picker", 340., 420., |window, cx| {
        let today = chrono::NaiveDate::from_ymd_opt(2028, 2, 20).unwrap();
        let picker = cx.new(|cx| {
            DateTimePicker::new("2028-02-23T18:30", window, cx)
                .min_date(Some("2028-02-08"))
                .with_today(today)
        });
        let view = cx.new(|_| DateStage { picker });
        padded(view)
    })]
}

fn sample_hits() -> Vec<monocode_view_pages::search::AppSearchHit> {
    use monocode_core::HarnessId;
    use monocode_view_pages::search::*;
    let now = now_ms();
    vec![
        AppSearchHit::Conversation(ConversationHit {
            id: "conversation:s1".into(),
            session_id: "s1".into(),
            cwd: "/Users/me/code/monocode".into(),
            harness: HarnessId::Claude,
            title: "Plan the release checklist".into(),
            updated_at: now - HOUR,
            score: 120,
            positions: vec![0, 1, 2, 3],
        }),
        AppSearchHit::Conversation(ConversationHit {
            id: "conversation:s2".into(),
            session_id: "s2".into(),
            cwd: "/Users/me/code/edefyn".into(),
            harness: HarnessId::Codex,
            title: "Explain the planner module".into(),
            updated_at: now - DAY,
            score: 90,
            positions: vec![12, 13, 14, 15],
        }),
        AppSearchHit::Message(MessageHit {
            id: "message:s1:b4".into(),
            session_id: "s1".into(),
            cwd: "/Users/me/code/monocode".into(),
            harness: HarnessId::Claude,
            title: "Plan the release checklist".into(),
            updated_at: now - HOUR,
            block_id: "b4".into(),
            role: "assistant".into(),
            preview: "…the plan is to ship the native app behind a flag first…".into(),
            score: 40,
        }),
        AppSearchHit::File(FileHit {
            id: "file:/Users/me/code/monocode/docs/release-plan.md".into(),
            path: "/Users/me/code/monocode/docs/release-plan.md".into(),
            relative: "docs/release-plan.md".into(),
            name: "release-plan.md".into(),
            score: 80,
            positions: vec![13, 14, 15, 16],
        }),
        AppSearchHit::File(FileHit {
            id: "file:/Users/me/code/monocode/crates/engine/src/planner.rs".into(),
            path: "/Users/me/code/monocode/crates/engine/src/planner.rs".into(),
            relative: "crates/engine/src/planner.rs".into(),
            name: "planner.rs".into(),
            score: 70,
            positions: vec![17, 18, 19, 20],
        }),
        AppSearchHit::Content(ContentHit {
            id: "content:/Users/me/code/monocode/src/app/App.tsx:120:9".into(),
            path: "/Users/me/code/monocode/src/app/App.tsx".into(),
            relative: "src/app/App.tsx".into(),
            name: "App.tsx".into(),
            line: 120,
            column: 9,
            preview: "const plan = await buildPlan(session, options);".into(),
        }),
        AppSearchHit::Project(ProjectHit {
            id: "project:/Users/me/code/planet".into(),
            path: "/Users/me/code/planet".into(),
            name: "planet".into(),
            score: 60,
            positions: vec![0, 1, 2, 3],
        }),
    ]
}

fn search_view(
    window: &mut gpui::Window,
    cx: &mut gpui::App,
    query: &str,
) -> gpui::Entity<monocode_view_pages::search::SearchView> {
    use monocode_view_pages::search::{LocalSearch, SearchView};
    let data = LocalSearch::new(sample_hits(), cx);
    let shared: Rc<dyn monocode_view_pages::search::SearchData> = Rc::new(data);
    let projects = projects();
    let view = cx.new(|cx| SearchView::new(shared, projects, window, cx));
    let query = query.to_string();
    view.update(cx, |view, cx| {
        view.open("/Users/me/code/monocode", Vec::new(), window, cx);
        view.set_query(&query, window, cx);
    });
    view
}

pub fn search_scenes() -> Vec<Scene> {
    vec![
        scene("search-empty", 1100., 700., |window, cx| {
            stage(search_view(window, cx, ""))
        }),
        scene("search-results", 1100., 700., |window, cx| {
            stage(search_view(window, cx, "plan"))
        }),
        scene("search-results-light", 1100., 700., |window, cx| {
            stage(search_view(window, cx, "plan"))
        })
        .light(),
    ]
}

/// A page body with the settings page's padding and width.
struct SettingsStage {
    child: gpui::AnyView,
}

impl Render for SettingsStage {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .size_full()
            .flex()
            .justify_center()
            .bg(theme.colors.background_base)
            .child(
                div()
                    .w_full()
                    .max_w(px(820.))
                    .px(px(32.))
                    .py(px(32.))
                    .child(self.child.clone()),
            )
    }
}

fn mcp_rows() -> monocode_view_pages::mcp::McpSettingsSnapshot {
    use monocode_view_pages::mcp::*;
    let row =
        |provider, name: &str, scope, path: &str, transport: &str, status: &str| McpServerRow {
            connection: McpConnection {
                provider,
                name: name.into(),
                scope,
                config_path: path.into(),
                transport: transport.into(),
                enabled: None,
            },
            status: status.into(),
        };
    McpSettingsSnapshot {
        servers: vec![
            row(
                McpProvider::Claude,
                "sentry",
                McpScope::User,
                "/Users/me/.claude.json",
                "http",
                "! Needs authentication",
            ),
            row(
                McpProvider::Claude,
                "playwright",
                McpScope::Local,
                "",
                "stdio",
                "✔ Connected",
            ),
            row(
                McpProvider::Codex,
                "docs",
                McpScope::User,
                "/Users/me/.codex/config.toml",
                "http",
                "configured",
            ),
            row(
                McpProvider::Cursor,
                "linear",
                McpScope::Project,
                "/Users/me/code/monocode/.cursor/mcp.json",
                "sse",
                "configured",
            ),
        ],
        error: String::new(),
        claude_error: String::new(),
    }
}

fn mcp_view(
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> gpui::Entity<monocode_view_pages::mcp::McpSettingsView> {
    use monocode_view_pages::mcp::{LocalMcp, McpData, McpSettingsView};
    let data = LocalMcp::new(cx);
    data.set_discovered("/Users/me/code/monocode", mcp_rows(), cx);
    let shared: Rc<dyn McpData> = Rc::new(data);
    let projects = projects();
    cx.new(|cx| McpSettingsView::new(shared, projects, "/Users/me/code/monocode", window, cx))
}

fn skills() -> Vec<monocode_view_pages::skills::DiscoveredSkill> {
    use monocode_view_pages::skills::DiscoveredSkill;
    let skill =
        |name: &str, description: &str, path: &str, scope: &str, source: &str| DiscoveredSkill {
            name: name.into(),
            description: description.into(),
            path: path.into(),
            scope: scope.into(),
            source: source.into(),
        };
    vec![
        skill(
            "release-notes",
            "Draft release notes from merged pull requests",
            "/Users/me/code/monocode/.agents/skills/release-notes/SKILL.md",
            "project",
            "agents",
        ),
        skill(
            "port-check",
            "Compare a Rust port against its TypeScript source",
            "/Users/me/code/monocode/.agents/skills/port-check/SKILL.md",
            "project",
            "agents",
        ),
        skill(
            "writing-rules",
            "Edit text to remove AI patterns",
            "/Users/me/.agents/skills/writing-rules/SKILL.md",
            "user",
            "agents",
        ),
        skill(
            "frontend-design",
            "Build polished interfaces",
            "/Users/me/.claude/skills/frontend-design/SKILL.md",
            "user",
            "claude",
        ),
    ]
}

const SKILL_TEXT: &str = "---\nname: release-notes\ndescription: Draft release notes from merged pull requests\n---\n\n# Release notes\n\nCollect the pull requests merged since the last tag and group them by area.\n\n## Steps\n\n1. Run `git log --merges` since the last tag.\n2. Group changes under **Features**, **Fixes**, and **Internal**.\n3. Keep each line under 80 characters.\n\n| Area | Owner |\n| --- | --- |\n| App | Lead |\n| Engine | Agents |\n";

fn skills_page(
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> (
    gpui::Entity<monocode_view_pages::skills::SkillsPage>,
    monocode_view_pages::skills::LocalSkills,
) {
    use monocode_view_pages::skills::{LocalSkills, SkillsData, SkillsPage};
    let data = LocalSkills::new(skills(), cx);
    data.set_file(
        "/Users/me/code/monocode/.agents/skills/release-notes/SKILL.md",
        SKILL_TEXT,
        cx,
    );
    data.save_disabled_paths(
        vec!["/Users/me/.claude/skills/frontend-design/SKILL.md".into()],
        cx,
    )
    .ok();
    let shared: Rc<dyn SkillsData> = Rc::new(data.clone());
    let page = cx.new(|cx| SkillsPage::new(shared, "/Users/me/code/monocode", window, cx));
    (page, data)
}

pub fn settings_scenes() -> Vec<Scene> {
    vec![
        scene("mcp-settings", 900., 760., |window, cx| {
            let view = mcp_view(window, cx);
            stage(cx.new(|_| SettingsStage { child: view.into() }))
        }),
        scene("mcp-settings-light", 900., 760., |window, cx| {
            let view = mcp_view(window, cx);
            stage(cx.new(|_| SettingsStage { child: view.into() }))
        })
        .light(),
        scene("mcp-add", 900., 760., |window, cx| {
            let view = mcp_view(window, cx);
            view.update(cx, |view, cx| view.open_add(window, cx));
            stage(cx.new(|_| SettingsStage { child: view.into() }))
        }),
        scene("mcp-add-provider", 900., 760., |window, cx| {
            let view = mcp_view(window, cx);
            view.update(cx, |view, cx| view.open_add(window, cx));
            let form = view.read(cx).add_form().cloned().unwrap();
            let picker = form.read(cx).provider_picker().clone();
            picker.update(cx, |picker, cx| picker.toggle(window, cx));
            stage(cx.new(|_| SettingsStage { child: view.into() }))
        }),
        scene("skills", 1200., 760., |window, cx| {
            let (page, _) = skills_page(window, cx);
            stage(page)
        }),
        scene("skills-preview", 1400., 760., |window, cx| {
            let (page, _) = skills_page(window, cx);
            let skill = skills()[0].clone();
            page.update(cx, |page, cx| page.open_preview(&skill, window, cx));
            stage(page)
        }),
        scene("skills-preview-narrow", 760., 900., |window, cx| {
            let (page, _) = skills_page(window, cx);
            let skill = skills()[0].clone();
            page.update(cx, |page, cx| page.open_preview(&skill, window, cx));
            stage(page)
        }),
        scene("skills-add", 1200., 760., |window, cx| {
            let (page, _) = skills_page(window, cx);
            page.update(cx, |page, cx| page.toggle_form(window, cx));
            stage(page)
        }),
        scene("skills-source", 1400., 760., |window, cx| {
            let (page, _) = skills_page(window, cx);
            let skill = skills()[0].clone();
            page.update(cx, |page, cx| {
                page.open_preview(&skill, window, cx);
                page.set_preview_mode(monocode_view_pages::widgets::MarkdownMode::Source, cx);
            });
            stage(page)
        }),
    ]
}

/// One row of the template table: id, category, popular, icon, name,
/// description, trigger kind, trigger event, and trigger label.
type TemplateRow<'a> = (
    &'a str,
    monocode_view_pages::automations::TemplateCategory,
    bool,
    monocode_view_pages::automations::TemplateIcon,
    &'a str,
    &'a str,
    monocode_view_pages::automations::model::AutomationTriggerKind,
    &'a str,
    &'a str,
);

fn template(row: TemplateRow) -> monocode_view_pages::automations::AutomationTemplate {
    use monocode_view_pages::automations::model::{AutomationScheduleKind, TemplateTrigger};
    let (id, category, popular, icon, name, description, kind, event, label) = row;
    monocode_view_pages::automations::AutomationTemplate {
        id: id.into(),
        category,
        popular,
        icon,
        name: name.into(),
        description: description.into(),
        prompt: format!("{description}."),
        trigger: TemplateTrigger {
            kind,
            event: event.into(),
            schedule_kind: AutomationScheduleKind::parse(event),
            time: Some("09:00".into()),
            day_of_week: Some(1),
            minute: None,
        },
        trigger_label: label.into(),
    }
}

/// A few templates from automationTemplates.ts.
pub fn sample_templates() -> Vec<monocode_view_pages::automations::AutomationTemplate> {
    use monocode_view_pages::automations::model::AutomationTriggerKind::{Github, Linear, Time};
    use monocode_view_pages::automations::{TemplateCategory as C, TemplateIcon as I};
    vec![
        template((
            "find-critical-bugs",
            C::Review,
            true,
            I::Alert,
            "Find critical bugs",
            "Analyze recent commits for high-severity correctness bugs and submit safe fixes",
            Time,
            "weekdays",
            "Weekdays at 09:00",
        )),
        template((
            "scan-vulnerabilities",
            C::Security,
            true,
            I::Search,
            "Scan codebase for vulnerabilities",
            "Review the full repository on a schedule and alert on validated high-impact security issues",
            Time,
            "weekly",
            "Monday at 10:00",
        )),
        template((
            "generate-docs",
            C::Research,
            true,
            I::File,
            "Generate docs",
            "Create and update developer documentation for recently changed or under-documented code",
            Time,
            "weekly",
            "Monday at 09:00",
        )),
        template((
            "add-test-coverage",
            C::Review,
            true,
            I::Check,
            "Add test coverage",
            "Review recent changes and add tests for high-risk logic that lacks adequate coverage",
            Time,
            "weekdays",
            "Weekdays at 11:00",
        )),
        template((
            "review-pull-requests",
            C::Review,
            false,
            I::Pr,
            "Review pull requests",
            "When a pull request is opened, review the diff for bugs, regressions, and missing tests",
            Github,
            "pull_request_opened",
            "Pull request opened",
        )),
        template((
            "triage-new-issues",
            C::Incidents,
            false,
            I::Inbox,
            "Triage new issues",
            "When a Linear issue is created, inspect the repo and add a concrete reproduction or next step",
            Linear,
            "issue_created",
            "Issue created",
        )),
    ]
}

fn sample_automations() -> Vec<monocode_view_pages::automations::Automation> {
    let now = now_ms();
    let automation = |id: &str,
                      name: &str,
                      cwd: &str,
                      harness: &str,
                      model: &str,
                      triggers: serde_json::Value,
                      enabled: bool,
                      last_run: Option<i64>| {
        let mut value = serde_json::json!({
            "id": id,
            "name": name,
            "prompt": "Review recent git history for high-severity correctness bugs. Only report issues you can validate from the current code.",
            "harness": harness,
            "model": model,
            "cwd": cwd,
            "workspaceMode": "worktree",
            "reuseSession": false,
            "runtimeMode": "auto",
            "triggerKind": "time",
            "triggerEvent": "weekdays",
            "scheduleKind": "weekdays",
            "minute": 0,
            "time": "09:00",
            "dayOfWeek": 1,
            "triggers": triggers,
            "missedRunGraceMinutes": 720,
            "enabled": enabled,
            "nextRunAt": now + DAY,
            "createdAt": now - 9 * DAY,
            "updatedAt": now - DAY,
        });
        if let Some(at) = last_run {
            value["lastRunAt"] = at.into();
        }
        serde_json::from_value(value).unwrap()
    };
    let time = |id: &str, event: &str, time: &str, day: i64| {
        serde_json::json!({
            "id": id, "kind": "time", "event": event, "scheduleKind": event, "minute": 15,
            "time": time, "dayOfWeek": day, "repos": [], "repo": "", "branch": "", "actor": "anyone",
        })
    };
    let github = |id: &str, event: &str| {
        serde_json::json!({
            "id": id, "kind": "github", "event": event, "scheduleKind": "weekdays", "minute": 0,
            "time": "09:00", "dayOfWeek": 1, "repos": [], "repo": "", "branch": "", "actor": "anyone",
        })
    };
    vec![
        automation(
            "a1",
            "Find critical bugs",
            "/Users/me/code/monocode",
            "claude",
            "claude:opus",
            serde_json::json!([
                time("t1", "weekdays", "09:00", 1),
                github("t2", "pull_request_opened")
            ]),
            true,
            Some(now - 3 * HOUR),
        ),
        automation(
            "a2",
            "Weekly changelog",
            "/Users/me/code/website",
            "codex",
            "codex:gpt-5.5",
            serde_json::json!([time("t3", "weekly", "16:00", 5)]),
            true,
            Some(now - 6 * DAY),
        ),
        automation(
            "a3",
            "Triage new issues",
            "/Users/me/code/edefyn",
            "claude",
            "claude:sonnet",
            serde_json::json!([github("t4", "issue_opened")]),
            false,
            None,
        ),
        automation(
            "a4",
            "Hourly flaky test sweep",
            "/Users/me/code/monocode",
            "cursor",
            "cursor:auto",
            serde_json::json!([time("t5", "hourly", "09:00", 1)]),
            true,
            Some(now - 40 * MINUTE),
        ),
    ]
}

fn sample_runs() -> Vec<monocode_view_pages::automations::AutomationRun> {
    let now = now_ms();
    let run = |id: &str,
               trigger: &str,
               status: &str,
               created: i64,
               minutes: Option<i64>,
               session: Option<&str>,
               event: Option<&str>| {
        let mut value = serde_json::json!({
            "id": id,
            "automationId": "a1",
            "trigger": trigger,
            "scheduledFor": created,
            "createdAt": created,
            "startedAt": created,
            "status": status,
        });
        if let Some(minutes) = minutes {
            value["completedAt"] = (created + minutes * MINUTE).into();
        }
        if let Some(session) = session {
            value["sessionId"] = session.into();
        }
        if let Some(event) = event {
            value["eventKind"] = "github".into();
            value["event"] = event.into();
        }
        serde_json::from_value(value).unwrap()
    };
    vec![
        run(
            "r1",
            "manual",
            "running",
            now - 4 * MINUTE,
            None,
            Some("s1"),
            None,
        ),
        run(
            "r2",
            "scheduled",
            "succeeded",
            now - 3 * HOUR,
            Some(12),
            Some("s2"),
            None,
        ),
        run(
            "r3",
            "event",
            "failed",
            now - DAY,
            Some(2),
            Some("s3"),
            Some("pull_request_opened"),
        ),
        run(
            "r4",
            "scheduled",
            "skipped",
            now - 2 * DAY,
            None,
            None,
            None,
        ),
        run(
            "r5",
            "scheduled",
            "succeeded",
            now - 3 * DAY,
            Some(74),
            Some("s5"),
            None,
        ),
        run(
            "r6",
            "scheduled",
            "cancelled",
            now - 4 * DAY,
            Some(0),
            Some("s6"),
            None,
        ),
    ]
}

fn automations_view(
    window: &mut gpui::Window,
    cx: &mut gpui::App,
    automations: Vec<monocode_view_pages::automations::Automation>,
) -> (
    gpui::Entity<monocode_view_pages::automations::AutomationsView>,
    monocode_view_pages::automations::LocalAutomations,
) {
    use monocode_view_pages::automations::{
        AutomationsData, AutomationsView, LocalAutomations, SessionFolderOption,
    };
    let data = LocalAutomations::new(
        automations,
        sample_templates(),
        vec!["/Users/me/code/monocode".into()],
        cx,
    );
    data.set_runs(sample_runs(), cx);
    data.set_folders(
        vec![SessionFolderOption {
            id: "f1".into(),
            name: "Nightly".into(),
        }],
        cx,
    );
    let shared: Rc<dyn AutomationsData> = Rc::new(data.clone());
    let projects = projects();
    let view = cx.new(|cx| {
        AutomationsView::new(
            shared,
            projects,
            Some("/Users/me/code/monocode"),
            window,
            cx,
        )
    });
    (view, data)
}

pub fn automation_scenes() -> Vec<Scene> {
    vec![
        scene("automations-picker", 1280., 820., |window, cx| {
            let (view, _) = automations_view(window, cx, sample_automations());
            stage(view)
        }),
        scene("automations-empty", 1280., 820., |window, cx| {
            let (view, _) = automations_view(window, cx, Vec::new());
            stage(view)
        }),
        scene("automations-editor", 1280., 900., |window, cx| {
            let (view, _) = automations_view(window, cx, sample_automations());
            view.update(cx, |view, cx| view.select("a1", cx));
            stage(view)
        }),
        scene("automations-editor-light", 1280., 900., |window, cx| {
            let (view, _) = automations_view(window, cx, sample_automations());
            view.update(cx, |view, cx| view.select("a1", cx));
            stage(view)
        })
        .light(),
        editor_scene("automations-history", |editor, _, cx| {
            editor.update(cx, |editor, cx| {
                editor.set_tab(monocode_view_pages::automations::EditorTab::History, cx)
            });
        }),
        editor_scene("automations-trigger-menu", |editor, window, cx| {
            editor.update(cx, |editor, cx| {
                editor.toggle_trigger_menu(window, cx);
                editor.show_category(
                    monocode_view_pages::automations::model::AutomationTriggerKind::Github,
                    cx,
                );
            });
        }),
        editor_scene("automations-actions-menu", |editor, _, cx| {
            editor.update(cx, |editor, cx| editor.toggle_actions_menu(cx));
        }),
        scene("automations-new", 1280., 900., |window, cx| {
            let (view, _) = automations_view(window, cx, sample_automations());
            view.update(cx, |view, cx| {
                let template = sample_templates()[0].clone();
                view.begin_from_template(&template, cx)
            });
            stage(view)
        }),
    ]
}

/// An automations scene that acts on the editor after the first frames.
fn editor_scene(
    name: &'static str,
    act: impl FnOnce(
        &gpui::Entity<monocode_view_pages::automations::AutomationEditor>,
        &mut gpui::Window,
        &mut gpui::App,
    ) + 'static,
) -> Scene {
    let slot: Rc<
        std::cell::RefCell<Option<gpui::Entity<monocode_view_pages::automations::AutomationsView>>>,
    > = Rc::default();
    let built = slot.clone();
    scene(name, 1280., 900., move |window, cx| {
        let (view, _) = automations_view(window, cx, sample_automations());
        view.update(cx, |view, cx| view.select("a1", cx));
        *built.borrow_mut() = Some(view.clone());
        stage(view)
    })
    .act(move |window, cx| {
        let view = slot.borrow().clone().unwrap();
        let editor = view.read(cx).editor().cloned().unwrap();
        act(&editor, window, cx);
    })
}
