//! Port of src/app/model/releaseNotes.ts: find one release's section in the
//! changelog for the "What's new" tab and modal.
//!
//! The TypeScript defaulted `changelog` to the bundled CHANGELOG.md. Here
//! callers pass `BUNDLED_CHANGELOG` for that.

pub use monocode_layout::ReleaseNotesTabSource;
use regex::Regex;

/// The repository's CHANGELOG.md, compiled in as the web build bundled it.
pub const BUNDLED_CHANGELOG: &str = include_str!("../../../CHANGELOG.md");

/// `ReleaseNotesDocument`.
#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseNotesDocument {
    pub source: ReleaseNotesTabSource,
    pub markdown: String,
}

/// `releaseNotesTitle`.
pub fn release_notes_title(version: &str) -> String {
    format!("What's new in MonoCode {version}")
}

/// `releaseNotesForVersion`: the `## [version]` section, heading included,
/// up to the next `## ` heading.
pub fn release_notes_for_version(version: &str, changelog: &str) -> Option<ReleaseNotesDocument> {
    let normalized = version.trim();
    if normalized.is_empty() || normalized == "Unreleased" {
        return None;
    }

    // `\d` in JavaScript is ASCII only, so the Rust patterns spell out [0-9].
    let heading = Regex::new(&format!(
        r"(?m)^## \[{}\](?: - [0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}})?\r?$",
        regex::escape(normalized)
    ))
    .expect("valid heading pattern");
    let found = heading.find(changelog)?;

    let next_heading = Regex::new(r"(?m)^## ").expect("valid heading pattern");
    let end = next_heading
        .find_at(changelog, found.end())
        .map_or(changelog.len(), |next| next.start());
    let markdown = changelog[found.start()..end].trim_end().to_string();

    Some(ReleaseNotesDocument {
        source: ReleaseNotesTabSource::new(normalized),
        markdown,
    })
}

/// `releaseNotesMarkdown`.
pub fn release_notes_markdown(source: &ReleaseNotesTabSource, changelog: &str) -> Option<String> {
    release_notes_for_version(&source.version, changelog).map(|release| release.markdown)
}

/// `ReleaseNotesPresentation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseNotesPresentation {
    pub version: String,
    pub date: Option<String>,
    pub markdown: String,
}

/// `presentReleaseNotes`: the changelog body for the "What's new" modal. The
/// version heading lives in the modal's chrome, so it is dropped here.
pub fn present_release_notes(version: &str, changelog: &str) -> Option<ReleaseNotesPresentation> {
    let release = release_notes_for_version(version, changelog)?;

    let heading = Regex::new(&format!(
        r"^## \[{}\](?: - ([0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}}))?\r?\n*",
        regex::escape(&release.source.version)
    ))
    .expect("valid heading pattern");
    let captures = heading.captures(&release.markdown);

    Some(ReleaseNotesPresentation {
        version: release.source.version.clone(),
        date: captures
            .as_ref()
            .and_then(|captures| captures.get(1))
            .map(|date| date.as_str().to_string()),
        markdown: match &captures {
            Some(captures) => release.markdown[captures.get(0).map_or(0, |m| m.end())..]
                .trim_start()
                .to_string(),
            None => release.markdown.clone(),
        },
    })
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `formatReleaseDate`: `2026-09-01` becomes `1 Sep 2026`. Anything else comes
/// back unchanged.
pub fn format_release_date(iso: &str) -> String {
    let pattern = Regex::new(r"^([0-9]{4})-([0-9]{2})-([0-9]{2})$").expect("valid date pattern");
    let Some(captures) = pattern.captures(iso) else {
        return iso.to_string();
    };
    let month_number: usize = captures[2].parse().unwrap_or(0);
    let Some(month) = month_number
        .checked_sub(1)
        .and_then(|index| MONTHS.get(index))
    else {
        return iso.to_string();
    };
    let day: u32 = captures[3].parse().unwrap_or(0);
    format!("{day} {month} {}", &captures[1])
}

#[cfg(test)]
mod tests {
    //! Port of src/app/model/releaseNotes.test.ts.

    use super::*;

    const FIXTURE: &str = "# Changelog

## [Unreleased]

### Added
- Future work.

## [0.1.3] - 2026-09-01

### Fixed
- Newer fix.

## [0.1.2] - 2026-08-31

### Added
- Requested feature.

## [0.1.1]

### Fixed
- Older fix.
";

    #[test]
    fn extracts_only_the_requested_release() {
        let release = release_notes_for_version("0.1.2", FIXTURE).unwrap();

        assert_eq!(release.source, ReleaseNotesTabSource::new("0.1.2"));
        assert_eq!(
            release_notes_title(&release.source.version),
            "What's new in MonoCode 0.1.2"
        );
        assert!(release.markdown.contains("## [0.1.2]"));
        assert!(!release.markdown.contains("## [0.1.3]"));
        assert!(!release.markdown.contains("## [0.1.1]"));
    }

    #[test]
    fn returns_none_for_unavailable_versions() {
        for version in ["", "   ", "Unreleased", "9.9.9"] {
            assert_eq!(
                release_notes_for_version(version, FIXTURE),
                None,
                "{version:?}"
            );
        }
    }

    #[test]
    fn requires_the_heading_to_occupy_the_complete_line() {
        let malformed = "## Prefix [0.1.2]\nNo.\n\n## [0.1.2] soon\nStill no.";
        assert_eq!(release_notes_for_version("0.1.2", malformed), None);
    }

    #[test]
    fn accepts_supported_headings() {
        for changelog in [
            "## [0.1.2]\n\nUndated.",
            "## [0.1.2] - 2026-08-31\n\nDated.",
        ] {
            assert_eq!(
                release_notes_for_version("0.1.2", changelog)
                    .map(|release| release.markdown)
                    .as_deref(),
                Some(changelog)
            );
        }
    }

    #[test]
    fn extracts_the_final_release_through_the_end_of_the_changelog() {
        let release = release_notes_for_version("0.1.1", FIXTURE).unwrap();
        assert!(release.markdown.contains("Older fix."));
    }

    #[test]
    fn escapes_the_requested_version_before_matching() {
        assert_eq!(
            release_notes_for_version("0.1.2+test", "## [0.1.2+test]\n\nExact."),
            Some(ReleaseNotesDocument {
                source: ReleaseNotesTabSource::new("0.1.2+test"),
                markdown: "## [0.1.2+test]\n\nExact.".into(),
            })
        );
    }

    #[test]
    fn handles_crlf_line_endings() {
        let changelog = "## [0.1.2] - 2026-08-31\r\n\r\nBody.\r\n## [0.1.1]\r\n";
        let release = release_notes_for_version("0.1.2", changelog).unwrap();
        assert_eq!(release.markdown, "## [0.1.2] - 2026-08-31\r\n\r\nBody.");
    }

    #[test]
    fn resolves_a_stored_source_against_the_bundled_changelog_shape() {
        let release = release_notes_for_version("0.1.2", FIXTURE).unwrap();
        assert_eq!(
            release_notes_markdown(&release.source, FIXTURE),
            Some(release.markdown)
        );
    }

    #[test]
    fn drops_the_version_heading_and_keeps_the_dated_body() {
        assert_eq!(
            present_release_notes("0.1.2", FIXTURE),
            Some(ReleaseNotesPresentation {
                version: "0.1.2".into(),
                date: Some("2026-08-31".into()),
                markdown: "### Added\n- Requested feature.".into(),
            })
        );
    }

    #[test]
    fn keeps_undated_releases_without_a_date() {
        assert_eq!(
            present_release_notes("0.1.1", FIXTURE),
            Some(ReleaseNotesPresentation {
                version: "0.1.1".into(),
                date: None,
                markdown: "### Fixed\n- Older fix.".into(),
            })
        );
    }

    #[test]
    fn present_returns_none_when_the_version_is_missing() {
        assert_eq!(present_release_notes("9.9.9", FIXTURE), None);
    }

    #[test]
    fn formats_a_changelog_iso_date() {
        assert_eq!(format_release_date("2026-09-01"), "1 Sep 2026");
    }

    #[test]
    fn leaves_invalid_dates_alone() {
        for value in [
            "soon",
            "2026-13-01",
            "2026-09-1",
            "2026-00-01",
            "２０２６-09-01",
        ] {
            assert_eq!(format_release_date(value), value);
        }
    }

    #[test]
    fn the_bundled_changelog_is_the_repository_changelog() {
        assert!(BUNDLED_CHANGELOG.starts_with("# Changelog"));
    }
}
