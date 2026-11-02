//! A transcript work phase's tile ("Ran 5 commands") as its own small view.
//! Hovering pushes the work's glyph out to the right as the expand chevron
//! slides in from the left, and an open phase keeps the chevron. The tile
//! animates by itself, so a hover redraws this tile, not the transcript.
use crate::{tab_tile, theme::*};
use gpui::{Context, Render, Transformation, Window, percentage, prelude::*};
use std::time::{Duration, Instant};

/// Seconds the glyph and the chevron take to trade places.
const MORPH: f32 = 0.3;

pub(crate) struct PhaseTile {
    group: usize,
    glyph: &'static str,
    /// The phase is running: its spinner stands in for the glyph.
    spinning: bool,
    /// The phase is open and settled, so the chevron stays.
    open: bool,
    /// The chevron's quarter turns, as the transcript animates it.
    rotation: f32,
    hovered: bool,
    /// Toward the chevron or back, and since when.
    morph: Option<(bool, Instant)>,
}

impl PhaseTile {
    pub(crate) fn new(group: usize) -> Self {
        Self {
            group,
            glyph: "terminal",
            spinning: false,
            open: false,
            rotation: 0.,
            hovered: false,
            morph: None,
        }
    }
    /// Takes the phase's current state from the transcript, redrawing only
    /// when something changed.
    pub(crate) fn set(
        &mut self,
        glyph: &'static str,
        spinning: bool,
        open: bool,
        rotation: f32,
        cx: &mut Context<Self>,
    ) {
        if (self.glyph, self.spinning, self.open, self.rotation)
            != (glyph, spinning, open, rotation)
        {
            (self.glyph, self.spinning, self.open, self.rotation) =
                (glyph, spinning, open, rotation);
            cx.notify();
        }
    }
    pub(crate) fn hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.hovered != hovered {
            self.hovered = hovered;
            cx.notify();
        }
    }
    /// How far the glyph has turned into the chevron, eased (0 glyph, 1
    /// chevron), and whether it is still moving.
    fn step(&mut self) -> (f32, bool) {
        let wanted = self.open || self.hovered;
        let now = Instant::now();
        let progress =
            |at: Instant| (now.saturating_duration_since(at).as_secs_f32() / MORPH).min(1.);
        match self.morph {
            // Already open when it first draws: start there. Only the
            // pointer arriving pushes the chevron in.
            None if self.open && !self.hovered => {
                self.morph = Some((true, now - Duration::from_secs_f32(MORPH)));
            }
            None if wanted => self.morph = Some((true, now)),
            None => return (0., false),
            // Changing course mid-way turns back from where it is.
            Some((was, at)) if was != wanted => {
                let t = progress(at);
                self.morph = Some((wanted, now - Duration::from_secs_f32((1. - t) * MORPH)));
            }
            Some(_) => {}
        }
        let (on, at) = self.morph.expect("set above");
        let t = progress(at);
        if !on && t >= 1. {
            self.morph = None;
        }
        let eased = crate::reveal::bezier(t, 0.22, 1., 0.36, 1.);
        (if on { eased } else { 1. - eased }, t < 1.)
    }
}

impl Render for PhaseTile {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (morph, moving) = self.step();
        if moving {
            window.request_animation_frame();
        }
        // The compaction step's dots stand in for the group's icon until its
        // work settles.
        let (face, mark, ink) = if self.spinning {
            (
                tab_tile::Face::Neutral,
                crate::session_mark::spinner(
                    ("phase-loader", self.group),
                    crate::pulse::Rate::Half,
                )
                .into_any_element(),
                tab_tile::tile_ink(),
            )
        } else {
            tab_tile::activity_face(self.glyph)
        };
        let chevron = icon("chevron-right", 14.)
            .text_color(ink)
            .with_transformation(Transformation::rotate(percentage(self.rotation * 0.25)))
            .into_any_element();
        tab_tile::push_tile(&tab_tile::ACTIVITY_TILE, face, mark, chevron, morph)
    }
}
