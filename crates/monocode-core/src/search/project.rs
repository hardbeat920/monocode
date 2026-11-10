//! Project content search ported from the Tauri search service.
use crate::files::list_project_files_sync_cancellable;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
const MAX_MATCHES: usize = 500;
const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_GIT_GREP_BYTES: usize = 4 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchOptions {
    pub cwd: String,
    pub query: String,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default)]
    pub whole_word: bool,
    #[serde(default)]
    pub regex: bool,
    pub include: Option<String>,
    pub exclude: Option<String>,
    #[serde(default)]
    pub search_id: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    pub path: String,
    pub relative: String,
    pub line: u32,
    pub column: u32,
    pub preview: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub matches: Vec<SearchMatch>,
    pub truncated: bool,
}

/// Byte ranges for the first highlighted match in each project-search preview.
/// Compile once for the complete result set, away from the UI's paint path.
pub fn preview_ranges(
    matches: &[SearchMatch],
    options: &SearchOptions,
) -> Vec<Option<std::ops::Range<usize>>> {
    let pattern = if options.regex {
        options.query.trim().to_owned()
    } else {
        regex::escape(options.query.trim())
    };
    let matcher = regex::RegexBuilder::new(&pattern)
        .case_insensitive(!options.case_sensitive)
        .build()
        .ok();
    matches
        .iter()
        .map(|m| {
            matcher
                .as_ref()?
                .find(m.preview.trim_end())
                .map(|found| found.range())
        })
        .collect()
}

pub fn search_project(
    root: &Path,
    options: &SearchOptions,
    cancel: &AtomicBool,
) -> Result<SearchResult, String> {
    if crate::files::remote::is_remote(root) {
        if options.query.trim().is_empty() || cancel.load(Ordering::Acquire) {
            return Ok(SearchResult {
                matches: Vec::new(),
                truncated: false,
            });
        }
        let mut options = serde_json::to_value(options).map_err(|error| error.to_string())?;
        options["cwd"] = serde_json::json!(root);
        let result = crate::files::remote::command(
            "search_project",
            serde_json::json!({"options":options}),
        )?;
        return Ok(if cancel.load(Ordering::Acquire) {
            SearchResult {
                matches: Vec::new(),
                truncated: false,
            }
        } else {
            result
        });
    }
    let expanded = crate::scm::expand_home(root);
    let root = expanded.as_path();
    let query = options.query.trim();
    if query.is_empty() || cancel.load(Ordering::Acquire) || !crate::files::is_indexable_root(root)
    {
        return Ok(SearchResult {
            matches: Vec::new(),
            truncated: false,
        });
    }

    if let Some(result) = git_grep(root, options, query, cancel) {
        return Ok(result);
    }

    scan_files(root, options, query, cancel)
}

fn git_grep(
    root: &Path,
    options: &SearchOptions,
    query: &str,
    cancel: &AtomicBool,
) -> Option<SearchResult> {
    git_grep_capped(root, options, query, MAX_GIT_GREP_BYTES, cancel)
}

fn git_grep_capped(
    root: &Path,
    options: &SearchOptions,
    query: &str,
    max_bytes: usize,
    cancel: &AtomicBool,
) -> Option<SearchResult> {
    let mut args = vec!["grep".to_string(), "-z".to_string(), "-n".to_string()];
    if !options.case_sensitive {
        args.push("-i".to_string());
    }
    if options.whole_word {
        args.push("-w".to_string());
    }
    args.push(if options.regex { "-E" } else { "-F" }.to_string());
    args.push("-e".to_string());
    args.push(query.to_string());

    // Terminate option parsing so an include glob starting with `-` is treated
    // as a pathspec instead of a git grep flag.
    args.push("--".to_string());
    args.extend(pathspecs(&options.include, &options.exclude));

    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (mut raw, mut truncated) =
        match crate::files::git_output_capped(root, &args, max_bytes, Some(cancel)) {
            Some(output) => output,
            None if cancel.load(Ordering::Acquire) => {
                return Some(SearchResult {
                    matches: Vec::new(),
                    truncated: false,
                });
            }
            None => return None,
        };
    if truncated {
        // The cap can land inside a match record. Keep only complete lines so the
        // parser never invents a match from a partial path, number, or preview.
        if let Some(line_end) = raw.iter().rposition(|byte| *byte == b'\n') {
            raw.truncate(line_end + 1);
        } else {
            raw.clear();
        }
    }

    let root = root.to_path_buf();
    let mut matches = Vec::new();
    let mut offset = 0;
    while offset < raw.len() {
        let Some((path_bytes, next)) = read_until(&raw[offset..], 0) else {
            break;
        };
        offset += next;
        if path_bytes.is_empty() {
            continue;
        }

        let Some((line_bytes, next)) = read_until(&raw[offset..], 0) else {
            break;
        };
        offset += next;

        let line_end = raw[offset..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| offset + index)
            .unwrap_or(raw.len());
        let preview_bytes = &raw[offset..line_end];
        offset = line_end + (usize::from(line_end < raw.len()));

        let relative = String::from_utf8_lossy(path_bytes).replace('\\', "/");
        let line = std::str::from_utf8(line_bytes)
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(1);
        let preview = String::from_utf8_lossy(preview_bytes).to_string();
        let path = crate::scm::path_to_js(&root.join(&relative));
        let column = match_column(
            &preview,
            query,
            options.case_sensitive,
            options.whole_word,
            options.regex,
        );
        matches.push(SearchMatch {
            path,
            relative,
            line,
            column,
            preview,
        });
        if matches.len() >= MAX_MATCHES {
            truncated = true;
            break;
        }
    }

    Some(SearchResult { matches, truncated })
}

fn read_until(bytes: &[u8], delimiter: u8) -> Option<(&[u8], usize)> {
    let end = bytes.iter().position(|byte| *byte == delimiter)?;
    Some((&bytes[..end], end + 1))
}

fn scan_files(
    root: &Path,
    options: &SearchOptions,
    query: &str,
    cancel: &AtomicBool,
) -> Result<SearchResult, String> {
    if options.regex || cancel.load(Ordering::Acquire) {
        return Ok(SearchResult {
            matches: Vec::new(),
            truncated: false,
        });
    }

    let files = list_project_files_sync_cancellable(&root.to_string_lossy(), Some(cancel))?;
    if cancel.load(Ordering::Acquire) {
        return Ok(SearchResult {
            matches: Vec::new(),
            truncated: false,
        });
    }
    let include = glob_tokens(&options.include);
    let exclude = glob_tokens(&options.exclude);
    let needle = query.to_string();

    let matcher = regex::RegexBuilder::new(&regex::escape(&needle))
        .case_insensitive(!options.case_sensitive)
        .build()
        .map_err(|e| e.to_string())?;
    let mut matches = Vec::new();
    let mut truncated = false;

    'files: for file in files {
        if cancel.load(Ordering::Acquire) {
            return Ok(SearchResult {
                matches: Vec::new(),
                truncated: false,
            });
        }
        if !matches_pathspec(&file.relative, &include, &exclude) {
            continue;
        }
        let path = PathBuf::from(&file.path);
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if bytes.contains(&0) {
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };

        for (index, line) in content.lines().enumerate() {
            if cancel.load(Ordering::Acquire) {
                return Ok(SearchResult {
                    matches: vec![],
                    truncated: false,
                });
            }
            if let Some(column) = find_with_matcher(line, &matcher, options.whole_word) {
                matches.push(SearchMatch {
                    path: file.path.clone(),
                    relative: file.relative.clone(),
                    line: (index + 1) as u32,
                    column,
                    preview: line.to_string(),
                });
                if matches.len() >= MAX_MATCHES {
                    truncated = true;
                    break 'files;
                }
            }
        }
    }

    Ok(SearchResult { matches, truncated })
}

fn find_on_line(line: &str, needle: &str, options: &SearchOptions) -> Option<u32> {
    let matcher = regex::RegexBuilder::new(&regex::escape(needle))
        .case_insensitive(!options.case_sensitive)
        .build()
        .ok()?;
    find_with_matcher(line, &matcher, options.whole_word)
}

fn find_with_matcher(line: &str, matcher: &regex::Regex, whole_word: bool) -> Option<u32> {
    let result = matcher
        .find_iter(line)
        .find(|m| !whole_word || is_word_boundary(line, m.start(), m.len()))
        .map(|m| (line[..m.start()].chars().count() + 1) as u32);
    result
}

fn is_word_boundary(line: &str, start: usize, len: usize) -> bool {
    let word = |ch: char| ch.is_alphanumeric() || ch == '_';
    !line[..start].chars().next_back().is_some_and(word)
        && !line[start + len..].chars().next().is_some_and(word)
}

fn match_column(
    line: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    regex: bool,
) -> u32 {
    if regex {
        return 1;
    }
    let needle = query.to_string();
    find_on_line(
        line,
        &needle,
        &SearchOptions {
            cwd: String::new(),
            query: query.to_string(),
            case_sensitive,
            whole_word,
            regex: false,
            include: None,
            exclude: None,
            search_id: String::new(),
        },
    )
    .unwrap_or(1)
}

fn glob_tokens(value: &Option<String>) -> Vec<String> {
    value
        .as_deref()
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn matches_pathspec(relative: &str, include: &[String], exclude: &[String]) -> bool {
    if !include.is_empty() && !include.iter().any(|glob| glob_match(glob, relative)) {
        return false;
    }
    !exclude.iter().any(|glob| glob_match(glob, relative))
}

fn glob_match(glob: &str, path: &str) -> bool {
    let glob = glob.trim_start_matches("./");
    if glob.contains('*') || glob.contains('?') {
        if let Some(prefix) = glob.strip_suffix('*') {
            let prefix = prefix.trim_end_matches('/');
            return path == prefix || path.starts_with(&format!("{prefix}/"));
        }
        if let Some(suffix) = glob.strip_prefix('*') {
            let suffix = suffix.trim_start_matches('/');
            return path.ends_with(suffix) || path.contains(suffix);
        }
        return path.contains(glob.trim_matches('*'));
    }
    path == glob || path.starts_with(&format!("{glob}/"))
}

fn pathspecs(include: &Option<String>, exclude: &Option<String>) -> Vec<String> {
    let mut specs = glob_tokens(include);
    for glob in glob_tokens(exclude) {
        specs.push(format!(":(exclude){glob}"));
    }
    specs
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options(query: &str) -> SearchOptions {
        SearchOptions {
            cwd: String::new(),
            query: query.into(),
            case_sensitive: false,
            whole_word: false,
            regex: false,
            include: None,
            exclude: None,
            search_id: String::new(),
        }
    }
    fn git(root: &Path, args: &[&str]) {
        assert!(std::process::Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }
    #[test]
    fn fallback_finds_literal_content_and_unicode_columns_and_skips_binary() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "first\né🙂 Needle.*\n").unwrap();
        std::fs::write(dir.path().join("binary"), b"\0Needle.*").unwrap();
        let result =
            search_project(dir.path(), &options("needle.*"), &AtomicBool::new(false)).unwrap();
        assert_eq!(result.matches.len(), 1);
        assert_eq!((result.matches[0].line, result.matches[0].column), (2, 4));
        assert!(!result.truncated);
        assert!(
            search_project(dir.path(), &options("absent"), &AtomicBool::new(false))
                .unwrap()
                .matches
                .is_empty()
        );
        assert!(
            search_project(dir.path(), &options("needle"), &AtomicBool::new(true))
                .unwrap()
                .matches
                .is_empty()
        );
    }
    #[test]
    fn git_search_honors_filters_and_never_parses_partial_records_at_cap() {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "--quiet"]);
        std::fs::write(dir.path().join("many.txt"), "needle\n".repeat(2000)).unwrap();
        std::fs::write(dir.path().join("other.rs"), "needle\n").unwrap();
        git(dir.path(), &["add", "."]);
        let cancel = AtomicBool::new(false);
        let mut opts = options("needle");
        opts.include = Some("*.rs".into());
        let found = search_project(dir.path(), &opts, &cancel).unwrap();
        assert_eq!(found.matches.len(), 1);
        assert_eq!(found.matches[0].relative, "other.rs");
        let capped =
            git_grep_capped(dir.path(), &options("needle"), "needle", 256, &cancel).unwrap();
        assert!(capped.truncated);
        assert!(!capped.matches.is_empty());
        assert!(capped
            .matches
            .iter()
            .all(|m| m.preview == "needle" && m.line > 0));
        let limited = search_project(dir.path(), &options("needle"), &cancel).unwrap();
        assert_eq!(limited.matches.len(), 500);
        assert!(limited.truncated);
        assert!(search_project(dir.path(), &options("nothing"), &cancel)
            .unwrap()
            .matches
            .is_empty());
    }
    #[test]
    fn whole_word_unicode_does_not_slice_inside_utf8() {
        let mut opts = options("é");
        opts.whole_word = true;
        assert_eq!(find_on_line("café é!", "é", &opts), Some(6));
    }
}
