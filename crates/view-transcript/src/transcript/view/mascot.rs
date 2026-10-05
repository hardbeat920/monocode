//! Port of src/features/projects/ui/ProjectMascot.tsx and the sprites in
//! src/features/projects/model/projectMascots.ts: an 8 by 8 pixel mascot,
//! hashed off a name, hopping between two frames while it works
//! (`.mascot-active`, a 460ms beat).

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, Bounds, ElementId, Hsla, IntoElement, ParentElement as _,
    Styled as _, canvas, div, fill, point, px, size,
};
use monocode_ui::u;

const GRID: usize = 8;

type Rows = [&'static str; GRID];

/// `REST` and `TALK`, in the order `Object.entries` lists them.
const MASCOTS: [(&str, Rows, Rows); 10] = [
    (
        "invader",
        [
            "..#..#..", ".######.", "##.##.##", "########", ".######.", ".#.##.#.", "#.#..#.#",
            "........",
        ],
        [
            "..#..#..", ".######.", "##.##.##", "########", ".######.", "#.####.#", ".#....#.",
            "#......#",
        ],
    ),
    (
        "ghost",
        [
            "..####..", ".######.", "##.##.##", "########", "########", "########", "########",
            "#.##.##.",
        ],
        [
            "..####..", ".######.", "#.##.###", "########", "########", "########", "########",
            ".##.##.#",
        ],
    ),
    (
        "robot",
        [
            "...#....", ".######.", ".#.##.#.", ".######.", ".#....#.", ".######.", "..#..#..",
            "........",
        ],
        [
            "....#...", ".######.", ".#.##.#.", ".######.", ".##..##.", ".######.", ".#....#.",
            "........",
        ],
    ),
    (
        "cat",
        [
            ".#....#.", ".##..##.", "########", "#.####.#", "########", "###..###", ".######.",
            "..#..#..",
        ],
        [
            ".#....#.", ".##..##.", "########", "#.####.#", "########", "########", ".######.",
            ".#....#.",
        ],
    ),
    (
        "skull",
        [
            ".######.", "########", "##.##.##", "########", ".##..##.", ".######.", ".#.##.#.",
            "........",
        ],
        [
            ".######.", "########", "##.##.##", "########", ".##..##.", ".######.", ".#....#.",
            "..####..",
        ],
    ),
    (
        "crab",
        [
            "#......#", ".#....#.", ".######.", "##.##.##", "########", "#.####.#", "#......#",
            "........",
        ],
        [
            "#......#", "##....##", ".######.", "##.##.##", "########", ".######.", "#.#..#.#",
            "........",
        ],
    ),
    (
        "mushroom",
        [
            "..####..", ".######.", "########", "##.##.##", "########", "...##...", "...##...",
            "..####..",
        ],
        [
            "........", "..####..", ".######.", "########", "##.##.##", "...##...", "...##...",
            "..####..",
        ],
    ),
    (
        "rocket",
        [
            "...##...", "..####..", "..#..#..", "..####..", ".######.", ".######.", "##....##",
            "..####..",
        ],
        [
            "...##...", "..####..", "..#..#..", "..####..", ".######.", ".######.", "##....##",
            "...##...",
        ],
    ),
    (
        "dino",
        [
            "...#####", "...##.##", "...#####", ".#######", "########", "#####...", ".##.##..",
            "..#..#..",
        ],
        [
            "...#####", "...##.##", "...#####", ".#######", "########", "#####...", "..##.##.",
            "..#...#.",
        ],
    ),
    (
        "frog",
        [
            "........", "##....##", "#.####.#", "########", "########", ".######.", "##....##",
            "........",
        ],
        [
            "##....##", "#.####.#", "########", "########", ".######.", "##....##", "#......#",
            "........",
        ],
    ),
];

/// `projectMascot(name)`: the sprite pair a name hashes to.
pub fn mascot_for(name: &str) -> (&'static str, &'static Rows, &'static Rows) {
    let mut hash: u32 = 0;
    // `charCodeAt` walks UTF-16 code units.
    for unit in name.encode_utf16() {
        hash = hash.wrapping_mul(131).wrapping_add(unit as u32);
    }
    let (name, rest, talk) = &MASCOTS[hash as usize % MASCOTS.len()];
    (name, rest, talk)
}

/// `mascotPath`: each row's filled runs as `(x, y, width)` cells.
pub fn runs(rows: &Rows) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    for (y, row) in rows.iter().enumerate() {
        let bytes = row.as_bytes();
        let mut x = 0;
        while x < bytes.len() {
            if bytes[x] != b'#' {
                x += 1;
                continue;
            }
            let mut run = 1;
            while x + run < bytes.len() && bytes[x + run] == b'#' {
                run += 1;
            }
            out.push((x, y, run));
            x += run;
        }
    }
    out
}

fn sprite(rows: &'static Rows, color: Hsla, lift: f32) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let cell = bounds.size.width / GRID as f32;
            for (x, y, width) in runs(rows) {
                let origin = point(
                    bounds.origin.x + cell * x as f32,
                    bounds.origin.y + cell * y as f32 - px(lift),
                );
                window.paint_quad(fill(
                    Bounds::new(origin, size(cell * width as f32, cell)),
                    color,
                ));
            }
        },
    )
    .size_full()
}

/// `<ProjectMascot project={name} active={…} />` at 14px.
pub fn mascot(id: impl Into<ElementId>, name: &str, color: Hsla, active: bool) -> impl IntoElement {
    let (_, rest, talk) = mascot_for(name);
    let frame = div().flex_none().size(u(14.));
    if !active {
        return frame.child(sprite(rest, color, 0.)).into_any_element();
    }
    frame
        .with_animation(
            id,
            Animation::new(Duration::from_millis(460)).repeat(),
            move |el, beat| {
                // A hard swap plus a one-pixel hop, arcade style.
                if beat < 0.5 {
                    el.child(sprite(rest, color, 0.))
                } else {
                    el.child(sprite(talk, color, 1.))
                }
            },
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_names_like_the_typescript() {
        // hash("a") = 97, 97 % 10 = 7.
        assert_eq!(mascot_for("a").0, "rocket");
        assert_eq!(mascot_for("").0, "invader");
        assert_eq!(
            mascot_for("Correctness review").0,
            mascot_for("Correctness review").0
        );
    }

    #[test]
    fn merges_filled_runs() {
        let (_, rest, _) = mascot_for("");
        let cells = runs(rest);
        assert_eq!(cells[0], (2, 0, 1));
        assert_eq!(cells[2], (1, 1, 6));
    }
}
