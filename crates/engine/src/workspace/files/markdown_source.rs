//! Port of src/features/files/model/markdownSource.ts.

/// `isAtxHeadingLine`: an ATX heading (`#` through `######`) at the start
/// of a source line, after at most three spaces.
pub fn is_atx_heading_line(line: &str) -> bool {
    // `\s{0,3}`: JavaScript whitespace, at most three characters.
    let rest = line.trim_start_matches(monocode_core::js::is_space);
    if line[..line.len() - rest.len()].chars().count() > 3 {
        return false;
    }
    let hashes = rest.len() - rest.trim_start_matches('#').len();
    if !(1..=6).contains(&hashes) {
        return false;
    }
    rest[hashes..]
        .chars()
        .next()
        .is_none_or(monocode_core::js::is_space)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_atx_headings() {
        assert!(is_atx_heading_line("# Title"));
        assert!(is_atx_heading_line("## Agent OS – Project Overview"));
        assert!(is_atx_heading_line("### What exists today"));
        assert!(is_atx_heading_line("###### Deep"));
    }

    #[test]
    fn allows_up_to_three_leading_spaces() {
        assert!(is_atx_heading_line("   ## Indented"));
        assert!(!is_atx_heading_line("    ## Too deep"));
    }

    #[test]
    fn rejects_hashes_that_are_not_headings() {
        assert!(!is_atx_heading_line("Not a heading"));
        assert!(!is_atx_heading_line("#hashtag"));
        assert!(!is_atx_heading_line("Text # not a heading"));
        assert!(!is_atx_heading_line("####### seven"));
    }
}
