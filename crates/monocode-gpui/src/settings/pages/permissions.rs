use crate::settings::{
    SettingsView,
    widgets::{note, secondary_button},
};
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
use monocode_core::permissions::{Grant, Store, Subject};
use std::path::{Path, PathBuf};

impl SettingsView {
    pub(super) fn permissions_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut page = vec![
            div()
                .relative()
                .child(self.reveal_probe("saved-permissions"))
                .child(self.project_picker(cx))
                .into_any_element(),
            note(
                "Requests saved with Always allow in this project apply across its worktrees, chats and restarts. Saved permissions are not used in plan or read-only turns.",
            ),
        ];
        if let Some(error) = &self.grants_error {
            page.push(
                div()
                    .id("saved-permissions-error")
                    .mt_4()
                    .role(gpui::Role::Alert)
                    .text_size(px(12.))
                    .text_color(gpui::rgb(0xf87171))
                    .child(error.clone())
                    .into_any_element(),
            );
        }
        let project = |path: &str| monocode_core::permissions::repository(Path::new(path));
        let selected = project(&self.selected_project);
        let known: Vec<PathBuf> = self.ctx.projects.iter().map(|(p, _)| project(p)).collect();
        let mut here: Vec<&Grant> = Vec::new();
        let mut elsewhere: Vec<&Grant> = Vec::new();
        for grant in &self.grants {
            if grant.key.project == selected {
                here.push(grant);
            } else if !known.contains(&grant.key.project) {
                elsewhere.push(grant);
            }
        }
        if here.is_empty() {
            page.push(note("No saved permissions for this project."));
        }
        page.push(self.grant_list("remove-permission", here, false, cx));
        // Grants for removed or archived projects have no picker entry, so
        // list them here to keep them removable.
        if !elsewhere.is_empty() {
            page.push(
                div()
                    .mt_6()
                    .text_size(px(14.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Other projects")
                    .into_any_element(),
            );
            page.push(note(
                "Saved for projects that are no longer in your project list.",
            ));
            page.push(self.grant_list("remove-other-permission", elsewhere, true, cx));
        }
        page
    }

    fn grant_list(
        &self,
        id: &'static str,
        mut grants: Vec<&Grant>,
        show_project: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        grants.sort_by(|a, b| {
            (&a.key.project, &a.key.provider, &a.key.account, &a.label).cmp(&(
                &b.key.project,
                &b.key.provider,
                &b.key.account,
                &b.label,
            ))
        });
        let mut list = div().mt_4();
        for (index, grant) in grants.into_iter().enumerate() {
            let key = grant.key.clone();
            let provider = monocode_core::provider::Kind::parse(&key.provider)
                .map_or(key.provider.as_str(), |kind| kind.label());
            let mut description = format!(
                "{} · {provider} · Account {}",
                kind(&key.subject),
                key.account
            );
            if show_project {
                description = format!("{} · {description}", key.project.display());
            }
            list = list.child(self.row(
                None,
                index == 0,
                grant.label.clone(),
                Some(description.into()),
                vec![
                        secondary_button(
                            SharedString::from(format!("{id}-{index}")),
                            "Remove",
                            false,
                            self.grants_busy,
                        )
                        .debug_selector(move || format!("{id}-{index}"))
                        .child("Remove")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.grants_busy {
                                return;
                            }
                            let store = Store::default();
                            let key = key.clone();
                            this.load_grants(
                                move || {
                                    store.remove(&key)?;
                                    store.list()
                                },
                                cx,
                            );
                        }))
                        .into_any_element(),
                    ],
            ));
        }
        list.into_any_element()
    }

    pub(crate) fn refresh_grants(&mut self, cx: &mut Context<Self>) {
        self.load_grants(|| Store::default().list(), cx);
    }

    fn load_grants(
        &mut self,
        work: impl FnOnce() -> Result<Vec<Grant>, String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        self.grants_busy = true;
        let work = cx.background_executor().spawn(async move { work() });
        self.grants_task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.grants_busy = false;
                match result {
                    Ok(grants) => {
                        this.grants = grants;
                        this.grants_error = None;
                    }
                    Err(error) => this.grants_error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

fn kind(subject: &Subject) -> &'static str {
    match subject {
        Subject::McpConfirmation { .. } => "MCP confirmation",
        Subject::McpTool { .. } => "MCP tool",
        Subject::Command { .. } => "Command",
        Subject::Edit { .. } => "File edits",
        Subject::Fetch { .. } => "Web fetch",
        Subject::Request { .. } => "Exact request",
    }
}
