//! Drawing for the git gutter, the removed-lines panel, and the overview ruler.
//!
//! Port of the view parts of src/features/files/editor/editorGit.ts:
//! `gitGutter` and `cm-gitInsertedLine` become markers and line tints,
//! `DeletedLinesWidget` and `gitHunkActions` become a panel that opens when
//! a marker is clicked, and `gitOverview` becomes the ruler on the right.
//! The ruler also marks search matches and the cursor line.
//!
//! Everything paints in one canvas laid over the editor. The canvas paints
//! after gpui-base's text element, so it reads that frame's line geometry
//! through `EditorState::range_to_bounds`.

use std::{ops::Range, sync::Arc};

use gpui::{
    App, Bounds, ContentMask, Context, DispatchPhase, Hsla, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, PathBuilder, Pixels, SharedString, Styled, TextAlign, TextRun,
    WeakEntity, Window, canvas, fill, point, px, quad, size,
};
use gpui_base::input::RopeExt as _;

use crate::{
    code_editor::{CodeEditor, GIT_GUTTER_WIDTH, GitSnapshot},
    git_diff::ChangeKind,
    icons::{IconKind, paint_icon},
    theme::EditorTheme,
};

/// `--editor-scrollbar-width`.
const RULER_WIDTH: f32 = 18.;
/// Removed lines shown before the panel says how many more there are.
const PEEK_MAX_LINES: usize = 12;
const PEEK_BAR_HEIGHT: f32 = 26.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PeekAction {
    Revert,
    Stage,
    Comment,
    Close,
}

struct Inputs {
    theme: EditorTheme,
    git: Arc<GitSnapshot>,
    peek: Option<usize>,
    matches: Arc<Vec<Range<usize>>>,
    can_revert: bool,
    can_stage: bool,
    can_comment: bool,
    stage_busy: bool,
}

/// The canvas laid over the editor.
pub(crate) fn overlay(
    editor: &CodeEditor,
    _: &mut Window,
    cx: &mut Context<CodeEditor>,
) -> impl IntoElement {
    let state = editor.state.clone();
    let weak = cx.entity().downgrade();
    let inputs = Inputs {
        theme: editor.theme.clone(),
        git: editor.git.clone(),
        peek: editor.peek,
        matches: if editor.find.open {
            editor.find.matches.clone()
        } else {
            Arc::new(Vec::new())
        },
        can_revert: !editor.is_read_only(),
        can_stage: editor.can_stage(),
        can_comment: editor.can_comment(),
        stage_busy: editor.stage_busy,
    };
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            paint_overlay(&state, weak, inputs, bounds, window, cx);
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Geometry of one buffer row in window coordinates.
#[derive(Debug, Clone, Copy)]
struct Row {
    top: Pixels,
    bottom: Pixels,
}

fn paint_overlay(
    state: &gpui::Entity<gpui_base::input::EditorState>,
    weak: WeakEntity<CodeEditor>,
    inputs: Inputs,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let theme = &inputs.theme;
    let git = &inputs.git;

    // Read everything from the editor first; painting text needs `cx` mutably.
    let (rows, visible, text_left, gutter_left, total_lines, cursor_row, match_rows, peek_rows) = {
        let st = state.read(cx);
        let (Some(_), Some(visible)) = (st.line_height(), st.visible_row_range()) else {
            return;
        };
        let text = st.text();
        let input_bounds = st.input_bounds();
        let row_at = |row: usize| -> Option<(Row, Pixels)> {
            let start = text.line_start_offset(row);
            let end = text.line_end_offset(row);
            let found = st.range_to_bounds(&(start..end))?;
            Some((
                Row {
                    top: found.top(),
                    bottom: found.bottom(),
                },
                found.left(),
            ))
        };
        let mut rows = Vec::with_capacity(visible.len());
        let mut text_left = None;
        for row in visible.clone() {
            let geometry = row_at(row);
            if text_left.is_none() {
                text_left = geometry.map(|(_, left)| left - st.scroll_offset().x);
            }
            rows.push(geometry.map(|(row, _)| row));
        }
        let total_lines = text.lines_len().max(1);
        let cursor_row = text.offset_to_point(st.cursor()).row;
        let match_rows: Vec<usize> = inputs
            .matches
            .iter()
            .map(|range| text.offset_to_point(range.start).row)
            .collect();
        let peek_rows = inputs.peek.and_then(|index| git.marked.get(index).cloned());
        (
            rows,
            visible,
            text_left.unwrap_or(input_bounds.left()),
            input_bounds.left() - px(GIT_GUTTER_WIDTH),
            total_lines,
            cursor_row,
            match_rows,
            peek_rows,
        )
    };
    let row = |index: usize| -> Option<Row> {
        index
            .checked_sub(visible.start)
            .and_then(|offset| rows.get(offset).copied().flatten())
    };
    let ruler_left = bounds.right() - px(RULER_WIDTH);
    let marker_x = gutter_left + px(GIT_GUTTER_WIDTH - 3. - 4.);
    let content_left = text_left - px(4.);

    let mut markers: Vec<(Bounds<Pixels>, usize)> = Vec::new();
    // `.cm-gutters` border-right.
    window.paint_quad(fill(
        Bounds::from_corners(
            point(content_left - px(2.), bounds.top()),
            point(content_left - px(1.), bounds.bottom()),
        ),
        theme.stroke,
    ));
    window.with_content_mask(Some(ContentMask { bounds }), |window| {
        for (index, marked) in git.marked.iter().enumerate() {
            let chunk = &git.chunks[index];
            let kind = git.kind(index);
            if chunk.is_insertion() {
                let first = marked.start.max(visible.start);
                let last = marked.end.min(visible.end);
                let span: Vec<Row> = (first..last).filter_map(&row).collect();
                let (Some(top), Some(bottom)) = (span.first(), span.last()) else {
                    continue;
                };
                let (top, bottom) = (top.top, bottom.bottom);
                // `cm-gitInsertedLine`: a tint over the line and a 3px bar.
                window.paint_quad(fill(
                    Bounds::from_corners(point(content_left, top), point(ruler_left, bottom)),
                    theme.inserted_line,
                ));
                window.paint_quad(fill(
                    Bounds::new(point(content_left, top), size(px(3.), bottom - top)),
                    theme.git_added,
                ));
                // The gutter marker.
                let color = if kind == ChangeKind::Modified {
                    theme.accent
                } else {
                    theme.git_added
                };
                window.paint_quad(fill(
                    Bounds::new(point(marker_x, top), size(px(3.), bottom - top)),
                    color,
                ));
                markers.push((
                    Bounds::from_corners(
                        point(gutter_left, top),
                        point(gutter_left + px(GIT_GUTTER_WIDTH), bottom),
                    ),
                    index,
                ));
            } else {
                let Some(line) = row(marked.start) else {
                    continue;
                };
                // A pure deletion: a triangle on the line boundary.
                let y = line.top;
                let mut path = PathBuilder::fill();
                path.move_to(point(marker_x - px(1.), y - px(4.)));
                path.line_to(point(marker_x + px(5.), y));
                path.line_to(point(marker_x - px(1.), y + px(4.)));
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, theme.git_deleted);
                }
                markers.push((
                    Bounds::from_corners(
                        point(gutter_left, y - px(6.)),
                        point(gutter_left + px(GIT_GUTTER_WIDTH), y + px(6.)),
                    ),
                    index,
                ));
            }
        }
    });

    paint_ruler(
        theme,
        git,
        &match_rows,
        cursor_row,
        total_lines,
        bounds,
        window,
    );

    // The removed-lines panel.
    let mut buttons: Vec<(Bounds<Pixels>, PeekAction)> = Vec::new();
    let mut panel_bounds = None;
    if let (Some(index), Some(marked)) = (inputs.peek, peek_rows) {
        let chunk = git.chunks[index];
        let anchor = if chunk.is_insertion() {
            let last = marked.end.saturating_sub(1);
            row(last).map(|row| row.bottom).or_else(|| {
                // The hunk ends below the viewport: hide the panel.
                (last < visible.start).then_some(bounds.top())
            })
        } else {
            row(marked.start).map(|row| row.top)
        };
        if let Some(anchor) = anchor {
            let line_height = theme.line_height_px();
            let deleted = &git.deleted[index];
            let shown = deleted.len().min(PEEK_MAX_LINES);
            let more = deleted.len() - shown;
            let body = line_height * (shown + usize::from(more > 0)) as f32;
            let height = body + px(PEEK_BAR_HEIGHT) + px(2.);
            let left = content_left;
            let right = ruler_left - px(4.);
            let mut top = anchor + px(2.);
            if top + height > bounds.bottom() {
                let chunk_top = row(marked.start).map_or(anchor, |row| row.top);
                top = (chunk_top - height - px(2.)).max(bounds.top());
            }
            let panel = Bounds::from_corners(point(left, top), point(right, top + height));
            panel_bounds = Some(panel);
            buttons = paint_peek(&inputs, index, panel, line_height, window, cx);
        }
    }

    // Mouse: markers toggle the panel, panel buttons run their action.
    let painted_hover = buttons
        .iter()
        .position(|(bounds, _)| bounds.contains(&window.mouse_position()));
    let move_buttons = buttons.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
        if phase != DispatchPhase::Bubble {
            return;
        }
        let now = move_buttons
            .iter()
            .position(|(bounds, _)| bounds.contains(&event.position));
        if now != painted_hover {
            window.refresh();
        }
    });
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
            return;
        }
        if let Some((_, action)) = buttons
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
        {
            cx.stop_propagation();
            let action = *action;
            let index = inputs.peek;
            let _ = weak.update(cx, |this, cx| {
                let Some(index) = index else {
                    return;
                };
                match action {
                    PeekAction::Revert => this.revert_hunk(index, window, cx),
                    PeekAction::Stage => this.stage_hunk(index, window, cx),
                    PeekAction::Comment => this.comment_hunk(index, window, cx),
                    PeekAction::Close => this.toggle_peek(index, cx),
                }
            });
            return;
        }
        if panel_bounds.is_some_and(|panel| panel.contains(&event.position)) {
            cx.stop_propagation();
            return;
        }
        if let Some((_, index)) = markers
            .iter()
            .find(|(bounds, _)| bounds.contains(&event.position))
        {
            cx.stop_propagation();
            let index = *index;
            let _ = weak.update(cx, |this, cx| this.toggle_peek(index, cx));
        }
    });
}

/// `gitOverview` plus search and cursor marks.
fn paint_ruler(
    theme: &EditorTheme,
    git: &GitSnapshot,
    match_rows: &[usize],
    cursor_row: usize,
    total_lines: usize,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) {
    let left = bounds.right() - px(RULER_WIDTH);
    let height = bounds.size.height;
    let total = total_lines.max(1) as f32;
    let y_for = |line: usize| bounds.top() + height * (line as f32 / total);

    for (index, marked) in git.marked.iter().enumerate() {
        let chunk = &git.chunks[index];
        let lines = if chunk.is_insertion() {
            marked.len().max(1)
        } else {
            git.deleted[index].len().max(1)
        };
        let top = y_for(marked.start);
        let tick_height = (height * (lines as f32 / total)).max(px(3.));
        let tick = Bounds::from_corners(
            point(left + px(3.), top),
            point(bounds.right() - px(2.), top + tick_height),
        );
        match git.kind(index) {
            ChangeKind::Added => window.paint_quad(fill(tick, theme.git_added)),
            ChangeKind::Deleted => window.paint_quad(fill(tick, theme.git_deleted)),
            ChangeKind::Modified => {
                let middle = tick.left() + tick.size.width / 2.;
                window.paint_quad(fill(
                    Bounds::from_corners(tick.origin, point(middle, tick.bottom())),
                    theme.git_deleted,
                ));
                window.paint_quad(fill(
                    Bounds::from_corners(point(middle, tick.top()), tick.bottom_right()),
                    theme.git_added,
                ));
            }
        }
    }

    let search = Hsla {
        a: 0.9,
        ..theme.search_match
    };
    let mut last_y = None;
    for row in match_rows {
        let y = y_for(*row);
        if last_y.is_some_and(|last: Pixels| (y - last).abs() < px(1.)) {
            continue;
        }
        last_y = Some(y);
        window.paint_quad(fill(
            Bounds::new(point(left + px(5.), y), size(px(RULER_WIDTH - 9.), px(2.))),
            search,
        ));
    }

    let y = y_for(cursor_row);
    window.paint_quad(fill(
        Bounds::new(point(left + px(2.), y), size(px(RULER_WIDTH - 4.), px(2.))),
        theme.content(0.6),
    ));
}

/// Paint the panel and return its buttons.
fn paint_peek(
    inputs: &Inputs,
    index: usize,
    panel: Bounds<Pixels>,
    line_height: Pixels,
    window: &mut Window,
    cx: &mut App,
) -> Vec<(Bounds<Pixels>, PeekAction)> {
    let theme = &inputs.theme;
    let git = &inputs.git;
    let deleted = &git.deleted[index];
    let first_old_line = git.first_old_line[index];

    window.paint_quad(quad(
        panel,
        px(6.),
        theme.panel_background,
        px(1.),
        theme.border,
        gpui::BorderStyle::Solid,
    ));

    let mono = gpui::font(theme.mono_font.clone());
    let ui = gpui::font(theme.ui_font.clone());
    let shape =
        |text: String, font: &gpui::Font, color: Hsla, font_size: Pixels, window: &mut Window| {
            let text: SharedString = text.into();
            let run = TextRun {
                len: text.len(),
                font: font.clone(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            window
                .text_system()
                .shape_line(text, font_size, &[run], None)
        };

    let inner = Bounds::from_corners(
        panel.origin + point(px(1.), px(1.)),
        panel.bottom_right() - point(px(1.), px(1.)),
    );
    window.with_content_mask(Some(ContentMask { bounds: inner }), |window| {
        let number_width = px(44.);
        let mut y = inner.top();
        let shown = deleted.len().min(PEEK_MAX_LINES);
        for (offset, line) in deleted.iter().take(shown).enumerate() {
            let row = Bounds::new(point(inner.left(), y), size(inner.size.width, line_height));
            window.paint_quad(fill(row, theme.deleted_line));
            window.paint_quad(fill(
                Bounds::new(row.origin, size(px(3.), line_height)),
                theme.git_deleted,
            ));
            let number = shape(
                (first_old_line + offset).to_string(),
                &mono,
                theme.line_number,
                theme.font_size,
                window,
            );
            let _ = number.paint(
                point(inner.left() + number_width - number.width - px(8.), y),
                line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
            let shaped = shape(
                line.replace('\t', "    "),
                &mono,
                theme.foreground,
                theme.font_size,
                window,
            );
            let _ = shaped.paint(
                point(inner.left() + number_width, y),
                line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
            y += line_height;
        }
        if deleted.len() > shown {
            let more = shape(
                format!("{} more removed lines", deleted.len() - shown),
                &ui,
                theme.content(0.5),
                px(11.),
                window,
            );
            let _ = more.paint(
                point(inner.left() + number_width, y),
                line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
    });

    // The action bar.
    let bar_top = panel.bottom() - px(PEEK_BAR_HEIGHT) - px(1.);
    window.paint_quad(fill(
        Bounds::new(point(inner.left(), bar_top), size(inner.size.width, px(1.))),
        theme.stroke,
    ));
    let mut actions: Vec<(PeekAction, IconKind, &str)> = Vec::new();
    if inputs.can_revert {
        actions.push((PeekAction::Revert, IconKind::Undo, "Revert change"));
    }
    if inputs.can_stage {
        actions.push((
            PeekAction::Stage,
            IconKind::Plus,
            if inputs.stage_busy {
                "Staging…"
            } else {
                "Stage change"
            },
        ));
    }
    if inputs.can_comment {
        actions.push((PeekAction::Comment, IconKind::Comment, "Comment on line"));
    }
    let mut buttons = Vec::new();
    let button_height = px(20.);
    let button_top = bar_top + (px(PEEK_BAR_HEIGHT) - button_height) / 2.;
    let mut x = inner.left() + px(4.);
    for (action, kind, label) in actions {
        let text = shape(label.to_string(), &ui, theme.content(0.8), px(11.), window);
        let width = px(12.) + px(6.) + text.width + px(14.);
        let button = Bounds::new(point(x, button_top), size(width, button_height));
        if button.contains(&window.mouse_position()) {
            window.paint_quad(fill(button, theme.hover).corner_radii(px(4.)));
        }
        paint_icon(
            kind,
            Bounds::new(
                point(x + px(6.), button_top + px(4.)),
                size(px(12.), px(12.)),
            ),
            theme.content(0.8),
            window,
            cx,
        );
        let _ = text.paint(
            point(x + px(22.), button_top),
            button_height,
            TextAlign::Left,
            None,
            window,
            cx,
        );
        buttons.push((button, action));
        x += width + px(4.);
    }
    let close = Bounds::new(
        point(inner.right() - px(24.), button_top),
        size(px(20.), button_height),
    );
    if close.contains(&window.mouse_position()) {
        window.paint_quad(fill(close, theme.hover).corner_radii(px(4.)));
    }
    paint_icon(
        IconKind::Close,
        Bounds::new(
            point(close.left() + px(3.), close.top() + px(3.)),
            size(px(14.), px(14.)),
        ),
        theme.content(0.62),
        window,
        cx,
    );
    buttons.push((close, PeekAction::Close));
    buttons
}
