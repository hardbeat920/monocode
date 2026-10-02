//! Context and dropdown menus. Port of the row and separator styles in
//! src/features/files/ui/ExplorerMenu.tsx, inside the popover frame.

use std::rc::Rc;

use gpui::{
    Anchor, App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    Point, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::styled::UiStyled as _;
use crate::widgets::popover::{popover_at, popover_frame};
use crate::{IconName, Theme, icon, u};

/// ExplorerMenu's default width in CSS px.
pub const MENU_WIDTH: f32 = 228.0;

#[derive(Clone, Debug, Default)]
pub struct MenuItem {
    pub id: SharedString,
    pub label: SharedString,
    pub description: Option<SharedString>,
    pub shortcut: Option<SharedString>,
    pub checked: bool,
    pub danger: bool,
    pub disabled: bool,
    /// Shows a chevron. Opening the submenu is the owner's job.
    pub submenu: bool,
    /// Keyboard highlight (`bg-selection`).
    pub highlighted: bool,
}

impl MenuItem {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            ..Default::default()
        }
    }

    pub fn description(mut self, text: impl Into<SharedString>) -> Self {
        self.description = Some(text.into());
        self
    }

    pub fn shortcut(mut self, keys: impl Into<SharedString>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn submenu(mut self) -> Self {
        self.submenu = true;
        self
    }

    pub fn highlighted(mut self, highlighted: bool) -> Self {
        self.highlighted = highlighted;
        self
    }
}

#[derive(Clone, Debug)]
pub enum MenuEntry {
    Item(MenuItem),
    Separator,
}

impl From<MenuItem> for MenuEntry {
    fn from(item: MenuItem) -> Self {
        Self::Item(item)
    }
}

type PickHandler = Rc<dyn Fn(&SharedString, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Menu {
    id: ElementId,
    entries: Vec<MenuEntry>,
    width: f32,
    animate: bool,
    on_pick: Option<PickHandler>,
}

pub fn menu(id: impl Into<ElementId>, entries: impl IntoIterator<Item = MenuEntry>) -> Menu {
    Menu {
        id: id.into(),
        entries: entries.into_iter().collect(),
        width: MENU_WIDTH,
        animate: true,
        on_pick: None,
    }
}

impl Menu {
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn animate(mut self, animate: bool) -> Self {
        self.animate = animate;
        self
    }

    /// Called with the picked item's id.
    pub fn on_pick(
        mut self,
        handler: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_pick = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for Menu {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = Theme::of(cx);
        let c = theme.colors;
        let mut list = div().flex().flex_col().p(u(4.));
        for (index, entry) in self.entries.into_iter().enumerate() {
            let item = match entry {
                MenuEntry::Separator => {
                    list = list.child(div().my(u(4.)).h(gpui::px(1.)).bg(theme.content(0.10)));
                    continue;
                }
                MenuEntry::Item(item) => item,
            };
            let (ink, fill, hover_fill, hover_ink) = if item.disabled {
                (theme.content(0.30), None, None, None)
            } else if item.danger {
                let soft = c.danger_soft;
                if item.highlighted {
                    (
                        soft,
                        Some(crate::color::with_alpha(c.danger_fill, 0.2)),
                        None,
                        None,
                    )
                } else {
                    (
                        crate::color::with_alpha(soft, 0.9),
                        None,
                        Some(crate::color::with_alpha(c.danger_fill, 0.15)),
                        Some(soft),
                    )
                }
            } else if item.highlighted {
                (c.content, Some(c.selection), None, None)
            } else {
                (c.content, None, Some(theme.content(0.05)), None)
            };
            let mut row = div()
                .id(index)
                .flex()
                .w_full()
                .items_center()
                .gap(u(12.))
                .px(u(8.))
                .rounded(u(theme.radius.lg))
                .text_px(theme.text.body)
                .leading(theme.leading.none)
                .text_color(ink);
            row = if item.description.is_some() {
                row.py(u(6.))
            } else {
                row.h(u(28.))
            };
            if let Some(fill) = fill {
                row = row.bg(fill);
            }
            if hover_fill.is_some() || hover_ink.is_some() {
                row = row.hover(move |s| {
                    let mut s = s;
                    if let Some(fill) = hover_fill {
                        s = s.bg(fill);
                    }
                    if let Some(ink) = hover_ink {
                        s = s.text_color(ink);
                    }
                    s
                });
            }
            let mut label = div()
                .flex_1()
                .min_w_0()
                .child(div().truncate().child(item.label.clone()));
            if let Some(description) = item.description.clone() {
                label = label.child(
                    div()
                        .mt(u(4.))
                        .text_px(theme.text.caption)
                        .leading(theme.leading.snug)
                        .text_color(theme.content(0.50))
                        .child(description),
                );
            }
            row = row.child(label);
            if item.submenu {
                row = row.child(
                    icon(IconName::ChevronRight)
                        .size(u(14.))
                        .text_color(theme.content(0.50)),
                );
            } else if item.checked {
                row = row.child(icon(IconName::Check).size(u(14.)).text_color(ink));
            } else if let Some(shortcut) = item.shortcut.clone() {
                row = row.child(
                    div()
                        .flex_none()
                        .text_px(theme.text.caption)
                        .text_color(theme.content(0.40))
                        .child(shortcut),
                );
            }
            if let (Some(handler), false) = (self.on_pick.clone(), item.disabled) {
                let id = item.id.clone();
                row = row.on_click(move |_, window, cx| handler(&id, window, cx));
            }
            list = list.child(row);
        }
        popover_frame(self.id)
            .width(self.width)
            .animate(self.animate)
            .child(list)
    }
}

/// A context menu at a window point. `on_dismiss` runs on any click outside.
pub fn context_menu(
    position: Point<Pixels>,
    menu: Menu,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    popover_at(
        position,
        Anchor::TopLeft,
        div()
            .on_mouse_down_out(move |_, window, cx| on_dismiss(window, cx))
            .child(menu),
        cx,
    )
}
