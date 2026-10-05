//! Ports of McpSettings.test.ts. The cache's own cases (shared requests,
//! health never delaying the list) are ported in the engine's
//! mcp_settings_cache; these check the page over [`LocalMcp`] and a held
//! data source.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use futures::channel::oneshot;
use gpui::{
    App, AppContext as _, Entity, Subscription, Task, TestAppContext, VisualTestContext, Window,
};

use super::{
    LocalMcp, McpCall, McpConnection, McpData, McpFilter, McpProvider, McpScope, McpServerRow,
    McpSettingsSnapshot, McpSettingsView,
};
use crate::data::{DataTask, Listener, StaticProjects};
use crate::test_support::{click, draw, exists, keys, mount, type_text};

fn row(
    provider: McpProvider,
    name: &str,
    scope: McpScope,
    path: &str,
    transport: &str,
) -> McpServerRow {
    McpServerRow {
        connection: McpConnection {
            provider,
            name: name.into(),
            scope,
            config_path: path.into(),
            transport: transport.into(),
            enabled: None,
        },
        status: "configured".into(),
    }
}

fn discovered() -> McpSettingsSnapshot {
    let mut sentry = row(
        McpProvider::Claude,
        "sentry",
        McpScope::User,
        "/home/.claude.json",
        "http",
    );
    sentry.status = "! Needs authentication".into();
    McpSettingsSnapshot {
        servers: vec![
            sentry,
            row(
                McpProvider::Codex,
                "docs",
                McpScope::User,
                "/home/.codex/config.toml",
                "http",
            ),
        ],
        ..Default::default()
    }
}

struct Page<'a> {
    view: Entity<McpSettingsView>,
    data: LocalMcp,
    cx: &'a mut VisualTestContext,
}

fn render<'a>(
    cx: &'a mut TestAppContext,
    cwd: &str,
    setup: impl FnOnce(&LocalMcp, &mut App) + 'static,
) -> Page<'a> {
    let slot: Rc<RefCell<Option<LocalMcp>>> = Rc::default();
    let built = slot.clone();
    let cwd = cwd.to_string();
    let (view, cx) = mount(cx, move |window, cx| {
        let data = LocalMcp::new(cx);
        setup(&data, cx);
        *built.borrow_mut() = Some(data.clone());
        let projects = Rc::new(StaticProjects::new(["/repo", "/other"]));
        cx.new(|cx| McpSettingsView::new(Rc::new(data), projects, &cwd, window, cx))
    });
    let data = slot.borrow().clone().unwrap();
    Page { view, data, cx }
}

fn names(page: &mut Page) -> Vec<String> {
    page.view.read_with(page.cx, |view, _| {
        view.visible()
            .iter()
            .map(|row| row.connection.name.clone())
            .collect()
    })
}

fn calls(page: &mut Page) -> Vec<McpCall> {
    let data = page.data.clone();
    page.cx.update(|_, cx| data.calls(cx))
}

fn select_project(page: &mut Page, path: &str) {
    let label = page
        .view
        .read_with(page.cx, |view, cx| view.picker().read(cx).trigger_label(cx));
    click(page.cx, &format!("project-picker-trigger {label}"));
    click(page.cx, &format!("project-picker-row {path}"));
}

#[gpui::test]
fn lists_servers_and_routes_sign_in_through_claude_mcp(cx: &mut TestAppContext) {
    let mut page = render(cx, "/repo", |data, cx| {
        data.set_discovered("/repo", discovered(), cx)
    });
    assert_eq!(names(&mut page), vec!["sentry", "docs"]);
    let status = page
        .view
        .read_with(page.cx, |view, _| view.servers()[0].status.clone());
    assert_eq!(status, "! Needs authentication");
    click(page.cx, "Sign in sentry");
    assert!(calls(&mut page).contains(&McpCall::Login {
        cwd: "/repo".into(),
        provider: McpProvider::Claude,
        name: "sentry".into(),
    }));
}

#[gpui::test]
fn filters_connections_by_provider(cx: &mut TestAppContext) {
    let mut page = render(cx, "/repo", |data, cx| {
        data.set_discovered("/repo", discovered(), cx)
    });
    click(page.cx, "mcp-chip Codex");
    assert_eq!(names(&mut page), vec!["docs"]);
    click(page.cx, "Sign in docs");
    assert!(calls(&mut page).contains(&McpCall::Login {
        cwd: "/repo".into(),
        provider: McpProvider::Codex,
        name: "docs".into(),
    }));
}

#[gpui::test]
fn shows_configured_rows_while_claude_health_is_still_pending(cx: &mut TestAppContext) {
    let mut page = render(cx, "/repo", |data, cx| {
        let mut pending = discovered();
        pending.servers[0].status = "configured".into();
        data.set_discovered("/repo", pending, cx)
    });
    assert_eq!(names(&mut page), vec!["sentry", "docs"]);
    assert!(!page.view.read_with(page.cx, |view, _| view.is_loading()));
    let data = page.data.clone();
    page.cx
        .update(|_, cx| data.publish("/repo", discovered(), cx));
    draw(page.cx);
    let status = page
        .view
        .read_with(page.cx, |view, _| view.servers()[0].status.clone());
    assert_eq!(status, "! Needs authentication");
}

#[gpui::test]
fn adds_a_standard_mcp_servers_entry_to_the_selected_provider_and_project(cx: &mut TestAppContext) {
    let mut page = render(cx, "/repo", |data, cx| {
        data.set_discovered("/repo", discovered(), cx)
    });
    select_project(&mut page, "/other");
    assert_eq!(
        page.view
            .read_with(page.cx, |view, _| view.cwd().to_string()),
        "/other"
    );
    click(page.cx, "Add MCP server");
    assert!(exists(page.cx, "mcp-picker Provider: Claude Code"));
    click(page.cx, "mcp-picker Provider: Claude Code");
    click(page.cx, "mcp-option Cursor");
    assert!(exists(page.cx, "mcp-picker Scope: Project"));
    click(page.cx, "mcp-config");
    let json = r#"{"mcpServers":{"new-server":{"command":"npx","args":["example"]}}}"#;
    type_text(page.cx, json);
    click(page.cx, "mcp-add-submit");
    let calls = calls(&mut page);
    assert!(calls.contains(&McpCall::Add {
        cwd: "/other".into(),
        provider: McpProvider::Cursor,
        scope: McpScope::Project,
        name: String::new(),
        config: json.into(),
    }));
    assert_eq!(
        calls.last(),
        Some(&McpCall::Load {
            cwd: "/other".into(),
            force: true
        })
    );
    assert!(
        page.view
            .read_with(page.cx, |view, _| view.add_form().is_none())
    );
}

#[gpui::test]
fn rejects_a_configuration_that_is_not_an_object(cx: &mut TestAppContext) {
    let mut page = render(cx, "/repo", |_, _| {});
    click(page.cx, "Add MCP server");
    click(page.cx, "mcp-config");
    type_text(page.cx, "[1, 2]");
    click(page.cx, "mcp-add-submit");
    assert!(exists(page.cx, "mcp-add-error"));
    assert!(
        !calls(&mut page)
            .iter()
            .any(|call| matches!(call, McpCall::Add { .. }))
    );
}

#[gpui::test]
fn navigates_provider_choices_with_arrow_keys(cx: &mut TestAppContext) {
    let page = render(cx, "/repo", |_, _| {});
    click(page.cx, "Add MCP server");
    click(page.cx, "mcp-picker Provider: Claude Code");
    let picker = page.view.read_with(page.cx, |view, cx| {
        view.add_form().unwrap().read(cx).provider_picker().clone()
    });
    keys(page.cx, "down");
    assert_eq!(
        picker.read_with(page.cx, |picker, _| picker.active()),
        Some(0)
    );
    keys(page.cx, "up");
    assert_eq!(
        picker.read_with(page.cx, |picker, _| picker.active()),
        Some(4)
    );
    keys(page.cx, "enter");
    assert_eq!(
        picker.read_with(page.cx, |picker, _| picker.value().to_string()),
        "opencode"
    );
}

#[gpui::test]
fn hides_sign_in_when_a_server_has_no_known_transport(cx: &mut TestAppContext) {
    let mut page = render(cx, "/repo", |data, cx| {
        data.set_discovered(
            "/repo",
            McpSettingsSnapshot {
                servers: vec![row(McpProvider::Claude, "unknown", McpScope::Local, "", "")],
                ..Default::default()
            },
            cx,
        )
    });
    assert_eq!(names(&mut page), vec!["unknown"]);
    assert!(!exists(page.cx, "Sign in unknown"));
    // A row without a config file picks the scope Remove uses.
    assert!(exists(page.cx, "Remove unknown"));
}

#[gpui::test]
fn shows_only_configured_provider_chips_until_the_filter_button_reveals_all(
    cx: &mut TestAppContext,
) {
    let page = render(cx, "/repo", |data, cx| {
        data.set_discovered("/repo", discovered(), cx)
    });
    assert!(exists(page.cx, "mcp-chip Codex"));
    assert!(!exists(page.cx, "mcp-chip Cursor"));
    click(page.cx, "Show all providers");
    assert!(exists(page.cx, "mcp-chip Cursor"));
    click(page.cx, "Show available providers");
    assert!(!exists(page.cx, "mcp-chip Cursor"));
}

#[gpui::test]
fn removes_a_claude_server_after_confirming(cx: &mut TestAppContext) {
    let mut page = render(cx, "/repo", |data, cx| {
        data.set_discovered("/repo", discovered(), cx)
    });
    click(page.cx, "Remove sentry");
    assert!(calls(&mut page).contains(&McpCall::Remove {
        cwd: "/repo".into(),
        name: "sentry".into(),
        scope: McpScope::User,
    }));
}

#[gpui::test]
fn reuses_the_cached_list_when_returning_and_reloads_on_refresh(cx: &mut TestAppContext) {
    let mut page = render(cx, "/repo", |data, cx| {
        data.set_discovered("/repo", discovered(), cx)
    });
    assert_eq!(names(&mut page), vec!["sentry", "docs"]);
    // Leave and come back: a second view reads the cache, with no load
    // spinner, and its load answers from the cache.
    let data = page.data.clone();
    let projects = Rc::new(StaticProjects::new(["/repo"]));
    let again = page.cx.update(|window, cx| {
        cx.new(|cx| McpSettingsView::new(Rc::new(data.clone()), projects, "/repo", window, cx))
    });
    assert!(!again.read_with(page.cx, |view, _| view.is_loading()));
    assert_eq!(again.read_with(page.cx, |view, _| view.servers().len()), 2);
    page.cx.update(|_, cx| {
        data.set_discovered(
            "/repo",
            McpSettingsSnapshot {
                servers: vec![row(
                    McpProvider::Cursor,
                    "updated",
                    McpScope::Project,
                    "/repo/.cursor/mcp.json",
                    "stdio",
                )],
                ..Default::default()
            },
            cx,
        )
    });
    click(page.cx, "mcp-refresh");
    assert_eq!(names(&mut page), vec!["updated"]);
}

#[gpui::test]
fn caches_discovery_failures_and_lets_refresh_retry(cx: &mut TestAppContext) {
    let mut page = render(cx, "/repo", |data, cx| {
        data.set_discovered(
            "/repo",
            McpSettingsSnapshot {
                error: "Discovery failed".into(),
                ..Default::default()
            },
            cx,
        )
    });
    assert_eq!(
        page.view
            .read_with(page.cx, |view, _| view.error().to_string()),
        "Discovery failed"
    );
    assert!(exists(page.cx, "mcp-error"));
    let data = page.data.clone();
    page.cx
        .update(|_, cx| data.set_discovered("/repo", discovered(), cx));
    click(page.cx, "mcp-refresh");
    assert_eq!(names(&mut page), vec!["sentry", "docs"]);
    assert!(!exists(page.cx, "mcp-error"));
}

/// Loads that answer only when the test says so.
#[derive(Clone, Default)]
struct HeldMcp {
    pending: Rc<RefCell<HashMap<String, oneshot::Sender<McpSettingsSnapshot>>>>,
    cache: Rc<RefCell<HashMap<String, McpSettingsSnapshot>>>,
}

impl HeldMcp {
    fn answer(&self, cwd: &str, snapshot: McpSettingsSnapshot) {
        self.cache.borrow_mut().insert(cwd.into(), snapshot.clone());
        if let Some(sender) = self.pending.borrow_mut().remove(cwd) {
            sender.send(snapshot).ok();
        }
    }
}

impl McpData for HeldMcp {
    fn cached(&self, cwd: &str, _: &App) -> Option<McpSettingsSnapshot> {
        self.cache.borrow().get(cwd).cloned()
    }

    fn subscribe(&self, _: &str, _: Listener, _: &mut App) -> Subscription {
        Subscription::new(|| {})
    }

    fn load(&self, cwd: &str, _: bool, cx: &mut App) -> Task<McpSettingsSnapshot> {
        let (sender, receiver) = oneshot::channel();
        self.pending.borrow_mut().insert(cwd.into(), sender);
        cx.background_spawn(async move { receiver.await.unwrap_or_default() })
    }

    fn login(&self, _: &str, _: McpProvider, _: &str, _: &mut App) -> DataTask<()> {
        Task::ready(Ok(()))
    }

    fn remove(&self, _: &str, _: &str, _: McpScope, _: &mut App) -> DataTask<()> {
        Task::ready(Ok(()))
    }

    fn add(
        &self,
        _: &str,
        _: McpProvider,
        _: McpScope,
        _: &str,
        _: &str,
        _: &mut App,
    ) -> DataTask<()> {
        Task::ready(Ok(()))
    }

    fn reveal(&self, _: &str, _: &mut App) -> DataTask<()> {
        Task::ready(Ok(()))
    }

    fn confirm(&self, _: &str, _: &str, _: &mut Window, _: &mut App) -> Task<bool> {
        Task::ready(true)
    }
}

#[gpui::test]
fn ignores_discovery_from_a_previous_project_after_cwd_changes(cx: &mut TestAppContext) {
    let held = HeldMcp::default();
    let data = held.clone();
    let (view, cx) = mount(cx, move |window, cx| {
        let projects = Rc::new(StaticProjects::new(["/old", "/new"]));
        cx.new(|cx| McpSettingsView::new(Rc::new(data), projects, "/old", window, cx))
    });
    assert!(view.read_with(cx, |view, _| view.is_loading()));
    view.update_in(cx, |view, window, cx| view.set_cwd("/new", window, cx));
    held.answer(
        "/new",
        McpSettingsSnapshot {
            servers: vec![row(
                McpProvider::Cursor,
                "new-project",
                McpScope::Project,
                "/new/.cursor/mcp.json",
                "stdio",
            )],
            ..Default::default()
        },
    );
    draw(cx);
    held.answer(
        "/old",
        McpSettingsSnapshot {
            servers: vec![row(
                McpProvider::Cursor,
                "old-project",
                McpScope::Project,
                "/old/.cursor/mcp.json",
                "stdio",
            )],
            ..Default::default()
        },
    );
    draw(cx);
    let names: Vec<String> = view.read_with(cx, |view, _| {
        view.servers()
            .iter()
            .map(|row| row.connection.name.clone())
            .collect()
    });
    assert_eq!(names, vec!["new-project"]);
    assert_eq!(view.read_with(cx, |view, _| view.filter()), McpFilter::All);
}
