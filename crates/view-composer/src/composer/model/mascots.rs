//! Port of src/features/projects/model/projectMascots.ts: the 8x8 pixel
//! mascots that stand in for a project's color dot. The composer runner
//! draws one patrolling the composer's top edge.
//!
//! `#` paints the project color and `.` stays transparent. Each mascot has a
//! `rest` frame and a `talk` frame swapped in while a turn is live.

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

/// A filled run in one row: `(x, y, width)` on the 8x8 grid. `mascotPath`
/// merged each row's runs into one rect so the SVG path stays short.
pub fn mascot_rects(rows: &[&str]) -> Vec<(usize, usize, usize)> {
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

/// `COIN_FACE_PATH` and friends from composerRunner.ts, as grid rows.
pub const COIN_FACE: [&str; 8] = [
    "........", "..####..", ".######.", "########", "########", ".######.", "..####..", "........",
];
pub const COIN_EDGE: [&str; 8] = [
    "........", "...##...", "...##...", "...##...", "...##...", "...##...", "...##...", "........",
];
pub const STAR_FACE: [&str; 8] = [
    "...##...", "...##...", "..####..", "########", "########", "..####..", "...##...", "...##...",
];
pub const STAR_EDGE: [&str; 8] = [
    "........", "...##...", "...##...", "..####..", "..####..", "...##...", "...##...", "........",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_each_rows_filled_runs() {
        assert_eq!(
            mascot_rects(&["##.#....", "........", ".######."]),
            vec![(0, 0, 2), (3, 0, 1), (1, 2, 6)]
        );
    }

    #[test]
    fn picks_a_stable_mascot_and_honors_an_explicit_name() {
        assert_eq!(
            project_mascot("monocode", None),
            project_mascot("monocode", None)
        );
        assert_eq!(project_mascot("monocode", Some("frog")).name, "frog");
        assert_eq!(
            project_mascot("monocode", Some("nope")),
            project_mascot("monocode", None)
        );
        // `(0 * 131 + 97) % 10` for "a".
        assert_eq!(project_mascot("a", None).name, PROJECT_MASCOTS[7].name);
    }

    #[test]
    fn every_frame_is_eight_by_eight() {
        for mascot in PROJECT_MASCOTS {
            for row in mascot.rest.iter().chain(mascot.talk.iter()) {
                assert_eq!(row.len(), MASCOT_GRID, "{}", mascot.name);
            }
        }
    }
}
