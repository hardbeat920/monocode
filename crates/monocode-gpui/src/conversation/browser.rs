//! The thread's browser: the Ask prompt on the composer while an agent waits
//! for the user's Allow, and the globe beside the composer's pills with the
//! thread's tabs ("Show in MonoCode", "Point at an element", take back,
//! close), the app this branch serves, and tabs to add from the browser.
use super::*;
use monocode_core::browser::{
    BrowserState,
    devservers::DevServer,
    pick::PickedElement,
    protocol::TabId,
    targets::FreeTab,
    view::{self as browser_view, Indicator},
};

/// Tailwind's teal-400: the browser feature's colour.
const TEAL: u32 = 0x2dd4bf;
/// How often the menu looks again for the app this branch serves.
const APPS_EVERY: std::time::Duration = std::time::Duration::from_secs(4);
const NO_FREE_TABS: &str =
    "No tab is free to give. Open a web page in the browser, then click Refresh.";

fn teal(alpha: f32) -> gpui::Hsla {
    let mut color: gpui::Hsla = gpui::rgb(TEAL).into();
    color.a = alpha;
    color
}

/// One of the thread's tabs, as the menu lists it.
#[derive(Clone, Debug, PartialEq)]
struct Tab {
    id: String,
    title: String,
    host: String,
    status: &'static str,
    open: bool,
}

/// What the thread shows of the browser module; a change redraws it.
#[derive(Clone, Debug, Default, PartialEq)]
struct View {
    waiting: bool,
    indicator: Option<Indicator>,
    tabs: Vec<Tab>,
    connected: bool,
    tabs_in_monocode: bool,
}

#[derive(Default)]
pub(super) struct State {
    view: View,
    answering: bool,
    error: Option<String>,
    menu: bool,
    anchor: AnchorSlot,
    menu_error: Option<String>,
    /// The tab "Point at an element" waits on.
    picking: Option<String>,
    free: Option<Vec<FreeTab>>,
    loading_free: bool,
    apps: Vec<DevServer>,
    _apps: Option<gpui::Task<()>>,
}

impl State {
    pub(super) fn new(session: i64, cx: &gpui::App) -> Self {
        State {
            view: view_of(session, crate::browser::state(cx).as_ref()),
            ..State::default()
        }
    }
}

fn view_of(session: i64, state: Option<&BrowserState>) -> View {
    let Some(state) = state else {
        return View::default();
    };
    let key = crate::browser::session_key(session);
    View {
        waiting: crate::browser::turn::request_pending(state, &key),
        indicator: browser_view::indicator(state, &key),
        tabs: browser_view::session_tabs(state, &key)
            .map(|tab| Tab {
                id: tab.tab.tab.0.clone(),
                title: if tab.tab.title.is_empty() {
                    browser_view::tab_host(&tab.tab.url)
                } else {
                    tab.tab.title.clone()
                },
                host: browser_view::tab_host(&tab.tab.url),
                status: browser_view::tab_status(tab),
                open: matches!(
                    tab.tab.state,
                    monocode_core::browser::protocol::TabState::Open
                ),
            })
            .collect(),
        connected: state.toggle_on
            && state.connection.state == monocode_core::browser::ConnectionState::Connected,
        tabs_in_monocode: state.settings.tabs_in_monocode,
    }
}

impl Conversation {
    /// Follow the browser module's state for this thread.
    pub(super) fn observe_browser(cx: &mut Context<Self>) -> Option<Subscription> {
        let browser = crate::browser::get(cx)?;
        Some(cx.observe(&browser, |this, browser, cx| {
            let view = view_of(this.summary.id, browser.read(cx).state.as_ref());
            if view == this.browser.view {
                return;
            }
            // "Show agent tabs: In MonoCode": a tab the agent just opened
            // shows beside the thread.
            if view.tabs_in_monocode {
                for tab in view.tabs.iter().filter(|tab| tab.open) {
                    let known = this.browser.view.tabs.iter().any(|old| old.id == tab.id);
                    if !known {
                        this.show_browser_tab(tab.id.clone(), cx);
                    }
                }
            }
            if !view.waiting {
                this.browser.error = None;
            }
            if view.indicator.is_none() && this.browser.menu {
                this.browser.menu = false;
            }
            this.browser.view = view;
            cx.notify();
        }))
    }

    fn browser_key(&self) -> String {
        crate::browser::session_key(self.summary.id)
    }

    fn answer_browser(&mut self, allow: bool, cx: &mut Context<Self>) {
        let key = self.browser_key();
        let Some(task) = crate::browser::query(cx, move |host| {
            host.answer_request(&key, crate::browser::WINDOW, allow);
            Ok(())
        }) else {
            return;
        };
        self.browser.answering = true;
        self.browser.error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.browser.answering = false;
                this.browser.error = result.err();
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn browser_shelf(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.browser.view.waiting {
            return div().into_any_element();
        }
        let busy = self.browser.answering;
        let button = |id: &'static str, label: &'static str, strong: bool| {
            div()
                .id(id)
                .debug_selector(move || id.into())
                .role(Role::Button)
                .aria_label(label)
                .flex_shrink_0()
                .h(px(24.))
                .px(px(6.))
                .rounded_md()
                .flex()
                .items_center()
                .text_color(content(if strong { 1. } else { 0.55 }))
                .child(label)
                .when(busy, |d| d.opacity(0.5))
                .when(!busy, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        };
        let message = self
            .browser
            .error
            .clone()
            .unwrap_or_else(|| "This agent wants to use the browser.".into());
        composer_shelf::shelf("browser-request-shelf")
            .debug_selector(|| "browser-request".into())
            .role(Role::Status)
            .child(
                div()
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_t(px(10.))
                    .border_1()
                    .border_b_0()
                    .border_color(teal(0.25))
                    .bg(teal(0.1))
                    .px_2()
                    .child(icon("globe", 14.).text_color(teal(1.)))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .text_color(if self.browser.error.is_some() {
                                gpui::rgb(0xf87171).into()
                            } else {
                                content(0.85)
                            })
                            .child(message),
                    )
                    .child(
                        button("browser-allow", "Allow for this thread", true).when(!busy, |d| {
                            d.on_click(cx.listener(|this, _, _, cx| this.answer_browser(true, cx)))
                        }),
                    )
                    .child(
                        button("browser-not-now", "Not now", false).when(!busy, |d| {
                            d.on_click(cx.listener(|this, _, _, cx| this.answer_browser(false, cx)))
                        }),
                    ),
            )
            .into_any_element()
    }

    /// The globe beside the composer's pills, while the thread has access.
    pub(super) fn browser_pill(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let indicator = self.browser.view.indicator?;
        let color = match indicator {
            Indicator::Dialog => gpui::rgb(0xfbbf24).into(),
            _ => teal(0.9),
        };
        Some(
            tool_pill("composer-browser", "Browser")
                .aria_label(indicator.label())
                .aria_expanded(self.browser.menu)
                .when(self.browser.menu, |d| d.bg(content(0.2)))
                .w(px(26.))
                .justify_center()
                .child(self.browser.anchor.probe())
                .child(icon("globe", 14.).text_color(color))
                .when(indicator == Indicator::Dialog, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(4.))
                            .right(px(4.))
                            .size(px(6.))
                            .rounded_full()
                            .bg(gpui::rgb(0xf59e0b)),
                    )
                })
                .on_click(cx.listener(|this, _, _, cx| this.toggle_browser_menu(cx)))
                .into_any_element(),
        )
    }

    fn toggle_browser_menu(&mut self, cx: &mut Context<Self>) {
        self.browser.menu = !self.browser.menu;
        self.browser.menu_error = None;
        if self.browser.menu {
            self.watch_branch_apps(cx);
        } else {
            self.browser._apps = None;
            self.browser.free = None;
        }
        cx.notify();
    }

    /// The pages this thread's checkout serves, read while the menu is open
    /// (a dev server may be starting).
    fn watch_branch_apps(&mut self, cx: &mut Context<Self>) {
        let key = self.browser_key();
        self.browser._apps = Some(cx.spawn(async move |this, cx| {
            loop {
                let key = key.clone();
                let apps = cx
                    .background_executor()
                    .spawn(async move {
                        monocode_core::browser::devservers::for_session(&key)
                            .into_iter()
                            .filter(|app| app.page)
                            .collect::<Vec<_>>()
                    })
                    .await;
                let open = this.update(cx, |this, cx| {
                    if this.browser.apps != apps {
                        this.browser.apps = apps;
                        cx.notify();
                    }
                    this.browser.menu
                });
                if !open.unwrap_or(false) {
                    break;
                }
                cx.background_executor().timer(APPS_EVERY).await;
            }
        }));
    }

    /// Run `work` on the host off the UI thread; an error shows in the menu.
    fn browser_action(
        &mut self,
        cx: &mut Context<Self>,
        close: bool,
        work: impl FnOnce(&std::sync::Arc<monocode_core::browser::BrowserHost>) -> Result<(), String>
        + Send
        + 'static,
    ) {
        let Some(task) = crate::browser::query(cx, work) else {
            return;
        };
        self.browser.menu_error = None;
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) if close => this.browser.menu = false,
                    Ok(()) => {}
                    Err(error) => this.browser.menu_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// "Show in MonoCode": the tab live beside the thread.
    fn show_browser_tab(&mut self, tab: String, cx: &mut Context<Self>) {
        cx.emit(OpenSessionChanges(crate::review::ReviewTab::Browser {
            session: self.summary.id,
            tab,
            own: false,
        }));
    }

    /// "Point at an element": the browser highlights what the pointer is
    /// over until the user clicks; the element's line goes into the message
    /// and its picture along with it.
    fn pick_browser_element(&mut self, tab: String, cx: &mut Context<Self>) {
        if self.browser.picking.as_deref() == Some(tab.as_str()) {
            self.cancel_browser_pick(cx);
            return;
        }
        let key = self.browser_key();
        let in_pane = self.browser.view.tabs_in_monocode;
        if in_pane {
            self.show_browser_tab(tab.clone(), cx);
        }
        let picked = tab.clone();
        let Some(task) = crate::browser::query(cx, move |host| {
            host.pick_element(&key, &TabId(picked), in_pane)
        }) else {
            return;
        };
        self.browser.picking = Some(tab.clone());
        self.browser.menu_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if this.browser.picking.as_deref() == Some(tab.as_str()) {
                    this.browser.picking = None;
                }
                match result {
                    Ok(Some(picked)) => {
                        this.browser.menu = false;
                        this.insert_browser_pick(picked, cx);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        this.browser.menu_error = Some(error);
                        this.browser.menu = true;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn cancel_browser_pick(&mut self, cx: &mut Context<Self>) {
        self.browser.picking = None;
        if let Some(host) = monocode_core::browser::host() {
            cx.background_executor()
                .spawn(async move { host.cancel_pick() })
                .detach();
        }
        cx.notify();
    }

    /// The picked element's line at the end of the draft, its picture as an
    /// attachment.
    fn insert_browser_pick(&mut self, picked: PickedElement, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            let draft = input.content.to_string();
            let separator = if draft.is_empty() || draft.ends_with(char::is_whitespace) {
                ""
            } else {
                " "
            };
            input.set_text(format!("{draft}{separator}{} ", picked.text.trim()), cx);
        });
        let Some(image) = picked.image else {
            return;
        };
        let store = self.store.clone();
        let id = self.summary.id;
        let work = cx.background_executor().spawn(async move {
            use base64::Engine as _;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(image)
                .map_err(|e| e.to_string())?;
            let root = locked(&store)?
                .thread_attachments_dir(id)?
                .unwrap_or_else(|| std::env::temp_dir().join("monocode-attachments"));
            std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
            let path = root.join(format!(
                "picked-element-{}.png",
                monocode_core::mono::new_id()
            ));
            std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            Ok::<_, String>(path)
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(path) => this.add_attachments(vec![path], cx),
                Err(error) => {
                    this.attachment_error = Some(error);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn load_free_browser_tabs(&mut self, cx: &mut Context<Self>) {
        let Some(task) = crate::browser::query(cx, |host| host.user_tabs()) else {
            return;
        };
        self.browser.loading_free = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.browser.loading_free = false;
                match result {
                    Ok(tabs) => this.browser.free = Some(tabs),
                    Err(error) => this.browser.menu_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The X beside "Add": close that page in the browser. With the last
    /// page gone, the MonoCode browser closes too.
    fn close_free_browser_tab(&mut self, free: String, cx: &mut Context<Self>) {
        let closed = free.clone();
        if let Some(tabs) = &mut self.browser.free {
            tabs.retain(|tab| tab.id != closed);
        }
        self.browser_action(cx, false, move |host| host.close_free_tab(&free));
    }

    fn add_free_browser_tab(&mut self, free: String, cx: &mut Context<Self>) {
        let key = self.browser_key();
        let given = free.clone();
        if let Some(tabs) = &mut self.browser.free {
            tabs.retain(|tab| tab.id != given);
        }
        self.browser_action(cx, false, move |host| {
            host.give_tab(&free, &key, crate::browser::WINDOW)
                .map(|_| ())
        });
    }

    /// The globe's menu: the thread's tabs and what to do with them.
    pub(super) fn browser_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.browser.menu {
            return None;
        }
        let view = &self.browser.view;
        let indicator = view.indicator?;
        let small = |id: SharedString, label: &'static str| {
            crate::settings::widgets::secondary_button(id, label, false, false).child(label)
        };
        let key = self.browser_key();
        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .px_1()
            .pb_2()
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_size(px(12.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(content(1.))
                    .child(indicator.label()),
            )
            .child(
                small("browser-turn-off".into(), "Turn off").on_click(cx.listener(
                    move |this, _, _, cx| {
                        let key = key.clone();
                        this.browser_action(cx, true, move |host| {
                            host.set_grant(&key, crate::browser::WINDOW, false);
                            Ok(())
                        });
                    },
                )),
            );
        let rows: Vec<AnyElement> = view
            .tabs
            .iter()
            .map(|tab| {
                let picking = self.browser.picking.as_deref() == Some(tab.id.as_str());
                let (pick, show, release, close) = (
                    tab.id.clone(),
                    tab.id.clone(),
                    tab.id.clone(),
                    tab.id.clone(),
                );
                let icon_button = menu_icon_button;
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded_lg()
                    .bg(content(0.03))
                    .px(px(10.))
                    .py(px(6.))
                    .text_size(px(12.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_color(content(1.))
                                    .child(tab.title.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_color(content(0.45))
                                    .child(format!("{} · {}", tab.host, tab.status)),
                            ),
                    )
                    .when(tab.open, |d| {
                        d.child(
                            icon_button(
                                format!("browser-pick-{}", tab.id),
                                "Point at an element",
                                "cursor-magic",
                            )
                            .when(picking, |d| d.bg(teal(0.2)))
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.pick_browser_element(pick.clone(), cx),
                            )),
                        )
                        .child(
                            icon_button(
                                format!("browser-show-{}", tab.id),
                                "Show in MonoCode",
                                "app-window",
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.browser.menu = false;
                                    this.show_browser_tab(show.clone(), cx);
                                    cx.notify();
                                },
                            )),
                        )
                        .child(
                            icon_button(
                                format!("browser-release-{}", tab.id),
                                "Take back the tab; it stays open for you",
                                "logout-01",
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    let tab = TabId(release.clone());
                                    this.browser_action(cx, false, move |host| {
                                        host.release_tab(&tab)
                                    });
                                },
                            )),
                        )
                    })
                    .child(
                        icon_button(format!("browser-close-{}", tab.id), "Close tab", "x")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let tab = TabId(close.clone());
                                this.browser_action(cx, false, move |host| {
                                    host.close_tab_by_user(&tab)
                                });
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        let apps: Vec<AnyElement> = self
            .browser
            .apps
            .iter()
            .map(|app| {
                let url = app.url.clone();
                let key = self.browser_key();
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_lg()
                    .bg(content(0.03))
                    .px(px(10.))
                    .py(px(6.))
                    .text_size(px(12.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_color(content(1.))
                                    .child(format!("localhost:{}", app.port)),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_color(content(0.45))
                                    .child(app.process.clone()),
                            ),
                    )
                    .child(
                        small(format!("browser-app-{}", app.port).into(), "Open in a tab")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let (url, key) = (url.clone(), key.clone());
                                this.browser_action(cx, true, move |host| {
                                    host.open_app(&key, crate::browser::WINDOW, &url)
                                        .map(|_| ())
                                });
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        let free = self.browser.free.clone();
        let loading_free = self.browser.loading_free;
        let free_label = if free.is_some() {
            "Refresh"
        } else {
            "Add a tab from the browser…"
        };
        let weak = cx.entity().downgrade();
        let body = div()
            .id("browser-menu")
            .debug_selector(|| "browser-menu".into())
            .role(Role::Dialog)
            .aria_label("Browser tabs of this thread")
            .max_h(px(360.))
            .overflow_y_scroll()
            .p_2()
            .child(header)
            .map(|d| {
                if rows.is_empty() {
                    d.child(
                        div()
                            .px_1()
                            .pb_1()
                            .text_size(px(12.))
                            .text_color(content(0.45))
                            .child("The agent hasn't opened a tab yet."),
                    )
                } else {
                    d.child(div().flex().flex_col().gap_1().children(rows))
                }
            })
            .when(!apps.is_empty(), |d| {
                d.child(
                    div()
                        .px_1()
                        .pt_2()
                        .child(
                            div()
                                .pb_1()
                                .text_size(px(12.))
                                .text_color(content(0.45))
                                .child("This branch's app"),
                        )
                        .child(div().flex().flex_col().gap_1().children(apps)),
                )
            })
            .when(self.browser.picking.is_some(), |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_1()
                        .pt_2()
                        .text_size(px(12.))
                        .text_color(content(0.65))
                        .child(div().min_w_0().flex_1().child(if view.tabs_in_monocode {
                            "Click the element in the tab beside the thread. The agent gets it in your message."
                        } else {
                            "Click the element in the browser window. The agent gets it in your message."
                        }))
                        .child(small("browser-pick-cancel".into(), "Cancel").on_click(
                            cx.listener(|this, _, _, cx| this.cancel_browser_pick(cx)),
                        )),
                )
            })
            .when(view.connected, |d| {
                d.child(
                    div()
                        .px_1()
                        .pt_2()
                        .child(
                            crate::settings::widgets::secondary_button(
                                "browser-free-tabs",
                                free_label,
                                false,
                                loading_free,
                            )
                            .when(free.is_some(), |d| d.child(icon("refresh-cw", 14.)))
                            .child(free_label)
                            .when(!loading_free, |d| {
                                d.on_click(
                                    cx.listener(|this, _, _, cx| this.load_free_browser_tabs(cx)),
                                )
                            }),
                        )
                        .when_some(free, |d, free| {
                            if free.is_empty() {
                                return d.child(
                                    div()
                                        .mt_2()
                                        .text_size(px(12.))
                                        .text_color(content(0.45))
                                        .child(NO_FREE_TABS),
                                );
                            }
                            d.child(div().mt_2().flex().flex_col().gap_1().children(
                                free.into_iter().map(|tab| {
                                    let id = tab.id.clone();
                                    let host = browser_view::tab_host(&tab.url);
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .rounded_lg()
                                        .bg(content(0.03))
                                        .px(px(10.))
                                        .py(px(6.))
                                        .text_size(px(12.))
                                        .child(
                                            div()
                                                .min_w_0()
                                                .flex_1()
                                                .child(
                                                    div().truncate().text_color(content(1.)).child(
                                                        if tab.title.is_empty() {
                                                            host.clone()
                                                        } else {
                                                            tab.title.clone()
                                                        },
                                                    ),
                                                )
                                                .child(
                                                    div()
                                                        .truncate()
                                                        .text_color(content(0.45))
                                                        .child(host),
                                                ),
                                        )
                                        .child({
                                            let id = id.clone();
                                            small(format!("browser-add-{}", tab.id).into(), "Add")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.add_free_browser_tab(id.clone(), cx)
                                                }))
                                        })
                                        .child(
                                            menu_icon_button(
                                                format!("browser-close-free-{}", tab.id),
                                                "Close tab",
                                                "x",
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.close_free_browser_tab(id.clone(), cx)
                                            })),
                                        )
                                }),
                            ))
                        }),
                )
            })
            .when_some(self.browser.menu_error.clone(), |d, error| {
                d.child(
                    div()
                        .px_1()
                        .pt_2()
                        .text_size(px(12.))
                        .text_color(gpui::rgb(0xf87171))
                        .child(error),
                )
            });
        Some(
            popover(
                "composer-browser-menu",
                Anchor::Slot(self.browser.anchor.clone()),
            )
            .side(Side::Top)
            .align(Align::End)
            .width(340.)
            .on_dismiss(move |_, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.browser.menu = false;
                    this.browser._apps = None;
                    this.browser.free = None;
                    cx.notify();
                });
            })
            .child(body)
            .into_any_element(),
        )
    }
}

/// A small icon button in the browser menu's rows.
fn menu_icon_button(
    id: String,
    label: &'static str,
    path: &'static str,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(SharedString::from(id))
        .role(Role::Button)
        .aria_label(label)
        .size(px(24.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .hover(|s| s.bg(content(0.1)))
        .child(icon(path, 14.).text_color(content(0.6)))
}
