//! Direct presentation port of AgentTranscript's ActivityPhaseGroup and PhaseStep.
use gpui::{
    AnyElement, App, Bounds, ContentMask, Element, ElementId, GlobalElementId, InspectorElementId,
    LayoutId, Pixels, StyledText, Window, point, prelude::*,
};
use monocode_core::tool::{ToolCall, ToolState};
use std::collections::HashSet;

/// Shade a fixed glyph layout. Animated StyledText highlights change font-run
/// boundaries (and kerning/ligatures) as neighboring colors become equal.
pub(crate) fn shimmer(text: String, elapsed: f32) -> AnyElement {
    Shimmer {
        text: StyledText::new(text),
        elapsed,
    }
    .into_any_element()
}

struct Shimmer {
    text: StyledText,
    elapsed: f32,
}
impl IntoElement for Shimmer {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Shimmer {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.text.request_layout(None, None, window, cx)
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        // The sweep repaints its whole transcript, so it runs at 15fps.
        crate::pulse::request(crate::pulse::Rate::Half, window, cx);
        self.text.prepaint(None, None, bounds, state, window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        let layout = self.text.layout();
        let line_height = layout.line_height();
        // Both callers use a single truncated line; retain StyledText's layout
        // so overflow, font fallback and the plain-text geometry stay identical.
        let lines = layout.line_layouts();
        window.paint_layer(bounds, |window| {
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                for wrapped in &lines {
                    let line = &wrapped.unwrapped_layout;
                    let baseline = (line_height - line.ascent - line.descent) / 2. + line.ascent;
                    let width = f32::from(line.width).max(1.);
                    let center = -0.25 + (self.elapsed / 1.6).fract() * 1.5;
                    for run in &line.runs {
                        for glyph in &run.glyphs {
                            let x = f32::from(glyph.position.x + line.font_size * 0.25) / width;
                            let sweep = (1. - (x - center).abs() / (2. / 7.)).max(0.);
                            let origin = bounds.origin
                                + point(glyph.position.x, baseline + glyph.position.y);
                            if glyph.is_emoji {
                                let _ = window.paint_emoji(
                                    origin,
                                    run.font_id,
                                    glyph.id,
                                    line.font_size,
                                );
                            } else {
                                let _ = window.paint_glyph(
                                    origin,
                                    run.font_id,
                                    glyph.id,
                                    line.font_size,
                                    crate::theme::content(0.4 + 0.6 * sweep),
                                );
                            }
                        }
                    }
                }
            });
        });
    }
}

/// A work icon by name. `monocode` is the app's own logo, for its tools.
pub(crate) fn glyph(name: &'static str, side: f32) -> gpui::Div {
    if name == "monocode" {
        return gpui::div()
            .size(gpui::px(side))
            .flex_shrink_0()
            .child(gpui::img("monocode.png").size(gpui::px(side)));
    }
    gpui::div()
        .size(gpui::px(side))
        .flex_shrink_0()
        .child(crate::theme::icon(name, side))
}
pub(crate) fn file_icon(path: &str, side: f32) -> AnyElement {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    crate::file_icons::file_type_icon(name, false, false, side)
}
pub(crate) fn summary(tools: &[ToolCall], live: bool) -> (String, &'static str) {
    let mut order = Vec::new();
    let mut reads = HashSet::new();
    let mut edits = HashSet::new();
    let mut searches = 0;
    let mut read_calls = 0;
    let mut edit_calls = 0;
    let mut commands = 0;
    let mut agents = 0;
    let mut failed_agents = 0;
    let mut others = 0;
    let mut charts = 0;
    let mut chart_steps = 0;
    let mut current = "think";
    for tool in tools {
        let (action, target) = tool.summary();
        let visual = tool.visual_step();
        let kind = match action.as_str() {
            _ if visual.is_some() => "visual",
            "Read" | "List" | "Search" | "Find" => "research",
            "Edit" | "Write" => "edit",
            "Run" => "run",
            "Agent" => "agent",
            _ => "other",
        };
        if !order.contains(&kind) {
            order.push(kind);
        }
        if live {
            current = kind;
        }
        if let Some(step) = visual {
            chart_steps += 1;
            if step == monocode_core::tool::VisualStep::Show {
                charts += 1;
            }
            continue;
        }
        match action.as_str() {
            "Read" => {
                read_calls += 1;
                reads.insert(target);
            }
            "Edit" | "Write" => {
                edit_calls += 1;
                edits.insert(target);
            }
            "Search" | "Find" | "List" => searches += 1,
            "Run" => commands += 1,
            "Agent" => {
                agents += 1;
                if matches!(tool.state, ToolState::Failed | ToolState::Cancelled) {
                    failed_agents += 1;
                }
            }
            _ => others += 1,
        }
    }
    if failed_agents > 0 {
        return (
            if failed_agents == 1 {
                "Subagent failed".into()
            } else {
                format!("{failed_agents} subagents failed")
            },
            "bot",
        );
    }
    let file_label = |paths: &HashSet<String>| {
        if paths.len() == 1 {
            paths
                .iter()
                .next()
                .unwrap()
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_owned()
        } else {
            format!("{} files", paths.len())
        }
    };
    let title = order
        .iter()
        .map(|kind| {
            let live = *kind == current;
            match *kind {
                "research" if searches > 0 && !reads.is_empty() => if live {
                    "Exploring the project"
                } else {
                    "Explored the project"
                }
                .into(),
                "research" if !reads.is_empty() => format!(
                    "{} {}",
                    if live { "Reading" } else { "Read" },
                    file_label(&reads)
                ),
                "research" => if live {
                    "Searching the project"
                } else {
                    "Searched the project"
                }
                .into(),
                "edit" => format!(
                    "{} {}",
                    if live { "Editing" } else { "Edited" },
                    file_label(&edits)
                ),
                "run" => format!(
                    "{} {}",
                    if live { "Running" } else { "Ran" },
                    if commands == 1 {
                        "a command".into()
                    } else {
                        format!("{commands} commands")
                    }
                ),
                "visual" if charts == 0 => if live {
                    "Planning a chart"
                } else {
                    "Planned a chart"
                }
                .into(),
                "visual" => format!(
                    "{} {}",
                    if live { "Drawing" } else { "Drew" },
                    if charts == 1 {
                        "a chart".into()
                    } else {
                        format!("{charts} charts")
                    }
                ),
                "agent" => format!(
                    "{} {}",
                    if live { "Running" } else { "Ran" },
                    if agents == 1 {
                        "a subagent".into()
                    } else {
                        format!("{agents} subagents")
                    }
                ),
                _ => format!(
                    "{} {}",
                    if live { "Running" } else { "Ran" },
                    if others == 1 {
                        "a tool".into()
                    } else {
                        format!("{others} tools")
                    }
                ),
            }
        })
        .collect::<Vec<String>>()
        .join(" · ");
    // React breaks equal call counts towards edit, run, agent, then research.
    let mut dominant = "terminal";
    let mut most = 0;
    for (count, icon) in [
        (edit_calls, "pencil-line"),
        (commands, "terminal"),
        (agents, "bot"),
        (read_calls + searches, "search"),
        (others, "terminal"),
        (chart_steps, "monocode"),
    ] {
        if count > most {
            most = count;
            dominant = icon;
        }
    }
    (title, dominant)
}
/// CSS .zen-phase-step: left 6px spine, 8px quarter curve, 20px content inset.
pub(crate) fn rail(last: bool) -> impl IntoElement {
    rail_branch(last, true)
}
pub(crate) fn rail_branch(last: bool, branch: bool) -> impl IntoElement {
    rail_branch_at(last, branch, 0.)
}
/// The same rail for a step whose content sits `offset` lower, as the
/// sidebar's `.orchestration-tree-step` does with its 2px padding.
pub(crate) fn rail_branch_at(last: bool, branch: bool, offset: f32) -> impl IntoElement {
    let color = crate::theme::rail_color();
    gpui::div()
        .absolute()
        .top_0()
        .left(gpui::px(6.))
        .w(gpui::px(9.))
        .h_full()
        .child(
            gpui::div()
                .absolute()
                .top_0()
                .left_0()
                .w(gpui::px(1.))
                .map(|d| {
                    if last {
                        d.h(gpui::px(7. + offset))
                    } else {
                        d.h_full()
                    }
                })
                .bg(color),
        )
        .when(branch, |d| {
            d.child(
                gpui::svg()
                    .path("tool-branch")
                    .absolute()
                    .left_0()
                    .top(gpui::px(6. + offset))
                    .size(gpui::px(9.))
                    .text_color(color),
            )
        })
}

#[cfg(test)]
mod shimmer_tests {
    use super::*;
    use gpui::{Context, Render, TestAppContext, canvas, div, px};
    use std::{cell::RefCell, rc::Rc};

    struct Preview {
        elapsed: f32,
        geometry: Rc<RefCell<Option<(Pixels, String)>>>,
    }
    impl Render for Preview {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let elapsed = self.elapsed;
            let geometry = self.geometry.clone();
            div()
                .text_size(px(14.))
                .line_height(px(20.))
                .truncate()
                .child(
                    canvas(
                        move |_, window, cx| {
                            let text = "GPT-6.1-Sol working for 29s · office AV café 漢字";
                            let shimmer = Shimmer {
                                text: StyledText::new(text),
                                elapsed,
                            };
                            let layout = shimmer.text.layout().clone();
                            let mut shimmer = shimmer.into_any_element();
                            let available = gpui::size(
                                gpui::AvailableSpace::Definite(px(220.)),
                                gpui::AvailableSpace::MinContent,
                            );
                            let measured = shimmer.layout_as_root(available, window, cx);
                            let plain = StyledText::new(text);
                            let plain_layout = plain.layout().clone();
                            let plain = plain
                                .into_any_element()
                                .layout_as_root(available, window, cx);
                            assert_eq!(measured, plain);
                            let glyphs = |layout: &gpui::TextLayout| {
                                layout
                                    .line_layouts()
                                    .iter()
                                    .map(|line| format!("{:?}", line.unwrapped_layout.runs))
                                    .collect::<String>()
                            };
                            assert_eq!(glyphs(&layout), glyphs(&plain_layout));
                            *geometry.borrow_mut() = Some((measured.width, glyphs(&layout)));
                        },
                        |_, _, _, _| {},
                    )
                    .w_full()
                    .h(px(20.)),
                )
        }
    }
    #[gpui::test]
    fn sweep_preserves_plain_text_kerning_ligatures_and_glyph_positions(cx: &mut TestAppContext) {
        let geometry = Rc::new(RefCell::new(None));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|_| Preview {
                    elapsed: 0.,
                    geometry: geometry.clone(),
                })
            })
            .unwrap()
        });
        let mut first = None;
        for elapsed in [0., 0.2, 0.6, 1., 1.59, 1.6] {
            window
                .update(cx, |view, _, cx| {
                    view.elapsed = elapsed;
                    cx.notify();
                })
                .unwrap();
            cx.refresh().unwrap();
            cx.run_until_parked();
            let current = geometry.borrow().clone().expect("shimmer was measured");
            if let Some(first) = &first {
                assert_eq!(first, &current);
            } else {
                first = Some(current);
            }
        }
    }
}
