//! ProjectMascot / SubagentMascot: the React transcript's two-frame pixel faces.
use crate::theme::content;
use gpui::{AnyElement, canvas, div, prelude::*, px, rgb};
use monocode_core::tool::ToolState;
use std::time::Duration;

type Frame = [&'static str; 8];

// Keep this order in sync with projectMascots.ts: the name hash indexes it.
const REST: [Frame; 10] = [
    [
        "..#..#..", ".######.", "##.##.##", "########", ".######.", ".#.##.#.", "#.#..#.#",
        "........",
    ],
    [
        "..####..", ".######.", "##.##.##", "########", "########", "########", "########",
        "#.##.##.",
    ],
    [
        "...#....", ".######.", ".#.##.#.", ".######.", ".#....#.", ".######.", "..#..#..",
        "........",
    ],
    [
        ".#....#.", ".##..##.", "########", "#.####.#", "########", "###..###", ".######.",
        "..#..#..",
    ],
    [
        ".######.", "########", "##.##.##", "########", ".##..##.", ".######.", ".#.##.#.",
        "........",
    ],
    [
        "#......#", ".#....#.", ".######.", "##.##.##", "########", "#.####.#", "#......#",
        "........",
    ],
    [
        "..####..", ".######.", "########", "##.##.##", "########", "...##...", "...##...",
        "..####..",
    ],
    [
        "...##...", "..####..", "..#..#..", "..####..", ".######.", ".######.", "##....##",
        "..####..",
    ],
    [
        "...#####", "...##.##", "...#####", ".#######", "########", "#####...", ".##.##..",
        "..#..#..",
    ],
    [
        "........", "##....##", "#.####.#", "########", "########", ".######.", "##....##",
        "........",
    ],
];

const TALK: [Frame; 10] = [
    [
        "..#..#..", ".######.", "##.##.##", "########", ".######.", "#.####.#", ".#....#.",
        "#......#",
    ],
    [
        "..####..", ".######.", "#.##.###", "########", "########", "########", "########",
        ".##.##.#",
    ],
    [
        "....#...", ".######.", ".#.##.#.", ".######.", ".##..##.", ".######.", ".#....#.",
        "........",
    ],
    [
        ".#....#.", ".##..##.", "########", "#.####.#", "########", "########", ".######.",
        ".#....#.",
    ],
    [
        ".######.", "########", "##.##.##", "########", ".##..##.", ".######.", ".#....#.",
        "..####..",
    ],
    [
        "#......#", "##....##", ".######.", "##.##.##", "########", ".######.", "#.#..#.#",
        "........",
    ],
    [
        "........", "..####..", ".######.", "########", "##.##.##", "...##...", "...##...",
        "..####..",
    ],
    [
        "...##...", "..####..", "..#..#..", "..####..", ".######.", ".######.", "##....##",
        "...##...",
    ],
    [
        "...#####", "...##.##", "...#####", ".#######", "########", "#####...", "..##.##.",
        "..#...#.",
    ],
    [
        "##....##", "#.####.#", "########", "########", ".######.", "##....##", "#......#",
        "........",
    ],
];

/// A 12px sprite centered in the same 14px icon slot as the React row.
/// Activity is independent of state: a pending run can already have settled.
pub(super) fn subagent_mascot(name: &str, state: &ToolState, active: bool) -> AnyElement {
    let color = match state {
        ToolState::Failed | ToolState::Cancelled => rgb(0xf87171).into(),
        ToolState::Running => content(0.70),
        ToolState::Completed => content(0.45),
    };
    div()
        .size(px(14.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .text_color(color)
        .child(project_mascot(name, active))
        .into_any_element()
}

fn project_mascot(name: &str, active: bool) -> AnyElement {
    // JavaScript charCodeAt iterates UTF-16 code units, rather than UTF-8 bytes
    // or Unicode scalars. Wrapping u32 arithmetic mirrors `>>> 0` exactly.
    let hash = name.encode_utf16().fold(0u32, |hash, ch| {
        hash.wrapping_mul(131).wrapping_add(u32::from(ch))
    });
    let index = hash as usize % REST.len();
    let slot = div().size(px(12.)).flex_shrink_0();
    if active {
        // Two poses, so the low clock rate is plenty. Reduced motion holds
        // the first pose and stops asking for frames.
        crate::pulse::looping(
            "subagent-mascot",
            Duration::from_millis(460),
            crate::pulse::Rate::Low,
            move |phase| slot.child(sprite(index, phase >= 0.5)),
        )
        .into_any_element()
    } else {
        slot.child(sprite(index, false)).into_any_element()
    }
}

fn sprite(index: usize, talk: bool) -> AnyElement {
    canvas(
        |_, window, _| window.text_style().color,
        move |bounds, color, window, _| {
            let rows = if talk { &TALK[index] } else { &REST[index] };
            let unit = bounds.size.width / 8.;
            let hop = if talk { px(-1.) } else { px(0.) };
            let origin = bounds.origin + gpui::point(px(0.), hop);
            // SVG's crispEdges snaps each shared boundary to physical pixels.
            // Merge filled runs as mascotPath does, preserving transparent eyes.
            let scale = window.scale_factor();
            let snap = |position: gpui::Pixels| (position * scale).round() / scale;
            for (y, row) in rows.iter().enumerate() {
                let pixels = row.as_bytes();
                let mut x = 0;
                while x < pixels.len() {
                    if pixels[x] != b'#' {
                        x += 1;
                        continue;
                    }
                    let begin = x;
                    while x < pixels.len() && pixels[x] == b'#' {
                        x += 1;
                    }
                    let left = snap(origin.x + unit * begin as f32);
                    let right = snap(origin.x + unit * x as f32);
                    let top = snap(origin.y + unit * y as f32);
                    let bottom = snap(origin.y + unit * (y + 1) as f32);
                    window.paint_quad(gpui::fill(
                        gpui::Bounds::new(
                            gpui::point(left, top),
                            gpui::size(right - left, bottom - top),
                        ),
                        color,
                    ));
                }
            }
        },
    )
    .size_full()
    .into_any_element()
}
