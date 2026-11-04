//! App-owned styling, provider assets and scheduling for the native arcade.
use crate::{fonts, pulse, theme};
use gpui::{Context, WeakFocusHandle, prelude::*};
use monocode_arcade::gpui::{Services, Style};
use monocode_core::provider::Kind;
use std::{
    hash::{BuildHasher, Hasher},
    rc::Rc,
};

pub(crate) use monocode_arcade::gpui::ArcadeView as Arcade;

pub(crate) fn style() -> Style {
    Style {
        foreground: theme::content(1.),
        surface: theme::shell_fill(),
        mono_font: fonts::mono().into(),
    }
}

fn services() -> Services {
    Services {
        clock: Rc::new(pulse::ComponentClock),
        provider_icon: Rc::new(|id, side, _, _| theme::provider_icon(id, side).into_any_element()),
    }
}

pub(crate) fn new(fallback: Option<WeakFocusHandle>, cx: &mut Context<Arcade>) -> Arcade {
    // Randomness is seeded once by the host; neither library accesses global
    // entropy/services. Composer owns a separate stream and is not coupled here.
    let seed = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish() as u32;
    Arcade::new(
        Kind::ALL.map(|kind| kind.id().to_owned()).into(),
        seed,
        style(),
        services(),
        fallback,
        cx,
    )
}
