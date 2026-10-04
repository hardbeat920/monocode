//! Port of the sprites in src/features/projects/model/projectMascots.ts,
//! src/features/projects/ui/ProjectMascot.tsx, and the happy mascot the
//! usage popover shows beside banked resets (`BankedResetMascot` in
//! UsageProviderChip.tsx and `.reset-mascot-*` in index.css).

use std::time::Duration;

use gpui::{
    Animation, AnimationExt as _, AnyElement, Bounds, BoxShadow, ElementId, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, PathBuilder, Styled as _, canvas,
    div, fill, point, px, size,
};
use monocode_ui::theme::CubicBezier;
use monocode_ui::u;

const GRID: usize = 8;

type Rows = [&'static str; GRID];

/// `REST`, in the order `Object.entries` lists them. The talk frames only
/// animate busy projects, which these views never show.
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

/// `projectMascot(project, name)`: an explicit, known `name` wins;
/// otherwise the pick hashed from `project`. Returns the name and the rest
/// frame.
pub fn project_mascot(project: &str, name: Option<&str>) -> (&'static str, &'static Rows) {
    if let Some((name, rows)) = name.and_then(|name| MASCOTS.iter().find(|(id, _)| *id == name)) {
        return (name, rows);
    }
    let mut hash: u32 = 0;
    // `charCodeAt` walks UTF-16 code units.
    for unit in project.encode_utf16() {
        hash = hash.wrapping_mul(131).wrapping_add(u32::from(unit));
    }
    let (name, rows) = &MASCOTS[hash as usize % MASCOTS.len()];
    (name, rows)
}

/// The filled cells of a sprite, as an 8 by 8 grid.
type Cells = [[bool; GRID]; GRID];

fn cells(rows: &Rows) -> Cells {
    let mut out = [[false; GRID]; GRID];
    for (y, row) in rows.iter().enumerate() {
        for (x, byte) in row.bytes().enumerate().take(GRID) {
            out[y][x] = byte == b'#';
        }
    }
    out
}

/// `mascotFacePlatePath`: fill only the middle of each face row (2 to 5),
/// from its first to its last filled cell.
fn face_plate(rows: &Rows) -> Cells {
    let mut out = [[false; GRID]; GRID];
    for (y, row) in rows.iter().enumerate() {
        if !(2..=5).contains(&y) {
            continue;
        }
        let (Some(first), Some(last)) = (row.find('#'), row.rfind('#')) else {
            continue;
        };
        for cell in out[y].iter_mut().take(last + 1).skip(first) {
            *cell = true;
        }
    }
    out
}

/// The happy face: the rest frame plus its face plate, with the eyes and
/// the smile cut back out (the SVG mask).
pub fn happy_cells(rows: &Rows) -> Cells {
    let rest = cells(rows);
    let plate = face_plate(rows);
    let mut out = [[false; GRID]; GRID];
    for y in 0..GRID {
        for x in 0..GRID {
            out[y][x] = rest[y][x] || plate[y][x];
        }
    }
    for (x, y) in [(2, 3), (5, 3), (2, 4), (5, 4), (3, 5), (4, 5)] {
        out[y][x] = false;
    }
    out
}

/// Each row's filled runs as `(x, y, width)`, as `mascotPath` merged them.
fn runs(cells: &Cells) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    for (y, row) in cells.iter().enumerate() {
        let mut x = 0;
        while x < GRID {
            if !row[x] {
                x += 1;
                continue;
            }
            let mut run = 1;
            while x + run < GRID && row[x + run] {
                run += 1;
            }
            out.push((x, y, run));
            x += run;
        }
    }
    out
}

fn sprite(cells: Cells, color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let cell = bounds.size.width / GRID as f32;
            for (x, y, width) in runs(&cells) {
                let origin = point(
                    bounds.origin.x + cell * x as f32,
                    bounds.origin.y + cell * y as f32,
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

/// `<ProjectMascot>` at `size` CSS px in `color`.
pub fn project_mascot_icon(
    project: &str,
    name: Option<&str>,
    color: Hsla,
    size: f32,
) -> AnyElement {
    let (_, rows) = project_mascot(project, name);
    div()
        .flex_none()
        .size(u(size))
        .child(sprite(cells(rows), color))
        .into_any_element()
}

/// A diamond spark (`.reset-mascot-spark`, a square turned 45 degrees).
fn spark(color: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let center = bounds.center();
            let half = bounds.size.width / 2.;
            let mut path = PathBuilder::fill();
            path.move_to(point(center.x, center.y - half));
            path.line_to(point(center.x + half, center.y));
            path.line_to(point(center.x, center.y + half));
            path.line_to(point(center.x - half, center.y));
            path.close();
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        },
    )
    .size_full()
}

/// `reset-mascot-happy-in`: 720ms. GPUI cannot scale or rotate a div, so
/// the hop keeps the fade and the vertical motion.
fn happy_offset(t: f32) -> (f32, f32) {
    let ease = CubicBezier(0.2, 0.9, 0.25, 1.0).ease(t);
    let keys = [
        (0.0, 0.0, 9.0),
        (0.30, 1.0, 0.0),
        (0.50, 1.0, -10.0),
        (0.68, 1.0, 0.0),
        (0.84, 1.0, -4.0),
        (1.0, 1.0, 0.0),
    ];
    for pair in keys.windows(2) {
        let (t0, o0, y0) = pair[0];
        let (t1, o1, y1) = pair[1];
        if ease <= t1 {
            let local = if t1 > t0 {
                (ease - t0) / (t1 - t0)
            } else {
                1.0
            };
            return (o0 + (o1 - o0) * local, y0 + (y1 - y0) * local);
        }
    }
    (1.0, 0.0)
}

/// `BankedResetMascot`: the project's mascot, smiling, with a glow and two
/// sparks. `animate` false draws the last frame, for screenshots.
pub fn banked_reset_mascot(
    id: impl Into<ElementId>,
    project: &str,
    name: Option<&str>,
    color: Hsla,
    animate: bool,
) -> AnyElement {
    let id: ElementId = id.into();
    let (mascot_name, rows) = project_mascot(project, name);
    let happy = happy_cells(rows);
    let child =
        |name: &'static str| ElementId::NamedChild(std::sync::Arc::new(id.clone()), name.into());
    let glow = div()
        .absolute()
        .top(u(14.))
        .left(u(8.))
        .right(u(8.))
        .bottom(u(3.))
        .rounded_full()
        .shadow(vec![BoxShadow {
            color: Hsla {
                a: color.a * 0.14,
                ..color
            },
            offset: point(px(0.), px(0.)),
            blur_radius: px(12.),
            spread_radius: px(0.),
            inset: false,
        }])
        .bg(Hsla {
            a: color.a * 0.10,
            ..color
        });
    let spark_color = Hsla {
        a: color.a * 0.7,
        ..color
    };
    let spark_a = div()
        .absolute()
        .top(u(8.))
        .right(u(7.))
        .size(u(7.))
        .child(spark(spark_color));
    let spark_b = div()
        .absolute()
        .top(u(19.))
        .left(u(3.))
        .size(u(4.))
        .child(spark(spark_color));
    let sprite_box = div().size(u(56.)).child(sprite(happy, color));
    let sprite_box: AnyElement = if animate {
        sprite_box
            .with_animation(
                child("hop"),
                Animation::new(Duration::from_millis(720)),
                |el, t| {
                    let (opacity, lift) = happy_offset(t);
                    el.opacity(opacity).mt(u(lift))
                },
            )
            .into_any_element()
    } else {
        sprite_box.into_any_element()
    };
    let (spark_a, spark_b): (AnyElement, AnyElement) = if animate {
        let fade = |el: gpui::Div, name: &'static str, delay: f32| {
            el.with_animation(
                child(name),
                Animation::new(Duration::from_millis(620 + (delay * 1000.) as u64)),
                move |el, t| {
                    // The spark waits out its delay, then fades in.
                    let total = 0.62 + delay;
                    let local = ((t * total - delay) / 0.62).clamp(0.0, 1.0);
                    el.opacity(CubicBezier(0.16, 1.0, 0.3, 1.0).ease(local))
                },
            )
            .into_any_element()
        };
        (
            fade(spark_a, "spark-a", 0.26),
            fade(spark_b, "spark-b", 0.36),
        )
    } else {
        (spark_a.into_any_element(), spark_b.into_any_element())
    };
    let selector = format!("mascot:happy:{mascot_name}");
    div()
        .absolute()
        .right(u(8.))
        .bottom(u(5.))
        .size(u(68.))
        .flex()
        .items_center()
        .justify_center()
        .debug_selector(move || selector)
        .child(glow)
        .child(spark_a)
        .child(spark_b)
        .child(sprite_box)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_names_like_the_typescript() {
        // hash("a") = 97, 97 % 10 = 7.
        assert_eq!(project_mascot("a", None).0, "rocket");
        assert_eq!(project_mascot("", None).0, "invader");
        assert_eq!(project_mascot("anything", Some("cat")).0, "cat");
        assert_eq!(project_mascot("a", Some("unknown")).0, "rocket");
    }

    #[test]
    fn cuts_the_smile_out_of_the_face_plate() {
        let (_, rows) = project_mascot("", Some("ghost"));
        let happy = happy_cells(rows);
        // The ghost's eye holes in row 2 fill in; the new eyes are cut below.
        assert!(happy[2][2]);
        assert!(!happy[3][2] && !happy[3][5] && !happy[4][2] && !happy[4][5]);
        assert!(!happy[5][3] && !happy[5][4]);
        assert!(happy[5][2] && happy[5][5]);
        // Rows outside the face keep the sprite as drawn.
        assert!(!happy[0][0] && happy[0][2]);
    }
}
