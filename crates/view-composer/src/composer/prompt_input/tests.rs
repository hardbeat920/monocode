//! GPUI tests for the prompt input: layout, caret movement, mouse, IME, and
//! highlight runs. The test platform's text system is monospace: every
//! character is 0.6em wide, so at 10px a character is 6px.

use std::rc::Rc;

use gpui::{
    AppContext as _, Context, Entity, EntityInputHandler as _, Hsla, IntoElement, Modifiers,
    MouseButton, ParentElement as _, Render, Styled as _, TestAppContext, TextRun,
    VisualTestContext, Window, div, font, point, px,
};
use monocode_ui::AppearanceSettings;

use gpui::Focusable as _;

use super::element::build_runs;
use super::*;

const CHAR: f32 = 6.0;
const LINE: f32 = 20.0;

struct Harness {
    input: Entity<PromptInput>,
}

impl Render for Harness {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(300.))
            .text_size(px(10.))
            .line_height(px(LINE))
            .child(self.input.clone())
    }
}

fn mount<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<PromptInput>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        monocode_ui::init(AppearanceSettings::default(), cx);
        super::init(cx);
    });
    let text = text.to_string();
    let (harness, cx) = cx.add_window_view(|window, cx| {
        let input = cx.new(|cx| {
            let mut input = PromptInput::new(window, cx);
            input.set_padding([0., 0., 0., 0.], cx);
            input.reset_text(text, cx);
            input
        });
        Harness { input }
    });
    let input = cx.update(|_, cx| harness.read(cx).input.clone());
    cx.update(|window, cx| {
        let handle = input.read(cx).focus_handle(cx);
        window.focus(&handle, cx);
    });
    draw(cx);
    (input, cx)
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.update(|window, cx| {
            window.draw(cx).clear();
        });
        cx.run_until_parked();
    }
}

fn caret(input: &Entity<PromptInput>, cx: &mut VisualTestContext) -> usize {
    cx.update(|_, cx| input.read(cx).cursor())
}

fn text(input: &Entity<PromptInput>, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| input.read(cx).text().to_string())
}

fn rows(input: &Entity<PromptInput>, cx: &mut VisualTestContext) -> Vec<std::ops::Range<usize>> {
    cx.update(|_, cx| {
        input
            .read(cx)
            .last_layout()
            .map(|layout| layout.rows.iter().map(|row| row.range.clone()).collect())
            .unwrap_or_default()
    })
}

/// 50 characters per 300px row.
fn long_text() -> String {
    (0..12)
        .map(|i| format!("word{i:02}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[gpui::test]
fn one_line_of_text_is_one_row_tall(cx: &mut TestAppContext) {
    let (input, cx) = mount(cx, "hello");
    let bounds = cx.update(|_, cx| input.read(cx).bounds().unwrap());
    assert_eq!(bounds.size.height, px(LINE));
    assert_eq!(rows(&input, cx), vec![0..5]);
}

#[gpui::test]
fn an_empty_prompt_keeps_one_row(cx: &mut TestAppContext) {
    let (input, cx) = mount(cx, "");
    let bounds = cx.update(|_, cx| input.read(cx).bounds().unwrap());
    assert_eq!(bounds.size.height, px(LINE));
}

#[gpui::test]
fn wraps_at_word_boundaries_and_grows(cx: &mut TestAppContext) {
    let text = long_text();
    let (input, cx) = mount(cx, &text);
    let rows = rows(&input, cx);
    assert_eq!(rows.len(), 2, "{rows:?}");
    // "word00 ... word06 " fills 49 characters; word07 starts the next row.
    assert_eq!(rows[1].start, text.find("word07").unwrap());
    let bounds = cx.update(|_, cx| input.read(cx).bounds().unwrap());
    assert_eq!(bounds.size.height, px(LINE * 2.));
}

#[gpui::test]
fn stops_growing_at_the_max_height_and_scrolls_to_the_caret(cx: &mut TestAppContext) {
    let text = (0..10)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let (input, cx) = mount(cx, &text);
    let end = text.len();
    cx.update(|_, cx| {
        input.update(cx, |input, cx| {
            input.set_max_height(Some(60.), cx);
            input.move_to(end, cx);
        })
    });
    draw(cx);
    let (bounds, scroll) = cx.update(|_, cx| {
        let input = input.read(cx);
        (input.bounds().unwrap(), input.scroll_y)
    });
    assert_eq!(bounds.size.height, px(60.));
    // The caret sits at the end, on the tenth row.
    assert_eq!(scroll, px(LINE * 10. - 60.));
}

#[gpui::test]
fn typing_inserts_at_the_caret_and_backspace_deletes(cx: &mut TestAppContext) {
    let (input, cx) = mount(cx, "");
    cx.simulate_input("hi there");
    assert_eq!(text(&input, cx), "hi there");
    cx.simulate_keystrokes("backspace backspace");
    assert_eq!(text(&input, cx), "hi the");
    cx.simulate_keystrokes("left left");
    cx.simulate_input("X");
    assert_eq!(text(&input, cx), "hi tXhe");
    assert_eq!(caret(&input, cx), 5);
}

#[gpui::test]
fn enter_inserts_a_newline_unless_an_owner_takes_it(cx: &mut TestAppContext) {
    let (input, cx) = mount(cx, "a");
    cx.simulate_keystrokes("enter");
    assert_eq!(text(&input, cx), "a\n");
    cx.simulate_keystrokes("shift-enter");
    assert_eq!(text(&input, cx), "a\n\n");
}

#[gpui::test]
fn up_and_down_keep_the_column_across_wrapped_rows(cx: &mut TestAppContext) {
    let text = long_text();
    let (input, cx) = mount(cx, &text);
    // Caret after "word01" on the first row: x = 13 characters.
    cx.update(|_, cx| input.update(cx, |input, cx| input.move_to(13, cx)));
    draw(cx);
    cx.simulate_keystrokes("down");
    let second_row = text.find("word07").unwrap();
    assert_eq!(caret(&input, cx), second_row + 13);
    cx.simulate_keystrokes("up");
    assert_eq!(caret(&input, cx), 13);
    // Up from the first row goes to the start, like a textarea.
    cx.simulate_keystrokes("up");
    assert_eq!(caret(&input, cx), 0);
    cx.simulate_keystrokes("down down");
    assert_eq!(caret(&input, cx), text.len());
}

#[gpui::test]
fn home_and_end_follow_the_visual_row(cx: &mut TestAppContext) {
    let text = long_text();
    let (input, cx) = mount(cx, &text);
    let second_row = text.find("word07").unwrap();
    cx.update(|_, cx| input.update(cx, |input, cx| input.move_to(second_row + 3, cx)));
    draw(cx);
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(Home), cx);
    });
    assert_eq!(caret(&input, cx), second_row);
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(End), cx);
    });
    assert_eq!(caret(&input, cx), text.len());
    // End on the first row stays on it: the caret sits upstream of the wrap.
    cx.update(|_, cx| input.update(cx, |input, cx| input.move_to(2, cx)));
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(End), cx);
    });
    draw(cx);
    let (offset, upstream) = cx.update(|_, cx| {
        let caret = input.read(cx).caret();
        (caret.offset, caret.upstream)
    });
    assert_eq!((offset, upstream), (second_row, true));
    let y = cx.update(|_, cx| {
        let input = input.read(cx);
        input.last_layout().unwrap().position_for(input.caret()).y
    });
    assert_eq!(y, px(0.));
}

#[gpui::test]
fn shift_arrows_extend_the_selection_and_typing_replaces_it(cx: &mut TestAppContext) {
    let (input, cx) = mount(cx, "hello world");
    cx.simulate_keystrokes("shift-left shift-left shift-left shift-left shift-left");
    assert_eq!(cx.update(|_, cx| input.read(cx).selection()), 6..11);
    cx.simulate_input("there");
    assert_eq!(text(&input, cx), "hello there");
}

#[gpui::test]
fn undo_reverts_a_run_of_typing(cx: &mut TestAppContext) {
    let (input, cx) = mount(cx, "");
    cx.simulate_input("abc");
    let undo = if cfg!(target_os = "macos") {
        "cmd-z"
    } else {
        "ctrl-z"
    };
    cx.simulate_keystrokes(undo);
    assert_eq!(text(&input, cx), "");
}

#[gpui::test]
fn clicking_places_the_caret_and_double_click_selects_a_word(cx: &mut TestAppContext) {
    let (input, cx) = mount(cx, "say hello there");
    let origin = cx.update(|_, cx| input.read(cx).bounds().unwrap().origin);
    // Between "l" and "l" in "hello": 7.4 characters in.
    let at = point(origin.x + px(CHAR * 7.4), origin.y + px(5.));
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    assert_eq!(caret(&input, cx), 7);
    cx.simulate_event(gpui::MouseDownEvent {
        position: at,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    assert_eq!(cx.update(|_, cx| input.read(cx).selection()), 4..9);
}

#[gpui::test]
fn ime_composition_marks_text_and_commits_it(cx: &mut TestAppContext) {
    let (input, cx) = mount(cx, "say ");
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
        })
    });
    let (marked, composing) = cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            (input.marked_text_range(window, cx), input.is_composing())
        })
    });
    assert_eq!(marked, Some(4..6));
    assert!(composing);
    // While composing, Enter belongs to the IME, so the marked text stays.
    assert_eq!(text(&input, cx), "say ni");
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_text_in_range(None, "你", window, cx)
        });
    });
    assert_eq!(text(&input, cx), "say 你");
    assert!(!cx.update(|_, cx| input.read(cx).is_composing()));
    // The IME asks where to draw its candidates: the caret's row.
    let bounds = cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.bounds_for_range(5..5, gpui::Bounds::default(), window, cx)
        })
    });
    draw(cx);
    assert!(bounds.is_some());
    let index = cx.update(|window, cx| {
        let origin = input.read(cx).bounds().unwrap().origin;
        input.update(cx, |input, cx| {
            input.character_index_for_point(
                point(origin.x + px(CHAR * 4.2), origin.y + px(4.)),
                window,
                cx,
            )
        })
    });
    assert_eq!(index, Some(4));
}

#[gpui::test]
fn a_first_line_indent_shifts_only_the_first_row(cx: &mut TestAppContext) {
    let text = long_text();
    let (input, cx) = mount(cx, &text);
    let decorator: Decorator = Rc::new(|_, _| PromptDecorations {
        first_line_indent: 13.,
        ..PromptDecorations::default()
    });
    cx.update(|_, cx| input.update(cx, |input, cx| input.set_decorator(Some(decorator), cx)));
    draw(cx);
    let (first, second) = cx.update(|_, cx| {
        let layout = input.read(cx).last_layout().unwrap();
        (layout.rows[0].x, layout.rows[1].x)
    });
    assert_eq!(first, px(13.));
    assert_eq!(second, px(0.));
    // The caret at offset 0 sits after the indent.
    let x = cx.update(|_, cx| {
        input
            .read(cx)
            .last_layout()
            .unwrap()
            .position_for(layout::Caret::new(0))
            .x
    });
    assert_eq!(x, px(13.));
}

#[gpui::test]
fn decorations_follow_the_text_as_it_changes(cx: &mut TestAppContext) {
    let (input, cx) = mount(cx, "say /plan");
    let decorator: Decorator = Rc::new(|text, _| PromptDecorations {
        spans: text
            .find("/plan")
            .map(|at| (at..at + 5, gpui::red()))
            .into_iter()
            .collect(),
        ..PromptDecorations::default()
    });
    cx.update(|_, cx| input.update(cx, |input, cx| input.set_decorator(Some(decorator), cx)));
    let spans =
        cx.update(|_, cx| input.update(cx, |input, cx| input.decorations(cx).spans.clone()));
    assert_eq!(spans, vec![(4..9, gpui::red())]);
    cx.update(|_, cx| input.update(cx, |input, cx| input.move_to(0, cx)));
    cx.simulate_input("x ");
    let spans =
        cx.update(|_, cx| input.update(cx, |input, cx| input.decorations(cx).spans.clone()));
    assert_eq!(spans, vec![(6..11, gpui::red())]);
}

fn base_run(len: usize) -> TextRun {
    TextRun {
        len,
        font: font("Helvetica"),
        color: gpui::white(),
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}

fn colors(runs: &[TextRun]) -> Vec<(usize, Hsla, bool)> {
    runs.iter()
        .map(|run| (run.len, run.color, run.underline.is_some()))
        .collect()
}

#[test]
fn highlight_spans_become_colored_runs() {
    let decorations = PromptDecorations {
        spans: vec![(4..9, gpui::red()), (14..18, gpui::blue())],
        ..PromptDecorations::default()
    };
    let runs = build_runs(20, &base_run(20), &decorations, None);
    assert_eq!(
        colors(&runs),
        vec![
            (4, gpui::white(), false),
            (5, gpui::red(), false),
            (5, gpui::white(), false),
            (4, gpui::blue(), false),
            (2, gpui::white(), false),
        ]
    );
}

#[test]
fn hidden_glyphs_paint_transparent_inside_their_span() {
    // A mention: `@` hidden under its icon, the rest in the mention color.
    let decorations = PromptDecorations {
        spans: vec![(4..12, gpui::blue())],
        hidden: vec![std::ops::Range { start: 4, end: 5 }],
        ..PromptDecorations::default()
    };
    let runs = build_runs(12, &base_run(12), &decorations, None);
    assert_eq!(
        colors(&runs),
        vec![
            (4, gpui::white(), false),
            (1, gpui::transparent_black(), false),
            (7, gpui::blue(), false),
        ]
    );
}

#[test]
fn marked_text_is_underlined() {
    let runs = build_runs(
        6,
        &base_run(6),
        &PromptDecorations::default(),
        Some(&(4..6)),
    );
    assert_eq!(
        colors(&runs),
        vec![(4, gpui::white(), false), (2, gpui::white(), true)]
    );
}
