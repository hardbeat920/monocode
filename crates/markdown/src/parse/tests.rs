//! Parser tests: block structure, GFM features, and incomplete input while
//! streaming.

use std::sync::Arc;

use super::mend::PENDING_LINK_URL;
use super::*;

fn stream_chunks(text: &str, chunk: usize) -> IncrementalParser {
    let mut parser = IncrementalParser::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + chunk).min(text.len());
        while end < text.len() && !text.is_char_boundary(end) {
            end += 1;
        }
        parser.append(&text[start..end]);
        start = end;
    }
    parser
}

fn paragraph(doc: &Document, ix: usize) -> &Inline {
    match &doc.blocks[ix].block {
        Block::Paragraph(inline) => inline,
        other => panic!("expected a paragraph, got {other:?}"),
    }
}

/// Check the span invariant: spans cover the text exactly and in order.
fn check_spans(inline: &Inline) {
    let mut at = 0;
    for span in &inline.spans {
        assert_eq!(span.range.start, at, "gap in {inline:?}");
        assert!(span.range.end > span.range.start);
        at = span.range.end;
    }
    assert_eq!(
        at,
        inline.text.len(),
        "spans do not reach the end: {inline:?}"
    );
}

fn check_all_spans(block: &Block) {
    match block {
        Block::Paragraph(inline)
        | Block::Heading {
            content: inline, ..
        } => check_spans(inline),
        Block::Quote(children) => children.iter().for_each(check_all_spans),
        Block::List(list) => list
            .items
            .iter()
            .flat_map(|item| &item.blocks)
            .for_each(check_all_spans),
        Block::Table(table) => table
            .header
            .iter()
            .chain(table.rows.iter().flatten())
            .for_each(check_spans),
        Block::Code(_) | Block::Rule => {}
    }
}

const CORPORA: &[&str] = &[
    "# Title\n\nHello **bold** and *italic* and `code` and ~~gone~~.\n",
    "Paragraph one\nlazy continuation\n\nParagraph two with a [link](https://x.dev).\n",
    "- item one\n- item two\n  - nested a\n  - nested b\n- item three\n\ntail\n",
    "1. first\n2. second\n\n   loose paragraph in item\n\n3. third\n",
    "```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\nafter code\n",
    "intro\n\n```\nunclosed fence streaming",
    "> quoted line\n> more quote\n>\n> - a list in a quote\n\nplain\n",
    "| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\ndone\n",
    "setext candidate\n===\n\nnext para\n---\n",
    "***\n\ntext between rules\n\n---\n",
    "- [x] done task\n- [ ] open task\n",
    "    indented code line one\n    line two\n\npara\n",
    "para with <span>inline html</span> inside\n\n<div>\nblock html\n</div>\n",
    "###### deep heading\n\n#### h4\n",
    "Título 🦀 with ünïcödé\r\n\r\n- ítem\r\n",
    "See https://example.com/a_b_(c) and www.example.org.\n\n![shot](/tmp/a.png \"t\")\n",
    "~~~py\nprint(1)\n~~~\n\n> ```js\n> let a = 1;\n> ```\n",
];

#[test]
fn incremental_matches_full_parse_on_streamed_corpora() {
    for (ix, corpus) in CORPORA.iter().enumerate() {
        let full = parse(corpus);
        for chunk in [1usize, 2, 3, 7, 16, 64] {
            let streamed = stream_chunks(corpus, chunk);
            assert_eq!(
                streamed.tree(),
                &full,
                "corpus {ix} diverged at chunk size {chunk}:\n{corpus}"
            );
        }
    }
}

#[test]
fn spans_always_cover_text() {
    for corpus in CORPORA {
        let mut parser = IncrementalParser::new();
        for (ix, _) in corpus.char_indices().skip(1) {
            parser.set_text(&corpus[..ix]);
            for top in &parser.display_tree().blocks {
                check_all_spans(&top.block);
            }
        }
    }
}

#[test]
fn appends_keep_finished_blocks_shared() {
    for corpus in CORPORA {
        let mut parser = IncrementalParser::new();
        let mut previous = parser.tree().clone();
        for (ix, _) in corpus.char_indices().skip(1) {
            parser.set_text(&corpus[..ix]);
            let current = parser.tree();
            let committed = previous.blocks.len().saturating_sub(2);
            assert!(current.blocks.len() >= committed);
            for i in 0..committed {
                assert!(
                    Arc::ptr_eq(&current.blocks[i], &previous.blocks[i]),
                    "block {i} was rebuilt:\n{corpus}"
                );
            }
            previous = current.clone();
        }
    }
}

#[test]
fn appends_reparse_only_the_tail() {
    let mut doc = String::new();
    for i in 0..200 {
        doc.push_str(&format!("Paragraph {i} with some **bold** words.\n\n"));
    }
    let mut parser = IncrementalParser::new();
    parser.set_text(&doc);
    doc.push_str("one more");
    parser.set_text(&doc);
    assert!(
        parser.last_parse_bytes() < 200,
        "{}",
        parser.last_parse_bytes()
    );
    // The last two blocks are reparsed.
    assert_eq!(parser.stable_prefix(), 198);
    assert_eq!(parser.tree(), &parse(&doc));
}

#[test]
fn link_definitions_fall_back_to_full_parses() {
    let corpus = "See [docs] for more.\n\nMore text.\n\n[docs]: https://example.com\n";
    let full = parse(corpus);
    for chunk in [1usize, 3, 9] {
        assert_eq!(stream_chunks(corpus, chunk).tree(), &full);
    }
    let inline = paragraph(&full, 0);
    assert!(inline.spans.iter().any(|s| s.style.link.is_some()));
}

#[test]
fn set_text_appends_or_resets() {
    let mut parser = IncrementalParser::new();
    parser.set_text("hello");
    parser.set_text("hello world");
    assert_eq!(parser.tree(), &parse("hello world"));
    parser.set_text("different");
    assert_eq!(parser.tree(), &parse("different"));
    assert_eq!(parser.source(), "different");
}

#[test]
fn block_structure_basics() {
    let doc = parse("## Head\n\npara **b _bi_** text\n\n```ts\nlet x = 1;\n```\n");
    assert_eq!(doc.len(), 3);
    let Block::Heading { level, content } = &doc.blocks[0].block else {
        panic!();
    };
    assert_eq!(*level, 2);
    assert_eq!(content.text, "Head");
    let inline = paragraph(&doc, 1);
    assert_eq!(inline.text, "para b bi text");
    let styles: Vec<_> = inline
        .spans
        .iter()
        .map(|s| {
            (
                &inline.text[s.range.clone()],
                s.style.strong,
                s.style.emphasis,
            )
        })
        .collect();
    assert_eq!(
        styles,
        vec![
            ("para ", false, false),
            ("b ", true, false),
            ("bi", true, true),
            (" text", false, false)
        ]
    );
    let Block::Code(code) = &doc.blocks[2].block else {
        panic!();
    };
    assert_eq!(code.info, "ts");
    assert_eq!(code.code, "let x = 1;");
    assert!(code.closed);
}

#[test]
fn span_sources_point_at_the_source_text() {
    let source = "intro **bold** `code` [link](https://x.dev) end";
    let doc = parse(source);
    let inline = paragraph(&doc, 0);
    for span in &inline.spans {
        let text = &inline.text[span.range.clone()];
        assert_eq!(&source[span.src..span.src + text.len()], text, "{span:?}");
    }
}

#[test]
fn nested_and_task_lists() {
    let doc = parse("- a\n  - a1\n  - a2\n- b\n\n1. one\n2. two\n\n- [x] done\n- [ ] open\n");
    let Block::List(list) = &doc.blocks[0].block else {
        panic!();
    };
    assert_eq!(list.start, None);
    assert_eq!(list.items.len(), 2);
    assert!(matches!(list.items[0].blocks[0], Block::Paragraph(_)));
    assert!(matches!(list.items[0].blocks[1], Block::List(_)));
    let Block::List(ordered) = &doc.blocks[1].block else {
        panic!();
    };
    assert_eq!(ordered.start, Some(1));
    let Block::List(tasks) = &doc.blocks[2].block else {
        panic!();
    };
    assert_eq!(tasks.items[0].task, Some(true));
    assert_eq!(tasks.items[1].task, Some(false));
    assert_eq!(block_text(&tasks.items[0].blocks[0]), "done");
}

#[test]
fn loose_task_items_keep_their_text() {
    let doc = parse("- [x] first\n\n- [ ] second\n");
    let Block::List(list) = &doc.blocks[0].block else {
        panic!();
    };
    assert_eq!(list.items[0].task, Some(true));
    assert_eq!(block_text(&list.items[0].blocks[0]), "first");
    assert_eq!(list.items[1].task, Some(false));
}

#[test]
fn tables_parse_header_rows_and_alignment() {
    let doc = parse("| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n");
    let Block::Table(table) = &doc.blocks[0].block else {
        panic!();
    };
    assert_eq!(table.header.len(), 3);
    assert_eq!(table.rows[0][1].text, "2");
    assert_eq!(table.align, vec![Align::Left, Align::Center, Align::Right]);
    assert_eq!(table.columns(), 3);
}

#[test]
fn images_and_links() {
    let doc = parse("Before ![alt **x**](a.png \"Title\") after [zed](https://zed.dev)");
    let inline = paragraph(&doc, 0);
    let image = inline
        .spans
        .iter()
        .find_map(|s| s.style.image.clone())
        .unwrap();
    assert_eq!(image.url, "a.png");
    assert_eq!(image.alt, "alt x");
    assert_eq!(image.title, "Title");
    let link = inline
        .spans
        .iter()
        .find(|s| s.style.link.is_some())
        .unwrap();
    assert_eq!(&inline.text[link.range.clone()], "zed");
}

#[test]
fn html_shows_as_text_and_br_breaks() {
    let doc = parse("a<br>b <kbd>x</kbd>\n\n<div>\nblock\n</div>\n");
    assert_eq!(paragraph(&doc, 0).text, "a\nb <kbd>x</kbd>");
    assert_eq!(paragraph(&doc, 1).text, "<div>\nblock\n</div>");
}

#[test]
fn empty_and_whitespace_sources() {
    assert!(parse("").is_empty());
    assert!(parse("\n\n  \n").is_empty());
    let mut parser = IncrementalParser::new();
    parser.append("");
    assert!(parser.tree().is_empty());
}

// Incomplete input while streaming.

#[test]
fn unclosed_fence_renders_finished_lines_as_code() {
    let mut parser = IncrementalParser::new();
    parser.set_text("Intro\n\n```rust\nfn main() {\n    let x");
    let display = parser.display_tree();
    assert_eq!(display.len(), 2);
    assert_eq!(block_text(&display.blocks[0].block), "Intro");
    let Block::Code(code) = &display.blocks[1].block else {
        panic!("expected code");
    };
    assert_eq!(code.info, "rust");
    assert_eq!(code.code, "fn main() {\n    let x");
    assert!(!code.closed);
}

#[test]
fn half_typed_closing_fence_is_hidden() {
    let mut parser = IncrementalParser::new();
    parser.set_text("```rust\nlet a = 1;\n``");
    let display = parser.display_tree();
    let Block::Code(code) = &display.blocks[0].block else {
        panic!();
    };
    assert_eq!(code.code, "let a = 1;");
    // The canonical tree keeps the literal line until the fence closes.
    let Block::Code(raw) = &parser.tree().blocks[0].block else {
        panic!();
    };
    assert_eq!(raw.code, "let a = 1;\n``");
    parser.append("`\n\nafter");
    let display = parser.display_tree();
    let Block::Code(code) = &display.blocks[0].block else {
        panic!();
    };
    assert_eq!(code.code, "let a = 1;");
    assert!(code.closed);
    assert_eq!(block_text(&display.blocks[1].block), "after");
}

#[test]
fn streaming_info_string_has_no_label_yet() {
    let mut parser = IncrementalParser::new();
    parser.set_text("```ru");
    let Block::Code(code) = &parser.display_tree().blocks[0].block else {
        panic!();
    };
    assert_eq!(code.info, "");
    parser.append("st\n");
    let Block::Code(code) = &parser.display_tree().blocks[0].block else {
        panic!();
    };
    assert_eq!(code.info, "rust");
}

#[test]
fn unterminated_bold_renders_bold_without_markers() {
    let mut parser = IncrementalParser::new();
    parser.set_text("Finished **bold** part and **still stream");
    let display = parser.display_tree();
    let inline = paragraph(&display, 0);
    assert_eq!(inline.text, "Finished bold part and still stream");
    let bold: Vec<_> = inline
        .spans
        .iter()
        .filter(|s| s.style.strong)
        .map(|s| &inline.text[s.range.clone()])
        .collect();
    assert_eq!(bold, vec!["bold", "still stream"]);
    // The canonical tree shows the literal marker.
    assert!(paragraph(parser.tree(), 0).text.contains("**still"));
}

#[test]
fn lone_bold_marker_stays_plain() {
    let mut parser = IncrementalParser::new();
    parser.set_text("Text then **");
    let inline = paragraph(&parser.display_tree(), 0).clone();
    assert_eq!(inline.text, "Text then **");
    assert!(inline.spans.iter().all(|s| !s.style.strong));
}

#[test]
fn half_table_row_keeps_the_table() {
    let mut parser = IncrementalParser::new();
    parser.set_text("| a | b |\n|---|---|\n| 1 | 2 |\n| 3 |");
    let display = parser.display_tree();
    assert_eq!(display.len(), 1);
    let Block::Table(table) = &display.blocks[0].block else {
        panic!("expected a table, got {display:?}");
    };
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0][0].text, "1");
    assert_eq!(table.rows[1][0].text, "3");
    // Missing cells read as empty, not as a broken table.
    assert!(table.rows[1].len() <= 2);
}

#[test]
fn table_header_waits_for_its_delimiter_row() {
    let mut parser = IncrementalParser::new();
    parser.set_text("Intro\n\n| a | b |\n| --");
    let display = parser.display_tree();
    assert_eq!(block_text(&display.blocks[1].block), "| a | b |");
    parser.append("- | --- |\n| 1 | 2 |\n");
    let display = parser.display_tree();
    assert!(matches!(display.blocks[1].block, Block::Table(_)));
}

#[test]
fn half_link_shows_text_without_url() {
    let full = "read [docs](https://example.com/long/path) now";
    let mut parser = IncrementalParser::new();
    for (ix, _) in full.char_indices().skip(1) {
        parser.set_text(&full[..ix]);
        let text = block_text(&parser.display_tree().blocks[0].block);
        assert!(!text.contains("http"), "url leaked at {ix}: {text:?}");
        assert!(!text.contains("]("), "markup leaked at {ix}: {text:?}");
    }
    parser.set_text("read [docs](https://exa");
    let display = parser.display_tree();
    let inline = paragraph(&display, 0);
    let link = inline
        .spans
        .iter()
        .find(|s| s.style.link.is_some())
        .unwrap();
    assert_eq!(&inline.text[link.range.clone()], "docs");
    assert_eq!(link.style.link.as_deref(), Some(PENDING_LINK_URL));
}

#[test]
fn list_marker_on_its_own_line_does_not_flash() {
    let mut parser = IncrementalParser::new();
    parser.set_text("Steps:\n-");
    let display = parser.display_tree();
    assert_eq!(display.len(), 1);
    assert!(matches!(display.blocks[0].block, Block::Paragraph(_)));
    assert_eq!(block_text(&display.blocks[0].block), "Steps:");
    parser.append(" first");
    let display = parser.display_tree();
    assert!(matches!(
        display.blocks.last().unwrap().block,
        Block::List(_)
    ));
}

#[test]
fn display_prefix_matches_canonical_tree() {
    for corpus in CORPORA {
        let mut parser = IncrementalParser::new();
        for (ix, _) in corpus.char_indices().skip(1) {
            parser.set_text(&corpus[..ix]);
            let display = parser.display_tree();
            let canonical = parser.tree();
            for i in 0..canonical.len().saturating_sub(1) {
                assert!(Arc::ptr_eq(&display.blocks[i], &canonical.blocks[i]));
            }
        }
    }
}

#[test]
fn display_converges_when_balanced() {
    let corpus = "a **b** *c* `d` [e](https://x.dev) ~~f~~";
    let mut parser = IncrementalParser::new();
    parser.set_text(corpus);
    assert_eq!(parser.display_tree(), *parser.tree());
}

#[test]
fn every_prefix_of_a_mixed_document_parses() {
    // No prefix may panic, and every prefix's display tree must keep the
    // span invariant.
    let corpus = concat!(
        "# Plan\n\nWe **will** do `three` things:\n\n1. Parse\n2. Render\n\n",
        "| col | other |\n|:---|---:|\n| `x` | **y** |\n\n",
        "```python\ndef f(x):\n    return x * 2\n```\n\n> Note: _quoted_ ~~text~~\n\n",
        "- [ ] task with [link](https://a.b/c)\n- [x] done\n\n---\n\n![img](https://x/y.png)\n",
    );
    let mut parser = IncrementalParser::new();
    for (ix, _) in corpus.char_indices().skip(1) {
        parser.set_text(&corpus[..ix]);
        for top in &parser.display_tree().blocks {
            check_all_spans(&top.block);
        }
    }
    assert_eq!(parser.tree(), &parse(&corpus[..corpus.len() - 1]));
}

/// hardBreaks.test.ts: a document's lines stay on their own lines (#591).
mod hard_breaks {
    use super::*;

    const HARD: ParseOptions = ParseOptions { hard_breaks: true };

    /// The prose of every paragraph and heading, in order.
    fn prose(text: &str) -> Vec<String> {
        fn walk(block: &Block, out: &mut Vec<String>) {
            match block {
                Block::Paragraph(inline)
                | Block::Heading {
                    content: inline, ..
                } => out.push(inline.text.clone()),
                Block::Quote(children) => children.iter().for_each(|child| walk(child, out)),
                Block::List(list) => list
                    .items
                    .iter()
                    .flat_map(|item| &item.blocks)
                    .for_each(|child| walk(child, out)),
                Block::Table(table) => out.extend(
                    table
                        .header
                        .iter()
                        .chain(table.rows.iter().flatten())
                        .map(|cell| cell.text.clone()),
                ),
                Block::Code(_) | Block::Rule => {}
            }
        }
        let doc = parse_with(text, HARD);
        let mut out = Vec::new();
        for top in &doc.blocks {
            check_all_spans(&top.block);
            walk(&top.block, &mut out);
        }
        out
    }

    #[test]
    fn keeps_consecutive_quote_lines_on_their_own_lines() {
        assert_eq!(
            prose("> first line\n> second line\n> third line"),
            ["first line\nsecond line\nthird line"]
        );
    }

    #[test]
    fn keeps_consecutive_paragraph_lines_on_their_own_lines() {
        assert_eq!(
            prose("first line\nsecond line"),
            ["first line\nsecond line"]
        );
    }

    #[test]
    fn breaks_inside_emphasis_and_links() {
        let doc = parse_with("**bold\ntext** and [link\ntext](https://example.com)", HARD);
        let inline = paragraph(&doc, 0);
        assert_eq!(inline.text, "bold\ntext and link\ntext");
        let strong = inline.spans.iter().find(|span| span.style.strong).unwrap();
        assert_eq!(&inline.text[strong.range.clone()], "bold\ntext");
        let link = inline
            .spans
            .iter()
            .find(|span| span.style.link.is_some())
            .unwrap();
        assert_eq!(&inline.text[link.range.clone()], "link\ntext");
    }

    #[test]
    fn breaks_between_lines_that_are_each_one_inline_element() {
        assert_eq!(prose("*a*\n*b*"), ["a\nb"]);
        assert_eq!(prose("`a`\n`b`"), ["a\nb"]);
        assert_eq!(prose("[a](https://x.com)\n[b](https://y.com)"), ["a\nb"]);
    }

    #[test]
    fn keeps_the_line_an_inline_element_opens() {
        for line in ["*second*", "`second`", "[second](https://x.com)"] {
            assert_eq!(
                prose(&format!("first\n{line}")),
                ["first\nsecond"],
                "{line}"
            );
        }
        assert_eq!(prose("> first\n> *second*"), ["first\nsecond"]);
    }

    #[test]
    fn keeps_the_line_after_a_raw_br() {
        assert_eq!(prose("first<br>second\nthird"), ["first\nsecond\nthird"]);
    }

    #[test]
    fn breaks_the_wrapped_lines_of_a_list_item() {
        assert_eq!(
            prose("- first item\n  continued"),
            ["first item\ncontinued"]
        );
    }

    #[test]
    fn adds_no_breaks_between_blocks() {
        for text in [
            "- a\n  - b\n- c",
            "- a\n\n- b",
            "- a\n\n  second para\n- b",
            "- a\n\n  ```txt\n  code line\n  ```\n- b",
            "1. first\n2. second\n\n   more of second",
        ] {
            for line in prose(text) {
                assert!(!line.contains('\n'), "{text:?} broke {line:?}");
            }
        }
    }

    #[test]
    fn leaves_a_markdown_hard_break_as_one_break() {
        for text in ["two spaces  \nnext line", "backslash\\\nnext line"] {
            let lines = prose(text);
            assert_eq!(lines.len(), 1);
            assert!(lines[0].ends_with("\nnext line"), "{lines:?}");
            assert_eq!(lines[0].matches('\n').count(), 1, "{lines:?}");
        }
    }

    #[test]
    fn drops_the_space_a_soft_break_left_behind() {
        assert_eq!(
            prose("trailing space \nnext line"),
            ["trailing space\nnext line"]
        );
    }

    #[test]
    fn does_not_end_a_paragraph_with_a_break() {
        assert_eq!(prose("one line\n"), ["one line"]);
    }

    #[test]
    fn leaves_code_alone() {
        let doc = parse_with("```txt\nline one\nline two\n```", HARD);
        match &doc.blocks[0].block {
            Block::Code(code) => assert_eq!(code.code, "line one\nline two"),
            other => panic!("expected code, got {other:?}"),
        }
        assert!(
            prose("a `code\nspan` b")
                .iter()
                .all(|line| line == "a code span b")
        );
    }

    #[test]
    fn leaves_a_reply_reflowing_by_default() {
        let doc = parse("first line\nsecond line");
        assert_eq!(paragraph(&doc, 0).text, "first line second line");
    }

    #[test]
    fn streams_the_same_as_a_full_parse() {
        for (ix, corpus) in CORPORA.iter().enumerate() {
            let full = parse_with(corpus, HARD);
            for chunk in [1usize, 3, 16] {
                let mut parser = IncrementalParser::with_options(HARD);
                let mut start = 0;
                while start < corpus.len() {
                    let mut end = (start + chunk).min(corpus.len());
                    while end < corpus.len() && !corpus.is_char_boundary(end) {
                        end += 1;
                    }
                    parser.append(&corpus[start..end]);
                    start = end;
                }
                assert_eq!(parser.tree(), &full, "corpus {ix} at chunk size {chunk}");
            }
        }
    }

    #[test]
    fn changing_the_option_reparses() {
        let mut parser = IncrementalParser::new();
        parser.set_text("first\nsecond");
        assert_eq!(paragraph(parser.tree(), 0).text, "first second");
        parser.set_options(HARD);
        assert_eq!(paragraph(parser.tree(), 0).text, "first\nsecond");
        assert_eq!(parser.source(), "first\nsecond");
    }
}
