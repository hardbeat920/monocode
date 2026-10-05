//! Port of the pure parts of src/features/terminal/model/terminalClose.ts.
//!
//! The TypeScript asked the PTY for its foreground process and showed a
//! Tauri `ask` dialog. Here the caller passes the foreground lookup and
//! shows the returned prompt itself, with `CLOSE_TERMINAL_DIALOG_TITLE` as
//! the title and a warning style.

use crate::layout::FilePaneTab;
use crate::terminal_tab::terminal_tab_label;

/// The dialog title the TypeScript passed to `ask`.
pub const CLOSE_TERMINAL_DIALOG_TITLE: &str = "MonoCode";

/// A terminal that still runs a foreground process.
#[derive(Debug, Clone, PartialEq)]
pub struct RunningTerminalFile {
    pub file: FilePaneTab,
    pub process: String,
}

/// `runningTerminals`: terminal files whose PTY reports a foreground process
/// other than the shell. `foreground` returns `None` for an idle shell or a
/// PTY that is already gone.
pub fn running_terminals(
    files: &[FilePaneTab],
    mut foreground: impl FnMut(&str) -> Option<String>,
) -> Vec<RunningTerminalFile> {
    let mut running = Vec::new();
    for file in files {
        if file.terminal != Some(true) {
            continue;
        }
        let process = foreground(&file.id)
            .map(|process| monocode_core::js::trim(&process).to_string())
            .unwrap_or_default();
        if !process.is_empty() {
            running.push(RunningTerminalFile {
                file: file.clone(),
                process,
            });
        }
    }
    running
}

/// The question `confirmCloseTerminal` and `confirmCloseTerminals` asked, or
/// `None` when nothing is running and the close needs no confirmation.
pub fn close_terminals_prompt(running: &[RunningTerminalFile]) -> Option<String> {
    match running {
        [] => None,
        [only] => Some(format!(
            "\"{}\" is still running in {}. Close this terminal anyway?",
            only.process,
            terminal_tab_label(&only.file)
        )),
        _ => {
            let lines = running
                .iter()
                .map(|entry| format!("• {} ({})", terminal_tab_label(&entry.file), entry.process))
                .collect::<Vec<_>>()
                .join("\n");
            Some(format!(
                "These terminals are still running:\n{lines}\n\nClose them anyway?"
            ))
        }
    }
}

/// `confirmCloseTerminal`: the prompt for closing one terminal.
pub fn close_terminal_prompt(
    file: &FilePaneTab,
    foreground: impl FnMut(&str) -> Option<String>,
) -> Option<String> {
    let running = running_terminals(std::slice::from_ref(file), foreground);
    let first = running.first()?;
    Some(format!(
        "\"{}\" is still running in {}. Close this terminal anyway?",
        first.process,
        terminal_tab_label(file)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::new_terminal_file;

    #[test]
    fn confirm_close_terminal_allows_close_when_only_the_shell_is_foreground() {
        let file = new_terminal_file("/repo", None, None);
        let mut asked = Vec::new();
        let prompt = close_terminal_prompt(&file, |id| {
            asked.push(id.to_string());
            None
        });
        assert_eq!(prompt, None);
        assert_eq!(asked, vec![file.id.clone()]);
    }

    #[test]
    fn confirm_close_terminal_prompts_when_a_process_is_running() {
        let file = new_terminal_file("/repo", Some("agent-terminal"), None);
        assert_eq!(
            close_terminal_prompt(&file, |_| Some("npm".into())).as_deref(),
            Some("\"npm\" is still running in agent-terminal. Close this terminal anyway?")
        );
    }

    #[test]
    fn confirm_close_terminals_summarizes_multiple_running_terminals() {
        let first = new_terminal_file("/repo", Some("dev"), None);
        let second = new_terminal_file("/repo", Some("test"), None);
        let running = running_terminals(&[first.clone(), second.clone()], |id| {
            if id == first.id {
                Some("vite".into())
            } else if id == second.id {
                Some("jest".into())
            } else {
                None
            }
        });
        assert_eq!(
            close_terminals_prompt(&running).as_deref(),
            Some(
                "These terminals are still running:\n• dev (vite)\n• test (jest)\n\nClose them anyway?"
            )
        );
    }
}
