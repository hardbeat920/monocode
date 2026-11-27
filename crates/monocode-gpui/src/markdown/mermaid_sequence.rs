//! The renderer wraps at whitespace but gives sequence participants a fixed
//! width. Add display-only breaks to long identifiers before measuring their
//! boxes, so both the top and bottom boxes grow to fit the resulting lines.
use unicode_segmentation::UnicodeSegmentation;

pub(super) fn wrap_participant(label: &str) -> String {
    let mut result = String::with_capacity(label.len());
    let mut run = String::new();
    for grapheme in label.graphemes(true) {
        if grapheme
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            run.push_str(grapheme);
        } else {
            wrap_run(&run, &mut result);
            run.clear();
            result.push_str(grapheme);
        }
    }
    wrap_run(&run, &mut result);
    result
}

fn wrap_run(run: &str, result: &mut String) {
    let graphemes: Vec<_> = run.graphemes(true).collect();
    // The app uses a monospace face. Reserve side padding in the 150-unit
    // boxes; non-ASCII clusters may occupy two cells.
    let limit = if run.is_ascii() { 12 } else { 6 };
    let mut start = 0;
    while graphemes.len() - start > limit {
        let end = (start + 1..=start + limit)
            .rev()
            .find(|&i| {
                graphemes[i - 1]
                    .chars()
                    .next()
                    .is_some_and(char::is_lowercase)
                    && graphemes[i].chars().next().is_some_and(char::is_uppercase)
            })
            .unwrap_or(start + limit);
        for g in &graphemes[start..end] {
            result.push_str(g);
        }
        result.push('\n');
        start = end;
    }
    for g in &graphemes[start..] {
        result.push_str(g);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn long_labels_fit_top_and_bottom_participant_boxes() {
        use mermaid_rs_renderer::{LayoutConfig, Theme, compute_layout, parse_mermaid_strict};
        let source = "sequenceDiagram\n participant M as MarketDataPublisher\n participant A as averylonglowercaseidentifier\n M->>A: Hello";
        let mut parsed = parse_mermaid_strict(source).unwrap();
        for node in parsed.graph.nodes.values_mut() {
            node.label = wrap_participant(&node.label);
        }
        let theme = Theme {
            font_family: "Lilex, monospace".into(),
            ..Theme::dark()
        };
        let layout = compute_layout(&parsed.graph, &theme, &LayoutConfig::default());
        for node in layout.nodes.values() {
            assert!(node.label.width + 16. <= node.width, "{node:?}");
            assert!(node.label.height + 24. <= node.height, "{node:?}");
        }
        let mermaid_rs_renderer::layout::DiagramData::Sequence(sequence) = &layout.diagram else {
            panic!("expected sequence diagram")
        };
        for node in &sequence.footboxes {
            assert!(node.label.width + 16. <= node.width, "{node:?}");
            assert!(node.label.height + 24. <= node.height, "{node:?}");
        }
        let (svg, _, _) = super::super::diagram_svg(source, true).unwrap();
        assert!(svg.contains(">MarketData</tspan>"));
        assert!(svg.contains(">Publisher</tspan>"));
    }
    #[test]
    fn wraps_identifiers_without_losing_text_or_splitting_graphemes() {
        assert_eq!(
            wrap_participant("MarketDataPublisher"),
            "MarketData\nPublisher"
        );
        assert_eq!(wrap_participant("MatchingEngine"), "Matching\nEngine");
        assert_eq!(
            wrap_participant("OrderBook and EventLog"),
            "OrderBook and EventLog"
        );
        for label in [
            "averylonglowercaseidentifier",
            "数据发布者数据发布者",
            "e\u{301}".repeat(20).as_str(),
        ] {
            let wrapped = wrap_participant(label);
            assert_eq!(wrapped.replace('\n', ""), label);
            assert!(
                wrapped
                    .lines()
                    .all(|line| line.graphemes(true).count() <= 12)
            );
        }
        assert_eq!(wrap_participant("A<br/>B & C"), "A<br/>B & C");
    }
}
