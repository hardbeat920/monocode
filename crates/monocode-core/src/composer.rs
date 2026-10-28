//! Composer tokens and turn preparation, independent of the UI.
use crate::{files::ProjectFile, skills::DiscoveredSkill};
use std::{
    io::Read,
    ops::Range,
    path::{Path, PathBuf},
};

pub const MAX_ATTACHMENTS: usize = 20;
pub const MAX_EMBED_BYTES: u64 = 20 * 1024 * 1024;

/// quoteDraft.ts: append selected rendered text without replacing a live draft.
pub fn append_selection_quote(draft: &str, text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = normalized.trim();
    if text.is_empty() {
        return draft.into();
    }
    let quote = text
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                ">".to_owned()
            } else {
                format!("> {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let separator = if draft.is_empty() || draft.ends_with("\n\n") {
        ""
    } else if draft.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{draft}{separator}{quote}\n\n")
}

#[cfg(test)]
mod quote_tests {
    use super::*;
    #[test]
    fn quote_preserves_existing_draft_blank_lines_and_unicode() {
        assert_eq!(
            append_selection_quote("draft", " one\r\n\r\ncafé 🙂 "),
            "draft\n\n> one\n>\n> café 🙂\n\n"
        );
        assert_eq!(append_selection_quote("draft\n", "x"), "draft\n\n> x\n\n");
        assert_eq!(append_selection_quote("draft\n\n", "x"), "draft\n\n> x\n\n");
        assert_eq!(append_selection_quote("unchanged", " \r\n "), "unchanged");
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Skill,
    Mention,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub trigger: Trigger,
    pub range: Range<usize>,
    pub query: String,
}

/// Byte offsets match the native editor. A trigger must start on a word boundary;
/// paths, URLs, emails, selections and IME marked text never become commands.
pub fn token_at(text: &str, cursor: usize) -> Option<Token> {
    if !text.is_char_boundary(cursor) {
        return None;
    }
    let start = text[..cursor]
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or(0, |(i, c)| i + c.len_utf8());
    let fragment = &text[start..cursor];
    let trigger = match fragment.chars().next()? {
        '/' => Trigger::Skill,
        '@' => Trigger::Mention,
        _ => return None,
    };
    let query = &fragment[1..];
    if query.contains(['@', '\n', '\r'])
        || (trigger == Trigger::Skill && query.contains(['/', '.', '\\']))
    {
        return None;
    }
    if text[..start]
        .lines()
        .last()
        .is_some_and(|line| line.trim_start().starts_with('>'))
    {
        return None;
    }
    let end = text[cursor..]
        .find(char::is_whitespace)
        .map_or(text.len(), |i| cursor + i);
    Some(Token {
        trigger,
        range: start..end,
        query: query.to_owned(),
    })
}

pub fn consume_command<'a>(text: &'a str, command: &str) -> Option<&'a str> {
    let text = text.trim_start();
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    text[..end]
        .eq_ignore_ascii_case(&format!("/{command}"))
        .then(|| text[end..].trim_start())
}

/// Prefer leaf and consecutive matches, then a compact fuzzy subsequence.
pub fn score(query: &str, value: &str) -> Option<i64> {
    let query = query.to_lowercase();
    let value = value.to_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    if value == query {
        return Some(10_000);
    }
    let leaf = value.rsplit('/').next().unwrap_or(&value);
    if leaf.starts_with(&query) {
        return Some(5_000 - value.len() as i64);
    }
    if let Some(at) = value.find(&query) {
        return Some(3_000 - at as i64 - value.len() as i64);
    }
    let mut rest = value.as_str();
    let mut gaps = 0;
    for ch in query.chars() {
        let at = rest.find(ch)?;
        gaps += at;
        rest = &rest[at + ch.len_utf8()..];
    }
    Some(1_000 - gaps as i64 - value.len() as i64)
}
pub fn rank_files<'a>(files: &'a [ProjectFile], query: &str, limit: usize) -> Vec<&'a ProjectFile> {
    let mut matches: Vec<_> = files
        .iter()
        .filter_map(|f| score(query, &f.relative).map(|s| (s, f)))
        .collect();
    matches.sort_unstable_by(|(a, x), (b, y)| b.cmp(a).then_with(|| x.relative.cmp(&y.relative)));
    matches.truncate(limit);
    matches.into_iter().map(|(_, file)| file).collect()
}
pub fn validate_attachments(paths: &[PathBuf]) -> Result<(), String> {
    if paths.len() > MAX_ATTACHMENTS {
        return Err(format!("Attach up to {MAX_ATTACHMENTS} files at a time."));
    }
    let mut total = 0;
    for path in paths {
        let meta = path
            .metadata()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if !meta.is_file() {
            return Err(format!("{} is not a file.", path.display()));
        }
        total += meta.len();
        if total > MAX_EMBED_BYTES {
            return Err("Attachments must total 20 MB or less.".into());
        }
    }
    Ok(())
}
pub fn image_mime(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}
pub fn image_data(path: &Path) -> Result<String, String> {
    use base64::Engine;
    let mut data = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_EMBED_BYTES + 1).read_to_end(&mut data))
        .map_err(|e| e.to_string())?;
    if data.len() as u64 > MAX_EMBED_BYTES {
        return Err("Image is larger than 20 MB.".into());
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(data))
}
/// Decode a saved desktop image attachment off the UI thread.
pub fn decode_image_data(data: &str) -> Result<Vec<u8>, String> {
    if crate::remote::attachments::is_attachment_source(data) {
        return crate::remote::attachments::read_attachment_source(data);
    }
    use base64::Engine;
    let data = data
        .strip_prefix("data:")
        .and_then(|data| data.split_once(',').map(|(_, data)| data))
        .unwrap_or(data);
    if data.len() as u64 > MAX_EMBED_BYTES.div_ceil(3) * 4 {
        return Err("Image is larger than 20 MB.".into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_EMBED_BYTES {
        return Err("Image is larger than 20 MB.".into());
    }
    Ok(bytes)
}
/// Shrink an attachment until its shorter side just covers `side` pixels, so a
/// chip never holds a full screenshot in memory. BGRA, as GPU sprites expect.
pub fn thumbnail_bgra(bytes: &[u8], side: u32) -> Result<image::RgbaImage, String> {
    let image = image::load_from_memory(bytes).map_err(|error| error.to_string())?;
    let (width, height) = (image.width(), image.height());
    let scale = (f64::from(side) / f64::from(width.min(height).max(1))).min(1.);
    let mut image = if scale < 1. {
        image.thumbnail_exact(
            ((f64::from(width) * scale).round() as u32).max(1),
            ((f64::from(height) * scale).round() as u32).max(1),
        )
    } else {
        image
    }
    .into_rgba8();
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok(image)
}
/// Resolve only exact, boundary-delimited known skill invocations. The original
/// draft remains the visible user message; these instructions travel on the wire.
pub fn prepare_prompt(text: &str, cwd: &str, attachments: &[PathBuf]) -> Result<String, String> {
    validate_attachments(attachments)?;
    let mut blocks = Vec::new();
    let mut context_bytes = 0;
    if text.split_whitespace().any(|word| word.starts_with('/')) {
        for skill in crate::skills::list_skills(cwd.to_owned(), None)? {
            if invoked(text, &skill) {
                let body = read_text(Path::new(&skill.path), 256 * 1024)?;
                context_bytes += body.len();
                if context_bytes > 512 * 1024 {
                    return Err(
                        "Selected skills and attachments exceed the 512 KB text context limit."
                            .into(),
                    );
                }
                blocks.push(format!("## /{}\nSkill location: {}\nFollow this skill's instructions; resolve its relative resources from its directory.\n\n{body}", skill.name, skill.path));
            }
        }
    }
    // Mentions use full project-relative paths, including quoted paths with spaces.
    let mentions = mentions(text);
    for relative in mentions {
        let path = Path::new(cwd).join(&relative);
        if let (Ok(root), Ok(path)) = (Path::new(cwd).canonicalize(), path.canonicalize()) {
            if path.starts_with(&root) && path.is_file() {
                blocks.push(format!("Referenced project file: {}", path.display()));
            }
        }
    }
    for path in attachments {
        let mut block = format!("Attached file: {}", path.display());
        if image_mime(path).is_none() {
            if let Ok(body) = read_text(path, 128 * 1024) {
                if !body.contains('\0') {
                    context_bytes += body.len();
                    if context_bytes > 512 * 1024 {
                        return Err(
                            "Selected skills and attachments exceed the 512 KB text context limit."
                                .into(),
                        );
                    }
                    block.push_str(&format!("\n<attached_file>\n{body}\n</attached_file>"));
                }
            }
        }
        blocks.push(block);
    }
    let text = if text.trim().is_empty() && !attachments.is_empty() {
        "The user attached these files. Use the conversation to determine what to do with them; ask if the intent is unclear."
    } else {
        text
    };
    if blocks.is_empty() {
        Ok(text.to_owned())
    } else {
        Ok(format!("{}\n\n---\n\n{text}", blocks.join("\n\n")))
    }
}
fn invoked(text: &str, skill: &DiscoveredSkill) -> bool {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('>'))
        .any(|line| {
            line.split_whitespace().any(|word| {
                word.strip_prefix('/').is_some_and(|name| {
                    name.trim_end_matches([',', '.', ';', '!', '?']) == skill.name
                })
            })
        })
}
fn read_text(path: &Path, max: usize) -> Result<String, String> {
    let mut data = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(max as u64 + 1).read_to_end(&mut data))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if data.len() > max {
        return Err(format!(
            "{} exceeds the text attachment limit ({max} bytes).",
            path.display()
        ));
    }
    String::from_utf8(data).map_err(|e| e.to_string())
}
pub fn mention_text(relative: &str) -> String {
    if relative.chars().any(char::is_whitespace) {
        format!("@\"{}\"", relative.replace('"', "\\\""))
    } else {
        format!("@{relative}")
    }
}
pub fn highlight_ranges(
    text: &str,
    known: &std::collections::HashSet<String>,
) -> Vec<Range<usize>> {
    static TOKENS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let pattern = TOKENS.get_or_init(|| {
        regex::Regex::new(r#"(?m)(?:^|\s)(/[a-z0-9][a-z0-9:-]*|@"(?:\\.|[^"\r\n])*"|@[^\s]+)"#)
            .expect("composer token pattern")
    });
    pattern
        .captures_iter(text)
        .filter_map(|capture| {
            let token = capture.get(1)?;
            if !known.contains(token.as_str()) {
                return None;
            }
            let line_start = text[..token.start()].rfind('\n').map_or(0, |i| i + 1);
            (!text[line_start..token.start()]
                .trim_start()
                .starts_with('>'))
            .then(|| token.range())
        })
        .collect()
}
fn mentions(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('>'))
        .flat_map(mentions_in_line)
        .collect()
}
fn mentions_in_line(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut chars = text.chars().peekable();
    let mut boundary = true;
    while let Some(c) = chars.next() {
        if c == '@' && boundary {
            let quoted = chars.peek() == Some(&'"');
            if quoted {
                chars.next();
            }
            let mut path = String::new();
            while let Some(&next) = chars.peek() {
                if (quoted && next == '"') || (!quoted && next.is_whitespace()) {
                    break;
                }
                path.push(next);
                chars.next();
            }
            if quoted {
                chars.next();
            }
            result.push(path);
        }
        boundary = c.is_whitespace();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thumbnails_cover_the_chip_and_swap_to_bgra() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(2738, 1864, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let thumb = thumbnail_bgra(&png, 108).unwrap();
        assert_eq!(thumb.dimensions(), (159, 108));
        assert_eq!(thumb.get_pixel(0, 0).0, [30, 20, 10, 255]);
        let small = thumbnail_bgra(&png, 4000).unwrap();
        assert_eq!(small.dimensions(), (2738, 1864));
        assert!(thumbnail_bgra(b"not an image", 108).is_err());
    }
    #[test]
    fn token_boundaries_and_unicode() {
        let text = "漢字 /rev tail";
        assert_eq!(token_at(text, 11).unwrap().range, 7..11);
        assert!(token_at("https://host", 12).is_none());
        assert!(token_at("me@host", 7).is_none());
        assert!(token_at("> /review", 9).is_none());
        assert!(token_at("/tmp/file", 9).is_none());
        assert_eq!(token_at("@src/ma", 7).unwrap().trigger, Trigger::Mention);
    }
    #[test]
    fn replacement_and_commands_have_exact_boundaries() {
        assert_eq!(consume_command(" /PLAN fix this", "plan"), Some("fix this"));
        assert_eq!(consume_command("/plan-extra", "plan"), None);
        assert_eq!(
            mentions("Use @\"src/my file.rs\" and @src/lib.rs"),
            vec!["src/my file.rs", "src/lib.rs"]
        );
        assert_eq!(
            mentions("> @quoted.rs\n  > @also-quoted.rs\nUse @actual.rs"),
            vec!["actual.rs"]
        );
        assert!(score("cpr", "src/composer.rs").is_some());
        assert!(score("xyz", "src/composer.rs").is_none());
    }
    #[test]
    fn skills_and_text_attachments_reach_the_prompt() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir_all(root.join(".agents/skills/native-test-skill")).unwrap();
        std::fs::write(
            root.join(".agents/skills/native-test-skill/SKILL.md"),
            "---\nname: native-test-skill\n---\nInspect before edits.",
        )
        .unwrap();
        let file = root.join("context.txt");
        std::fs::write(&file, "useful context").unwrap();
        let prompt =
            prepare_prompt("/native-test-skill do it", root.to_str().unwrap(), &[file]).unwrap();
        assert!(prompt.contains("Inspect before edits."));
        assert!(prompt.contains("useful context"));
        assert!(prompt.ends_with("/native-test-skill do it"));
    }
}
