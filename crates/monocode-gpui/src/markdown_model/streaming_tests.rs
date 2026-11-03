use super::*;

fn content(tree: &BlockTree) -> Vec<&Block> {
    tree.blocks.iter().map(|block| &block.block).collect()
}

#[test]
fn appends_and_display_repairs_match_full_parses_at_every_character() {
    let samples = [
        "Intro **bold** café.\n\nAnother *paragraph*.\n\nLast words.",
        "Intro\n\nHeading\n---\n\na | b\n-- | --\nc | d\n",
        "# heading\n\n- first\n\n  paragraph\n\n3. next\n\nend",
        "before\n\n> quote\ncontinued\n\n> second\n\nend",
        ">a\n|b\n>|-c\n\nend\n\nmore",
        "before\n\n```rust\nlet x = **literal**;\n```\n\ntext **open",
        "[docs][ref]\n\nfirst\n\nsecond\n\n[ref]: https://example.com\n",
        "[docs]\n\n> [docs]: https://example.com\n\nend",
        "first\n\nRead [the docs](https://example.com/path) and `code`.",
        "first\n\n<div>\nignored\n</div>\n\nlast paragraph",
        "a\n\n$$x^2$$\n\n- [x] task\n  - nested\n\nlast",
        "first\n\n![image](file.png)\n\n***\n\n    indented\n\nlast",
        "<a>\n```\n_\n\n---\n# \n3- # a```\n",
        "é|```\n$$x$$`>---\n`\n\n$$x$$",
        "_3[x]$$x$$[x]. - >\n- a",
    ];
    for source in samples {
        let mut parser = MarkdownParser::new();
        for end in (0..=source.len()).filter(|&at| source.is_char_boundary(at)) {
            let text = &source[..end];
            parser.set_text(text);
            assert_eq!(
                content(parser.tree()),
                content(&parse_full(text)),
                "canonical {text:?}"
            );
            assert_eq!(
                content(&parser.display_tree()),
                content(&parse_full(&mend_stream(text))),
                "display {text:?}"
            );
        }
        parser.set_text("replacement **open");
        assert_eq!(
            content(parser.tree()),
            content(&parse_full("replacement **open"))
        );
    }
}

#[test]
fn mixed_markdown_appends_preserve_canonical_and_display_semantics() {
    let pieces = [
        "a",
        "é",
        "\n",
        "\n\n",
        "- ",
        ">",
        "3",
        ". ",
        "**",
        "_",
        "`",
        "```\n",
        "# ",
        "---\n",
        "|",
        "[x]",
        "[r]: /url\n",
        "<a>",
        "$$x$$",
    ];
    let mut seed = 41u64;
    for _ in 0..150 {
        let mut source = String::new();
        let mut parser = MarkdownParser::new();
        for _ in 0..30 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            source.push_str(pieces[(seed as usize >> 16) % pieces.len()]);
            parser.set_text(&source);
            assert_eq!(
                content(parser.tree()),
                content(&parse_full(&source)),
                "{source:?}"
            );
            assert_eq!(
                content(&parser.display_tree()),
                content(&parse_full(&mend_stream(&source))),
                "display {source:?}"
            );
        }
    }
}

#[test]
fn stable_blocks_and_display_trees_are_reused_and_parse_work_stays_in_the_tail() {
    let mut parser = MarkdownParser::new();
    let mut text = String::new();
    for n in 0..400 {
        text.push_str(&format!("Paragraph {n} with **formatted** text.\n\n"));
        parser.set_text(&text);
        let display = parser.display_tree();
        let repeated = parser.display_tree();
        assert!(
            display
                .blocks
                .iter()
                .zip(&repeated.blocks)
                .all(|(a, b)| Arc::ptr_eq(a, b))
        );
    }
    assert!(
        parser.parsed_bytes < text.len() * 4,
        "{} bytes parsed for {} bytes of text",
        parser.parsed_bytes,
        text.len()
    );
    let first = parser.tree.blocks[0].clone();
    text.push_str("last");
    parser.set_text(&text);
    assert!(Arc::ptr_eq(&first, &parser.tree.blocks[0]));
    parser.finish_display();
    assert!(parser.display.is_empty());
}
