use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::fs::{expand_home, list_project_files_sync, MAX_TEXT_FILE_BYTES};

const MAX_MATCHES: usize = 500;
const MAX_FILE_BYTES: u64 = 512 * 1024;
// ponytail: 4 MiB holds 500 normal previews; raise only if match previews stop being bounded.
const MAX_GIT_GREP_BYTES: usize = 4 * 1024 * 1024;

#[derive(Deserialize)]
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

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    pub path: String,
    pub relative: String,
    pub line: u32,
    pub column: u32,
    pub preview: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub matches: Vec<SearchMatch>,
    pub truncated: bool,
}

type SearchKey = (PathBuf, String);
static ACTIVE_SEARCHES: Mutex<Option<HashMap<SearchKey, Arc<AtomicBool>>>> = Mutex::new(None);

fn begin_search(root: &Path, search_id: &str) -> Arc<AtomicBool> {
    let token = Arc::new(AtomicBool::new(false));
    if let Ok(mut active) = ACTIVE_SEARCHES.lock() {
        let searches = active.get_or_insert_with(HashMap::new);
        if let Some(previous) =
            searches.insert((root.to_path_buf(), search_id.to_string()), token.clone())
        {
            previous.store(true, Ordering::Release);
        }
    }
    token
}

fn finish_search(root: &Path, search_id: &str, token: &Arc<AtomicBool>) {
    if let Ok(mut active) = ACTIVE_SEARCHES.lock() {
        if let Some(searches) = active.as_mut() {
            let key = (root.to_path_buf(), search_id.to_string());
            if searches
                .get(&key)
                .is_some_and(|current| Arc::ptr_eq(current, token))
            {
                searches.remove(&key);
            }
        }
    }
}

fn cancel_search(root: &Path, search_id: &str) {
    if let Ok(active) = ACTIVE_SEARCHES.lock() {
        if let Some(token) = active
            .as_ref()
            .and_then(|searches| searches.get(&(root.to_path_buf(), search_id.to_string())))
        {
            token.store(true, Ordering::Release);
        }
    }
}

#[tauri::command]
pub fn cancel_project_search(cwd: String, search_id: String) {
    cancel_search(&expand_home(&cwd), &search_id);
}

#[tauri::command]
pub async fn search_project(options: SearchOptions) -> Result<SearchResult, String> {
    let root = expand_home(&options.cwd);
    if !root.is_dir() {
        return Err(format!("{}: Not a directory", root.display()));
    }
    let search_id = options.search_id.clone();
    let token = begin_search(&root, &search_id);
    let result = match tauri::async_runtime::spawn_blocking({
        let root = root.clone();
        let token = token.clone();
        move || search_project_sync(&root, &options, &token)
    })
    .await
    {
        Ok(result) => result,
        Err(error) => {
            finish_search(&root, &search_id, &token);
            return Err(error.to_string());
        }
    };
    finish_search(&root, &search_id, &token);
    result
}

fn search_project_sync(
    root: &Path,
    options: &SearchOptions,
    cancel: &AtomicBool,
) -> Result<SearchResult, String> {
    let query = options.query.trim();
    if query.is_empty() || cancel.load(Ordering::Acquire) {
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
        match crate::fs::git_output_capped(root, &args, max_bytes, Some(cancel)) {
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
        let path = crate::fs::path_to_js(&root.join(&relative));
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

    let files = list_project_files_sync(&root.to_string_lossy())?;
    let include = glob_tokens(&options.include);
    let exclude = glob_tokens(&options.exclude);
    let needle = if options.case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };

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
        if !meta.is_file() || meta.len() > MAX_FILE_BYTES.min(MAX_TEXT_FILE_BYTES) {
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
            if let Some(column) = find_on_line(line, &needle, options) {
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
    let haystack = if options.case_sensitive {
        line.to_string()
    } else {
        line.to_lowercase()
    };
    let mut start = 0;
    while let Some(index) = haystack[start..].find(needle) {
        let column = start + index;
        if options.whole_word && !is_word_boundary(line, column, needle.len()) {
            start = column + 1;
            continue;
        }
        return Some((column + 1) as u32);
    }
    None
}

fn is_word_boundary(line: &str, start: usize, len: usize) -> bool {
    let before = line[..start].chars().next_back();
    let after = line[start + len..].chars().next();
    let word = |ch: char| ch.is_alphanumeric() || ch == '_';
    !before.is_some_and(word) && !after.is_some_and(word)
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
    let needle = if case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };
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
    use std::io::ErrorKind;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

    struct Tmp(PathBuf);

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tmp(label: &str) -> Tmp {
        loop {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "monocode-search-{label}-{}-{stamp}-{seq}",
                std::process::id()
            ));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Tmp(dir),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("{error}"),
            }
        }
    }

    fn git(dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    fn options(dir: &Path, query: &str) -> SearchOptions {
        SearchOptions {
            cwd: dir.to_string_lossy().into_owned(),
            query: query.to_string(),
            case_sensitive: true,
            whole_word: false,
            regex: false,
            include: None,
            exclude: None,
            search_id: String::new(),
        }
    }

    #[test]
    fn git_grep_stops_reading_at_the_output_cap() {
        let dir = tmp("git-grep-cap");
        if !git(&dir.0, &["init", "--quiet"]) {
            return;
        }
        let body = "find me\n".repeat(2_000);
        std::fs::write(dir.0.join("many.txt"), &body).unwrap();
        assert!(git(&dir.0, &["add", "many.txt"]));

        let result = git_grep_capped(
            &dir.0,
            &options(&dir.0, "find me"),
            "find me",
            256,
            &AtomicBool::new(false),
        )
        .unwrap();

        assert!(result.truncated);
        assert!(!result.matches.is_empty());
        assert!(result.matches.len() < 2_000);
        assert!(result
            .matches
            .iter()
            .all(|found| found.preview == "find me" && found.line > 0));
    }

    #[test]
    fn git_grep_no_match_is_still_an_empty_success() {
        let dir = tmp("git-grep-empty");
        if !git(&dir.0, &["init", "--quiet"]) {
            return;
        }
        std::fs::write(dir.0.join("empty.txt"), "nothing here\n").unwrap();
        assert!(git(&dir.0, &["add", "empty.txt"]));

        let result = git_grep_capped(
            &dir.0,
            &options(&dir.0, "absent"),
            "absent",
            256,
            &AtomicBool::new(false),
        )
        .unwrap();

        assert!(result.matches.is_empty());
        assert!(!result.truncated);
    }

    #[test]
    fn git_grep_cancelled_search_returns_empty_instead_of_falling_back() {
        let dir = tmp("git-grep-cancelled");
        if !git(&dir.0, &["init", "--quiet"]) {
            return;
        }
        std::fs::write(dir.0.join("many.txt"), "find me\n".repeat(100)).unwrap();
        assert!(git(&dir.0, &["add", "many.txt"]));

        let result = git_grep_capped(
            &dir.0,
            &options(&dir.0, "find me"),
            "find me",
            256,
            &AtomicBool::new(true),
        )
        .unwrap();

        assert!(result.matches.is_empty());
        assert!(!result.truncated);
    }

    #[test]
    fn a_new_search_cancels_the_previous_owner() {
        let dir = tmp("search-cancel-owner");
        let first = begin_search(&dir.0, "owner");
        let second = begin_search(&dir.0, "owner");

        assert!(first.load(Ordering::Acquire));
        assert!(!second.load(Ordering::Acquire));

        cancel_search(&dir.0, "owner");
        assert!(second.load(Ordering::Acquire));

        finish_search(&dir.0, "owner", &first);
        finish_search(&dir.0, "owner", &second);
    }

    #[test]
    fn git_grep_treats_hyphen_prefixed_include_as_pathspec() {
        let dir = tmp("hyphen-pathspec");
        if !git(&dir.0, &["init", "--quiet"]) {
            return;
        }
        // `-l` is both a valid file name and git grep's files-with-matches flag.
        std::fs::write(dir.0.join("-l"), "find me\n").unwrap();
        std::fs::write(dir.0.join("other.txt"), "find me too\n").unwrap();
        assert!(git(&dir.0, &["add", "--", "-l", "other.txt"]));

        let result = git_grep(
            &dir.0,
            &SearchOptions {
                include: Some("-l".to_string()),
                ..options(&dir.0, "find me")
            },
            "find me",
            &AtomicBool::new(false),
        )
        .unwrap();

        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].relative, "-l");
        assert_eq!(result.matches[0].preview, "find me");
    }
}
