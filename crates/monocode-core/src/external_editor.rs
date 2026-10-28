//! Opening a project folder in an installed code editor, ported from
//! src-tauri/src/external_editor.rs.
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(target_os = "macos")]
use crate::scm::shell::dirs_home;
use crate::scm::shell::{apply_gui_env, expand_home, resolve_gui_binary};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternalEditor {
    pub id: &'static str,
    pub name: &'static str,
}

struct EditorDefinition {
    id: &'static str,
    name: &'static str,
    commands: &'static [&'static str],
    #[cfg(target_os = "macos")]
    mac_apps: &'static [&'static str],
    #[cfg(windows)]
    windows_paths: &'static [(&'static str, &'static str)],
}

const EDITORS: &[EditorDefinition] = &[
    EditorDefinition {
        id: "vscode",
        name: "Visual Studio Code",
        commands: &["code"],
        #[cfg(target_os = "macos")]
        mac_apps: &["Visual Studio Code.app"],
        #[cfg(windows)]
        windows_paths: &[
            ("LOCALAPPDATA", "Programs/Microsoft VS Code/Code.exe"),
            ("ProgramFiles", "Microsoft VS Code/Code.exe"),
            ("ProgramFiles(x86)", "Microsoft VS Code/Code.exe"),
        ],
    },
    EditorDefinition {
        id: "vscode-insiders",
        name: "Visual Studio Code Insiders",
        commands: &["code-insiders"],
        #[cfg(target_os = "macos")]
        mac_apps: &["Visual Studio Code - Insiders.app"],
        #[cfg(windows)]
        windows_paths: &[
            (
                "LOCALAPPDATA",
                "Programs/Microsoft VS Code Insiders/Code - Insiders.exe",
            ),
            (
                "ProgramFiles",
                "Microsoft VS Code Insiders/Code - Insiders.exe",
            ),
            (
                "ProgramFiles(x86)",
                "Microsoft VS Code Insiders/Code - Insiders.exe",
            ),
        ],
    },
    EditorDefinition {
        id: "vscodium",
        name: "VSCodium",
        commands: &["codium"],
        #[cfg(target_os = "macos")]
        mac_apps: &["VSCodium.app"],
        #[cfg(windows)]
        windows_paths: &[
            ("LOCALAPPDATA", "Programs/VSCodium/VSCodium.exe"),
            ("ProgramFiles", "VSCodium/VSCodium.exe"),
            ("ProgramFiles(x86)", "VSCodium/VSCodium.exe"),
        ],
    },
    EditorDefinition {
        id: "cursor",
        name: "Cursor",
        commands: &["cursor"],
        #[cfg(target_os = "macos")]
        mac_apps: &["Cursor.app"],
        #[cfg(windows)]
        windows_paths: &[
            ("LOCALAPPDATA", "Programs/cursor/Cursor.exe"),
            ("ProgramFiles", "Cursor/Cursor.exe"),
            ("ProgramFiles(x86)", "Cursor/Cursor.exe"),
        ],
    },
    EditorDefinition {
        id: "zed",
        name: "Zed",
        commands: &["zed"],
        #[cfg(target_os = "macos")]
        mac_apps: &["Zed.app", "Zed Preview.app"],
        #[cfg(windows)]
        windows_paths: &[
            ("LOCALAPPDATA", "Programs/Zed/Zed.exe"),
            ("ProgramFiles", "Zed/Zed.exe"),
        ],
    },
    EditorDefinition {
        id: "windsurf",
        name: "Windsurf",
        commands: &["windsurf"],
        #[cfg(target_os = "macos")]
        mac_apps: &["Windsurf.app"],
        #[cfg(windows)]
        windows_paths: &[
            ("LOCALAPPDATA", "Programs/Windsurf/Windsurf.exe"),
            ("ProgramFiles", "Windsurf/Windsurf.exe"),
        ],
    },
    EditorDefinition {
        id: "sublime-text",
        name: "Sublime Text",
        commands: &["subl", "sublime_text"],
        #[cfg(target_os = "macos")]
        mac_apps: &["Sublime Text.app"],
        #[cfg(windows)]
        windows_paths: &[
            ("ProgramFiles", "Sublime Text/sublime_text.exe"),
            ("ProgramFiles(x86)", "Sublime Text/sublime_text.exe"),
        ],
    },
];

enum EditorLauncher {
    Command(PathBuf),
    #[cfg(target_os = "macos")]
    MacApp(PathBuf),
}

fn definition(id: &str) -> Option<&'static EditorDefinition> {
    EDITORS.iter().find(|editor| editor.id == id)
}

#[cfg(target_os = "macos")]
fn installed_mac_app(editor: &EditorDefinition) -> Option<PathBuf> {
    let user_applications = dirs_home().map(|home| PathBuf::from(home).join("Applications"));
    editor.mac_apps.iter().find_map(|name| {
        user_applications
            .as_ref()
            .map(|root| root.join(name))
            .filter(|path| path.is_dir())
            .or_else(|| {
                let path = Path::new("/Applications").join(name);
                path.is_dir().then_some(path)
            })
    })
}

#[cfg(windows)]
fn installed_windows_app(editor: &EditorDefinition) -> Option<PathBuf> {
    editor
        .windows_paths
        .iter()
        .find_map(|(variable, relative)| {
            let root = std::env::var_os(variable)?;
            let path = PathBuf::from(root).join(relative);
            path.is_file().then_some(path)
        })
}

fn resolve_editor(editor: &EditorDefinition) -> Option<EditorLauncher> {
    #[cfg(target_os = "macos")]
    if let Some(path) = installed_mac_app(editor) {
        return Some(EditorLauncher::MacApp(path));
    }

    #[cfg(windows)]
    if let Some(path) = installed_windows_app(editor) {
        return Some(EditorLauncher::Command(path));
    }

    editor
        .commands
        .iter()
        .find_map(|command| resolve_gui_binary(command))
        .map(EditorLauncher::Command)
}

/// Editors found on this machine. Blocking: the first call may read the
/// login shell's PATH, so callers run it on a background executor.
pub fn installed() -> Vec<ExternalEditor> {
    EDITORS
        .iter()
        .filter(|editor| resolve_editor(editor).is_some())
        .map(|editor| ExternalEditor {
            id: editor.id,
            name: editor.name,
        })
        .collect()
}

/// Opens the folder `cwd` in the editor `editor_id`. Blocking, like [`installed`].
pub fn open(editor_id: &str, cwd: &str) -> Result<(), String> {
    let editor = definition(editor_id).ok_or_else(|| "Unknown external editor.".to_string())?;
    let cwd = expand_home(Path::new(cwd));
    if !cwd.is_dir() {
        return Err(format!("{} is not a folder.", cwd.display()));
    }
    let launcher =
        resolve_editor(editor).ok_or_else(|| format!("{} is no longer installed.", editor.name))?;

    #[cfg(target_os = "macos")]
    let mut command = match launcher {
        EditorLauncher::MacApp(app) => {
            let mut command = Command::new("/usr/bin/open");
            command.arg("-a").arg(app).arg(&cwd);
            command
        }
        EditorLauncher::Command(program) => {
            let mut command = Command::new(program);
            command.arg(&cwd);
            command
        }
    };

    #[cfg(not(target_os = "macos"))]
    let mut command = match launcher {
        EditorLauncher::Command(program) => {
            let mut command = Command::new(program);
            command.arg(&cwd);
            command
        }
    };

    apply_gui_env(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open {}: {error}", editor.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_ids_and_names_are_unique() {
        let mut ids = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        for editor in EDITORS {
            assert!(ids.insert(editor.id), "duplicate editor id: {}", editor.id);
            assert!(
                names.insert(editor.name),
                "duplicate editor name: {}",
                editor.name
            );
            assert!(!editor.commands.is_empty());
        }
    }

    #[test]
    fn unknown_editor_is_rejected_before_launch() {
        let error = open("not-an-editor", ".").unwrap_err();
        assert_eq!(error, "Unknown external editor.");
    }
}
