//! Streaming markdown renderer for MonoCode agent replies and notes.
//!
//! Port of `src/features/sessions/ui/AgentMarkdown.tsx` and
//! `src/features/sessions/ui/wordFade.tsx`. See the crate README for the
//! design.
//!
//! The transcript embeds a [`MarkdownView`] per message:
//!
//! ```ignore
//! monocode_markdown::init(cx); // once, binds copy and select-all
//! let view = cx.new(|cx| {
//!     let mut view = MarkdownView::new(cx);
//!     view.set_style(app_theme_to_markdown_style(theme), cx);
//!     view.on_link_click(|link, window, cx| open_link(&link.url, window, cx));
//!     view
//! });
//! view.update(cx, |view, cx| {
//!     view.set_streaming(true, cx);
//!     view.push_str(delta, cx);
//! });
//! ```

pub mod fade;
pub mod highlight;
pub mod parse;
mod prepare;
mod render;
pub mod selection;
pub mod style;
mod view;

pub use parse::{
    Block, Document, IncrementalParser, Inline, InlineStyle, ParseOptions, parse, parse_with,
};
pub use render::{ImageResolver, default_image_source};
pub use style::{BlockMargins, MarkdownStyle, SyntaxColors};
pub use view::{
    Copy, KEY_CONTEXT, LinkClick, LinkHandler, MarkdownView, RenderedText, SelectAll, init,
};
