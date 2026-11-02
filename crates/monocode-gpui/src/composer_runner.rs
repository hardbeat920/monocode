//! Composer animation host adapter: placement, project appearance, transcript
//! obstacle coordinates and the app's shared clock. Simulation/drawing are native.
use crate::transcript::TranscriptView;
use gpui::{Bounds, Context, Entity, Pixels, prelude::*, px};
use monocode_mascots::{
    MascotId,
    composer::Obstacle,
    gpui::composer::{ComposerView, Props, Services, Style},
};
use std::{
    hash::{BuildHasher, Hasher},
    rc::Rc,
};

pub(crate) use monocode_mascots::gpui::composer::ComposerView as Runner;

pub(crate) fn props(busy: bool, name: &str) -> Props {
    Props {
        busy,
        mascot: MascotId::find(name),
        rim_y: 64.,
    }
}
pub(crate) fn style(color: u32) -> Style {
    Style {
        color: gpui::rgb(color).into(),
    }
}

pub(crate) fn new(transcript: Entity<TranscriptView>, cx: &mut Context<Runner>) -> Runner {
    let weak_transcript = transcript.downgrade();
    let services = Services {
        clock: Rc::new(crate::pulse::ComponentClock),
        obstacle: Rc::new(move |bounds, rim, cx| {
            let jump = weak_transcript
                .upgrade()
                .and_then(|view| view.read(cx).jump_bounds());
            obstacle(bounds, rim, jump)
        }),
    };
    let seed = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish() as u32;
    let mut view = ComposerView::new(seed, props(false, "invader"), style(0x459cf7), services);
    view.watch_geometry(&transcript, cx);
    view
}

/// Native ledge policy is unchanged: the composer border, not React's review
/// ledge. Keep f32 window-coordinate subtraction before entering the f64 model.
fn obstacle(bounds: Bounds<Pixels>, rim: f32, jump: Option<Bounds<Pixels>>) -> Option<Obstacle> {
    let top = bounds.top().as_f32() + rim;
    jump.filter(|b| {
        b.right() > bounds.left()
            && b.left() < bounds.right()
            && b.bottom().as_f32() >= top - 48.
            && b.top().as_f32() <= top + 12.
    })
    .map(|b| Obstacle {
        left: f64::from((b.left() - bounds.left()).as_f32()),
        right: f64::from((b.right() - bounds.left()).as_f32()),
        height: f64::from((top - b.top().as_f32() + 10.).max(28.)),
    })
}

pub(crate) fn placement() -> gpui::StyleRefinement {
    gpui::StyleRefinement::default()
        .absolute()
        .left_0()
        .right_0()
        .top(px(-64.))
        .h(px(80.))
}
