use crate::{
    settings::{
        SettingsView,
        management::Target,
        widgets::{note, secondary_button},
    },
    theme::*,
};
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
impl SettingsView {
    pub(super) fn mcp_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let project = self.project_picker(cx);
        let busy = self.mcp_busy || self.management.as_ref().is_some_and(|e| e.busy);
        let mut page = vec![
            div()
                .relative()
                .child(self.reveal_probe("mcp-servers"))
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap_3()
                .child(
                    div()
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child("MCP connections"),
                                )
                                .child(project),
                        )
                        .child(
                            div()
                                .mt_1()
                                .text_size(px(12.))
                                .text_color(content(0.55))
                                .child("Servers for this project and your accounts."),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(self.refresh_button("refresh-mcp", "Refresh", cx))
                        .child(
                            secondary_button(
                                "mcp-show-all",
                                if self.mcp_show_all {
                                    "Show available providers"
                                } else {
                                    "Show all providers"
                                },
                                false,
                                false,
                            )
                            .child(icon("filter", 14.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.mcp_show_all = !this.mcp_show_all;
                                cx.notify();
                            })),
                        )
                        .child(
                            secondary_button("add-mcp", "Add MCP server", false, busy)
                                .debug_selector(|| "settings-add-mcp".into())
                                .child(icon("plus", 14.))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    if !busy {
                                        this.open_management(Target::Mcp, window, cx);
                                    }
                                })),
                        ),
                )
                .into_any_element(),
        ];
        let filter = self.mcp_filter.clone();
        let counts = std::iter::once(("all", "All"))
            .chain(
                [
                    ("claude", "Claude Code"),
                    ("claude_desktop", "Claude Desktop"),
                    ("codex", "Codex"),
                    ("cursor", "Cursor"),
                    ("opencode", "OpenCode"),
                ]
                .into_iter()
                .filter(|(p, _)| {
                    self.mcp_show_all || self.discovery.mcp.iter().any(|s| s.provider == *p)
                }),
            )
            .map(|(id, label)| {
                let count = self
                    .discovery
                    .mcp
                    .iter()
                    .filter(|s| id == "all" || s.provider == id)
                    .count();
                (id, format!("{label} {count}"))
            })
            .collect::<Vec<_>>();
        let options = counts
            .iter()
            .map(|(id, label)| (*id, label.as_str()))
            .collect::<Vec<_>>();
        page.push(
            div()
                .mt_6()
                .child(self.segmented(
                    "mcp-filter",
                    "Filter MCP servers by provider",
                    &filter,
                    &options,
                    cx,
                    |this, v, cx| {
                        this.mcp_filter = v.into();
                        cx.notify();
                    },
                ))
                .into_any_element(),
        );
        if let Some(error) = &self.discovery.mcp_error {
            page.push(
                div()
                    .id("settings-page-error")
                    .role(gpui::Role::Alert)
                    .mt_3()
                    .text_size(px(12.))
                    .text_color(gpui::rgb(0xf87171))
                    .child(error.clone())
                    .into_any_element(),
            );
        }
        if self.discovery_loading {
            page.push(note("Checking servers…"));
            return page;
        }
        let servers = self
            .discovery
            .mcp
            .iter()
            .filter(|s| filter == "all" || filter == s.provider)
            .cloned()
            .collect::<Vec<_>>();
        if servers.is_empty() {
            page.push(note("No MCP servers configured for this provider."));
        } else {
            page.push(
                div()
                    .mt_6()
                    .overflow_hidden()
                    .rounded_xl()
                    .border_1()
                    .border_color(content(0.1))
                    .bg(content(0.03))
                    .children(servers.into_iter().enumerate().map(|(ix, server)| {
                        let mut row = div()
                            .id(SharedString::from(format!("mcp-server-{ix}")))
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_3()
                            .px_4()
                            .py(px(14.))
                            .border_b_1()
                            .border_color(content(0.05))
                            .child(provider_icon(
                                if server.provider == "claude_desktop" {
                                    "claude"
                                } else {
                                    &server.provider
                                },
                                16.,
                            ))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .child(server.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .mt_1()
                                            .text_size(px(12.))
                                            .text_color(content(0.45))
                                            .child(format!(
                                                "{} · {} · {} · {}",
                                                if server.provider == "claude_desktop" {
                                                    "Claude Desktop"
                                                } else {
                                                    monocode_core::provider::Kind::from_id(
                                                        &server.provider,
                                                    )
                                                    .label()
                                                },
                                                server.scope,
                                                server.transport,
                                                server.status
                                            )),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(11.))
                                            .text_color(content(0.35))
                                            .child(server.path.display().to_string()),
                                    ),
                            );
                        if server.provider != "claude_desktop"
                            && !server.transport.is_empty()
                            && !matches!(server.transport.as_str(), "stdio" | "local" | "ws")
                        {
                            let login = server.clone();
                            row = row.child(
                                secondary_button(
                                    SharedString::from(format!("mcp-login-{ix}")),
                                    "Sign in",
                                    false,
                                    busy,
                                )
                                .child("Sign in")
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.mcp_action(login.clone(), false, cx),
                                )),
                            );
                        }
                        if server.provider == "claude" {
                            let mut remove = server.clone();
                            if server.path.as_os_str().is_empty() {
                                let name = server.name.clone();
                                let scope = self
                                    .mcp_remove_scopes
                                    .get(&name)
                                    .cloned()
                                    .unwrap_or_else(|| "local".into());
                                remove.scope = scope.clone();
                                row = row.child(self.segmented(
                                    "mcp-remove-scope",
                                    "Scope to remove this server from",
                                    &scope,
                                    &[("local", "Local"), ("project", "Project"), ("user", "User")],
                                    cx,
                                    move |this, value, cx| {
                                        this.mcp_remove_scopes.insert(name.clone(), value.into());
                                        cx.notify();
                                    },
                                ));
                            }
                            row = row.child(
                                secondary_button(
                                    SharedString::from(format!("mcp-remove-{ix}")),
                                    "Remove",
                                    false,
                                    busy,
                                )
                                .child("Remove")
                                .on_click(cx.listener(
                                    move |_, _, window, cx| {
                                        let prompt = window.prompt(
                                            gpui::PromptLevel::Warning,
                                            "Remove MCP server",
                                            Some(&format!(
                                                "Remove {} from {} scope?",
                                                remove.name, remove.scope
                                            )),
                                            &["Remove", "Cancel"],
                                            cx,
                                        );
                                        let server = remove.clone();
                                        cx.spawn(async move |this, cx| {
                                            if prompt.await == Ok(0) {
                                                let _ = this.update(cx, |this, cx| {
                                                    this.mcp_action(server, true, cx)
                                                });
                                            }
                                        })
                                        .detach();
                                    },
                                )),
                            );
                        } else {
                            let path = server.path;
                            row = row.child(
                                secondary_button(
                                    SharedString::from(format!("mcp-show-{ix}")),
                                    "Show config",
                                    false,
                                    false,
                                )
                                .child("Show config")
                                .on_click(move |_, _, cx| cx.reveal_path(&path)),
                            );
                        }
                        row
                    }))
                    .into_any_element(),
            );
        }
        page.push(note("Live connection status is available for Claude Code."));
        page
    }
    pub(crate) fn refresh_mcp_health(&mut self, cx: &mut Context<Self>) {
        let cwd = self.selected_project.clone();
        let expected = cwd.clone();
        let work = cx
            .background_executor()
            .spawn(async move { monocode_core::mcp::claude_health(&cwd) });
        self.mcp_health = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.selected_project != expected {
                    return;
                }
                match result {
                    Ok(output) => {
                        for line in output.lines() {
                            let Some((name, detail)) = line.split_once(": ") else {
                                continue;
                            };
                            let name = name.trim();
                            if name.is_empty()
                                || !name
                                    .bytes()
                                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
                            {
                                continue;
                            }
                            let lower = detail.to_lowercase();
                            let status = if lower.contains("connected") {
                                "Connected"
                            } else if lower.contains("authentication")
                                || lower.contains("needs auth")
                            {
                                "Needs authentication"
                            } else if lower.contains("failed") {
                                "Failed"
                            } else if lower.contains("disabled") {
                                "Disabled"
                            } else {
                                "Configured"
                            };
                            let mut found = false;
                            for server in &mut this.discovery.mcp {
                                if server.provider == "claude" && server.name == name {
                                    if server.status != "Disabled" {
                                        server.status = status.into();
                                    }
                                    found = true;
                                }
                            }
                            if !found {
                                this.discovery.mcp.push(super::super::data::McpServer {
                                    name: name.into(),
                                    provider: "claude".into(),
                                    scope: "local".into(),
                                    path: Default::default(),
                                    transport: if lower.contains("http") { "http" } else { "" }
                                        .into(),
                                    status: status.into(),
                                });
                            }
                        }
                    }
                    Err(e) => {
                        this.discovery.mcp_error =
                            Some(format!("Claude connection status unavailable: {e}"))
                    }
                }
                cx.notify();
            });
        }));
    }
    fn mcp_action(
        &mut self,
        server: super::super::data::McpServer,
        remove: bool,
        cx: &mut Context<Self>,
    ) {
        if self.mcp_busy {
            return;
        }
        self.mcp_busy = true;
        let cwd = self.selected_project.clone();
        let expected = cwd.clone();
        let work = cx.background_executor().spawn(async move {
            if remove {
                monocode_core::mcp::remove(&cwd, &server.name, &server.scope)
            } else {
                monocode_core::mcp::login(&cwd, &server.provider, &server.name)
            }
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.mcp_busy = false;
                if this.selected_project != expected {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(()) => this.refresh(cx),
                    Err(e) => this.discovery.mcp_error = Some(e),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
