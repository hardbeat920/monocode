//! A provider's raw patch, split per file and read into numbered lines. The
//! desktop review and a paired phone both draw from this.
use crate::diff::{Block, Line, LineKind};

/// The path a file section's `+++` header names, if it has one.
pub fn section_path(section: &str) -> Option<String> {
    section
        .lines()
        .find_map(|line| line.strip_prefix("+++ "))
        .filter(|path| *path != "/dev/null")
        .map(|path| {
            let path = serde_json::from_str::<String>(path).unwrap_or_else(|_| path.to_string());
            path.strip_prefix("b/").unwrap_or(&path).to_string()
        })
}

fn hunk_start(value: &str) -> u32 {
    value
        .trim_start_matches(['+', '-'])
        .split(',')
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}

/// The patch cut at each `diff --git` header, in file order.
pub fn sections(patch: &str) -> Vec<&str> {
    patch.split("diff --git ").skip(1).collect()
}

/// The section for `path`, else the one at `index` (providers list files in
/// patch order), else none.
pub fn section_for<'a>(sections: &[&'a str], path: &str, index: usize) -> &'a str {
    sections
        .iter()
        .find(|section| section_path(section).as_deref() == Some(path))
        .copied()
        .or_else(|| sections.get(index).copied())
        .unwrap_or("")
}

pub fn is_binary(section: &str) -> bool {
    section.contains("Binary files ") || section.contains("GIT binary patch")
}

/// One file section's hunks as numbered lines; empty when it has none.
pub fn blocks(section: &str) -> Vec<Block> {
    let (mut old, mut new) = (0, 0);
    let mut lines = vec![];
    let mut in_hunk = false;
    for raw in section.lines() {
        if raw.starts_with("@@ ") {
            let mut parts = raw.split_whitespace().skip(1);
            old = hunk_start(parts.next().unwrap_or("-1"));
            new = hunk_start(parts.next().unwrap_or("+1"));
            in_hunk = true;
            lines.push(Line {
                kind: LineKind::Hunk,
                text: raw.into(),
                old_number: None,
                new_number: None,
                chunk: None,
            });
            continue;
        }
        if !in_hunk {
            continue;
        }
        let (kind, old_number, new_number) = match raw.as_bytes().first() {
            Some(b'+') => {
                let n = new;
                new += 1;
                (LineKind::Add, None, Some(n))
            }
            Some(b'-') => {
                let n = old;
                old += 1;
                (LineKind::Del, Some(n), None)
            }
            Some(b' ') => {
                let (o, n) = (old, new);
                old += 1;
                new += 1;
                (LineKind::Context, Some(o), Some(n))
            }
            _ => continue,
        };
        lines.push(Line {
            kind,
            text: raw[1..].into(),
            old_number,
            new_number,
            chunk: None,
        });
    }
    if lines.is_empty() {
        vec![]
    } else {
        vec![Block::Hunk { lines, chunk: None }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/src/a.rs b/src/a.rs\nindex 1..2 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -3,3 +3,4 @@ fn x()\n keep\n-old\n+new\n+added\n tail\ndiff --git a/img.png b/img.png\nBinary files a/img.png and b/img.png differ\n";

    #[test]
    fn a_patch_splits_per_file_and_numbers_its_lines() {
        let sections = sections(PATCH);
        assert_eq!(sections.len(), 2);
        assert_eq!(section_path(sections[0]).as_deref(), Some("src/a.rs"));
        assert!(is_binary(sections[1]));
        let lines = blocks(sections[0]).remove(0).lines().to_vec();
        let shape: Vec<_> = lines
            .iter()
            .map(|l| (l.kind, l.text.as_str(), l.old_number, l.new_number))
            .collect();
        assert_eq!(
            shape,
            vec![
                (LineKind::Hunk, "@@ -3,3 +3,4 @@ fn x()", None, None),
                (LineKind::Context, "keep", Some(3), Some(3)),
                (LineKind::Del, "old", Some(4), None),
                (LineKind::Add, "new", None, Some(4)),
                (LineKind::Add, "added", None, Some(5)),
                (LineKind::Context, "tail", Some(5), Some(6)),
            ]
        );
        assert!(blocks(sections[1]).is_empty());
    }

    #[test]
    fn a_file_falls_back_to_its_position_when_no_header_names_it() {
        let sections = sections(PATCH);
        assert!(section_for(&sections, "src/a.rs", 5).contains("+new"));
        assert!(section_for(&sections, "renamed.rs", 1).contains("Binary"));
        assert_eq!(section_for(&sections, "x", 9), "");
    }
}
