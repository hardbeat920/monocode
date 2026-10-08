/// Recover a preimage only when every hunk matches at its declared position.
/// Searching for matching lines can silently attribute unrelated edits to the
/// session, so malformed, shifted, or overlapping hunks must fail closed.
pub(crate) fn reverse_diff(after: &[u8], diff: &str) -> Option<Vec<u8>> {
    let after: Vec<&[u8]> = after.split_inclusive(|byte| *byte == b'\n').collect();
    let mut patch = diff.split_inclusive('\n').peekable();
    let mut before: Vec<&[u8]> = Vec::new();
    let mut cursor = 0;
    let mut saw_hunk = false;

    while let Some(header) = patch.next() {
        let header = header.strip_prefix("@@ -")?;
        let (old_range, header) = header.split_once(" +")?;
        let (new_range, _) = header.split_once(" @@")?;
        let (old_start, old_count) = hunk_range(old_range)?;
        let (new_start, new_count) = hunk_range(new_range)?;
        if old_count == 0 && new_count == 0 {
            return None;
        }
        let new_end = new_start.checked_add(new_count)?;
        if new_start < cursor || new_end > after.len() {
            return None;
        }
        before.extend_from_slice(after.get(cursor..new_start)?);
        if before.len() != old_start {
            return None;
        }
        cursor = new_start;
        let old_end = old_start.checked_add(old_count)?;
        while before.len() < old_end || cursor < new_end {
            let line = patch.next()?.as_bytes();
            let (&kind, mut content) = line.split_first()?;
            if !matches!(kind, b' ' | b'+' | b'-') || !content.ends_with(b"\n") {
                return None;
            }
            if patch
                .peek()
                .is_some_and(|line| line.trim_end_matches('\n') == "\\ No newline at end of file")
            {
                patch.next();
                content = content.strip_suffix(b"\n")?;
                if content.is_empty() {
                    return None;
                }
            }
            if kind != b'-' {
                if cursor >= new_end || after.get(cursor).copied()? != content {
                    return None;
                }
                cursor += 1;
            }
            if kind != b'+' {
                if before.len() >= old_end {
                    return None;
                }
                before.push(content);
            }
        }
        saw_hunk = true;
    }
    before.extend_from_slice(&after[cursor..]);
    if !saw_hunk
        || before
            .iter()
            .take(before.len().saturating_sub(1))
            .any(|line| !line.ends_with(b"\n"))
    {
        return None;
    }
    Some(before.concat())
}

/// Unified diff empty ranges name the preceding line; nonempty ranges are
/// one-based. Return a zero-based boundary and the number of lines in the hunk.
fn hunk_range(range: &str) -> Option<(usize, usize)> {
    let (start, count) = range.split_once(',').unwrap_or((range, "1"));
    if start.is_empty()
        || count.is_empty()
        || !start.bytes().all(|byte| byte.is_ascii_digit())
        || !count.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let start: usize = start.parse().ok()?;
    let count: usize = count.parse().ok()?;
    Some((
        if count == 0 {
            start
        } else {
            start.checked_sub(1)?
        },
        count,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_git_generated_hunks() {
        use std::process::Command;
        use std::time::{SystemTime, UNIX_EPOCH};

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "monocode-exact-diff-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        let corpus = [
            "",
            "a\n",
            "a",
            "a\nb\nc\n",
            "a\nrepeat\nb\nrepeat\nc\n",
            "\n",
            "a\r\nb\r\n",
            "a\nb",
            "left\n\nright",
            "旧\n新\n",
        ];
        for before in corpus {
            for after in corpus {
                std::fs::write(dir.join("before"), before).unwrap();
                std::fs::write(dir.join("after"), after).unwrap();
                for context in ["--unified=0", "--unified=3"] {
                    let output = Command::new("git")
                        .current_dir(&dir)
                        .args([
                            "-c",
                            "core.autocrlf=false",
                            "diff",
                            "--no-index",
                            "--no-ext-diff",
                            "--no-textconv",
                            "--no-color",
                            "--text",
                            context,
                            "--",
                            "before",
                            "after",
                        ])
                        .output()
                        .unwrap();
                    if before == after {
                        assert!(output.status.success());
                        continue;
                    }
                    assert_eq!(output.status.code(), Some(1));
                    let patch = String::from_utf8(output.stdout).unwrap();
                    let hunks = &patch[patch.find("@@ ").unwrap()..];
                    assert_eq!(
                        reverse_diff(after.as_bytes(), hunks),
                        Some(before.as_bytes().to_vec()),
                        "{hunks}"
                    );
                }
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_shifted_hunks_and_out_of_bounds_empty_ranges() {
        for (after, diff) in [
            ("external\nnew\n", "@@ -1 +1 @@\n-old\n+new\n"),
            ("new\nexternal\n", "@@ -2 +2 @@\n-old\n+new\n"),
            ("keep\n", "@@ -9 +8,0 @@\n-old\n"),
        ] {
            assert_eq!(reverse_diff(after.as_bytes(), diff), None, "{diff}");
        }
    }

    #[test]
    fn recovers_exact_ranges_including_line_count_changes() {
        for (after, diff, before) in [
            ("new\n", "@@ -1 +1 @@ label\n-old\n+new\n", "old\n"),
            ("new\nkeep\n", "@@ -0,0 +1 @@\n+new\n", "keep\n"),
            ("a\nnew\nb\n", "@@ -1,0 +2 @@\n+new\n", "a\nb\n"),
            ("keep\n", "@@ -1 +0,0 @@\n-old\n", "old\nkeep\n"),
            ("a\nb\n", "@@ -2 +1,0 @@\n-old\n", "a\nold\nb\n"),
            ("a\n", "@@ -2 +1,0 @@\n-old\n", "a\nold\n"),
            ("new\n", "@@ -0,0 +1 @@\n+new\n", ""),
            ("", "@@ -1 +0,0 @@\n-old\n", "old\n"),
            (
                "a\nx\ny\nb\nc\ne\nf\n",
                "@@ -1,0 +2,2 @@\n+x\n+y\n@@ -4 +5,0 @@\n-d\n",
                "a\nb\nc\nd\ne\nf\n",
            ),
            (
                "same\nnew\nsame\nnew\n",
                "@@ -4 +4 @@\n-old\n+new\n",
                "same\nnew\nsame\nold\n",
            ),
        ] {
            assert_eq!(
                reverse_diff(after.as_bytes(), diff),
                Some(before.as_bytes().to_vec()),
                "{diff}"
            );
        }
    }

    #[test]
    fn preserves_exact_line_endings() {
        for (after, diff, before) in [
            ("new\n", "@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n", "old"),
            ("new", "@@ -1 +1 @@\n-old\n+new\n\\ No newline at end of file\n", "old\n"),
            ("new", "@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n\\ No newline at end of file\n", "old"),
            ("new\nlast", "@@ -1,2 +1,2 @@\n-old\n+new\n last\n\\ No newline at end of file\n", "old\nlast"),
            ("new\r\n", "@@ -1 +1 @@\n-old\r\n+new\r\n", "old\r\n"),
            ("新\n", "@@ -1 +1 @@\n-旧\n+新\n", "旧\n"),
        ] {
            assert_eq!(reverse_diff(after.as_bytes(), diff), Some(before.as_bytes().to_vec()), "{diff}");
        }
        assert_eq!(
            reverse_diff(b"\xff\nnew\n", "@@ -2 +2 @@\n-old\n+new\n"),
            Some(b"\xff\nold\n".to_vec()),
        );
    }

    #[test]
    fn rejects_malformed_or_inconsistent_hunks() {
        for (after, diff) in [
            ("new\n", ""),
            ("new\n", "@@ -0,0 +0,0 @@\n"),
            ("", "@@ -1 +0,0 @@\n-\n\\ No newline at end of file\n"),
            ("new\n", "@@ -0 +1 @@\n-old\n+new\n"),
            ("new\n", "@@ -1 +0 @@\n-old\n+new\n"),
            ("new\n", "@@ -1,2 +1 @@\n-old\n+new\n"),
            ("new\n", "@@ -1 +1,2 @@\n-old\n+new\n"),
            ("new\n", "@@ -2 +1 @@\n-old\n+new\n"),
            ("new\n", "@@ -1 +1 @@\n-old\n+new\n+extra\n"),
            ("new\n", "@@ -1 +1 @@\n-old\n+new\n\\ No newline at end of file\n"),
            ("new", "@@ -1 +1 @@\n-old\n+new\n"),
            ("new\n", "@@ -1 +1 @@\n\\ No newline at end of file\n-old\n+new\n"),
            ("new", "@@ -1 +1 @@\n-old\n+new\n\\ No newline at end of file\n\\ No newline at end of file\n"),
            ("new\nlast\n", "@@ -1 +1 @@\n-old\n\\ No newline at end of file\n+new\n"),
            ("new\n", "@@ -1 +1 @@\n-old\n+new\n@@ -1 +1 @@\n-old\n+new\n"),
            ("new\n", "@@ -18446744073709551616 +1 @@\n-old\n+new\n"),
            ("new\n", "@@ -1 +1,18446744073709551615 @@\n-old\n+new\n"),
            ("new\n", "@@ -1 +1 @@\n-old\n+new"),
            ("new\n", "@@ -1 +1 @@\n wrong\n"),
        ] {
            assert_eq!(reverse_diff(after.as_bytes(), diff), None, "{diff}");
        }
    }
}
