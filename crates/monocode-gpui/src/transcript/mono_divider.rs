//! Text replies in a combined Mono turn get a small, angular halftone break.
use super::*;
use crate::visuals::dither::{CELL, Grid, inked};

pub(super) fn insert(rows: &mut Vec<UiRow>, messages: &[Message], from: usize) {
    let tail = rows.split_off(from);
    for row in tail {
        let prose = |row: &UiRow| matches!(&row.body, Body::Markdown(md) if !matches!(md.part, Part::Visual { .. }));
        if row.key.1 == 16
            && prose(&row)
            && let Some(previous) = rows.last().filter(|previous| prose(previous))
            && previous.message != row.message
            && let (Some(before), Some(after)) =
                (messages.get(previous.message), messages.get(row.message))
            && before.role == "assistant"
            && after.role == "assistant"
            && before.turn == after.turn
        {
            rows.push(UiRow {
                key: (row.key.0, 8),
                body: Body::MonoDivider,
                message: row.message,
                first: false,
                last: false,
                work: false,
                work_tail: false,
                thought_phase: None,
            });
        }
        rows.push(row);
    }
}

pub(super) fn render() -> gpui::AnyElement {
    div()
        .debug_selector(|| "mono-response-divider".into())
        .flex()
        .child(
            canvas(
                |_, _, _| {},
                |bounds, _, window, _| {
                    let grid = Grid::new(
                        bounds.origin,
                        bounds.size.width.as_f32(),
                        bounds.size.height.as_f32(),
                    );
                    grid.paint(window, |col, row| {
                        let (x, y) = Grid::center(col, row);
                        // Six sharp teeth, using the charts' Bayer pattern
                        // and pixel-snapped cells instead of a smooth path.
                        let tooth = ((x / (CELL * 8.)).fract() * 2. - 1.).abs();
                        let line = CELL + tooth * (bounds.size.height.as_f32() - CELL * 2.);
                        ((y - line).abs() <= CELL && inked(col, row, 0.75)).then_some(content(0.4))
                    });
                },
            )
            .w(px(144.))
            .h(px(18.))
            // Previous prose supplies 8px below; the next supplies 12px above.
            .mt_1(),
        )
        .into_any_element()
}
