//! Pinned tabs leave the strip for icon cards at its left edge. Up to
//! [`ROW_MAX`] pins sit side by side in a row; past that they rest stacked
//! like a split tab's panes, and fan out into the row when the pointer
//! comes over them, pushing the strip's tabs along. Each card's title
//! shows beneath it on hover.
use super::*;
use crate::tab_tile::{FanCard, fan_card, fan_tile, fan_width};
use crate::workspace::TitleTab;

/// How long the deck takes to fan out or gather back, in seconds.
const FAN_DURATION: f32 = 0.24;
/// Between the deck and the strip's first tab, split by a hairline.
const DOCK_GAP: f32 = 6.;
/// The most pins that sit in a plain row; more gather into a deck.
const ROW_MAX: usize = 3;

/// The deck's fan: where it was heading, from where, and since when.
pub(super) struct PinFan {
    /// Whether the pointer is over the deck.
    hovered: bool,
    open: bool,
    from: f32,
    start: std::time::Instant,
}

impl Default for PinFan {
    fn default() -> Self {
        Self {
            hovered: false,
            open: false,
            from: 0.,
            start: std::time::Instant::now(),
        }
    }
}

impl PinFan {
    /// How far the deck has fanned out (eased, 0 to 1), and whether it is
    /// still moving.
    fn at(&self, now: std::time::Instant, reduced: bool) -> (f32, bool) {
        let to = if self.open { 1. } else { 0. };
        if reduced {
            return (to, false);
        }
        let t = now.saturating_duration_since(self.start).as_secs_f32() / FAN_DURATION;
        if t >= 1. {
            return (to, false);
        }
        let eased = crate::reveal::bezier(t, 0.22, 1., 0.36, 1.);
        (self.from + (to - self.from) * eased, true)
    }

    fn set(&mut self, open: bool, reduced: bool) {
        if self.open == open {
            return;
        }
        let now = std::time::Instant::now();
        self.from = self.at(now, reduced).0;
        self.open = open;
        self.start = now;
    }
}

/// Which card tops the stacked deck, and the shuffle dealing it there:
/// the card it replaced and when that began.
#[derive(Default)]
pub(super) struct PinDeck {
    front: Option<TitleTab>,
    shuffle: Option<(TitleTab, std::time::Instant)>,
}

/// What a pinned card shows.
struct PinCard {
    tab: TitleTab,
    face: Face,
    mark: AnyElement,
    title: SharedString,
}

impl Shell {
    pub(super) fn tab_pinned(&self, tab: &TitleTab) -> bool {
        self.workspace.pinned_tabs.contains(tab)
    }

    /// Pins `tab` to the end of the deck, or puts it back at the strip's
    /// left end.
    pub(super) fn toggle_pin_tab(&mut self, tab: TitleTab, cx: &mut Context<Self>) {
        if !self.workspace.title_order.contains(&tab) {
            return;
        }
        if self.tab_pinned(&tab) {
            self.workspace.pinned_tabs.retain(|t| *t != tab);
            // Unpinned, it rejoins the strip where the deck let go of it.
            self.reorder_title_tab(&tab, 0, cx);
        } else {
            self.workspace.pinned_tabs.push(tab);
        }
        self.tab_drag = None;
        self.tab_shifts.clear();
        self.pin_card_hover = None;
        self.persist(cx);
        self.tabs_frame.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    pub(super) fn tab_menu_target(&self, tab: &TitleTab) -> MenuTarget {
        match tab {
            TitleTab::Session(id) => MenuTarget::Tab(*id),
            TitleTab::Mono { id, .. } => MenuTarget::MonoTab(id.clone()),
            TitleTab::Search => MenuTarget::SearchTab,
            TitleTab::App(view) => MenuTarget::AppTab(*view),
        }
    }

    /// The pinned tabs in this scope, in pin order.
    fn pinned_visible(&self, cx: &App) -> Vec<TitleTab> {
        self.visible_tabs(cx)
            .into_iter()
            .filter(|tab| self.tab_pinned(tab))
            .collect()
    }

    /// `live` lets a working session's card borrow its live mark, which can
    /// only draw in one place a frame.
    fn pin_card(&self, tab: &TitleTab, live: bool, cx: &mut Context<Self>) -> PinCard {
        let neutral = |name: &'static str| {
            icon(name, 14.)
                .text_color(crate::tab_tile::tile_ink())
                .into_any_element()
        };
        let (face, mark, title): (Face, AnyElement, SharedString) = match tab {
            TitleTab::Session(id) => {
                let copy = self.tab_copy(*id, cx);
                let (face, mark) = match copy.panes.get(copy.focus) {
                    Some(pane) => self.pane_tile(&pane.mark, live),
                    None => (Face::Neutral, div().into_any_element()),
                };
                (face, mark, copy.headline.clone().into())
            }
            TitleTab::Mono { id, .. } => {
                let mono = self
                    .monos
                    .as_ref()
                    .and_then(|m| m.read(cx).roster.get(id).cloned());
                match mono {
                    Some(mono) => (
                        Face::Neutral,
                        crate::monos::avatar::avatar(&mono.mascot, mono.color, "idle", 14., true)
                            .into_any_element(),
                        mono.label().into(),
                    ),
                    None => (Face::Neutral, div().into_any_element(), "Mono".into()),
                }
            }
            TitleTab::Search => (Face::Neutral, neutral("search"), "Search".into()),
            TitleTab::App(view) => (Face::Neutral, neutral(view.icon()), view.label().into()),
        };
        PinCard {
            tab: tab.clone(),
            face,
            mark,
            title,
        }
    }

    /// The pinned deck and the width it holds at the strip's left edge,
    /// or nothing when no tab here is pinned. `closing` are tabs still
    /// easing out of the strip, which already draw their own live marks.
    pub(super) fn pinned_dock(
        &mut self,
        closing: &HashSet<TitleTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(AnyElement, f32)> {
        let pinned = self.pinned_visible(cx);
        if pinned.is_empty() {
            self.pin_fan = PinFan::default();
            self.pin_card_hover = None;
            return None;
        }
        let reduced = cx.reduce_motion();
        let count = pinned.len();
        let stacks = count > ROW_MAX;
        // A card's context menu takes the pointer off the deck; it stays
        // spread until the menu closes.
        let menu_on_pin = self.menu.as_ref().is_some_and(|menu| {
            pinned
                .iter()
                .any(|tab| self.tab_menu_target(tab) == menu.target)
        });
        let (fan, moving) = if stacks {
            let open = self.pin_fan.hovered || menu_on_pin;
            self.pin_fan.set(open, reduced);
            self.pin_fan.at(std::time::Instant::now(), reduced)
        } else {
            self.pin_fan = PinFan {
                hovered: self.pin_fan.hovered,
                ..PinFan::default()
            };
            (1., false)
        };
        if moving {
            window.request_animation_frame();
        }
        let active = self.active_title_tab(cx);
        let front = active
            .as_ref()
            .and_then(|tab| pinned.iter().position(|p| p == tab));
        // The deck keeps the last pin chosen on top while a strip tab is
        // open, and shuffles the next one chosen to the front.
        let top = front
            .or_else(|| {
                let kept = self.pin_deck.front.as_ref()?;
                pinned.iter().position(|p| p == kept)
            })
            .unwrap_or(0);
        let now = std::time::Instant::now();
        if self.pin_deck.front.as_ref() != Some(&pinned[top]) {
            let was = self.pin_deck.front.replace(pinned[top].clone());
            self.pin_deck.shuffle = was
                .filter(|was| stacks && !reduced && pinned.contains(was))
                .map(|was| (was, now));
        }
        if !stacks {
            self.pin_deck.shuffle = None;
        }
        let shuffle = self.pin_deck.shuffle.as_ref().and_then(|(was, start)| {
            let t = now.saturating_duration_since(*start).as_secs_f32() / TILE_SHUFFLE_DURATION;
            let was = pinned.iter().position(|p| p == was)?;
            (t < 1.).then_some((was, t))
        });
        if shuffle.is_none() {
            self.pin_deck.shuffle = None;
        } else {
            window.request_animation_frame();
        }
        let inset = crate::tab_tile::tab_tile_inset();
        // Snapped to device pixels, as the tabs it pushes along are.
        let scale = window.scale_factor();
        let width = ((fan_width(&TAB_TILE, count, fan) + inset * 2.) * scale).round() / scale;
        let hovered_card = self
            .pin_card_hover
            .clone()
            .filter(|tab| pinned.contains(tab));
        let cards: Vec<(PinCard, FanCard)> = pinned
            .iter()
            .enumerate()
            .map(|(ix, tab)| {
                let card = self.pin_card(tab, !closing.contains(tab), cx);
                (card, fan_card(&TAB_TILE, ix, top, fan, shuffle))
            })
            .collect();
        let caption = hovered_card.as_ref().and_then(|tab| {
            let (card, place) = cards.iter().find(|(c, _)| c.tab == *tab)?;
            Some((card.title.clone(), place.x, fan))
        });
        let mut order: Vec<usize> = (0..cards.len()).collect();
        order.sort_by(|a, b| cards[*b].1.z.total_cmp(&cards[*a].1.z));
        let mut cards: Vec<Option<(PinCard, FanCard)>> = cards.into_iter().map(Some).collect();
        let plate_top = (TITLE_BAR_HEIGHT - TAB_PLATE_HEIGHT) / 2.;
        let card_elements = order.into_iter().filter_map(|ix| {
            let (card, place) = cards[ix].take()?;
            let PinCard {
                tab,
                face,
                mark,
                title,
            } = card;
            let selected = active.as_ref() == Some(&tab);
            let hovered = hovered_card.as_ref() == Some(&tab);
            let (open, menu, close, hover) = (tab.clone(), tab.clone(), tab.clone(), tab.clone());
            let key = pinned.iter().position(|p| *p == tab).unwrap_or(ix);
            Some(
                div()
                    .id(("pinned-tab", key))
                    .debug_selector(move || format!("pinned-tab-{key}"))
                    .role(Role::Tab)
                    .aria_label(title)
                    .aria_selected(selected)
                    .cursor_pointer()
                    .absolute()
                    .left(px(inset + place.x))
                    .top(px(inset + place.y))
                    .w(px(TAB_TILE.w))
                    .h(px(TAB_TILE.h))
                    .when(place.opacity < 1., |d| d.opacity(place.opacity))
                    .child(fan_tile(&TAB_TILE, &place, face, mark))
                    // The selected card carries a short accent bar under it,
                    // as a tab plate does with its fill.
                    .when(selected, |d| {
                        d.child(
                            div()
                                .absolute()
                                .bottom(px(-inset + 0.5))
                                .left(px((TAB_TILE.w - 10.) / 2.))
                                .w(px(10.))
                                .h(px(2.))
                                .rounded_full()
                                .bg(accent_color()),
                        )
                    })
                    .when(hovered && !selected, |d| {
                        d.child(
                            div()
                                .absolute()
                                .bottom(px(-inset + 0.5))
                                .left(px((TAB_TILE.w - 6.) / 2.))
                                .w(px(6.))
                                .h(px(2.))
                                .rounded_full()
                                .bg(content(0.35)),
                        )
                    })
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.pin_card_hover = Some(hover.clone());
                        } else if this.pin_card_hover.as_ref() == Some(&hover) {
                            this.pin_card_hover = None;
                        }
                        this.tabs_frame.update(cx, |_, cx| cx.notify());
                    }))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            let target = this.tab_menu_target(&menu);
                            this.open_menu(target, Anchor::Point(event.position), window, cx);
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Middle,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            match close.session() {
                                Some(id) => this.close(id, window, cx),
                                None => {
                                    this.close_title_tabs(
                                        std::slice::from_ref(&close),
                                        None,
                                        window,
                                        cx,
                                    );
                                }
                            }
                        }),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.skip_tab_click = false;
                        this.open_title_tab(&open, window, cx);
                    }))
                    .into_any_element(),
            )
        });
        let deck = div()
            .id("pinned-tab-deck")
            .debug_selector(|| "pinned-tab-deck".into())
            .role(Role::TabList)
            .aria_label("Pinned tabs")
            .absolute()
            .left(px(STRIP_LEAD))
            .top(px(plate_top))
            .w(px(width))
            .h(px(TAB_PLATE_HEIGHT))
            .rounded_md()
            .child(
                div()
                    .absolute()
                    .size_full()
                    .rounded_md()
                    .bg(title_tab_fill(front.is_some(), hovered_card.is_some())),
            )
            .children(card_elements)
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                // The deck spreads or gathers on the next frame, which
                // also weighs an open card menu.
                this.pin_fan.hovered = *hovered;
                if !*hovered {
                    this.pin_card_hover = None;
                }
                this.tabs_frame.update(cx, |_, cx| cx.notify());
            }));
        // The hovered card's title, in a pill hanging just below the bar.
        let caption = caption
            .filter(|(_, _, shown)| *shown > 0.05)
            .map(|(title, x, shown)| {
                div()
                    .debug_selector(|| "pinned-tab-caption".into())
                    .absolute()
                    .left(px(STRIP_LEAD + inset + x))
                    .top(px(plate_top + TAB_PLATE_HEIGHT + 6.))
                    .max_w(px(260.))
                    .h(px(24.))
                    .px_2()
                    .flex()
                    .items_center()
                    .opacity(shown)
                    .rounded_md()
                    .border_1()
                    .border_color(content(0.1))
                    .bg(pane_fill())
                    .shadow_md()
                    .text_size(px(12.))
                    .text_color(content(1.))
                    .child(div().min_w_0().truncate().child(title))
            });
        // The slot grows with the fan, so the strip's tabs give way to it.
        let slot_w = STRIP_LEAD + width + DOCK_GAP;
        let slot = div()
            .debug_selector(|| "pinned-tab-slot".into())
            .flex_shrink_0()
            .w(px(slot_w))
            .h(px(TITLE_BAR_HEIGHT))
            .relative()
            .child(
                div()
                    .absolute()
                    .right(px(0.))
                    .top(px((TITLE_BAR_HEIGHT - 16.) / 2.))
                    .w(px(1.))
                    .h(px(16.))
                    .bg(content(0.1)),
            )
            .child(deck)
            // The caption hangs below the bar, past the strip's clip.
            .children(caption.map(|c| gpui::deferred(c).with_priority(3)));
        Some((slot.into_any_element(), slot_w))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::tests::window;
    use gpui::TestAppContext;

    fn add_session(shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>) -> i64 {
        let summary = locked(&shell.store)
            .unwrap()
            .create("codex", &shell.workspace.project)
            .unwrap();
        let id = summary.id;
        shell.add(summary, cx);
        let focus = shell.worktree_focus.get(&shell.workspace.project).cloned();
        shell.seed_worktree(id, focus.as_ref(), cx);
        shell.open(id, window, cx);
        id
    }

    #[gpui::test]
    fn pinned_tabs_lead_the_order_leave_the_strip_and_survive_bulk_closes(cx: &mut TestAppContext) {
        let (window, _, first) = window(cx);
        let (second, third) = window
            .update(cx, |shell, w, cx| {
                (add_session(shell, w, cx), add_session(shell, w, cx))
            })
            .unwrap();
        cx.run_until_parked();
        let (store, summaries, saved) = window
            .update(cx, |shell, w, cx| {
                let [a, b, c] = [first, second, third].map(TitleTab::Session);
                shell.run_menu(MenuTarget::Tab(third), "pin", None, w, cx);
                shell.run_menu(MenuTarget::Tab(second), "pin", None, w, cx);
                assert_eq!(shell.visible_tabs(cx), [c.clone(), b.clone(), a.clone()]);
                assert_eq!(shell.strip_tabs(cx), std::slice::from_ref(&a));
                // Dropping onto the strip counts only its own slots.
                shell.reorder_title_tab(&a, 0, cx);
                assert_eq!(shell.visible_tabs(cx), [c.clone(), b.clone(), a.clone()]);
                shell.run_menu(MenuTarget::Tab(first), "others", None, w, cx);
                assert_eq!(shell.visible_tabs(cx), [c.clone(), b.clone(), a.clone()]);
                shell.run_menu(MenuTarget::Tab(second), "unpin", None, w, cx);
                assert_eq!(shell.workspace.pinned_tabs, std::slice::from_ref(&c));
                assert_eq!(shell.strip_tabs(cx), [b.clone(), a.clone()]);
                let db = locked(&shell.store).unwrap();
                (
                    shell.store.clone(),
                    db.sessions().unwrap(),
                    serde_json::to_string(&shell.workspace).unwrap(),
                )
            })
            .unwrap();
        let restored = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| Shell::new(store, summaries, "/work/monocode".into(), Some(saved), cx))
            })
            .unwrap()
        });
        cx.run_until_parked();
        restored
            .update(cx, |shell, w, cx| {
                assert_eq!(shell.workspace.pinned_tabs, [TitleTab::Session(third)]);
                shell.close(third, w, cx);
                assert!(shell.workspace.pinned_tabs.is_empty());
            })
            .unwrap();
    }

    #[gpui::test]
    fn a_few_pins_sit_in_a_row_and_more_fan_out_of_a_deck(cx: &mut TestAppContext) {
        let (window, _, first) = window(cx);
        cx.update(|cx| cx.set_reduce_motion(true));
        let ids = window
            .update(cx, |shell, w, cx| {
                let mut ids = vec![first];
                for _ in 0..4 {
                    ids.push(add_session(shell, w, cx));
                }
                for id in &ids[..3] {
                    shell.toggle_pin_tab(TitleTab::Session(*id), cx);
                }
                ids
            })
            .unwrap();
        cx.run_until_parked();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        // Three pins lie side by side, with no deck to spread.
        let row = visual.debug_bounds("pinned-tab-deck").unwrap();
        let (a, b) = (
            visual.debug_bounds("pinned-tab-0").unwrap(),
            visual.debug_bounds("pinned-tab-1").unwrap(),
        );
        assert!(b.origin.x >= a.origin.x + a.size.width);
        assert!(
            visual
                .debug_bounds(Box::leak(format!("title-tab-{first}").into_boxed_str()))
                .is_none()
        );
        visual.simulate_mouse_move(row.center(), None, Default::default());
        visual.run_until_parked();
        visual.refresh().unwrap();
        assert_eq!(
            visual.debug_bounds("pinned-tab-deck").unwrap().size.width,
            row.size.width
        );
        visual.simulate_mouse_move(gpui::point(px(900.), px(400.)), None, Default::default());
        visual.run_until_parked();
        // A fourth gathers them into a deck, which spreads on hover.
        window
            .update(visual, |shell, _, cx| {
                shell.toggle_pin_tab(TitleTab::Session(ids[3]), cx)
            })
            .unwrap();
        visual.run_until_parked();
        visual.refresh().unwrap();
        let deck = visual.debug_bounds("pinned-tab-deck").unwrap();
        assert!(deck.size.width < row.size.width);
        visual.simulate_mouse_move(deck.center(), None, Default::default());
        visual.run_until_parked();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("pinned-tab-deck").unwrap().size.width > row.size.width);
        let card = visual.debug_bounds("pinned-tab-0").unwrap();
        visual.simulate_click(card.center(), Default::default());
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                assert_eq!(shell.active_title_tab(cx), Some(TitleTab::Session(first)));
            })
            .unwrap();
    }

    #[gpui::test]
    fn cmd_w_leaves_a_pin_open_and_the_strip_keeps_a_blank_tab(cx: &mut TestAppContext) {
        let (window, _, first) = window(cx);
        cx.update(bind_keys);
        let second = window.update(cx, add_session).unwrap();
        window
            .update(cx, |shell, w, cx| {
                shell.toggle_pin_tab(TitleTab::Session(first), cx);
                shell.open(first, w, cx);
            })
            .unwrap();
        cx.run_until_parked();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.simulate_keystrokes("cmd-w");
        visual.run_until_parked();
        window
            .update(visual, |shell, w, cx| {
                assert_eq!(shell.workspace.pinned_tabs, [TitleTab::Session(first)]);
                assert_eq!(shell.active_title_tab(cx), Some(TitleTab::Session(first)));
                // Closing the strip's last tab leaves a blank one, not the pin.
                shell.open(second, w, cx);
                shell.close_title_tabs(&[TitleTab::Session(second)], None, w, cx);
            })
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |shell, _, cx| {
                let strip = shell.strip_tabs(cx);
                assert_eq!(strip.len(), 1);
                assert_eq!(shell.active_title_tab(cx), Some(strip[0].clone()));
            })
            .unwrap();
    }

    #[gpui::test]
    fn choosing_a_stacked_pin_by_number_shuffles_it_to_the_front(cx: &mut TestAppContext) {
        let (window, _, first) = window(cx);
        cx.update(bind_keys);
        let ids = window
            .update(cx, |shell, w, cx| {
                let mut ids = vec![first];
                for _ in 0..4 {
                    ids.push(add_session(shell, w, cx));
                }
                for id in &ids[..4] {
                    shell.toggle_pin_tab(TitleTab::Session(*id), cx);
                }
                shell.open(first, w, cx);
                ids
            })
            .unwrap();
        cx.run_until_parked();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        visual.simulate_keystrokes("cmd-2");
        visual.run_until_parked();
        visual.refresh().unwrap();
        window
            .update(visual, |shell, _, cx| {
                assert_eq!(shell.active_title_tab(cx), Some(TitleTab::Session(ids[1])));
                assert_eq!(shell.pin_deck.front, Some(TitleTab::Session(ids[1])));
                let (was, _) = shell.pin_deck.shuffle.clone().unwrap();
                assert_eq!(was, TitleTab::Session(first));
            })
            .unwrap();
    }

    #[gpui::test]
    fn a_spread_deck_stays_open_while_a_card_menu_is(cx: &mut TestAppContext) {
        let (window, _, first) = window(cx);
        cx.update(|cx| cx.set_reduce_motion(true));
        window
            .update(cx, |shell, w, cx| {
                let mut ids = vec![first];
                for _ in 0..4 {
                    ids.push(add_session(shell, w, cx));
                }
                for id in &ids[..4] {
                    shell.toggle_pin_tab(TitleTab::Session(*id), cx);
                }
            })
            .unwrap();
        cx.run_until_parked();
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        let resting = visual.debug_bounds("pinned-tab-deck").unwrap();
        visual.simulate_mouse_move(resting.center(), None, Default::default());
        visual.run_until_parked();
        visual.refresh().unwrap();
        let spread = visual.debug_bounds("pinned-tab-deck").unwrap().size.width;
        assert!(spread > resting.size.width);
        let card = visual.debug_bounds("pinned-tab-1").unwrap();
        visual.simulate_mouse_down(card.center(), MouseButton::Right, Default::default());
        visual.simulate_mouse_up(card.center(), MouseButton::Right, Default::default());
        visual.simulate_mouse_move(gpui::point(px(900.), px(400.)), None, Default::default());
        visual.run_until_parked();
        visual.refresh().unwrap();
        assert!(
            window
                .update(visual, |shell, _, _| shell.menu.is_some())
                .unwrap()
        );
        assert_eq!(
            visual.debug_bounds("pinned-tab-deck").unwrap().size.width,
            spread
        );
        // Once the menu is gone, the deck gathers back up.
        window
            .update(visual, |shell, _, cx| shell.close_menu(cx))
            .unwrap();
        visual.run_until_parked();
        visual.refresh().unwrap();
        assert_eq!(
            visual.debug_bounds("pinned-tab-deck").unwrap().size.width,
            resting.size.width
        );
    }
}
