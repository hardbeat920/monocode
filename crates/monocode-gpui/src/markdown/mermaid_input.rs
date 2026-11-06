//! Checks applied before parsing and before passing source-derived styles to
//! the SVG emitter. Keep these separate from the post-parse rendering limits.
use mermaid_rs_renderer::ir::{Graph, NodeStyle};

const MAX_PARSE_EXPANSIONS: usize = super::MAX_GRAPH_ITEMS;

pub(super) fn expansion_budget(source: &str) -> Result<(), String> {
    // Style values are cloned for each styled node. Bound those command lines
    // before parsing too, rather than validating a huge value after fanout.
    for line in source.lines() {
        if line.len() > 2048
            && line.split(';').any(|statement| {
                matches!(
                    statement.split_whitespace().next(),
                    Some("style" | "classDef" | "linkStyle")
                )
            })
        {
            return Err("Diagram style command exceeds the parser budget.".into());
        }
    }
    // The parser expands every pair of grouped endpoints, and duplicates the
    // middle group in chains. Count ALL ampersands, even inside labels/comments:
    // lexical ambiguity must overestimate the budget, never bypass it.
    // With A extra group members and E connectors, expansions are bounded by
    // E + 2A + floor(A²/4). Each member contributes to at most two adjacent
    // connectors; the product term is bounded by the two bipartite sums.
    // Counting runs of connector punctuation also overcounts labels/IDs, but
    // bounds every connector supported by the pinned flowchart/block parser.
    let extra = source.bytes().filter(|&b| b == b'&').count();
    let mut connectors = 0usize;
    let mut in_connector = false;
    for byte in source.bytes() {
        let connector = matches!(byte, b'-' | b'=' | b'~');
        connectors += usize::from(connector && !in_connector);
        in_connector = connector;
    }
    let edges = connectors
        .saturating_add(extra.saturating_mul(2))
        .saturating_add(extra.saturating_mul(extra) / 4);
    // Class assignments also have a Cartesian node/class fanout. Use all raw
    // commas so quoted punctuation cannot hide assignments from this estimate.
    let classes = source
        .split(['\n', ';'])
        .filter(|line| line.split_whitespace().next() == Some("class"))
        .count();
    let commas = source.bytes().filter(|&b| b == b',').count();
    let assignments = if classes == 0 {
        0
    } else {
        classes
            .saturating_add(commas)
            .saturating_add(commas.saturating_mul(commas) / 4)
    };
    if edges > MAX_PARSE_EXPANSIONS || assignments > MAX_PARSE_EXPANSIONS {
        return Err("Diagram exceeds the parser expansion budget.".into());
    }
    Ok(())
}

fn attribute(value: &str) -> bool {
    // Colors and dash arrays do not need XML delimiters, entities, URLs, or
    // escape sequences. This intentionally permits a small CSS value alphabet;
    // invalid colors are harmlessly ignored by the SVG parser.
    value.len() <= 128
        && value.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'#' | b'.' | b',' | b'%' | b'(' | b')' | b'+' | b'-' | b' '
                )
        })
}

fn width(value: Option<f32>) -> bool {
    value.is_none_or(|value| value.is_finite() && (0.0..=1000.0).contains(&value))
}

fn node_style(style: &NodeStyle) -> bool {
    [
        &style.fill,
        &style.stroke,
        &style.text_color,
        &style.stroke_dasharray,
        &style.line_color,
    ]
    .into_iter()
    .flatten()
    .all(|value| attribute(value))
        && width(style.stroke_width)
}

pub(super) fn validate_styles(graph: &Graph) -> Result<(), String> {
    let nodes = graph
        .class_defs
        .values()
        .chain(graph.node_styles.values())
        .chain(graph.subgraph_styles.values())
        .all(node_style);
    let edges = graph
        .edge_styles
        .values()
        .chain(graph.edge_style_default.iter())
        .all(|style| {
            [&style.stroke, &style.dasharray, &style.label_color]
                .into_iter()
                .flatten()
                .all(|value| attribute(value))
                && width(style.stroke_width)
        });
    let boxes = graph
        .sequence_boxes
        .iter()
        .filter_map(|item| item.color.as_deref())
        .all(attribute);
    let shapes = graph.c4.shapes.iter().all(|item| {
        [&item.bg_color, &item.border_color, &item.font_color]
            .into_iter()
            .flatten()
            .all(|value| attribute(value))
    });
    let boundaries = graph.c4.boundaries.iter().all(|item| {
        [&item.bg_color, &item.border_color, &item.font_color]
            .into_iter()
            .flatten()
            .all(|value| attribute(value))
    });
    let relations = graph.c4.rels.iter().all(|item| {
        [&item.line_color, &item.text_color]
            .into_iter()
            .flatten()
            .all(|value| attribute(value))
    });
    if !(nodes && edges && boxes && shapes && boundaries && relations) {
        return Err("Diagram contains an unsupported or unsafe style value.".into());
    }
    Ok(())
}
