//! The native account usage footer and running terminal controls.
use super::Shell;
use gpui::{AppContext as _, Context, IntoElement};
use monocode_view_settings::accounts::model::RateLimitProvider;
use monocode_view_settings::accounts::{
    UsageFooter, UsageFooterCallbacks, UsageFooterProps, UsageFooterSession,
};
use std::rc::Rc;

impl Shell {
    pub(super) fn render_footer(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let props = if let Some(workspace) = &self.workspace {
            let workspace = workspace.read(cx);
            UsageFooterProps {
                providers: RateLimitProvider::ALL.to_vec(),
                session: workspace
                    .active_session(cx)
                    .map(|session| UsageFooterSession {
                        id: Some(session.id),
                        harness: session.harness,
                        model: Some(session.model),
                        auth_required: false,
                        provider_account_id: session.provider_account_id,
                    }),
                project: Some(workspace.sidebar_cwd(cx)),
                terminals: workspace.running_terminals(cx),
                terminal_open: workspace.running_terminal_open(cx),
                project_terminal_active: workspace.terminals().read(cx).is_focused(),
                ..Default::default()
            }
        } else {
            UsageFooterProps::default()
        };
        if self.usage_footer.is_none() {
            let host = Rc::new(crate::adapters::settings::AccountsAdapter::new());
            let target = cx.weak_entity();
            let callbacks = UsageFooterCallbacks {
                on_toggle_terminal: Some(Rc::new({
                    let target = target.clone();
                    move |id, _, cx| {
                        target
                            .update(cx, |shell, cx| {
                                if let Some(workspace) = &shell.workspace {
                                    workspace.update(cx, |workspace, cx| {
                                        workspace.toggle_running_terminal(&id, cx)
                                    });
                                }
                            })
                            .ok();
                    }
                })),
                on_new_terminal: Some(Rc::new({
                    let target = target.clone();
                    move |_, _, cx| {
                        target
                            .update(cx, |shell, cx| {
                                if let Some(workspace) = &shell.workspace {
                                    workspace
                                        .update(cx, |workspace, cx| workspace.new_terminal(cx));
                                }
                            })
                            .ok();
                    }
                })),
                on_show_terminal: Some(Rc::new({
                    let target = target.clone();
                    move |_, _, cx| {
                        target
                            .update(cx, |shell, cx| {
                                if let Some(workspace) = &shell.workspace {
                                    workspace.update(cx, |workspace, cx| {
                                        workspace.show_project_terminal(cx)
                                    });
                                }
                            })
                            .ok();
                    }
                })),
                on_manage_accounts: Some(Rc::new(move |_, _, cx| {
                    target
                        .update(cx, |shell, cx| {
                            if let Some(services) = monocode_app::boot::AppServices::try_global(cx)
                            {
                                monocode_settings::settings_store::save_settings_section(
                                    &services.kv,
                                    monocode_core::settings::SettingsSectionId::Providers,
                                );
                            }
                            shell.open_page(crate::slots::Page::Settings, cx)
                        })
                        .ok();
                })),
                ..Default::default()
            };
            self.usage_footer =
                Some(cx.new(|cx| UsageFooter::new(host, props.clone(), callbacks, cx)));
        }
        let footer = self.usage_footer.as_ref().unwrap().clone();
        if footer.read(cx).props() != &props {
            footer.update(cx, |footer, cx| footer.set_props(props, cx));
        }
        footer
    }
}
