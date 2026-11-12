//! The current Omarchy palette, refreshed off the UI thread.
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Theme {
    pub background: u32,
    pub foreground: u32,
    pub accent: u32,
    pub light: bool,
}

static CURRENT: OnceLock<Mutex<Option<Theme>>> = OnceLock::new();

pub(crate) fn current() -> Option<Theme> {
    *CURRENT.get_or_init(|| Mutex::new(None)).lock().unwrap()
}

fn watch_root_at(home: &Path) -> Option<PathBuf> {
    let root = home.join(".local/state/omarchy/current");
    root.is_dir().then_some(root)
}

pub(crate) fn watch_root() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    watch_root_at(Path::new(&std::env::var_os("HOME")?))
}

pub(crate) fn available() -> bool {
    watch_root().is_some_and(|root| root.join("theme/colors.toml").is_file())
}

fn parse(output: &str) -> Option<Theme> {
    let value = |key| {
        output.lines().find_map(|line| {
            let (name, value) = line.split_once('\t')?;
            (name == key).then_some(value)
        })
    };
    let color = |key| {
        let hex = value(key)?.strip_prefix('#')?;
        (hex.len() == 6)
            .then(|| u32::from_str_radix(hex, 16).ok())
            .flatten()
    };
    Some(Theme {
        background: color("background")?,
        foreground: color("foreground")?,
        accent: color("accent")?,
        light: match value("mode")? {
            "light" => true,
            "dark" => false,
            _ => return None,
        },
    })
}

pub(crate) fn refresh() -> bool {
    if !available() {
        return false;
    }
    let next = std::process::Command::new("omarchy-theme-color")
        .arg("--all")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|output| parse(&output));
    let Some(next) = next else {
        return false;
    };
    let next = Some(next);
    let mut current = CURRENT.get_or_init(|| Mutex::new(None)).lock().unwrap();
    if *current == next {
        return false;
    }
    *current = next;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn watches_for_palette_created_after_startup() {
        let home =
            std::env::temp_dir().join(format!("monocode-omarchy-watch-{}", std::process::id()));
        let current = home.join(".local/state/omarchy/current");
        std::fs::create_dir_all(&current).unwrap();
        let root = watch_root_at(&home).expect("the stable directory enables discovery");
        assert!(!root.join("theme/colors.toml").exists());
        let (_watch, changes) = crate::fs_watch::watch(std::slice::from_ref(&root))
            .expect("watch starts without a palette");
        std::fs::create_dir(root.join("theme")).unwrap();
        std::fs::write(root.join("theme/colors.toml"), "mode = 'dark'").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while changes.is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!changes.is_empty(), "creating the palette wakes discovery");
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn reads_resolved_palette_and_rejects_invalid_colors() {
        let input = "mode\tlight\nbackground\t#ffffff\nforeground\t#123456\naccent\t#abcdef\n";
        assert_eq!(
            parse(input),
            Some(Theme {
                background: 0xffffff,
                foreground: 0x123456,
                accent: 0xabcdef,
                light: true
            })
        );
        assert_eq!(parse(&input.replace("#123456", "invalid")), None);
        assert_eq!(parse("mode\tdark\n"), None);
    }
}
