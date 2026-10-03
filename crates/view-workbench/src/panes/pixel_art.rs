//! Pixel sprites drawn on a grid: the project mascots from
//! src/features/projects/model/projectMascots.ts, `mascotPath`, and a
//! sprite element that paints `#` cells as filled squares.
//!
//! The mascot table matches the copy in view-composer's mascots model. View
//! crates cannot depend on each other, so it is repeated here.

use gpui::{Bounds, Hsla, IntoElement, Pixels, Styled, Window, canvas, point, px, size};

/// `MASCOT_GRID`.
pub const MASCOT_GRID: usize = 8;

/// One mascot's two frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mascot {
    pub name: &'static str,
    pub rest: [&'static str; MASCOT_GRID],
    pub talk: [&'static str; MASCOT_GRID],
}

/// `PROJECT_MASCOTS` in the order the TypeScript listed them.
pub const PROJECT_MASCOTS: [Mascot; 10] = [
    Mascot {
        name: "invader",
        rest: [
            "..#..#..", ".######.", "##.##.##", "########", ".######.", ".#.##.#.", "#.#..#.#",
            "........",
        ],
        talk: [
            "..#..#..", ".######.", "##.##.##", "########", ".######.", "#.####.#", ".#....#.",
            "#......#",
        ],
    },
    Mascot {
        name: "ghost",
        rest: [
            "..####..", ".######.", "##.##.##", "########", "########", "########", "########",
            "#.##.##.",
        ],
        talk: [
            "..####..", ".######.", "#.##.###", "########", "########", "########", "########",
            ".##.##.#",
        ],
    },
    Mascot {
        name: "robot",
        rest: [
            "...#....", ".######.", ".#.##.#.", ".######.", ".#....#.", ".######.", "..#..#..",
            "........",
        ],
        talk: [
            "....#...", ".######.", ".#.##.#.", ".######.", ".##..##.", ".######.", ".#....#.",
            "........",
        ],
    },
    Mascot {
        name: "cat",
        rest: [
            ".#....#.", ".##..##.", "########", "#.####.#", "########", "###..###", ".######.",
            "..#..#..",
        ],
        talk: [
            ".#....#.", ".##..##.", "########", "#.####.#", "########", "########", ".######.",
            ".#....#.",
        ],
    },
    Mascot {
        name: "skull",
        rest: [
            ".######.", "########", "##.##.##", "########", ".##..##.", ".######.", ".#.##.#.",
            "........",
        ],
        talk: [
            ".######.", "########", "##.##.##", "########", ".##..##.", ".######.", ".#....#.",
            "..####..",
        ],
    },
    Mascot {
        name: "crab",
        rest: [
            "#......#", ".#....#.", ".######.", "##.##.##", "########", "#.####.#", "#......#",
            "........",
        ],
        talk: [
            "#......#", "##....##", ".######.", "##.##.##", "########", ".######.", "#.#..#.#",
            "........",
        ],
    },
    Mascot {
        name: "mushroom",
        rest: [
            "..####..", ".######.", "########", "##.##.##", "########", "...##...", "...##...",
            "..####..",
        ],
        talk: [
            "........", "..####..", ".######.", "########", "##.##.##", "...##...", "...##...",
            "..####..",
        ],
    },
    Mascot {
        name: "rocket",
        rest: [
            "...##...", "..####..", "..#..#..", "..####..", ".######.", ".######.", "##....##",
            "..####..",
        ],
        talk: [
            "...##...", "..####..", "..#..#..", "..####..", ".######.", ".######.", "##....##",
            "...##...",
        ],
    },
    Mascot {
        name: "dino",
        rest: [
            "...#####", "...##.##", "...#####", ".#######", "########", "#####...", ".##.##..",
            "..#..#..",
        ],
        talk: [
            "...#####", "...##.##", "...#####", ".#######", "########", "#####...", "..##.##.",
            "..#...#.",
        ],
    },
    Mascot {
        name: "frog",
        rest: [
            "........", "##....##", "#.####.#", "########", "########", ".######.", "##....##",
            "........",
        ],
        talk: [
            "##....##", "#.####.#", "########", "########", ".######.", "##....##", "#......#",
            "........",
        ],
    },
];

/// A filled run in one row: `(x, y, width)`. `mascotPath` merged each
/// row's runs into one rect so the SVG path stayed short.
pub fn pixel_rects<S: AsRef<str>>(rows: &[S]) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    for (y, row) in rows.iter().enumerate() {
        let bytes = row.as_ref().as_bytes();
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

/// `projectMascot`: an explicit `name` wins; otherwise a stable hash of the
/// project name picks one.
pub fn project_mascot(project: &str, name: Option<&str>) -> Mascot {
    if let Some(chosen) = name.and_then(|name| PROJECT_MASCOTS.iter().find(|m| m.name == name)) {
        return *chosen;
    }
    let mut hash: u32 = 0;
    for unit in project.encode_utf16() {
        hash = hash.wrapping_mul(131).wrapping_add(unit as u32);
    }
    PROJECT_MASCOTS[hash as usize % PROJECT_MASCOTS.len()]
}

/// Paints `rows` scaled to fit `bounds`, keeping square cells.
pub fn paint_sprite<S: AsRef<str>>(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    rows: &[S],
    color: Hsla,
) {
    let columns = rows.iter().map(|row| row.as_ref().len()).max().unwrap_or(0);
    if columns == 0 || rows.is_empty() {
        return;
    }
    let cell = (f32::from(bounds.size.width) / columns as f32)
        .min(f32::from(bounds.size.height) / rows.len() as f32);
    let origin = point(
        bounds.origin.x + (bounds.size.width - px(cell * columns as f32)) / 2.0,
        bounds.origin.y + (bounds.size.height - px(cell * rows.len() as f32)) / 2.0,
    );
    for (x, y, width) in pixel_rects(rows) {
        window.paint_quad(gpui::fill(
            Bounds::new(
                point(
                    origin.x + px(x as f32 * cell),
                    origin.y + px(y as f32 * cell),
                ),
                size(px(width as f32 * cell), px(cell)),
            ),
            color,
        ));
    }
}

/// A sprite element: `rows` drawn in `color`, filling the element's box.
pub fn sprite(
    rows: impl IntoIterator<Item = &'static str>,
    color: Hsla,
) -> impl IntoElement + Styled {
    let rows: Vec<&'static str> = rows.into_iter().collect();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| paint_sprite(window, bounds, &rows, color),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_each_rows_filled_runs() {
        assert_eq!(
            pixel_rects(&["##.#....", "........", ".######."]),
            vec![(0, 0, 2), (3, 0, 1), (1, 2, 6)]
        );
    }

    #[test]
    fn picks_a_stable_mascot_and_honors_an_explicit_name() {
        assert_eq!(
            project_mascot("monocode", None),
            project_mascot("monocode", None)
        );
        assert_eq!(project_mascot("anything", Some("frog")).name, "frog");
        assert_eq!(project_mascot("a", None).name, PROJECT_MASCOTS[7].name);
    }
}
