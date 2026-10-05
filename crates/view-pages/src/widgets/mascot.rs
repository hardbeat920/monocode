//! Port of src/features/projects/ui/ProjectMascot.tsx and the sprite table
//! in src/features/projects/model/projectMascots.ts: an 8 by 8 pixel mascot
//! standing in for a project's color dot.
//!
//! The engine's projects package owns the same table; the view keeps its own
//! copy because it does not link the engine.

use gpui::{Bounds, Hsla, IntoElement, Styled as _, canvas, fill, point, size};

const GRID: usize = 8;

type Rows = [&'static str; GRID];

/// The resting frame of each mascot, in the order `Object.entries` lists them.
const MASCOTS: [(&str, Rows); 10] = [
    (
        "invader",
        [
            "..#..#..", ".######.", "##.##.##", "########", ".######.", ".#.##.#.", "#.#..#.#",
            "........",
        ],
    ),
    (
        "ghost",
        [
            "..####..", ".######.", "##.##.##", "########", "########", "########", "########",
            "#.##.##.",
        ],
    ),
    (
        "robot",
        [
            "...#....", ".######.", ".#.##.#.", ".######.", ".#....#.", ".######.", "..#..#..",
            "........",
        ],
    ),
    (
        "cat",
        [
            ".#....#.", ".##..##.", "########", "#.####.#", "########", "###..###", ".######.",
            "..#..#..",
        ],
    ),
    (
        "skull",
        [
            ".######.", "########", "##.##.##", "########", ".##..##.", ".######.", ".#.##.#.",
            "........",
        ],
    ),
    (
        "crab",
        [
            "#......#", ".#....#.", ".######.", "##.##.##", "########", "#.####.#", "#......#",
            "........",
        ],
    ),
    (
        "mushroom",
        [
            "..####..", ".######.", "########", "##.##.##", "########", "...##...", "...##...",
            "..####..",
        ],
    ),
    (
        "rocket",
        [
            "...##...", "..####..", "..#..#..", "..####..", ".######.", ".######.", "##....##",
            "..####..",
        ],
    ),
    (
        "dino",
        [
            "...#####", "...##.##", "...#####", ".#######", "########", "#####...", ".##.##..",
            "..#..#..",
        ],
    ),
    (
        "frog",
        [
            "........", "##....##", "#.####.#", "########", "########", ".######.", "##....##",
            "........",
        ],
    ),
];

/// `projectMascot(project, name)`: an explicit pick wins, an unknown one
/// falls back to the hash of the project name.
pub fn mascot_name(project: &str, name: Option<&str>) -> &'static str {
    mascot_rows(project, name).0
}

fn mascot_rows(project: &str, name: Option<&str>) -> (&'static str, &'static Rows) {
    if let Some((found, rows)) = name
        .filter(|name| !name.is_empty())
        .and_then(|name| MASCOTS.iter().find(|(entry, _)| *entry == name))
    {
        return (found, rows);
    }
    let mut hash: u32 = 0;
    // `charCodeAt` walks UTF-16 code units.
    for unit in project.encode_utf16() {
        hash = hash.wrapping_mul(131).wrapping_add(u32::from(unit));
    }
    let (name, rows) = &MASCOTS[hash as usize % MASCOTS.len()];
    (name, rows)
}

/// `mascotPath`: each row's filled runs as `(x, y, width)` cells.
fn runs(rows: &Rows) -> Vec<(usize, usize, usize)> {
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

/// The mascot sprite, filling its parent. Size the parent (`size-3` is
/// `u(12.)`).
pub fn mascot_sprite(project: &str, name: Option<&str>, color: Hsla) -> impl IntoElement {
    let (_, rows) = mascot_rows(project, name);
    let cells = runs(rows);
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let cell = bounds.size.width / GRID as f32;
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
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_names_like_the_typescript() {
        assert_eq!(mascot_name("a", None), "rocket");
        assert_eq!(mascot_name("ab", None), "crab");
        assert_eq!(mascot_name("alpha", Some("ghost")), "ghost");
        assert_eq!(
            mascot_name("alpha", Some("nope")),
            mascot_name("alpha", None)
        );
    }

    #[test]
    fn merges_filled_runs() {
        let (_, rows) = mascot_rows("", None);
        let cells = runs(rows);
        assert_eq!(cells[0], (2, 0, 1));
        assert_eq!(cells[2], (1, 1, 6));
    }
}
