//! Pull requests and issues opened from a Mono stay in its detail panel,
//! shown by an Inbox that has no list of its own.
use super::*;
use crate::inbox::{self, Inbox};

pub(super) struct Page {
    mono: String,
    view: Entity<Inbox>,
    _events: Subscription,
}

impl Monos {
    /// Show a GitHub or Linear work item link as a page of the current Mono.
    /// False for links the Inbox cannot show, which belong to the browser.
    pub(crate) fn open_work_item(&mut self, url: &str, cx: &mut Context<Self>) -> bool {
        if monocode_core::inbox::link_target(url).is_none() {
            return false;
        }
        let Some(mono) = self.current().cloned() else {
            return false;
        };
        let view = match &self.work_item {
            Some(page) if page.mono == mono.id => page.view.clone(),
            _ => {
                let projects: Vec<PathBuf> = mono.projects.iter().map(PathBuf::from).collect();
                let cwd = projects
                    .first()
                    .cloned()
                    .unwrap_or_else(|| self.root.clone());
                let store = self.store.clone();
                let view = cx.new(|cx| Inbox::detail(store, projects, cwd, cx));
                let events = cx.subscribe(&view, |this, _, event: &inbox::Event, cx| {
                    match event {
                        inbox::Event::Close => {
                            if this.route == Some(Route::WorkItem) {
                                this.route = None;
                            }
                        }
                        inbox::Event::OpenSession(id) => this.route = Some(Route::Session(*id)),
                        inbox::Event::StartSession { item, prompt } => {
                            let (item, prompt) = (item.as_ref().clone(), prompt.clone());
                            let _ = this.owner.update(cx, |shell, cx| {
                                shell.mono_start_work_item_session(item, prompt, cx)
                            });
                        }
                        inbox::Event::ItemsChanged(_) => return,
                    }
                    cx.notify();
                });
                self.work_item = Some(Page {
                    mono: mono.id.clone(),
                    view: view.clone(),
                    _events: events,
                });
                view
            }
        };
        if !view.update(cx, |inbox, cx| inbox.open_url(url, cx)) {
            return false;
        }
        self.route = Some(Route::WorkItem);
        self.show_overview(cx);
        cx.notify();
        true
    }

    /// A link followed in a Mono's chat, which may be in a floating window:
    /// that Mono's detail panel in the main window shows it.
    pub(super) fn open_work_item_from(
        &mut self,
        mono: &str,
        url: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        if monocode_core::inbox::link_target(url).is_none() || self.roster.get(mono).is_none() {
            return false;
        }
        self.active = Some(mono.into());
        if let Some(handle) = self.parent_window {
            let app: &mut App = cx;
            app.defer(move |cx| {
                let _ = handle.update(cx, |_, window, _| window.activate_window());
            });
        }
        self.open_work_item(url, cx)
    }

    pub(super) fn work_item_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let view = self.work_item.as_ref().map(|page| page.view.clone());
        let item = view.as_ref().and_then(|view| view.read(cx).shown());
        let title = item.as_ref().map_or_else(
            || "Opening…".to_owned(),
            |item| {
                let kind = if item.kind == "pr" { "PR" } else { "Issue" };
                format!("{kind} #{} · {}", item.number, item.title)
            },
        );
        let url = item.map(|item| item.url);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.page_header(&title, cx).when_some(url, |d, url| {
                d.child(
                    icon_button("mono-work-item-browser", "Open in browser", "external-link")
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                )
            }))
            .when_some(view, |d, view| {
                d.child(div().flex_1().min_h_0().child(view))
            })
            .into_any_element()
    }
}
