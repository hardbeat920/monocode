use super::*;

pub(crate) fn search_background(current: bool) -> gpui::Hsla {
    let mut color = if current {
        accent_color()
    } else {
        rgb(0xe2c08d).into()
    };
    color.a = if current { 0.62 } else { 0.46 };
    color
}
pub(crate) fn row_text(row: &MarkdownRow) -> String {
    match &row.part {
        Part::Text { runs, .. } => runs.iter().map(|r| r.text.as_str()).collect(),
        Part::Table { cells, .. } => cells
            .iter()
            .flatten()
            .flatten()
            .map(|r| r.text.as_str())
            .collect(),
        Part::Code { fence, lines } => lines
            .clone()
            .map(|ix| fence.code[fence.lines[ix].clone()].to_owned())
            .collect::<Vec<_>>()
            .join("\n"),
        Part::Math(text) => text.clone(),
        Part::Visual { caption, .. } => caption.clone(),
        Part::Rule => String::new(),
    }
}
pub(crate) fn mark_row(row: &mut MarkdownRow, query: &str, current: bool) {
    let mark = |runs: &mut Vec<InlineRun>| {
        let text = runs.iter().map(|r| r.text.as_str()).collect::<String>();
        let matches = monocode_core::search::literal_ranges(&text, query);
        if matches.is_empty() {
            return;
        }
        let mut marked = Vec::new();
        let mut offset = 0;
        for run in runs.drain(..) {
            let end = offset + run.text.len();
            let mut cuts = vec![offset, end];
            for m in &matches {
                if m.start > offset && m.start < end {
                    cuts.push(m.start);
                }
                if m.end > offset && m.end < end {
                    cuts.push(m.end);
                }
            }
            cuts.sort_unstable();
            cuts.dedup();
            for span in cuts.windows(2) {
                let mut style = run.style.clone();
                if matches.iter().any(|m| m.start < span[1] && m.end > span[0]) {
                    style.search = Some(current);
                }
                marked.push(InlineRun {
                    text: run.text[span[0] - offset..span[1] - offset].to_owned(),
                    style,
                });
            }
            offset = end;
        }
        *runs = marked;
    };
    match &mut row.part {
        Part::Text { runs, .. } => mark(runs),
        Part::Table { cells, .. } => {
            for runs in cells.iter_mut().flatten() {
                mark(runs);
            }
        }
        _ => (),
    }
}
/// Compose matches with syntax colours without overlapping shaped text runs.
pub(crate) fn mark_styles(
    text: &str,
    styles: Vec<(Range<usize>, HighlightStyle)>,
    query: &str,
    current: bool,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let matches = monocode_core::search::literal_ranges(text, query);
    if matches.is_empty() {
        return styles;
    }
    let mut cuts = vec![0, text.len()];
    for (r, _) in &styles {
        cuts.extend([r.start, r.end]);
    }
    for r in &matches {
        cuts.extend([r.start, r.end]);
    }
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .map(|span| {
            let range = span[0]..span[1];
            let mut style = styles
                .iter()
                .find(|(r, _)| r.start <= range.start && r.end >= range.end)
                .map(|(_, s)| *s)
                .unwrap_or_default();
            if matches
                .iter()
                .any(|m| m.start < range.end && m.end > range.start)
            {
                style.background_color = Some(search_background(current));
            }
            (range, style)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_span_inline_styles_and_preserve_utf8_and_syntax() {
        let mut row = MarkdownRow {
            part: Part::Text {
                runs: vec![
                    InlineRun {
                        text: "Find ca".into(),
                        style: Default::default(),
                    },
                    InlineRun {
                        text: "fé here".into(),
                        style: crate::markdown_model::InlineStyle {
                            bold: true,
                            ..Default::default()
                        },
                    },
                ],
                heading: 0,
            },
            indent: 0,
            quote: 0,
            marker: None,
            gap: 0.,
            word_start: 0,
        };
        mark_row(&mut row, "café", true);
        assert_eq!(row_text(&row), "Find café here");
        let Part::Text { runs, .. } = row.part else {
            unreachable!()
        };
        assert_eq!(
            runs.iter()
                .filter(|r| r.style.search.is_some())
                .map(|r| r.text.as_str())
                .collect::<String>(),
            "café"
        );
        let styles = mark_styles(
            "é needle",
            vec![(
                0..9,
                HighlightStyle {
                    color: Some(rgb(0xffffff).into()),
                    ..Default::default()
                },
            )],
            "needle",
            true,
        );
        assert_eq!(styles.last().unwrap().0, 3..9);
        assert!(styles.last().unwrap().1.background_color.is_some());
    }
}
