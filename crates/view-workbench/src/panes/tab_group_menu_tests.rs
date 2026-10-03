//! Port of TabGroupMenu.test.ts. The first case checked CSS (no internal
//! scroll region, opens to the right); here it checks the frame opens with
//! every row drawn.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, px};

use super::*;
use crate::panes::test_support::{draw, init};

/// `notificationMuteActions()`.
fn mute_actions() -> Vec<SubmenuEntry> {
    [
        ("mute:1", "1 hour"),
        ("mute:8", "8 hours"),
        ("mute:24", "1 day"),
        ("mute:forever", "Until I turn it back on"),
    ]
    .into_iter()
    .map(|(id, label)| SubmenuEntry::Item {
        id: id.into(),
        label: label.into(),
        disabled: false,
    })
    .collect()
}

fn props() -> TabGroupMenuProps {
    let mut mute = TabGroupMenuExtraItem::new(
        "notifications-mute",
        "Mute notifications",
        IconName::BellOff,
    );
    mute.submenu = Some(mute_actions());
    TabGroupMenuProps {
        position: point(px(20.), px(20.)),
        group_id: "private".into(),
        label: "Private".into(),
        current_color: "#7c3aed".into(),
        mascot_project: "private".into(),
        leading_action: Some(TabGroupMenuExtraItem::new(
            "notifications-resume",
            "Resume notifications",
            IconName::BellOff,
        )),
        extra_items: vec![mute],
        ..TabGroupMenuProps::default()
    }
}

struct Harness<'a> {
    menu: Entity<TabGroupMenu>,
    events: Rc<RefCell<Vec<TabGroupMenuEvent>>>,
    cx: &'a mut VisualTestContext,
}

fn mount(cx: &mut TestAppContext) -> Harness<'_> {
    cx.update(init);
    let (menu, cx) = cx.add_window_view(|window, cx| {
        let mut menu = TabGroupMenu::new(props(), window, cx);
        menu.set_animate(false);
        menu
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&menu, move |_, event: &TabGroupMenuEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach();
    });
    draw(cx);
    Harness { menu, events, cx }
}

impl Harness<'_> {
    fn center(&mut self, selector: String) -> gpui::Point<Pixels> {
        let selector: &'static str = Box::leak(selector.into_boxed_str());
        self.cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("no element {selector}"))
            .center()
    }

    fn hover(&mut self, row: &str) {
        let at = self.center(format!("tab-group-row:{row}"));
        self.cx.simulate_mouse_move(at, None, Modifiers::none());
        draw(self.cx);
    }

    fn move_to(&mut self, at: gpui::Point<Pixels>) {
        self.cx.simulate_mouse_move(at, None, Modifiers::none());
        draw(self.cx);
    }

    fn submenu_open(&self) -> bool {
        self.menu
            .read_with(self.cx, |menu, _| menu.open_submenu().is_some())
    }

    fn wait(&mut self, ms: u64) {
        self.cx.executor().advance_clock(Duration::from_millis(ms));
        draw(self.cx);
    }
}

#[gpui::test]
fn fully_expands_the_context_menu(cx: &mut TestAppContext) {
    let h = mount(cx);
    for row in [
        "notifications-resume",
        "new-tab",
        "new-window",
        "close-group",
        "ungroup",
        "delete-group",
        "notifications-mute",
    ] {
        assert!(
            h.cx.debug_bounds(Box::leak(format!("tab-group-row:{row}").into_boxed_str()))
                .is_some(),
            "{row}"
        );
    }
    assert!(h.cx.debug_bounds("tab-group-name").is_some());
    assert_eq!(h.menu.read_with(h.cx, |menu, cx| menu.name(cx)), "Private");
}

#[gpui::test]
fn closes_the_mute_submenu_when_the_pointer_enters_the_leading_action(cx: &mut TestAppContext) {
    let mut h = mount(cx);
    h.hover("notifications-mute");
    assert!(h.submenu_open());
    assert!(h.cx.debug_bounds("tab-group-submenu").is_some());
    h.hover("notifications-resume");
    assert!(!h.submenu_open());
}

#[gpui::test]
fn closes_the_mute_submenu_on_a_standard_action_and_allows_reopening_it(cx: &mut TestAppContext) {
    let mut h = mount(cx);
    h.hover("notifications-mute");
    h.hover("new-tab");
    assert!(!h.submenu_open());
    h.hover("notifications-mute");
    assert!(h.submenu_open());
    let item = h.center("tab-group-submenu-item:mute:8".into());
    h.move_to(item);
    h.cx.simulate_click(item, Modifiers::none());
    draw(h.cx);
    assert!(
        h.events
            .borrow()
            .contains(&TabGroupMenuEvent::ExtraPick("mute:8".into()))
    );
    assert_eq!(h.events.borrow().last(), Some(&TabGroupMenuEvent::Close));
}

#[gpui::test]
fn closes_only_the_submenu_after_the_pointer_leaves_both_panels(cx: &mut TestAppContext) {
    for leave_from_main in [true, false] {
        let mut h = mount(&mut *cx);
        h.hover("notifications-mute");
        let main = h.cx.debug_bounds("tab-group-menu").unwrap();
        let submenu = h.cx.debug_bounds("tab-group-submenu").unwrap();
        let outside = point(
            main.origin.x + main.size.width + px(2.),
            main.origin.y + main.size.height + px(40.),
        );
        let in_main = point(main.origin.x + px(20.), main.origin.y + px(80.));
        let in_submenu = point(submenu.center().x, submenu.origin.y + px(6.));

        h.move_to(outside);
        h.wait(100);
        h.move_to(in_submenu);
        h.wait(200);
        assert!(h.submenu_open());

        h.move_to(outside);
        h.wait(100);
        h.move_to(in_main);
        h.wait(200);
        assert!(h.submenu_open());

        h.move_to(if leave_from_main { in_main } else { in_submenu });
        h.move_to(outside);
        h.wait(179);
        assert!(h.submenu_open());
        h.wait(1);
        assert!(!h.submenu_open());
        assert!(h.cx.debug_bounds("tab-group-menu").is_some());
        assert!(!h.events.borrow().contains(&TabGroupMenuEvent::Close));
    }
}

#[gpui::test]
fn picks_colors_mascots_and_group_actions(cx: &mut TestAppContext) {
    let mut h = mount(cx);
    let color = h.center("tab-group-row:delete-group".into());
    h.cx.simulate_click(color, Modifiers::none());
    draw(h.cx);
    assert!(
        h.events
            .borrow()
            .contains(&TabGroupMenuEvent::Pick(TabGroupMenuAction::DeleteGroup))
    );
    assert_eq!(
        parse_css_color("hsl(0 100% 50%)"),
        Some(monocode_ui::color::hex(0xff0000))
    );
    assert_eq!(
        parse_css_color("#00ff00"),
        Some(monocode_ui::color::hex(0x00ff00))
    );
    assert_eq!(parse_css_color("tomato"), None);
}
