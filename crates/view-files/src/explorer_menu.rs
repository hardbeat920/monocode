//! Port of src/features/files/ui/ExplorerMenu.tsx: the explorer's context
//! menu, with keyboard navigation and one level of submenus.
//!
//! The row styles match `monocode_ui::widgets::menu`, which ports the same
//! component without state. This entity adds what the React component did
//! with state: the active row, hover tracking, submenus that open on hover
//! and close 180 ms after the pointer leaves, and focus.

use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Anchor, AnchoredPositionMode, AnyElement, App, Bounds, Context, EventEmitter, FocusHandle,
    Focusable, InteractiveElement, IntoElement, KeyBinding, MouseButton, ParentElement, Pixels,
    Point, Render, SharedString, StatefulInteractiveElement, Styled, Task, Window, actions,
    anchored, deferred, div, point, prelude::FluentBuilder as _, px,
};
use monocode_ui::widgets::popover_frame;
use monocode_ui::{IconName, Theme, UiStyled as _, color::with_alpha, icon, u};

/// `MENU_WIDTH`.
pub const MENU_WIDTH: f32 = 228.0;
/// How long a submenu stays open after the pointer leaves it.
pub const SUBMENU_CLOSE_DELAY: Duration = Duration::from_millis(180);

const KEY_CONTEXT: &str = "ExplorerMenu";

actions!(
    explorer_menu,
    [
        /// Highlight the next item.
        SelectNext,
        /// Highlight the previous item.
        SelectPrevious,
        /// Pick the highlighted item, or open its submenu.
        Confirm,
        /// Open the highlighted item's submenu.
        OpenSubmenu,
        /// Close the submenu, or go back to the menu that opened this one.
        CloseSubmenu,
        /// Close the menu.
        Cancel,
    ]
);

pub(crate) fn init(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, context),
        KeyBinding::new("up", SelectPrevious, context),
        KeyBinding::new("enter", Confirm, context),
        KeyBinding::new("space", Confirm, context),
        KeyBinding::new("right", OpenSubmenu, context),
        KeyBinding::new("left", CloseSubmenu, context),
        KeyBinding::new("escape", Cancel, context),
    ]);
}

/// `MenuAction`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuAction {
    pub id: SharedString,
    pub label: SharedString,
    pub description: Option<SharedString>,
    pub shortcut: Option<SharedString>,
    pub disabled: bool,
    pub danger: bool,
    /// `None` is a plain item; `Some` is a checkbox item.
    pub checked: Option<bool>,
}

impl MenuAction {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            ..Default::default()
        }
    }

    pub fn shortcut(mut self, shortcut: impl Into<SharedString>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }
}

/// `ExplorerMenuItem`.
#[derive(Clone, Debug, PartialEq)]
pub enum ExplorerMenuItem {
    Separator,
    Item {
        action: MenuAction,
        submenu: Vec<MenuAction>,
    },
}

impl ExplorerMenuItem {
    pub fn item(action: MenuAction) -> Self {
        Self::Item {
            action,
            submenu: Vec::new(),
        }
    }

    pub fn with_submenu(action: MenuAction, submenu: Vec<MenuAction>) -> Self {
        Self::Item { action, submenu }
    }

    pub fn action(&self) -> Option<&MenuAction> {
        match self {
            Self::Item { action, .. } => Some(action),
            Self::Separator => None,
        }
    }

    fn submenu(&self) -> &[MenuAction] {
        match self {
            Self::Item { submenu, .. } => submenu,
            Self::Separator => &[],
        }
    }
}

impl From<MenuAction> for ExplorerMenuItem {
    fn from(action: MenuAction) -> Self {
        Self::item(action)
    }
}

/// Where the menu opens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuAnchor {
    /// At a window point, like a context menu.
    Point(Point<Pixels>),
    /// To the right of an element, 4 px away.
    Beside(Bounds<Pixels>),
}

/// What the menu reports to its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExplorerMenuEvent {
    /// An enabled item or submenu item was picked.
    Pick(SharedString),
    /// Escape, or a click outside the menu.
    Dismiss,
    /// Left arrow with no submenu open, when the menu has a back target.
    Back,
    /// The pointer entered (`true`) or left (`false`) the menu.
    Hover(bool),
}

/// `itemIndexAt`: the first item at or after `start` in direction `dir`.
fn item_index_at(items: &[ExplorerMenuItem], start: usize, dir: isize) -> usize {
    let mut index = start as isize;
    while index >= 0 && (index as usize) < items.len() {
        if items[index as usize].action().is_some() {
            return index as usize;
        }
        index += dir;
    }
    start
}

type HeaderBuilder = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

struct SubmenuState {
    index: usize,
}

/// The menu entity. Render it from the owner while it is open.
pub struct ExplorerMenu {
    anchor: MenuAnchor,
    items: Vec<ExplorerMenuItem>,
    width: f32,
    header: Option<HeaderBuilder>,
    has_back: bool,
    animate: bool,
    active: usize,
    submenu: Option<SubmenuState>,
    submenu_active: Option<usize>,
    submenu_hovered: bool,
    close_timer: Option<Task<()>>,
    focus_handle: FocusHandle,
}

impl EventEmitter<ExplorerMenuEvent> for ExplorerMenu {}

impl Focusable for ExplorerMenu {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ExplorerMenu {
    /// A menu over `items`. It takes focus, like `autoFocus`.
    pub fn new(
        anchor: MenuAnchor,
        items: Vec<ExplorerMenuItem>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        Self {
            anchor,
            active: item_index_at(&items, 0, 1),
            items,
            width: MENU_WIDTH,
            header: None,
            has_back: false,
            animate: true,
            submenu: None,
            submenu_active: None,
            submenu_hovered: false,
            close_timer: None,
            focus_handle,
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Content above the items, followed by a separator.
    pub fn header(
        mut self,
        header: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.header = Some(Rc::new(header));
        self
    }

    /// Left arrow emits [`ExplorerMenuEvent::Back`] (`onBack`).
    pub fn with_back(mut self) -> Self {
        self.has_back = true;
        self
    }

    /// Turns the open animation off, for screenshots.
    pub fn animate(mut self, animate: bool) -> Self {
        self.animate = animate;
        self
    }

    pub fn items(&self) -> &[ExplorerMenuItem] {
        &self.items
    }

    /// The highlighted item's index in `items`.
    pub fn active(&self) -> usize {
        self.active
    }

    /// The open submenu's parent index, and its highlighted row.
    pub fn open_submenu(&self) -> Option<(usize, Option<usize>)> {
        self.submenu
            .as_ref()
            .map(|submenu| (submenu.index, self.submenu_active))
    }

    fn submenu_items(&self) -> Option<&[MenuAction]> {
        let submenu = self.submenu.as_ref()?;
        let items = self.items.get(submenu.index)?.submenu();
        (!items.is_empty()).then_some(items)
    }

    fn cancel_close(&mut self) {
        self.close_timer = None;
    }

    fn close_submenu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_close();
        self.submenu = None;
        self.submenu_active = None;
        self.submenu_hovered = false;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn schedule_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_close();
        self.close_timer = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(SUBMENU_CLOSE_DELAY).await;
            this.update_in(cx, |this, window, cx| {
                this.close_timer = None;
                this.close_submenu(window, cx);
            })
            .ok();
        }));
    }

    /// `move`: step through items, separators skipped, wrapping around.
    fn step(&mut self, dir: isize) {
        let ids: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.action().map(|_| index))
            .collect();
        if ids.is_empty() {
            return;
        }
        let from = ids.iter().position(|index| *index == self.active);
        let len = ids.len() as isize;
        let next = match from {
            Some(from) => (from as isize + dir + len) % len,
            None => (dir - 1 + len) % len,
        };
        self.active = ids[next as usize];
    }

    pub fn select_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        self.move_highlight(1, cx);
    }

    pub fn select_previous(&mut self, _: &SelectPrevious, _: &mut Window, cx: &mut Context<Self>) {
        self.move_highlight(-1, cx);
    }

    fn move_highlight(&mut self, dir: isize, cx: &mut Context<Self>) {
        if let Some(count) = self.submenu_items().map(<[MenuAction]>::len) {
            let count = count as isize;
            self.submenu_active = Some(match self.submenu_active {
                None if dir == 1 => 0,
                None => (count - 1) as usize,
                Some(current) => ((current as isize + dir + count) % count) as usize,
            });
        } else {
            self.step(dir);
        }
        cx.notify();
    }

    pub fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
        self.activate(false, window, cx);
    }

    pub fn open_submenu_action(
        &mut self,
        _: &OpenSubmenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.submenu_items().is_some() {
            return;
        }
        self.activate(true, window, cx);
    }

    /// Enter, Space, and ArrowRight.
    fn activate(&mut self, arrow_right: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(items) = self.submenu_items() {
            if arrow_right {
                return;
            }
            if let Some(item) = self.submenu_active.and_then(|index| items.get(index))
                && !item.disabled
            {
                cx.emit(ExplorerMenuEvent::Pick(item.id.clone()));
            }
            return;
        }
        let Some(ExplorerMenuItem::Item { action, submenu }) = self.items.get(self.active) else {
            return;
        };
        if action.disabled {
            return;
        }
        if !submenu.is_empty() {
            self.cancel_close();
            self.submenu = Some(SubmenuState { index: self.active });
            self.submenu_active = Some(0);
            cx.notify();
        } else if !arrow_right {
            let _ = window;
            cx.emit(ExplorerMenuEvent::Pick(action.id.clone()));
        }
    }

    pub fn close_submenu_action(
        &mut self,
        _: &CloseSubmenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.submenu_items().is_some() {
            self.close_submenu(window, cx);
        } else if self.has_back {
            cx.emit(ExplorerMenuEvent::Back);
        }
    }

    pub fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        if self.submenu.is_some() {
            self.close_submenu(window, cx);
        } else {
            cx.emit(ExplorerMenuEvent::Dismiss);
        }
    }

    fn hover_item(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_close();
        self.active = index;
        let opens = self
            .items
            .get(index)
            .and_then(|item| {
                item.action()
                    .map(|action| (action.disabled, item.submenu().len()))
            })
            .is_some_and(|(disabled, count)| !disabled && count > 0);
        if opens {
            if self.submenu.as_ref().is_none_or(|open| open.index != index) {
                self.submenu = Some(SubmenuState { index });
                self.submenu_active = None;
            }
        } else if self.submenu.is_some() {
            self.close_submenu(window, cx);
        }
        cx.notify();
    }

    fn click_item(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(ExplorerMenuItem::Item { action, submenu }) = self.items.get(index) else {
            return;
        };
        if action.disabled {
            return;
        }
        if submenu.is_empty() {
            cx.emit(ExplorerMenuEvent::Pick(action.id.clone()));
        } else {
            self.cancel_close();
            self.submenu = Some(SubmenuState { index });
            self.submenu_active = Some(0);
            cx.notify();
        }
    }

    fn render_row(
        &self,
        id: impl Into<gpui::ElementId>,
        action: &MenuAction,
        has_submenu: bool,
        highlighted: bool,
        cx: &App,
    ) -> gpui::Stateful<gpui::Div> {
        let theme = Theme::of(cx);
        let c = theme.colors;
        let (ink, fill, hover_fill) = if action.disabled {
            (theme.content(0.30), None, None)
        } else if action.danger {
            if highlighted {
                (c.danger_soft, Some(with_alpha(c.danger_fill, 0.2)), None)
            } else {
                (
                    with_alpha(c.danger_soft, 0.9),
                    None,
                    Some(with_alpha(c.danger_fill, 0.15)),
                )
            }
        } else if highlighted {
            (c.content, Some(c.selection), None)
        } else {
            (c.content, None, Some(theme.content(0.05)))
        };
        let mut label = div()
            .flex_1()
            .min_w_0()
            .child(div().truncate().child(action.label.clone()));
        if let Some(description) = action.description.clone() {
            label = label.child(
                div()
                    .mt(u(4.))
                    .text_px(theme.text.caption)
                    .leading(theme.leading.snug)
                    .text_color(theme.content(0.50))
                    .child(description),
            );
        }
        let trailing = if has_submenu {
            Some(
                icon(IconName::ChevronRight)
                    .size(u(14.))
                    .text_color(theme.content(0.50))
                    .into_any_element(),
            )
        } else if action.checked == Some(true) {
            Some(
                icon(IconName::Check)
                    .size(u(14.))
                    .text_color(ink)
                    .into_any_element(),
            )
        } else {
            action.shortcut.clone().map(|shortcut| {
                div()
                    .flex_none()
                    .text_px(theme.text.caption)
                    .text_color(theme.content(0.40))
                    .child(shortcut)
                    .into_any_element()
            })
        };
        div()
            .id(id)
            .flex()
            .w_full()
            .items_center()
            .gap(u(12.))
            .px(u(8.))
            .rounded(u(theme.radius.lg))
            .text_px(theme.text.body)
            .leading(theme.leading.none)
            .text_color(ink)
            .map(|row| {
                if action.description.is_some() {
                    row.py(u(6.))
                } else {
                    row.h(u(28.))
                }
            })
            .when_some(fill, |row, fill| row.bg(fill))
            .when_some(hover_fill, |row, fill| {
                row.hover(move |style| style.bg(fill))
            })
            .child(label)
            .children(trailing)
    }

    fn render_separator(&self, cx: &App) -> impl IntoElement {
        div().my(u(4.)).h(px(1.)).bg(Theme::of(cx).content(0.10))
    }

    fn render_submenu(
        &self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let open = self.submenu.as_ref()?;
        if open.index != index {
            return None;
        }
        let items = self.submenu_items()?.to_vec();
        let mut list = div().flex().flex_col().p(u(4.));
        for (sub_index, action) in items.iter().enumerate() {
            let highlighted = self.submenu_active == Some(sub_index);
            let disabled = action.disabled;
            let picked = action.id.clone();
            let row = self
                .render_row(("submenu-item", sub_index), action, false, highlighted, cx)
                .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                    if *hovered {
                        this.cancel_close();
                        this.submenu_active = Some(sub_index);
                        window.focus(&this.focus_handle, cx);
                        cx.notify();
                    }
                }))
                .when(!disabled, |row| {
                    row.on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(ExplorerMenuEvent::Pick(picked.clone()));
                    }))
                });
            list = list.child(row);
        }
        let rem = window.rem_size();
        // The row sits inside the menu's 4 px padding; the submenu opens 4 px
        // to the right of the menu and lines its first row up with this one.
        let offset = point(
            u(self.width - 4.0 + 4.0).to_pixels(rem),
            u(-4.0 - 1.0).to_pixels(rem),
        );
        let layer = Theme::of(cx).layer.submenu;
        let frame = popover_frame(("explorer-submenu", index))
            .width(MENU_WIDTH)
            .animate(self.animate)
            .child(list);
        Some(
            deferred(
                anchored()
                    .position_mode(AnchoredPositionMode::Local)
                    .position(offset)
                    .anchor(Anchor::TopLeft)
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        div()
                            .id("explorer-submenu-frame")
                            .on_hover(cx.listener(|this, hovered: &bool, window, cx| {
                                this.submenu_hovered = *hovered;
                                if *hovered {
                                    this.cancel_close();
                                } else {
                                    this.schedule_close(window, cx);
                                }
                            }))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(frame),
                    ),
            )
            .with_priority(layer)
            .into_any_element(),
        )
    }
}

impl Render for ExplorerMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let layer = theme.layer.popover;
        let mut list = div().flex().flex_col().p(u(4.));
        if let Some(header) = self.header.clone() {
            list = list
                .child(header(window, cx))
                .child(self.render_separator(cx));
        }
        for index in 0..self.items.len() {
            let item = self.items[index].clone();
            let ExplorerMenuItem::Item { action, submenu } = item else {
                list = list.child(self.render_separator(cx));
                continue;
            };
            let highlighted = index == self.active;
            let submenu_view = self.render_submenu(index, window, cx);
            let row = self
                .render_row(
                    ("explorer-item", index),
                    &action,
                    !submenu.is_empty(),
                    highlighted,
                    cx,
                )
                .relative()
                .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
                    if *hovered {
                        this.hover_item(index, window, cx);
                    }
                }))
                .on_click(cx.listener(move |this, _, _, cx| this.click_item(index, cx)))
                .children(submenu_view);
            list = list.child(row);
        }
        let frame = popover_frame("explorer-menu")
            .width(self.width)
            .animate(self.animate)
            .child(list);
        let content = div()
            .id("explorer-menu")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::confirm))
            .on_action(cx.listener(Self::open_submenu_action))
            .on_action(cx.listener(Self::close_submenu_action))
            .on_action(cx.listener(Self::cancel))
            .on_hover(cx.listener(|this, hovered: &bool, window, cx| {
                if *hovered {
                    this.cancel_close();
                } else if this.submenu.is_some() && !this.submenu_hovered {
                    this.schedule_close(window, cx);
                }
                cx.emit(ExplorerMenuEvent::Hover(*hovered));
            }))
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                if !this.submenu_hovered {
                    cx.emit(ExplorerMenuEvent::Dismiss);
                }
            }))
            .child(frame);
        let positioned = match self.anchor {
            MenuAnchor::Point(position) => anchored()
                .position(position)
                .anchor(Anchor::TopLeft)
                .snap_to_window_with_margin(px(8.))
                .child(content),
            MenuAnchor::Beside(bounds) => anchored()
                .position(point(bounds.right() + px(4.), bounds.top()))
                .anchor(Anchor::TopLeft)
                .snap_to_window_with_margin(px(8.))
                .child(content),
        };
        deferred(positioned).with_priority(layer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Entity, TestAppContext, VisualTestContext};

    fn items() -> Vec<ExplorerMenuItem> {
        vec![
            ExplorerMenuItem::Separator,
            MenuAction::new("new-file", "New File").into(),
            MenuAction::new("cut", "Cut").disabled(true).into(),
            ExplorerMenuItem::Separator,
            ExplorerMenuItem::with_submenu(
                MenuAction::new("open-with", "Open With"),
                vec![
                    MenuAction::new("editor", "Editor"),
                    MenuAction::new("preview", "Preview"),
                ],
            ),
        ]
    }

    fn open(
        cx: &mut TestAppContext,
    ) -> (
        Entity<ExplorerMenu>,
        &mut VisualTestContext,
        std::rc::Rc<std::cell::RefCell<Vec<ExplorerMenuEvent>>>,
    ) {
        cx.update(crate::test_support::init);
        let (menu, cx) = cx.add_window_view(|window, cx| {
            ExplorerMenu::new(
                MenuAnchor::Point(point(px(10.), px(10.))),
                items(),
                window,
                cx,
            )
            .animate(false)
        });
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = events.clone();
        cx.update(|_, cx| {
            cx.subscribe(&menu, move |_, event: &ExplorerMenuEvent, _| {
                sink.borrow_mut().push(event.clone());
            })
            .detach();
        });
        (menu, cx, events)
    }

    #[gpui::test]
    fn starts_on_the_first_item_and_skips_separators(cx: &mut TestAppContext) {
        let (menu, cx, events) = open(cx);
        assert_eq!(menu.read_with(cx, |menu, _| menu.active()), 1);
        cx.simulate_keystrokes("down");
        assert_eq!(menu.read_with(cx, |menu, _| menu.active()), 2);
        cx.simulate_keystrokes("down down");
        // Wraps from the last item to the first.
        assert_eq!(menu.read_with(cx, |menu, _| menu.active()), 1);
        cx.simulate_keystrokes("up");
        assert_eq!(menu.read_with(cx, |menu, _| menu.active()), 4);
        cx.simulate_keystrokes("up enter");
        // The disabled item does not pick.
        assert!(events.borrow().is_empty());
        cx.simulate_keystrokes("up enter");
        assert_eq!(
            events.borrow().as_slice(),
            [ExplorerMenuEvent::Pick("new-file".into())]
        );
    }

    #[gpui::test]
    fn opens_and_walks_a_submenu_from_the_keyboard(cx: &mut TestAppContext) {
        let (menu, cx, events) = open(cx);
        cx.simulate_keystrokes("up right");
        assert_eq!(
            menu.read_with(cx, |menu, _| menu.open_submenu()),
            Some((4, Some(0)))
        );
        cx.simulate_keystrokes("down");
        assert_eq!(
            menu.read_with(cx, |menu, _| menu.open_submenu()),
            Some((4, Some(1)))
        );
        cx.simulate_keystrokes("left");
        assert_eq!(menu.read_with(cx, |menu, _| menu.open_submenu()), None);
        cx.simulate_keystrokes("enter down enter");
        assert_eq!(
            events.borrow().as_slice(),
            [ExplorerMenuEvent::Pick("preview".into())]
        );
    }

    #[gpui::test]
    fn escape_closes_the_submenu_before_the_menu(cx: &mut TestAppContext) {
        let (_menu, cx, events) = open(cx);
        cx.simulate_keystrokes("up enter escape");
        assert!(events.borrow().is_empty());
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().as_slice(), [ExplorerMenuEvent::Dismiss]);
    }
}
