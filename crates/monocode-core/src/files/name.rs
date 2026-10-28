//! Unix file-name checks for the explorer, ported from fileName.ts.
use serde::{Deserialize, Serialize};

use crate::scm::text::{js_len, js_space};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NameIssueKind {
    Empty,
    Slash,
    Exists,
    Invalid,
    Whitespace,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct NameIssue {
    pub kind: NameIssueKind,
    /// The well-formed name that was checked.
    pub name: String,
    pub severity: Severity,
}

/// Strip surrounding tabs and trailing slashes.
pub fn well_formed_file_name(raw: &str) -> String {
    raw.trim_matches('\t')
        .trim_end_matches(['/', '\\'])
        .to_string()
}

pub(super) fn path_segments(name: &str) -> Vec<String> {
    well_formed_file_name(name)
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

fn blank(name: &str) -> bool {
    !name.is_empty() && name.chars().all(js_space)
}

fn is_valid_basename(name: &str) -> bool {
    !(name.is_empty()
        || blank(name)
        || name.contains(['/', '\\'])
        || name == "."
        || name == ".."
        || js_len(name) > 255)
}

/// First problem with a new or renamed entry's name, if any. `siblings`
/// compare case-insensitively. `/` in `raw` nests.
pub fn validate_file_name(raw: &str, siblings: &[String]) -> Option<NameIssue> {
    let name = well_formed_file_name(raw);
    let issue = |kind, severity| {
        Some(NameIssue {
            kind,
            name: name.clone(),
            severity,
        })
    };
    if name.is_empty() || blank(&name) {
        return issue(NameIssueKind::Empty, Severity::Error);
    }
    if name.starts_with(['/', '\\']) {
        return issue(NameIssueKind::Slash, Severity::Error);
    }
    let lower = name.to_lowercase();
    if siblings
        .iter()
        .any(|sibling| sibling.to_lowercase() == lower)
    {
        return issue(NameIssueKind::Exists, Severity::Error);
    }
    let segments = path_segments(&name);
    if segments.iter().any(|segment| !is_valid_basename(segment)) {
        return issue(NameIssueKind::Invalid, Severity::Error);
    }
    if segments
        .iter()
        .any(|segment| segment.starts_with(js_space) || segment.ends_with(js_space))
    {
        return issue(NameIssueKind::Whitespace, Severity::Warning);
    }
    None
}

/// Last path segment of `raw`, or empty.
pub fn leaf_name(raw: &str) -> String {
    path_segments(raw).pop().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(raw: &str, siblings: &[&str]) -> Option<NameIssueKind> {
        let siblings: Vec<String> = siblings.iter().map(|s| s.to_string()).collect();
        validate_file_name(raw, &siblings).map(|issue| issue.kind)
    }

    #[test]
    fn well_formed_strips_tabs_and_trailing_slashes() {
        assert_eq!(well_formed_file_name("\tsrc/\\/"), "src");
        assert_eq!(well_formed_file_name(" a\t"), " a");
        assert_eq!(well_formed_file_name(""), "");
    }

    #[test]
    fn validates_names() {
        assert_eq!(kind("", &[]), Some(NameIssueKind::Empty));
        assert_eq!(kind("   ", &[]), Some(NameIssueKind::Empty));
        assert_eq!(kind("/", &[]), Some(NameIssueKind::Empty));
        assert_eq!(kind("/etc", &[]), Some(NameIssueKind::Slash));
        assert_eq!(kind("\\x", &[]), Some(NameIssueKind::Slash));
        assert_eq!(
            kind("README.md", &["readme.MD"]),
            Some(NameIssueKind::Exists)
        );
        assert_eq!(kind("a/../b", &[]), Some(NameIssueKind::Invalid));
        assert_eq!(kind(".", &[]), Some(NameIssueKind::Invalid));
        assert_eq!(kind("a/ /b", &[]), Some(NameIssueKind::Invalid));
        assert_eq!(kind(&"x".repeat(256), &[]), Some(NameIssueKind::Invalid));
        assert_eq!(kind(&"x".repeat(255), &[]), None);
        assert_eq!(kind("a/ b", &[]), Some(NameIssueKind::Whitespace));
        assert_eq!(kind("src/new.ts/", &["src"]), None);
        assert_eq!(kind("notes.md", &["other.md"]), None);
        let issue = validate_file_name("a.md", &["A.md".into()]).unwrap();
        assert_eq!(
            (issue.name.as_str(), issue.severity),
            ("a.md", Severity::Error)
        );
        assert_eq!(
            validate_file_name(" x", &[]).unwrap().severity,
            Severity::Warning
        );
    }

    #[test]
    fn leaf_name_takes_the_last_segment() {
        assert_eq!(leaf_name("src/lib/mod.rs"), "mod.rs");
        assert_eq!(leaf_name("src\\lib/"), "lib");
        assert_eq!(leaf_name("/"), "");
    }
}
