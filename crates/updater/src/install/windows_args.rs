//! Argument escaping for the Windows installers, from tauri-plugin-updater
//! 2.10.1 (`src/updater.rs`, Apache-2.0 OR MIT). Pure string code, so the
//! tests run on every platform.
#![cfg_attr(not(any(windows, test)), allow(dead_code))]

use std::ffi::OsStr;

/// Quotes one of the app's own arguments for the NSIS installer's `/ARGS`,
/// which hands them back to the relaunched app. Adapted from the standard
/// library's Windows argument quoting, with `/` also forcing quotes so NSIS
/// does not read it as one of its own switches.
pub(crate) fn escape_nsis_current_exe_arg(arg: &OsStr) -> String {
    let arg = arg.to_string_lossy();
    let mut cmd: Vec<char> = Vec::new();

    let quote = arg.chars().any(|c| c == ' ' || c == '\t' || c == '/') || arg.is_empty();
    if quote {
        cmd.push('"');
    }
    let mut backslashes: usize = 0;
    for x in arg.chars() {
        if x == '\\' {
            backslashes += 1;
        } else {
            if x == '"' {
                // n+1 backslashes, for 2n+1 in total before an inner quote.
                cmd.extend((0..=backslashes).map(|_| '\\'));
            }
            backslashes = 0;
        }
        cmd.push(x);
    }
    if quote {
        // n more backslashes, for 2n in total before the closing quote.
        cmd.extend((0..backslashes).map(|_| '\\'));
        cmd.push('"');
    }
    cmd.into_iter().collect()
}

/// Quotes one argument for msiexec's `LAUNCHAPPARGS="..."` property, where a
/// quote is written as two quotes.
pub(crate) fn escape_msi_property_arg(arg: impl AsRef<OsStr>) -> String {
    let mut arg = arg.as_ref().to_string_lossy().to_string();

    // An empty argument would vanish in ShellExecute.
    if arg.is_empty() {
        return "\"\"\"\"".to_string();
    } else if !arg.contains(' ') && !arg.contains('"') {
        return arg;
    }

    if arg.contains('"') {
        arg = arg.replace('"', r#""""""#);
    }

    if arg.starts_with('-') {
        if let Some((a1, a2)) = arg.split_once('=') {
            format!("{a1}=\"\"{a2}\"\"")
        } else {
            format!("\"\"{arg}\"\"")
        }
    } else {
        format!("\"\"{arg}\"\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASES: [&str; 11] = [
        "something",
        "--flag",
        "--empty=",
        "--arg=value",
        "some space",
        "--arg value",
        "--arg=unwrapped space",
        "--arg=\"wrapped\"",
        "--arg=\"wrapped space\"",
        "--arg=midword\"wrapped space\"",
        "",
    ];

    #[test]
    fn it_escapes_correctly_for_msi() {
        let escaped = [
            "something",
            "--flag",
            "--empty=",
            "--arg=value",
            "\"\"some space\"\"",
            "\"\"--arg value\"\"",
            "--arg=\"\"unwrapped space\"\"",
            r#"--arg=""""""wrapped"""""""#,
            r#"--arg=""""""wrapped space"""""""#,
            r#"--arg=""midword""""wrapped space"""""""#,
            "\"\"\"\"",
        ];
        for (orig, escaped) in CASES.iter().zip(escaped) {
            assert_eq!(escape_msi_property_arg(orig), escaped);
        }
    }

    #[test]
    fn it_escapes_correctly_for_nsis() {
        let escaped = [
            "something",
            "--flag",
            "--empty=",
            "--arg=value",
            "\"some space\"",
            "\"--arg value\"",
            "\"--arg=unwrapped space\"",
            "--arg=\\\"wrapped\\\"",
            "\"--arg=\\\"wrapped space\\\"\"",
            "\"--arg=midword\\\"wrapped space\\\"\"",
            "\"\"",
        ];
        for (orig, escaped) in CASES.iter().zip(escaped) {
            assert_eq!(escape_nsis_current_exe_arg(OsStr::new(orig)), escaped);
        }
    }
}
