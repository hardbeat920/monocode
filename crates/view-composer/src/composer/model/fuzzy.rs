//! Port of src/shared/lib/fuzzy.ts: sequential fuzzy match. Every query
//! character must appear in order, with bonuses for consecutive runs, path
//! separators, and camelCase boundaries.
//!
//! Positions and lengths count UTF-16 code units, as JavaScript strings do.
//!
//! Copied from monocode-engine's `runtime::util::fuzzy`. Delete this copy once
//! the view crates depend on the engine.

/// `FuzzyHit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyHit {
    pub score: i64,
    pub positions: Vec<usize>,
}

/// `fuzzyMatch`: whitespace-separated tokens all have to match.
pub fn fuzzy_match(query: &str, text: &str) -> Option<FuzzyHit> {
    let tokens: Vec<&str> = query.split_whitespace().collect();
    if tokens.is_empty() {
        return Some(FuzzyHit {
            score: 0,
            positions: Vec::new(),
        });
    }
    if tokens.len() == 1 {
        return match_token(tokens[0], text);
    }
    let mut positions = Vec::new();
    let mut score = 0;
    for token in tokens {
        let hit = match_token(token, text)?;
        score += hit.score;
        positions.extend(hit.positions);
    }
    positions.sort_unstable();
    Some(FuzzyHit { score, positions })
}

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn match_token(query: &str, text: &str) -> Option<FuzzyHit> {
    if query.is_empty() {
        return Some(FuzzyHit {
            score: 0,
            positions: Vec::new(),
        });
    }
    let needle = units(&query.to_lowercase());
    let hay = units(&text.to_lowercase());
    // TODO(port): like the TypeScript, the bonuses read the original text at
    // indexes into the lowercased one, which drift if lowercasing changes the
    // length.
    let original = units(text);
    let mut positions = Vec::new();
    let mut score: i64 = 0;
    let mut consecutive: i64 = 0;
    let mut qi = 0;
    let mut i = 0;
    while i < hay.len() && qi < needle.len() {
        if hay[i] != needle[qi] {
            consecutive = 0;
            i += 1;
            continue;
        }
        positions.push(i);
        consecutive += 1;
        score += 1 + consecutive * 4;
        if i == 0 || is_break(original.get(i - 1).copied()) {
            score += 14;
        } else if is_upper(original.get(i).copied()) && !is_upper(original.get(i - 1).copied()) {
            score += 10;
        }
        qi += 1;
        i += 1;
    }
    if qi != needle.len() {
        return None;
    }
    score -= hay.len() as i64 - needle.len() as i64;
    Some(FuzzyHit { score, positions })
}

fn is_break(unit: Option<u16>) -> bool {
    matches!(
        unit.and_then(|unit| char::from_u32(unit as u32)),
        Some('/' | '\\' | '-' | '_' | '.' | ' ')
    )
}

fn is_upper(unit: Option<u16>) -> bool {
    unit.is_some_and(|unit| (b'A' as u16..=b'Z' as u16).contains(&unit))
}

/// `scorePath`: prefer filename hits over directory-only hits. Positions
/// index `relative`.
pub fn score_path(query: &str, relative: &str, name: &str) -> Option<FuzzyHit> {
    let needle = monocode_core::js::trim(query);
    if needle.is_empty() {
        return Some(FuzzyHit {
            score: 0,
            positions: Vec::new(),
        });
    }
    let offset = monocode_core::js::len(relative).saturating_sub(monocode_core::js::len(name));
    if let Some(hit) = fuzzy_match(needle, name) {
        return Some(FuzzyHit {
            score: hit.score + 400,
            positions: hit
                .positions
                .into_iter()
                .map(|index| index + offset)
                .collect(),
        });
    }
    fuzzy_match(needle, relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_in_order_with_boundary_bonuses() {
        let hit = fuzzy_match("ab", "a/b").unwrap();
        assert_eq!(hit.positions, vec![0, 2]);
        // a: 1 + 4 + 14 at the start; b: 1 + 4 + 14 after "/"; minus 1 skipped.
        assert_eq!(hit.score, 37);
        assert!(fuzzy_match("ba", "ab").is_none());
        assert_eq!(
            fuzzy_match("  ", "x").unwrap().positions,
            Vec::<usize>::new()
        );
    }

    #[test]
    fn camel_case_and_tokens() {
        let hit = fuzzy_match("fb", "fooBar").unwrap();
        assert_eq!(hit.positions, vec![0, 3]);
        assert_eq!(hit.score, 19 + 15 - 4);
        let multi = fuzzy_match("bar foo", "fooBar").unwrap();
        assert_eq!(multi.positions, vec![0, 1, 2, 3, 4, 5]);
        assert!(fuzzy_match("foo zed", "fooBar").is_none());
    }

    #[test]
    fn path_scores_prefer_the_file_name() {
        let hit = score_path("app", "src/app/App.tsx", "App.tsx").unwrap();
        assert_eq!(hit.positions, vec![8, 9, 10]);
        assert!(hit.score > 400);
        let dir = score_path("src", "src/app/App.tsx", "App.tsx").unwrap();
        assert_eq!(dir.positions, vec![0, 1, 2]);
        assert!(dir.score < 400);
    }
}
