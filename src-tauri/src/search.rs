use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::fs::{expand_home, list_project_files_sync, MAX_TEXT_FILE_BYTES};

const MAX_MATCHES: usize = 500;
const MAX_FILE_BYTES: u64 = 512 * 1024;

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

#[tauri::command]
pub async fn search_project(options: SearchOptions) -> Result<SearchResult, String> {
    tauri::async_runtime::spawn_blocking(move || search_project_sync(&options))
        .await
        .map_err(|e| e.to_string())?
}

fn search_project_sync(options: &SearchOptions) -> Result<SearchResult, String> {
    let query = options.query.trim();
    if query.is_empty() {
        return Ok(SearchResult {
            matches: Vec::new(),
            truncated: false,
        });
    }

    let root = expand_home(&options.cwd);
    if !root.is_dir() {
        return Err(format!("{}: Not a directory", root.display()));
    }

    if let Some(result) = git_grep(&root, options, query) {
        return Ok(result);
    }

    scan_files(&root, options, query)
}

fn git_grep(root: &Path, options: &SearchOptions, query: &str) -> Option<SearchResult> {
    let mut cmd = Command::new("git");
    crate::hide_window_console(&mut cmd);
    cmd.arg("-C").arg(root).arg("grep").arg("-z").arg("-n");
    if !options.case_sensitive {
        cmd.arg("-i");
    }
    if options.whole_word {
        cmd.arg("-w");
    }
    if options.regex {
        cmd.arg("-E");
    } else {
        cmd.arg("-F");
    }
    cmd.arg("-e").arg(query);

    for spec in pathspecs(&options.include, &options.exclude) {
        cmd.arg(spec);
    }

    let output = cmd.output().ok()?;
    if !output.status.success() && !output.stdout.is_empty() {
        // git grep exits 1 when there are no matches.
        if output.status.code() != Some(1) {
            return None;
        }
    }
    if !output.status.success() && output.stdout.is_empty() {
        return Some(SearchResult {
            matches: Vec::new(),
            truncated: false,
        });
    }

    let root = root.to_path_buf();
    let mut matches = Vec::new();
    let mut truncated = false;
    let mut offset = 0;
    while offset < output.stdout.len() {
        let Some((path_bytes, next)) = read_until(&output.stdout[offset..], 0) else {
            break;
        };
        offset += next;
        if path_bytes.is_empty() {
            continue;
        }

        let Some((line_bytes, next)) = read_until(&output.stdout[offset..], 0) else {
            break;
        };
        offset += next;

        let line_end = output.stdout[offset..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| offset + index)
            .unwrap_or(output.stdout.len());
        let preview_bytes = &output.stdout[offset..line_end];
        offset = line_end + (usize::from(line_end < output.stdout.len()));

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

fn scan_files(root: &Path, options: &SearchOptions, query: &str) -> Result<SearchResult, String> {
    if options.regex {
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
