//! The project glyph cards and pickers show: `ProjectLogoIcon` when the
//! project has a logo, else `ProjectMascot` in the project color.

use std::path::PathBuf;

use gpui::{
    Hsla, IntoElement, ObjectFit, ParentElement as _, Styled as _, StyledImage as _, div, img,
};
use monocode_ui::u;

use super::mascot::mascot_sprite;
use crate::data::ProjectMark;

/// The mark at `logo_size` (a logo) or `mascot_size` (a mascot) CSS px.
/// `ink` colors a mascot without a project color, as `currentColor` did.
pub fn project_mark(
    mark: &ProjectMark,
    logo_size: f32,
    mascot_size: f32,
    ink: Hsla,
) -> impl IntoElement {
    if let Some(logo) = mark.logo.clone() {
        return div()
            .flex_none()
            .size(u(logo_size))
            .rounded(u(2.))
            .overflow_hidden()
            .child(
                img(PathBuf::from(logo))
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
            .into_any_element();
    }
    div()
        .flex_none()
        .size(u(mascot_size))
        .child(mascot_sprite(
            &mark.seed,
            mark.mascot.as_deref(),
            mark.color.unwrap_or(ink),
        ))
        .into_any_element()
}
