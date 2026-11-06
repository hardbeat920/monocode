//! The searchable, two-level trigger picker from AutomationsView.
//! Connection state comes from the retained Inbox snapshot; opening this menu
//! never authenticates a provider or performs a network request.
use super::*;
use crate::popover::{Menu, MenuAction, MenuEvent, MenuItem, Popover, menu_popover};

const CATEGORIES: [(&str, &str, &str); 6] = [
    ("time", "Scheduled", "clock"),
    ("github", "GitHub", "inbox-github"),
    ("linear", "Linear", "inbox-linear"),
    ("jira", "Jira", "inbox-jira"),
    ("gitlab", "GitLab", "inbox-gitlab"),
    ("azuredevops", "Azure DevOps", "inbox-azuredevops"),
];

struct SearchHeader {
    input: Entity<Input>,
}
impl Render for SearchHeader {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(44.))
            .mx(px(-4.))
            .mt(px(-4.))
            .mb(px(4.))
            .px(px(12.))
            .border_b_1()
            .border_color(content(0.1))
            .flex()
            .items_center()
            .gap(px(10.))
            .child(icon("search", 14.).text_color(content(0.45)))
            .child(div().flex_1().min_w_0().child(self.input.clone()))
    }
}

pub(super) struct TriggerMenu {
    search: Entity<SearchHeader>,
    menu: Entity<Menu>,
    connected: HashMap<String, bool>,
    _subscriptions: Vec<Subscription>,
}
impl EventEmitter<MenuEvent> for TriggerMenu {}
impl TriggerMenu {
    fn items(
        search: &Entity<SearchHeader>,
        connected: &HashMap<String, bool>,
        cx: &App,
    ) -> Vec<MenuItem> {
        let header = search.read(cx);
        let query = header.input.read(cx).content.to_lowercase();
        let mut items = vec![MenuItem::Custom(
            search.clone().into(),
            header.input.read(cx).focus_handle(cx),
        )];
        for (kind, label, mark) in CATEGORIES {
            if !label.to_lowercase().contains(&query) {
                continue;
            }
            let ready = kind == "time" || connected.get(kind).copied().unwrap_or(false);
            let color = match kind {
                "jira" => rgb(0x2684ff).into(),
                "gitlab" => rgb(0xfc6d26).into(),
                _ => content(if ready { 0.8 } else { 0.35 }),
            };
            let action = MenuAction::new(kind, label).icon(mark).icon_color(color);
            let action = if kind == "gitlab" {
                action.icon_artwork("inbox-gitlab-color.svg")
            } else {
                action
            };
            let action = if ready {
                action.submenu(
                    editor::events(kind)
                        .into_iter()
                        .map(|(value, label)| MenuAction::new(format!("{kind}:{value}"), label))
                        .collect(),
                )
            } else {
                action.disabled(true).value("Not connected")
            };
            items.push(action.into());
        }
        if items.len() == 1 {
            items.push(MenuItem::Note("No matching triggers".into()));
        }
        items
    }
    pub(super) fn new(
        connected: HashMap<String, bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input =
            cx.new(|cx| Input::field("Search triggers", "Search triggers", content(0.35), cx));
        let search = cx.new(|_| SearchHeader {
            input: input.clone(),
        });
        let items = Self::items(&search, &connected, cx);
        let menu = cx.new(|cx| {
            Menu::new("Choose automation trigger", items, window, cx)
                .width(250.)
                .row_metrics(36., 14.)
                .search_navigation()
                .submenu_width(220.)
        });
        let changed = cx.subscribe(&input, |this, _, _: &input::Changed, cx| this.refresh(cx));
        let picked = cx.subscribe(&menu, |_, _, event: &MenuEvent, cx| {
            cx.emit(match event {
                MenuEvent::Picked(id, bounds) => MenuEvent::Picked(id.clone(), *bounds),
                MenuEvent::Dismissed => MenuEvent::Dismissed,
            });
        });
        Self {
            search,
            menu,
            connected,
            _subscriptions: vec![changed, picked],
        }
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let items = Self::items(&self.search, &self.connected, cx);
        self.menu
            .update(cx, |menu, cx| menu.replace_items(items, cx));
        cx.notify();
    }
    pub(super) fn set_connections(
        &mut self,
        connected: HashMap<String, bool>,
        cx: &mut Context<Self>,
    ) {
        if self.connected != connected {
            self.connected = connected;
            self.refresh(cx);
        }
    }
    pub(super) fn popover(&self, anchor: Anchor, cx: &App) -> Popover {
        menu_popover(&self.menu, anchor, cx).gap(4.)
    }
}

impl Automations {
    pub(crate) fn set_trigger_connections(
        &mut self,
        connected: HashMap<String, bool>,
        cx: &mut Context<Self>,
    ) {
        if let Some(menu) = &self.trigger_menu {
            menu.update(cx, |menu, cx| menu.set_connections(connected.clone(), cx));
        }
        self.trigger_connections = connected;
    }
    pub(crate) fn set_trigger_suggestions(&mut self, suggestions: HashMap<String, Vec<String>>) {
        self.trigger_suggestions = suggestions;
    }
    pub(super) fn open_trigger_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor.is_none()
            || self
                .editor
                .as_ref()
                .and_then(|editor| editor.draft.triggers.as_ref())
                .is_some_and(|triggers| triggers.len() >= 20)
        {
            return;
        }
        if self.trigger_menu.is_some() {
            self.trigger_menu = None;
            cx.notify();
            return;
        }
        let connected = self.trigger_connections.clone();
        let menu = cx.new(|cx| TriggerMenu::new(connected, window, cx));
        self.trigger_subscription = Some(cx.subscribe(&menu, |this, _, event: &MenuEvent, cx| {
            match event {
                MenuEvent::Picked(id, _) => {
                    this.apply_choice(Choice::AddTrigger, id.to_string(), cx)
                }
                MenuEvent::Dismissed => {}
            }
            this.trigger_menu = None;
            cx.notify();
        }));
        self.choice = None;
        self.models = None;
        self.project_picker = None;
        self.trigger_menu = Some(menu);
        cx.notify();
    }
}
