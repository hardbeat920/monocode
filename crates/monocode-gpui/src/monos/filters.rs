use super::*;
use crate::popover::{MenuAction, MenuItem};
use serde::{Deserialize, Serialize};
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(super) struct Filters {
    pub show_archived: bool,
    pub show_unavailable: bool,
    pub hidden_harnesses: Vec<String>,
    pub time: String,
    pub working: bool,
    pub needs_approval: bool,
    pub done: bool,
}
impl Filters {
    pub(super) fn matches(
        &self,
        launch: &mono::Launch,
        view: Option<&crate::shell::monos::MonoSession>,
    ) -> bool {
        if self.show_unavailable {
            return view.is_none();
        }
        let Some(view) = view else {
            return false;
        };
        let now = monocode_core::time::now_ms();
        let since = match self.time.as_str() {
            "today" => chrono::Local::now()
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .and_then(|d| chrono::TimeZone::from_local_datetime(&chrono::Local, &d).earliest())
                .map(|d| d.timestamp_millis().max(0) as u64)
                .unwrap_or(0),
            "7d" => now.saturating_sub(7 * 86400000),
            "30d" => now.saturating_sub(30 * 86400000),
            _ => 0,
        };
        (!self.show_archived || view.archived)
            && !self.hidden_harnesses.contains(&launch.harness)
            && view.updated_at >= since
            && (!(self.working || self.needs_approval || self.done)
                || self.working && view.busy
                || self.needs_approval && view.needs_input
                || self.done && !view.busy && !view.needs_input && view.has_messages)
    }
}
impl Monos {
    pub(super) fn filter_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let f = &self.session_filters;
        let mut items: Vec<MenuItem> = vec![
            MenuAction::new("archived", "Show archived")
                .checked(f.show_archived)
                .into(),
            MenuAction::new("unavailable", "Unavailable sessions")
                .checked(f.show_unavailable)
                .into(),
            MenuItem::Separator,
        ];
        for (id, label) in [
            ("all", "All time"),
            ("today", "Today"),
            ("7d", "Last 7 days"),
            ("30d", "Last 30 days"),
        ] {
            items.push(
                MenuAction::new(format!("time:{id}"), label)
                    .checked(if id == "all" {
                        f.time.is_empty() || f.time == "all"
                    } else {
                        f.time == id
                    })
                    .into(),
            );
        }
        items.push(MenuItem::Separator);
        for (id, label, on) in [
            ("working", "Working", f.working),
            ("approval", "Needs approval", f.needs_approval),
            ("done", "Done", f.done),
        ] {
            items.push(MenuAction::new(id, label).checked(on).into());
        }
        items.push(MenuItem::Separator);
        for kind in Kind::ALL {
            if self
                .current()
                .is_some_and(|m| m.launches.iter().any(|l| l.harness == kind.id()))
            {
                items.push(
                    MenuAction::new(format!("provider:{}", kind.id()), kind.label())
                        .checked(!f.hidden_harnesses.iter().any(|h| h == kind.id()))
                        .into(),
                );
            }
        }
        items.push(MenuItem::Separator);
        items.push(MenuAction::new("reset", "Reset filters").into());
        let menu = cx.new(|cx| Menu::new("Filter Mono sessions", items, window, cx).width(228.));
        let events = cx.subscribe(&menu, |this, _, event: &MenuEvent, cx| {
            if let MenuEvent::Picked(id, _) = event {
                let f = &mut this.session_filters;
                match id.as_ref() {
                    "archived" => f.show_archived = !f.show_archived,
                    "unavailable" => f.show_unavailable = !f.show_unavailable,
                    "working" => f.working = !f.working,
                    "approval" => f.needs_approval = !f.needs_approval,
                    "done" => f.done = !f.done,
                    "reset" => *f = Filters::default(),
                    id if id.starts_with("time:") => f.time = id[5..].into(),
                    id if id.starts_with("provider:") => {
                        let id = id[9..].to_string();
                        if f.hidden_harnesses.contains(&id) {
                            f.hidden_harnesses.retain(|h| h != &id);
                        } else {
                            f.hidden_harnesses.push(id);
                        }
                    }
                    _ => (),
                };
                let store = this.store.clone();
                let text = serde_json::to_string(f).unwrap_or_default();
                cx.background_executor()
                    .spawn(async move {
                        let _ = locked(&store)
                            .and_then(|db| db.save_setting("mono-session-filters", &text));
                    })
                    .detach();
            }
            this.picker = None;
            cx.notify();
        });
        self.picker = Some(OpenPicker {
            menu,
            anchor: Anchor::Slot(self.session_filter_anchor.clone()),
            width: 228.,
            align: crate::popover::Align::Start,
            _events: vec![events],
        });
        cx.notify();
    }
}
