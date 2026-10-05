//! Port of src/features/quick-composer/ui/QuickProjectIcon.tsx: the saved
//! logo, else the project's mascot in its color. A logo that fails to load
//! falls back to the mascot.

use std::path::PathBuf;

use gpui::{
    AnyElement, Bounds, Hsla, IntoElement, ObjectFit, ParentElement as _, Styled as _,
    StyledImage as _, canvas, div, fill, img, point, size,
};
use monocode_ui::color::parse_hex;
use monocode_ui::u;
use monocode_view_composer::composer::model::mascots::{MASCOT_GRID, mascot_rects, project_mascot};

use crate::model::appearance::{MascotIcon, ProjectAppearance, ProjectIcon, project_icon};

/// The pixel mascot filling a `size` CSS px square.
pub fn mascot_element(mascot: &MascotIcon, size_px: f32, fallback_ink: Hsla) -> AnyElement {
    let color = parse_hex(&mascot.color).unwrap_or(fallback_ink);
    let cells = mascot_rects(&project_mascot(&mascot.seed, mascot.name.as_deref()).rest);
    div()
        .flex_none()
        .size(u(size_px))
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let cell = bounds.size.width / MASCOT_GRID as f32;
                    for (x, y, width) in &cells {
                        let origin = point(
                            bounds.origin.x + cell * *x as f32,
                            bounds.origin.y + cell * *y as f32,
                        );
                        window.paint_quad(fill(
                            Bounds::new(origin, size(cell * *width as f32, cell)),
                            color,
                        ));
                    }
                },
            )
            .size_full(),
        )
        .into_any_element()
}

/// `QuickProjectIcon` at `size` CSS px (`size-3` is 12).
pub fn quick_project_icon(
    project_path: &str,
    appearance: &ProjectAppearance,
    size_px: f32,
    fallback_ink: Hsla,
) -> AnyElement {
    match project_icon(project_path, appearance) {
        ProjectIcon::Mascot(mascot) => mascot_element(&mascot, size_px, fallback_ink),
        ProjectIcon::Logo { path, mascot } => div()
            .flex_none()
            .size(u(size_px))
            .rounded(u(2.))
            .overflow_hidden()
            .child(
                img(PathBuf::from(path))
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .with_fallback(move || mascot_element(&mascot, size_px, fallback_ink)),
            )
            .into_any_element(),
    }
}
