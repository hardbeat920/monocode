//! Port of src/features/terminal/model/terminalTab.ts.

use monocode_core::js;
use monocode_core::paths::basename;

use crate::layout::FilePaneTab;

/// `TerminalMetaPatch`: a live PTY title, cwd, or foreground update.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerminalMetaPatch {
    pub title: Option<String>,
    pub cwd: Option<String>,
    /// `Some(None)` clears a running command; `None` leaves it unchanged.
    pub foreground: Option<Option<String>>,
}

/// `RunningTerminal`: a terminal whose foreground process is not the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningTerminal {
    pub id: String,
    pub process: String,
    pub cwd: String,
    pub label: String,
}

/// `defaultTerminalTitle`: the tab label from the working directory.
pub fn default_terminal_title(cwd: &str) -> String {
    let name = basename(cwd);
    if name.is_empty() || name == "/" {
        return "Terminal".into();
    }
    name
}

/// `terminalTabLabel`: the dynamic title (process or directory) stored on `path`.
pub fn terminal_tab_label(file: &FilePaneTab) -> String {
    let title = js::trim(&file.path);
    if title.is_empty() {
        default_terminal_title(&file.cwd)
    } else {
        title.to_string()
    }
}

/// `applyTerminalMeta`: apply a live PTY title, cwd, or foreground patch.
/// Returns an equal copy of `file` when nothing changes, where the
/// TypeScript returned the same object; callers compare with `==`.
pub fn apply_terminal_meta(file: &FilePaneTab, patch: &TerminalMetaPatch) -> FilePaneTab {
    if file.terminal != Some(true) {
        return file.clone();
    }
    let path = patch.title.clone().unwrap_or_else(|| file.path.clone());
    let cwd = patch.cwd.clone().unwrap_or_else(|| file.cwd.clone());
    let foreground = match &patch.foreground {
        None => file.foreground.clone(),
        Some(value) => value
            .as_deref()
            .map(js::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    };
    if path == file.path && cwd == file.cwd && foreground == file.foreground {
        return file.clone();
    }
    FilePaneTab {
        path,
        cwd,
        foreground,
        ..file.clone()
    }
}

/// `listRunningTerminals`: terminals whose foreground process is not the shell.
pub fn list_running_terminals<'a>(
    files: impl IntoIterator<Item = &'a FilePaneTab>,
) -> Vec<RunningTerminal> {
    let mut running = Vec::new();
    for file in files {
        let process = file.foreground.as_deref().map(js::trim).unwrap_or("");
        if file.terminal != Some(true) || process.is_empty() {
            continue;
        }
        running.push(RunningTerminal {
            id: file.id.clone(),
            process: process.to_string(),
            cwd: file.cwd.clone(),
            label: default_terminal_title(&file.cwd),
        });
    }
    running
}

/// `runningTerminalChipLabel`: `vite`, or `vite · jest`, or `vite ×2`.
pub fn running_terminal_chip_label(terminals: &[RunningTerminal]) -> String {
    if terminals.is_empty() {
        return String::new();
    }
    let mut order: Vec<(&str, usize)> = Vec::new();
    for terminal in terminals {
        match order.iter_mut().find(|(name, _)| *name == terminal.process) {
            Some((_, count)) => *count += 1,
            None => order.push((&terminal.process, 1)),
        }
    }
    order
        .iter()
        .map(|(name, n)| {
            if *n > 1 {
                format!("{name} ×{n}")
            } else {
                (*name).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// What `scanOscCwd` found in one chunk of PTY output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OscCwdScan {
    pub cwd: Option<String>,
    /// Unmatched tail to prepend to the next chunk, at most 256 UTF-16 units.
    pub rest: String,
}

const OSC_CWD_PREFIX: &str = "\x1b]7;file://";

/// One match of `OSC_CWD` at `start`, which holds the prefix:
/// `/\x1b\]7;file:\/\/[^/]*(\/[^\x07\x1b]*)(?:\x07|\x1b\\)/`. Returns the
/// captured path and the end of the match.
fn match_osc_cwd(text: &str, start: usize) -> Option<(&str, usize)> {
    let after_prefix = start + OSC_CWD_PREFIX.len();
    // `[^/]*` then `/`: the host is the longest run without a slash.
    let host_len = text[after_prefix..].find('/')?;
    let path_start = after_prefix + host_len;
    let path_len = text[path_start + 1..]
        .find(['\x07', '\x1b'])
        .map(|n| n + 1)?;
    let path_end = path_start + path_len;
    let rest = &text[path_end..];
    let end = if rest.starts_with('\x07') {
        path_end + 1
    } else if rest.starts_with("\x1b\\") {
        path_end + 2
    } else {
        return None;
    };
    Some((&text[path_start..path_end], end))
}

/// `decodeOscPath`.
fn decode_osc_path(raw: &str) -> String {
    crate::js::decode_uri_component(raw).unwrap_or_else(|| raw.to_string())
}

/// `scanOscCwd`: scan PTY output for OSC 7 cwd reports from shell integration.
pub fn scan_osc_cwd(chunk: &str, buffer: &str) -> OscCwdScan {
    let merged = format!("{buffer}{chunk}");
    let mut cwd = None;
    let mut last = 0;
    let mut from = 0;
    while let Some(found) = merged[from..].find(OSC_CWD_PREFIX) {
        let start = from + found;
        match match_osc_cwd(&merged, start) {
            Some((raw, end)) => {
                let path = decode_osc_path(raw);
                if !path.is_empty() {
                    cwd = Some(path);
                }
                last = end;
                from = end;
            }
            None => from = start + 1,
        }
    }
    let tail = &merged[last..];
    let units: Vec<u16> = tail.encode_utf16().collect();
    let rest = if units.len() > 256 {
        String::from_utf16_lossy(&units[units.len() - 256..])
    } else {
        tail.to_string()
    };
    OscCwdScan { cwd, rest }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::new_terminal_file;

    #[test]
    fn default_terminal_title_uses_the_directory_basename() {
        assert_eq!(
            default_terminal_title("/Users/dev/agent-terminal"),
            "agent-terminal"
        );
        assert_eq!(default_terminal_title("/"), "Terminal");
    }

    #[test]
    fn terminal_tab_label_prefers_the_dynamic_title_on_the_tab() {
        let file = new_terminal_file("/repo", Some("npm"), None);
        assert_eq!(terminal_tab_label(&file), "npm");
    }

    #[test]
    fn apply_terminal_meta_records_a_foreground_process_and_clears_it() {
        let file = new_terminal_file("/repo", Some("repo"), None);
        let running = apply_terminal_meta(
            &file,
            &TerminalMetaPatch {
                title: Some("vite".into()),
                foreground: Some(Some("vite".into())),
                ..Default::default()
            },
        );
        assert_eq!(running.path, "vite");
        assert_eq!(running.foreground.as_deref(), Some("vite"));
        assert_ne!(running, file);

        let idle = apply_terminal_meta(
            &running,
            &TerminalMetaPatch {
                title: Some("repo".into()),
                foreground: Some(None),
                ..Default::default()
            },
        );
        assert_eq!(idle.path, "repo");
        assert_eq!(idle.foreground, None);
    }

    #[test]
    fn apply_terminal_meta_returns_the_same_value_when_nothing_changes() {
        let patch = TerminalMetaPatch {
            foreground: Some(Some("vite".into())),
            ..Default::default()
        };
        let file = apply_terminal_meta(&new_terminal_file("/repo", Some("vite"), None), &patch);
        assert_eq!(apply_terminal_meta(&file, &patch), file);
    }

    #[test]
    fn list_running_terminals_skips_idle_shells() {
        let idle = new_terminal_file("/repo", None, None);
        let running = apply_terminal_meta(
            &new_terminal_file("/repo", Some("dev"), None),
            &TerminalMetaPatch {
                foreground: Some(Some("vite".into())),
                ..Default::default()
            },
        );
        assert_eq!(
            list_running_terminals([&idle, &running]),
            vec![RunningTerminal {
                id: running.id.clone(),
                process: "vite".into(),
                cwd: "/repo".into(),
                label: "repo".into(),
            }]
        );
    }

    fn running(id: &str, process: &str) -> RunningTerminal {
        RunningTerminal {
            id: id.into(),
            process: process.into(),
            cwd: format!("/{id}"),
            label: id.into(),
        }
    }

    #[test]
    fn running_terminal_chip_label_joins_unique_names_and_collapses_duplicates() {
        assert_eq!(
            running_terminal_chip_label(&[running("a", "vite"), running("b", "jest")]),
            "vite · jest"
        );
        assert_eq!(
            running_terminal_chip_label(&[running("a", "vite"), running("b", "vite")]),
            "vite ×2"
        );
    }

    #[test]
    fn scan_osc_cwd_extracts_cwd_from_osc_7_reports() {
        let scan = scan_osc_cwd("\x1b]7;file://host/Users/dev/repo\x07", "");
        assert_eq!(scan.cwd.as_deref(), Some("/Users/dev/repo"));
        assert_eq!(scan.rest, "");
    }

    #[test]
    fn scan_osc_cwd_keeps_a_trailing_buffer_for_split_sequences() {
        let scan = scan_osc_cwd("/repo\x07", "\x1b]7;file://host/Users/dev");
        assert_eq!(scan.cwd.as_deref(), Some("/Users/dev/repo"));
        assert_eq!(scan.rest, "");
    }

    #[test]
    fn scan_osc_cwd_decodes_paths_and_accepts_st_terminators() {
        let scan = scan_osc_cwd("out\x1b]7;file://h/a%20b\x1b\\more", "");
        assert_eq!(scan.cwd.as_deref(), Some("/a b"));
        assert_eq!(scan.rest, "more");
        let partial = scan_osc_cwd("\x1b]7;file://h/a", "");
        assert_eq!(partial.cwd, None);
        assert_eq!(partial.rest, "\x1b]7;file://h/a");
    }
}
