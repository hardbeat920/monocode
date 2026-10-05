//! Text selection across every text element of one rendered message.
//!
//! GPUI has no selection for plain text elements, and a message renders as
//! many of them (paragraphs, list items, cells, code). Each frame the renderer
//! lists its painted text elements in document order. A selection is an
//! anchor and a head, each an element key plus a byte offset. It resolves
//! against that list into one range per element: partial in the anchor and
//! head elements, whole in every element between them. Copy joins the
//! ranges with the separator each element asks for.
//!
//! This is the pure half; geometry and mouse handling live in `render`.
//! The document-order model follows zeronsh/comet's `markdown/selection.rs`
//! (MIT, Copyright (c) 2026 Wing).

use std::ops::Range;

/// Stable identity of a rendered text element: the top-level block index in
/// the high bits and the element's order within the block in the low bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ElementKey(pub u64);

impl ElementKey {
    pub fn new(block: usize, sub: usize) -> Self {
        Self(((block as u64) << 24) | (sub as u64 & 0xff_ffff))
    }

    pub fn block(self) -> usize {
        (self.0 >> 24) as usize
    }
}

/// What goes between this element's copied text and the previous element's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Separator {
    /// Next cell in a table row.
    Tab,
    /// Next line in the same block (list items, table rows).
    Line,
    /// A new block.
    Paragraph,
}

impl Separator {
    fn as_str(self) -> &'static str {
        match self {
            Separator::Tab => "\t",
            Separator::Line => "\n",
            Separator::Paragraph => "\n\n",
        }
    }
}

/// One painted text element as the selection sees it.
#[derive(Clone, Debug)]
pub struct Element {
    pub key: ElementKey,
    /// Display text, exactly as laid out.
    pub text: gpui::SharedString,
    /// Byte ranges of display-only characters (inline code padding) that
    /// copy leaves out.
    pub hidden: Vec<Range<usize>>,
    pub separator: Separator,
    /// Triple click selects a line instead of the whole element (code).
    pub line_mode: bool,
}

/// A selection endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub key: ElementKey,
    pub offset: usize,
}

/// Selection state for one message.
#[derive(Clone, Debug, Default)]
pub struct Selection {
    anchor: Option<Point>,
    head: Option<Point>,
    /// The anchor end when a double or triple click started the selection,
    /// so dragging keeps the whole word or line.
    anchor_end: Option<Point>,
    dragging: bool,
    /// Every element is selected (select all).
    all: bool,
}

impl Selection {
    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    pub fn is_empty(&self) -> bool {
        if self.all {
            return false;
        }
        match (self.anchor, self.head) {
            (Some(a), Some(h)) => a == h && self.anchor_end.is_none_or(|end| end == a),
            _ => true,
        }
    }

    /// Start a drag at `point`.
    pub fn begin(&mut self, point: Point) {
        *self = Selection {
            anchor: Some(point),
            head: Some(point),
            anchor_end: None,
            dragging: true,
            all: false,
        };
    }

    /// Start a drag with a range already selected (double or triple click).
    pub fn begin_range(&mut self, key: ElementKey, range: Range<usize>) {
        *self = Selection {
            anchor: Some(Point {
                key,
                offset: range.start,
            }),
            head: Some(Point {
                key,
                offset: range.end,
            }),
            anchor_end: Some(Point {
                key,
                offset: range.end,
            }),
            dragging: true,
            all: false,
        };
    }

    /// Move the head of the drag. Returns whether anything changed.
    pub fn drag_to(&mut self, point: Point, order: &[ElementKey]) -> bool {
        if !self.dragging || self.head == Some(point) {
            return false;
        }
        if let (Some(anchor), Some(anchor_end)) = (self.anchor, self.anchor_end) {
            // Keep the clicked word or line selected whichever way the drag
            // goes.
            let before = compare(point, anchor, order).is_lt();
            if before {
                self.head = Some(point);
                self.anchor = Some(anchor_end.max_by(anchor, order));
                self.anchor_end = Some(anchor_end.min_by(anchor, order));
            } else {
                self.anchor = Some(anchor.min_by(anchor_end, order));
                self.anchor_end = Some(anchor.max_by(anchor_end, order));
                self.head = Some(if compare(point, self.anchor_end.unwrap(), order).is_lt() {
                    self.anchor_end.unwrap()
                } else {
                    point
                });
            }
            return true;
        }
        self.head = Some(point);
        true
    }

    /// Finish the drag. Returns whether a non-empty selection remains.
    pub fn end(&mut self) -> bool {
        self.dragging = false;
        if self.is_empty() {
            *self = Selection::default();
            false
        } else {
            true
        }
    }

    pub fn clear(&mut self) -> bool {
        let had = !self.is_empty() || self.dragging;
        *self = Selection::default();
        had
    }

    pub fn select_all(&mut self) {
        *self = Selection {
            all: true,
            ..Selection::default()
        };
    }

    /// The selected byte range of each element, by element index.
    pub fn ranges(&self, elements: &[Element]) -> Vec<(usize, Range<usize>)> {
        if self.all {
            return elements
                .iter()
                .enumerate()
                .filter(|(_, e)| !e.text.is_empty())
                .map(|(ix, e)| (ix, 0..e.text.len()))
                .collect();
        }
        let (Some(anchor), Some(head)) = (self.anchor, self.head) else {
            return Vec::new();
        };
        let index = |key: ElementKey| elements.iter().position(|e| e.key == key);
        let (Some(a), Some(h)) = (index(anchor.key), index(head.key)) else {
            // An endpoint is no longer rendered: keep what still resolves.
            return self.partial_ranges(elements, anchor, head);
        };
        let (start, end) = if (a, anchor.offset) <= (h, head.offset) {
            ((a, anchor.offset), (h, head.offset))
        } else {
            ((h, head.offset), (a, anchor.offset))
        };
        let mut out = Vec::new();
        for (ix, element) in elements.iter().enumerate().take(end.0 + 1).skip(start.0) {
            let len = element.text.len();
            let from = if ix == start.0 { start.1.min(len) } else { 0 };
            let to = if ix == end.0 { end.1.min(len) } else { len };
            if from < to {
                out.push((ix, from..to));
            }
        }
        out
    }

    /// Ranges when one endpoint's element disappeared (the block was
    /// reparsed into a different shape): select from the surviving endpoint
    /// to the end of the message in the drag direction.
    fn partial_ranges(
        &self,
        elements: &[Element],
        anchor: Point,
        head: Point,
    ) -> Vec<(usize, Range<usize>)> {
        let index = |key: ElementKey| elements.iter().position(|e| e.key == key);
        match (index(anchor.key), index(head.key)) {
            (Some(a), None) if head.key > anchor.key => elements
                .iter()
                .enumerate()
                .skip(a)
                .map(|(ix, e)| {
                    let from = if ix == a {
                        anchor.offset.min(e.text.len())
                    } else {
                        0
                    };
                    (ix, from..e.text.len())
                })
                .filter(|(_, r)| !r.is_empty())
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The selected text, joined with each element's separator.
    pub fn text(&self, elements: &[Element]) -> Option<String> {
        let ranges = self.ranges(elements);
        if ranges.is_empty() {
            return None;
        }
        let mut out = String::new();
        for (i, (ix, range)) in ranges.iter().enumerate() {
            let element = &elements[*ix];
            if i > 0 {
                out.push_str(element.separator.as_str());
            }
            push_visible(&mut out, &element.text, range.clone(), &element.hidden);
        }
        Some(out)
    }
}

impl Point {
    fn min_by(self, other: Point, order: &[ElementKey]) -> Point {
        if compare(self, other, order).is_le() {
            self
        } else {
            other
        }
    }

    fn max_by(self, other: Point, order: &[ElementKey]) -> Point {
        if compare(self, other, order).is_ge() {
            self
        } else {
            other
        }
    }
}

fn compare(a: Point, b: Point, order: &[ElementKey]) -> std::cmp::Ordering {
    let position = |key: ElementKey| order.iter().position(|k| *k == key);
    match (position(a.key), position(b.key)) {
        (Some(x), Some(y)) => (x, a.offset).cmp(&(y, b.offset)),
        _ => (a.key, a.offset).cmp(&(b.key, b.offset)),
    }
}

fn push_visible(out: &mut String, text: &str, range: Range<usize>, hidden: &[Range<usize>]) {
    let mut at = range.start;
    for skip in hidden {
        if skip.end <= at || skip.start >= range.end {
            continue;
        }
        if skip.start > at {
            out.push_str(&text[at..skip.start]);
        }
        at = at.max(skip.end);
    }
    if at < range.end {
        out.push_str(&text[at..range.end]);
    }
}

/// The word around `offset` for a double click: a run of letters, digits,
/// and `_`, a run of spaces, or the single character under the pointer.
pub fn word_range(text: &str, offset: usize) -> Range<usize> {
    let mut offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let at = text[offset..].chars().next();
    let before = text[..offset].chars().next_back();
    let class = match at {
        Some(c) if is_word(c) => Some(true),
        _ if before.is_some_and(is_word) => {
            // Clicking just past a word selects the word.
            return word_range(text, offset - before.map_or(0, char::len_utf8));
        }
        Some(c) if c.is_whitespace() => Some(false),
        Some(c) => return offset..offset + c.len_utf8(),
        None => return offset..offset,
    };
    let matches = |c: char| match class {
        Some(true) => is_word(c),
        _ => c.is_whitespace() && c != '\n',
    };
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| matches(*c))
        .last()
        .map_or(offset, |(ix, _)| ix);
    let end = text[offset..]
        .char_indices()
        .take_while(|(_, c)| matches(*c))
        .last()
        .map_or(offset, |(ix, c)| offset + ix + c.len_utf8());
    start..end
}

/// The line around `offset` for a triple click in code.
pub fn line_range(text: &str, offset: usize) -> Range<usize> {
    let offset = offset.min(text.len());
    let start = text[..offset].rfind('\n').map_or(0, |ix| ix + 1);
    let end = text[offset..]
        .find('\n')
        .map_or(text.len(), |ix| offset + ix);
    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(block: usize, sub: usize, text: &str, separator: Separator) -> Element {
        Element {
            key: ElementKey::new(block, sub),
            text: text.to_string().into(),
            hidden: Vec::new(),
            separator,
            line_mode: false,
        }
    }

    fn sample() -> Vec<Element> {
        vec![
            element(0, 0, "first paragraph", Separator::Paragraph),
            element(1, 0, "item one", Separator::Paragraph),
            element(1, 1, "item two", Separator::Line),
            element(2, 0, "last", Separator::Paragraph),
        ]
    }

    fn keys(elements: &[Element]) -> Vec<ElementKey> {
        elements.iter().map(|e| e.key).collect()
    }

    fn point(block: usize, sub: usize, offset: usize) -> Point {
        Point {
            key: ElementKey::new(block, sub),
            offset,
        }
    }

    #[test]
    fn selection_inside_one_element() {
        let elements = sample();
        let mut selection = Selection::default();
        selection.begin(point(0, 0, 6));
        assert!(selection.is_empty());
        selection.drag_to(point(0, 0, 15), &keys(&elements));
        assert!(selection.end());
        assert_eq!(selection.text(&elements).as_deref(), Some("paragraph"));
    }

    #[test]
    fn selection_across_blocks_uses_separators() {
        let elements = sample();
        let mut selection = Selection::default();
        selection.begin(point(0, 0, 6));
        selection.drag_to(point(2, 0, 2), &keys(&elements));
        assert_eq!(
            selection.text(&elements).as_deref(),
            Some("paragraph\n\nitem one\nitem two\n\nla")
        );
        // A backwards drag selects the same text.
        let mut backwards = Selection::default();
        backwards.begin(point(2, 0, 2));
        backwards.drag_to(point(0, 0, 6), &keys(&elements));
        assert_eq!(backwards.text(&elements), selection.text(&elements));
    }

    #[test]
    fn click_without_drag_clears_on_release() {
        let mut selection = Selection::default();
        selection.begin(point(0, 0, 3));
        assert!(!selection.end());
        assert!(selection.is_empty());
    }

    #[test]
    fn double_click_word_survives_dragging_both_ways() {
        let elements = vec![element(0, 0, "alpha beta gamma", Separator::Paragraph)];
        let order = keys(&elements);
        let mut selection = Selection::default();
        selection.begin_range(ElementKey::new(0, 0), word_range("alpha beta gamma", 7));
        assert_eq!(selection.text(&elements).as_deref(), Some("beta"));
        selection.drag_to(point(0, 0, 14), &order);
        assert_eq!(selection.text(&elements).as_deref(), Some("beta gam"));
        selection.drag_to(point(0, 0, 2), &order);
        assert_eq!(selection.text(&elements).as_deref(), Some("pha beta"));
    }

    #[test]
    fn hidden_padding_is_not_copied() {
        let mut code = element(0, 0, "run \u{a0}cargo\u{a0} now", Separator::Paragraph);
        code.hidden = vec![4..6, 11..13];
        let mut selection = Selection::default();
        selection.select_all();
        assert_eq!(selection.text(&[code]).as_deref(), Some("run cargo now"));
    }

    #[test]
    fn select_all_and_clear() {
        let elements = sample();
        let mut selection = Selection::default();
        selection.select_all();
        assert_eq!(
            selection.text(&elements).as_deref(),
            Some("first paragraph\n\nitem one\nitem two\n\nlast")
        );
        assert!(selection.clear());
        assert_eq!(selection.text(&elements), None);
    }

    #[test]
    fn table_cells_copy_with_tabs() {
        let elements = vec![
            element(0, 0, "a", Separator::Paragraph),
            element(0, 1, "b", Separator::Tab),
            element(0, 2, "1", Separator::Line),
            element(0, 3, "2", Separator::Tab),
        ];
        let mut selection = Selection::default();
        selection.select_all();
        assert_eq!(selection.text(&elements).as_deref(), Some("a\tb\n1\t2"));
    }

    #[test]
    fn words_and_lines() {
        let text = "let foo_bar = 12;";
        assert_eq!(&text[word_range(text, 5)], "foo_bar");
        assert_eq!(&text[word_range(text, 11)], "foo_bar");
        assert_eq!(&text[word_range(text, 12)], "=");
        assert_eq!(&text[word_range(text, 15)], "12");
        let unicode = "héllo wörld";
        assert_eq!(&unicode[word_range(unicode, 2)], "héllo");
        let code = "one\ntwo\nthree";
        assert_eq!(&code[line_range(code, 5)], "two");
        assert_eq!(&code[line_range(code, 0)], "one");
        assert_eq!(&code[line_range(code, code.len())], "three");
    }
}
