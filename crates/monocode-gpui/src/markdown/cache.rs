//! Prepared text is shared across fade frames and bounded to the viewport's
//! working set. Font metrics and colors remain owned by GPUI/the current theme.
use super::*;
use crate::markdown_model::InlineStyle;
use std::collections::HashMap;

pub(super) struct PreparedRun {
    pub range: Range<usize>,
    pub style: InlineStyle,
    pub word_start: usize,
    pub words: usize,
    pub pieces: Vec<(Range<usize>, Option<usize>)>,
}
pub(super) struct PreparedInline {
    pub text: SharedString,
    pub runs: Vec<PreparedRun>,
    pub fonts: Vec<(Range<usize>, SharedString)>,
    pub links: Vec<Range<usize>>,
    pub urls: Vec<String>,
}
impl PreparedInline {
    pub fn new(runs: &[InlineRun], heading: bool) -> Self {
        let mut text = String::new();
        let mut prepared = Vec::new();
        let mut fonts = Vec::new();
        let mut links = Vec::new();
        let mut urls = Vec::new();
        let mut word = 0;
        for run in runs {
            let start = text.len();
            let mut style = run.style.clone();
            style.bold |= heading;
            if let Some(task) = &style.task {
                text.push_str(if task.checked { "☑ " } else { "☐ " });
            }
            let mut cursor = text.len();
            text.push_str(&run.text);
            let range = start..text.len();
            let word_start = word;
            let mut pieces = Vec::new();
            if !style.code && !style.math && style.link.is_none() {
                for piece in run.text.split_inclusive(char::is_whitespace) {
                    let index = (!piece.trim().is_empty()).then(|| {
                        let index = word;
                        word += 1;
                        index
                    });
                    pieces.push((cursor..cursor + piece.len(), index));
                    cursor += piece.len();
                }
            }
            if style.code || style.math {
                fonts.push((range.clone(), crate::fonts::mono().into()));
            }
            if let Some(url) = &style.link {
                links.push(range.clone());
                urls.push(url.clone());
            }
            prepared.push(PreparedRun {
                range,
                style,
                word_start,
                words: word - word_start,
                pieces,
            });
        }
        Self {
            text: text.into(),
            runs: prepared,
            fonts,
            links,
            urls,
        }
    }
}

pub(crate) struct PreparedRow {
    pub row: Arc<MarkdownRow>,
    pub selection: SharedString,
    pub(super) inline: Option<PreparedInline>,
}
struct Entry {
    source: Arc<MarkdownRow>,
    prepared: Arc<PreparedRow>,
    frame: u64,
    bytes: usize,
}
#[derive(Default)]
pub(crate) struct RenderCache {
    entries: HashMap<(usize, usize), Entry>,
    frame: u64,
    base: Option<String>,
    search: Option<(usize, String)>,
}
const CACHE_ROWS: usize = 128;
const CACHE_BYTES: usize = 2 * 1024 * 1024;
fn runs_bytes(runs: &[InlineRun]) -> usize {
    std::mem::size_of_val(runs)
        + runs
            .iter()
            .map(|run| run.text.capacity() + run.style.link.as_ref().map_or(0, String::capacity))
            .sum::<usize>()
}
fn row_bytes(row: &MarkdownRow) -> usize {
    std::mem::size_of::<MarkdownRow>()
        + row.marker.as_ref().map_or(0, String::capacity)
        + match &row.part {
            Part::Text { runs, .. } => runs_bytes(runs),
            Part::Table { cells, align, .. } => {
                cells
                    .iter()
                    .flatten()
                    .map(|runs| runs_bytes(runs))
                    .sum::<usize>()
                    + std::mem::size_of_val(align.as_slice())
            }
            Part::Math(text) => text.capacity(),
            _ => 0,
        }
}
impl RenderCache {
    pub fn retained_bytes(&self) -> usize {
        self.entries.values().map(|entry| entry.bytes).sum()
    }
    pub fn begin_frame(&mut self, base: &Option<String>, search: &Option<(usize, String)>) {
        if &self.base != base || &self.search != search {
            self.entries.clear();
            self.base.clone_from(base);
            self.search.clone_from(search);
        }
        let previous = self.frame;
        self.entries.retain(|_, entry| entry.frame == previous);
        self.frame = self.frame.wrapping_add(1);
    }
    pub fn get(&mut self, key: (usize, usize), source: &Arc<MarkdownRow>) -> Arc<PreparedRow> {
        if let Some(entry) = self.entries.get_mut(&key)
            && (Arc::ptr_eq(&entry.source, source) || entry.source == *source)
        {
            entry.frame = self.frame;
            return entry.prepared.clone();
        }
        if self.entries.len() >= CACHE_ROWS {
            // A large layout probe must not retain the whole transcript.
            self.entries.retain(|_, entry| entry.frame == self.frame);
            if self.entries.len() >= CACHE_ROWS {
                self.entries.clear();
            }
        }
        let row = if self.base.is_some() || self.search.is_some() {
            let mut row = (**source).clone();
            if let Some(base) = &self.base {
                resolve_file_links(&mut row, base);
            }
            if let Some((at, query)) = &self.search {
                mark_row(&mut row, query, *at == key.0);
            }
            Arc::new(row)
        } else {
            source.clone()
        };
        let inline = match &row.part {
            Part::Text { runs, heading } if !runs.iter().any(|run| run.style.code) => {
                Some(PreparedInline::new(runs, *heading > 0))
            }
            _ => None,
        };
        let prepared = Arc::new(PreparedRow {
            selection: if matches!(row.part, Part::Code { .. }) {
                SharedString::default()
            } else {
                selection_text(&row).into()
            },
            row,
            inline,
        });
        let bytes = std::mem::size_of::<Entry>()
            + prepared.selection.len()
            + prepared.inline.as_ref().map_or(0, |inline| {
                inline.text.len()
                    + inline.runs.capacity() * std::mem::size_of::<PreparedRun>()
                    + inline
                        .runs
                        .iter()
                        .map(|run| {
                            run.pieces.capacity()
                                * std::mem::size_of::<(Range<usize>, Option<usize>)>()
                        })
                        .sum::<usize>()
                    + inline.urls.iter().map(String::capacity).sum::<usize>()
                    + inline
                        .runs
                        .iter()
                        .map(|run| run.style.link.as_ref().map_or(0, String::capacity))
                        .sum::<usize>()
                    + inline.fonts.capacity() * std::mem::size_of::<(Range<usize>, SharedString)>()
                    + inline.links.capacity() * std::mem::size_of::<Range<usize>>()
            })
            + if Arc::ptr_eq(source, &prepared.row) {
                0
            } else {
                row_bytes(&prepared.row)
            };
        if bytes > CACHE_BYTES {
            return prepared;
        }
        if self.retained_bytes() + bytes > CACHE_BYTES {
            self.entries.clear();
        }
        self.entries.insert(
            key,
            Entry {
                source: source.clone(),
                prepared: prepared.clone(),
                frame: self.frame,
                bytes,
            },
        );
        prepared
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(text: &str) -> Arc<MarkdownRow> {
        Arc::new(
            rows(&Block::Paragraph {
                runs: vec![InlineRun {
                    text: text.into(),
                    style: Default::default(),
                }],
            })
            .remove(0),
        )
    }

    #[test]
    fn prepared_text_is_reused_then_released_and_content_changes_invalidate_it() {
        let mut cache = RenderCache::default();
        cache.begin_frame(&None, &None);
        let source = row("already here café");
        let first = cache.get((1, 16), &source);
        assert_eq!(&*first.inline.as_ref().unwrap().text, "already here café");
        cache.begin_frame(&None, &None);
        assert!(Arc::ptr_eq(&first, &cache.get((1, 16), &source)));
        let changed = cache.get((1, 16), &row("new text"));
        assert!(!Arc::ptr_eq(&first, &changed));
        cache.begin_frame(&None, &None);
        cache.begin_frame(&None, &None);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn prepared_cache_respects_memory_and_row_limits_and_resolves_new_searches() {
        let mut cache = RenderCache::default();
        cache.begin_frame(&None, &None);
        for n in 0..500 {
            cache.get((n, 16), &row(&"many words ".repeat(150)));
            assert!(cache.entries.len() <= CACHE_ROWS);
            assert!(cache.retained_bytes() <= CACHE_BYTES);
        }
        cache.begin_frame(&None, &Some((2, "café".into())));
        let prepared = cache.get((2, 16), &row("read café"));
        assert!(
            prepared
                .inline
                .as_ref()
                .unwrap()
                .runs
                .iter()
                .any(|run| run.style.search == Some(true))
        );
    }
}
