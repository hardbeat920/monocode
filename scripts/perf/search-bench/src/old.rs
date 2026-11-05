//! The pre-refactor bridge: a fresh QuickJS runtime, the whole editor bundle,
//! and `monocodeSearch` from CodeMirror, exactly as `editor_analysis::search`
//! did before commit 03de5bb7.

use crate::editor_search::SearchResult;
use rquickjs::{Context, Function, Runtime};
use std::time::{Duration, Instant};

pub fn search(
    bundle: &str,
    source: &str,
    query: &str,
    replacement: &str,
    case_sensitive: bool,
    whole_word: bool,
    regex: bool,
) -> Result<SearchResult, String> {
    let runtime = Runtime::new().map_err(|e| e.to_string())?;
    runtime.set_memory_limit(256 * 1024 * 1024);
    runtime.set_max_stack_size(4 * 1024 * 1024);
    let deadline = Instant::now() + Duration::from_secs(3);
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline)));
    let context = Context::full(&runtime).map_err(|e| e.to_string())?;
    let json = context
        .with(|ctx| -> rquickjs::Result<String> {
            ctx.eval::<(), _>(bundle)?;
            let function: Function = ctx.globals().get("monocodeSearch")?;
            function.call((
                source,
                query,
                replacement,
                case_sensitive,
                whole_word,
                regex,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut result: SearchResult = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    let offsets = byte_offsets(source);
    for m in &mut result.matches {
        m.from = offsets[m.from.min(offsets.len() - 1)];
        m.to = offsets[m.to.min(offsets.len() - 1)];
    }
    Ok(result)
}

fn byte_offsets(source: &str) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(source.len() + 1);
    for (at, ch) in source.char_indices() {
        offsets.extend(std::iter::repeat_n(at, ch.len_utf16()));
    }
    offsets.push(source.len());
    offsets
}
