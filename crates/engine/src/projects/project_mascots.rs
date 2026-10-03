//! Port of src/features/projects/model/projectMascots.ts.
//!
//! 8 by 8 pixel mascots used in place of a project's color dot. `#` paints
//! the project color and `.` stays transparent so the surface shows through.
//!
//! Each mascot has two frames: `rest`, and `talk`, which the rail swaps in
//! on a loop while the project has a turn in flight.

use std::sync::LazyLock;

/// `GRID`: rows per frame and cells per row.
pub const MASCOT_GRID: usize = 8;

type MascotRows = [&'static str; MASCOT_GRID];

/// `ProjectMascot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMascot {
    pub name: &'static str,
    pub rest: MascotRows,
    pub talk: MascotRows,
    /// SVG path data over an 8 by 8 view box, one per frame.
    pub rest_path: String,
    pub talk_path: String,
}

/// `REST` and `TALK`, in the order `Object.entries(REST)` lists them.
const FRAMES: [(&str, MascotRows, MascotRows); 10] = [
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

/// `mascotPath`: each row's filled runs merge into one rect so the path
/// stays short.
pub fn mascot_path<S: AsRef<str>>(rows: &[S]) -> String {
    let mut path = String::new();
    for (y, row) in rows.iter().enumerate() {
        let row = row.as_ref().as_bytes();
        let mut x = 0;
        while x < row.len() {
            if row[x] != b'#' {
                x += 1;
                continue;
            }
            let mut run = 1;
            while row.get(x + run) == Some(&b'#') {
                run += 1;
            }
            path.push_str(&format!("M{x} {y}h{run}v1h-{run}z"));
            x += run;
        }
    }
    path
}

/// `PROJECT_MASCOTS`.
pub static PROJECT_MASCOTS: LazyLock<Vec<ProjectMascot>> = LazyLock::new(|| {
    FRAMES
        .iter()
        .map(|(name, rest, talk)| ProjectMascot {
            name,
            rest: *rest,
            talk: *talk,
            rest_path: mascot_path(rest),
            talk_path: mascot_path(talk),
        })
        .collect()
});

/// Whether `name` is a mascot on the roster.
pub fn is_project_mascot(name: &str) -> bool {
    FRAMES.iter().any(|(entry, _, _)| *entry == name)
}

/// `projectMascot`: a stable pick per project, mixed differently from the
/// color hash so a project's mascot and color vary independently. An
/// explicit `name` wins; an unknown one falls back to the hash.
pub fn project_mascot(project: &str, name: Option<&str>) -> &'static ProjectMascot {
    let roster = &*PROJECT_MASCOTS;
    if let Some(chosen) = name
        .filter(|name| !name.is_empty())
        .and_then(|name| roster.iter().find(|mascot| mascot.name == name))
    {
        return chosen;
    }
    let mut hash: u32 = 0;
    for unit in project.encode_utf16() {
        hash = hash.wrapping_mul(131).wrapping_add(u32::from(unit));
    }
    &roster[hash as usize % roster.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_every_sprite_on_the_shared_grid() {
        assert_eq!(PROJECT_MASCOTS.len(), 10);
        for mascot in PROJECT_MASCOTS.iter() {
            for frame in [&mascot.rest, &mascot.talk] {
                assert_eq!(frame.len(), MASCOT_GRID);
                for row in frame {
                    assert_eq!(row.len(), MASCOT_GRID);
                    assert!(row.bytes().all(|cell| cell == b'#' || cell == b'.'));
                }
            }
            assert!(!mascot.rest_path.is_empty());
            assert!(!mascot.talk_path.is_empty());
            assert_ne!(mascot.talk_path, mascot.rest_path);
        }
    }

    #[test]
    fn merges_filled_runs_into_one_rect_each() {
        assert_eq!(mascot_path(&["##..###."]), "M0 0h2v1h-2zM4 0h3v1h-3z");
        assert_eq!(mascot_path(&["........"]), "");
    }

    #[test]
    fn picks_the_same_mascot_for_the_same_project() {
        assert_eq!(
            project_mascot("~/code/monocode", None).name,
            project_mascot("~/code/monocode", None).name
        );
    }

    #[test]
    fn honors_an_explicit_pick_and_ignores_unknown_names() {
        assert_eq!(project_mascot("alpha", Some("ghost")).name, "ghost");
        assert_eq!(
            project_mascot("alpha", Some("nope")).name,
            project_mascot("alpha", None).name
        );
    }

    #[test]
    fn spreads_projects_across_the_roster() {
        let names: std::collections::HashSet<&str> = [
            "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
        ]
        .into_iter()
        .map(|project| project_mascot(project, None).name)
        .collect();
        assert!(names.len() > 3);
    }

    #[test]
    fn hash_matches_the_typescript_pick() {
        // (0 * 131 + 97) = 97 for "a"; 97 % 10 = 7, the eighth mascot.
        assert_eq!(project_mascot("a", None).name, "rocket");
        // "ab": 97 * 131 + 98 = 12805; 12805 % 10 = 5.
        assert_eq!(project_mascot("ab", None).name, "crab");
    }
}
