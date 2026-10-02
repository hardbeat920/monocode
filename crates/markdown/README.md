# monocode-markdown

Streaming markdown renderer for MonoCode agent replies and notes, drawn with GPUI. It replaces `AgentMarkdown.tsx` (Streamdown, Shiki through `@streamdown/code`, Mermaid through `@streamdown/mermaid`) and `wordFade.tsx`.

## Using it

```rust
monocode_markdown::init(cx); // once: binds cmd-c / ctrl-c and cmd-a / ctrl-a in the "MarkdownView" context

let view = cx.new(|cx| {
    let mut view = MarkdownView::new(cx);
    view.set_style(theme_to_markdown_style(&theme), cx);
    view.on_link_click(|link, window, cx| open_link(&link.url, window, cx));
    view
});

// While the agent writes:
view.update(cx, |view, cx| {
    view.set_streaming(true, cx);
    view.push_str(delta, cx);
});
// When the turn ends:
view.update(cx, |view, cx| view.set_streaming(false, cx));
```

The main types:

- `MarkdownView`: the entity to embed. `new`, `with_text` (shows text at once, for history), `set_text` (an extension streams in as an append, anything else replaces the message), `push_str`, `set_streaming`, `set_style`, `set_reasoning` (the dimmer `.agent-reasoning` colors), `set_reduced_motion`, `on_link_click`, `set_image_resolver`, `selected_text`, `clear_selection`, `is_animating`, `document`, `rendered_text`.
- `MarkdownStyle`: every color, font, size, and margin. `MarkdownStyle::dark()` and `MarkdownStyle::light()` carry the values from `src/styles/index.css` and Streamdown's classes; `MarkdownStyle::with_content(...)` derives the `color-mix` shades from one content color. The crate never reads the app theme.
- `LinkClick { url }`: what the link callback gets. Without a callback, `http`, `https`, and `mailto` links open in the browser and other links do nothing, as in AgentMarkdown. A link whose URL is still streaming does not respond.
- `default_image_source` and `set_image_resolver`: the default loads `http(s)` URLs, absolute and `~/` paths, `file://` URLs, and `data:image/…;base64` URLs. AgentMarkdown only shows data URLs, note assets, and inbox media, so the app should pass a resolver with that policy.
- `parse`, `IncrementalParser`, `Document`, `Block`, `Inline`: the parser layer, usable without the view (notes search, plain-text export).
- `highlight::{syntaxes_blocking, CodeHighlight}`: the highlighter, for other views that show code.

## Design

### Parsing

pulldown-cmark 0.13 with tables, task lists, and strikethrough, plus GFM literal autolinks (`https://`, `http://`, `www.`), which pulldown-cmark lacks. Raw HTML shows as text, except `<br>`.

`IncrementalParser` reparses from the start of the second-to-last top-level block on each append and keeps every earlier block's `Arc`, so an append costs O(tail) and the view keeps its per-block caches. A source with a link reference definition falls back to full parses, since a definition can change text anywhere. The tests stream corpora at several chunk sizes and require the result to equal a full parse.

While text streams, the view shows a display tree in which the last block is mended, after Streamdown's `remend`:

- An unclosed `**`, `*`, `_`, `~~`, or backtick span gets its closer, so the styling is right from the first character and the line does not reflow when the real closer arrives. A lone marker with nothing after it stays literal.
- `[text](partial-url` shows the link text, styled, without the URL.
- A last line that is only a block marker waiting for content (`-`, `1.`, `#`, `=`, a partial table delimiter row) is hidden, so the paragraph above does not flash into a heading or gain a stray character.
- An unclosed code fence shows its finished lines as code. A half-typed closing fence is hidden, and the language label waits for the info line to finish.
- A half table row renders as a row with the cells that have arrived.

When the stream ends the view switches to the tree as written, so a marker that never closed shows literally.

### Rendering

Each top-level block is prepared once (display string, text runs, link and inline-code ranges) and reused while the parser keeps its `Arc`. Block margins collapse the way CSS margins do. Code blocks are one text element per block with a line number column, a header with the language or file label (`parseCodeFence` is ported, including `12:20:path` citations and `startLine=`), a horizontal scroller that lets vertical wheel events through, and a copy button that writes the code without its trailing newline and shows a check mark for 1.5 seconds.

Mermaid has no Rust renderer, so Mermaid fences render as highlighted code with a `mermaid` label, through a small bundled grammar.

### Highlighting: syntect with two-face

The highlighter is syntect 5.3 running the two-face 0.5 grammar set (bat's Sublime syntaxes) on the Oniguruma regex engine. The reasons, against tree-sitter:

- Shiki, which the React app uses, runs TextMate grammars. Sublime syntaxes use the same scope names, so the token classes match what users see today, and the `github-dark` and `github-light` scope selectors port directly (`highlight::SELECTORS`).
- syntect keeps the parse state after each line, so a code block that streams in only pays for its new lines. tree-sitter would reparse the block and rerun highlight queries on each append.
- One dependency covers about 200 languages, including every language in AgentMarkdown's file-name map. Each tree-sitter grammar is a separate C crate that adds build time.
- syntect and two-face are MIT (two-face also Apache-2.0); Oniguruma is BSD-2-Clause.

The pure-Rust `fancy-regex` engine was tried first and measured 4.5 to 7 times slower (600 lines of TypeScript: 529ms against 77ms), too slow to highlight a streaming block on the UI thread, so the crate uses Oniguruma, which the `cc` crate compiles from C. Loading the grammar set takes tens of milliseconds, so it loads off the UI thread on first use and code shows uncolored until then. A block that grows by up to 12 lines in a frame highlights on the UI thread; bigger work, such as a finished reply opened from history, runs as a background job and the block colors when it returns.

### Frame cost

The first frame of a message lays out every block. After that, a block more than a viewport away from the visible part of the view renders as an empty box of its measured height, and its text still takes part in selection and copy. A block lays out again when it changes, when the view's width changes, or when it comes near the visible area. So a frame costs about one screen of text plus the streaming tail, however long the reply.

`tests/perf.rs` streams and redraws a 2,160-line reply with 30 code blocks in a 760x900 headless window with CoreText shaping. Release build, M3 Pro:

| Measure | Time |
| --- | --- |
| Full parse | 1.2ms |
| Streamed parse, 14,709 appends of 7 bytes | 86ms total, worst append 0.34ms, at most 2,207 bytes reparsed |
| First frame, every block laid out and all code highlighted | 183ms |
| Redraw with nothing new | 2.8ms (p50) |
| Streaming frames, 12 bytes each, paced and fading | 5.2ms p50, 9.1ms p95, 19.7ms max |

The headless test dispatcher runs background highlight jobs on the frame's thread, so these numbers include highlighting that runs on a worker thread in the app. `MONOCODE_MARKDOWN_TRACE=1` prints each frame's parse, prepare, and element-building time.

### Selection

GPUI has no selection for text elements, and a message is many of them. Each frame, every text element registers its layout in document order. A selection is an anchor and a head, each an element key and a byte offset; it resolves to a partial range in the end elements and whole ranges between them. Drag, double click (word), triple click (paragraph, or line in code), select all, and copy work across paragraphs, lists, tables, quotes, and code. Copy joins elements with a blank line between blocks, a newline between list items and table rows, and a tab between cells, and leaves out the padding around inline code. Selection spans one message; the transcript needs its own layer to select across messages.

### Fade

`fade::Pacer` ports `usePacedText`: streamed text is let out a word at a time at a rate that closes on the backlog over 0.22s (at least 90 characters a second), a word still being written is held for 150ms, and text present before streaming shows at once. `fade::RevealTimeline` records when the reveal reached each source offset, and each word's color fades in over 320ms with CSS `ease-out`. Links, inline code, and code blocks do not fade, as in `UNFADED_TAGS`. With `App::reduce_motion` or `set_reduced_motion(true)` nothing fades; the paced reveal still runs, as in the React app, where reduced motion only disables the CSS animation.

## Running

```sh
cargo run -p monocode-markdown --example stream            # streams examples/sample.md into a window
cargo run -p monocode-markdown --example stream -- --screenshot out.png [--light] [--width 760] [--height 1400] [--offset 900] [--stream-to 0.3 --fade-ms 120]
cargo test -p monocode-markdown -j 4
cargo test -p monocode-markdown --release --test perf -- --nocapture   # timing numbers
```

`--screenshot` renders offscreen through `HeadlessAppContext` with the platform text system and the Metal headless renderer (macOS only), and writes a PNG at 2x scale.

## Known gaps

- Inline code uses the body font size. GPUI text runs cannot change size inside a line, so the chip is larger than the CSS `0.8em`.
- Images sit on their own line between text runs; they do not flow inline with text.
- No file-type icons in code block headers, and inline code that names a file is not clickable yet (`parse::fence::inline_file_name` is ported for when it is).
- Task checkboxes replace the list bullet instead of sitting after it, and they are not clickable.
- Selection does not extend across messages.
- The first frame of a long finished reply lays out every block (183ms for the 2,160-line test reply in release), because block heights are unknown until then.

## Third-party code

The incremental boundary strategy, the delimiter scanner in `parse/mend.rs`, the autolink rules, and the document-order selection model are adapted from zeronsh/comet (`crates/ui/src/markdown`), MIT License, Copyright (c) 2026 Wing.
