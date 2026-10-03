//! Port of src/features/files/model/fileName.ts: Unix file-name checks for
//! the explorer (empty, slashes, invalid names).

use std::collections::HashSet;

/// `NameIssue`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameIssue {
    /// Error: nothing but whitespace.
    Empty,
    /// Error: starts with a slash.
    Slash,
    /// Error: a sibling has this name.
    Exists { name: String },
    /// Error: a segment is `.`, `..`, too long, or blank.
    Invalid { name: String },
    /// Warning: a segment starts or ends with whitespace.
    Whitespace,
}

impl NameIssue {
    /// `severity`: every issue is an error except leading or trailing
    /// whitespace.
    pub fn is_error(&self) -> bool {
        !matches!(self, NameIssue::Whitespace)
    }
}

/// `wellFormedFileName`: no leading or trailing tabs, no trailing slashes.
pub fn well_formed_file_name(filename: &str) -> String {
    if filename.is_empty() {
        return String::new();
    }
    filename
        .trim_matches('\t')
        .trim_end_matches(['/', '\\'])
        .to_string()
}

/// `pathSegments`.
pub fn path_segments(name: &str) -> Vec<String> {
    well_formed_file_name(name)
        .split(['/', '\\'])
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect()
}

/// `/^\s+$/` and the empty string.
fn is_blank(value: &str) -> bool {
    value.chars().all(monocode_core::js::is_space)
}

/// `isValidBasename`.
fn is_valid_basename(name: &str) -> bool {
    if is_blank(name) {
        return false;
    }
    if name.contains('/') || name.contains('\\') {
        return false;
    }
    if name == "." || name == ".." {
        return false;
    }
    monocode_core::js::len(name) <= 255
}

/// `validateFileName`.
pub fn validate_file_name<'a>(
    raw: &str,
    sibling_names: impl IntoIterator<Item = &'a str>,
) -> Option<NameIssue> {
    let name = well_formed_file_name(raw);

    if is_blank(&name) {
        return Some(NameIssue::Empty);
    }

    if name.starts_with('/') || name.starts_with('\\') {
        return Some(NameIssue::Slash);
    }

    let siblings: HashSet<String> = sibling_names.into_iter().map(str::to_lowercase).collect();
    if siblings.contains(&name.to_lowercase()) {
        return Some(NameIssue::Exists { name });
    }

    let names = path_segments(&name);
    if names.iter().any(|segment| !is_valid_basename(segment)) {
        return Some(NameIssue::Invalid { name });
    }

    let edge_space = |segment: &String| {
        segment
            .chars()
            .next()
            .is_some_and(monocode_core::js::is_space)
            || segment
                .chars()
                .last()
                .is_some_and(monocode_core::js::is_space)
    };
    if names.iter().any(edge_space) {
        return Some(NameIssue::Whitespace);
    }

    None
}

/// `leafName`: the last path segment.
pub fn leaf_name(raw: &str) -> String {
    path_segments(raw).pop().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_names_like_the_explorer() {
        assert_eq!(validate_file_name("  ", []), Some(NameIssue::Empty));
        assert_eq!(validate_file_name("/a", []), Some(NameIssue::Slash));
        assert_eq!(
            validate_file_name("README.md", ["readme.MD"]),
            Some(NameIssue::Exists {
                name: "README.md".into()
            })
        );
        assert_eq!(
            validate_file_name("a/../b", []),
            Some(NameIssue::Invalid {
                name: "a/../b".into()
            })
        );
        assert_eq!(validate_file_name("a/ b", []), Some(NameIssue::Whitespace));
        assert!(!NameIssue::Whitespace.is_error());
        assert_eq!(validate_file_name("\tsrc/new.ts/", []), None);
    }

    #[test]
    fn splits_and_reads_leaf_names() {
        assert_eq!(path_segments("a\\b/c/"), vec!["a", "b", "c"]);
        assert_eq!(leaf_name("src/lib/a.ts"), "a.ts");
        assert_eq!(leaf_name(""), "");
        assert_eq!(well_formed_file_name("\tname\t"), "name");
    }
}
