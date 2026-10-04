//! Port of src/features/skills/model/slashCommands.ts: picker helpers kept
//! free of skill discovery so the floating composer can use them too.
//!
//! Positions are byte offsets into the composer text, which is how GPUI's
//! text input reports them. The TypeScript used UTF-16 offsets.

use std::cmp::Ordering;

use crate::runtime::util::fuzzy::fuzzy_match;
use crate::submit::quote_draft::is_markdown_blockquote_position;

use super::Skill;

/// `SlashToken`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashToken {
    pub start: usize,
    pub end: usize,
    pub query: String,
}

/// `MAX_PICKER`.
pub const MAX_PICKER: usize = 50;

/// `localeCompare` for skill names with the OS default locale.
fn locale_compare(a: &str, b: &str) -> Ordering {
    monocode_locale::compare(a, b)
}

/// `rankSkills`. Pass `usize::MAX` for `Number.POSITIVE_INFINITY`.
pub fn rank_skills(skills: &[Skill], query: &str, limit: usize) -> Vec<Skill> {
    let needle = monocode_core::js::trim(query).to_lowercase();
    if needle.is_empty() {
        let mut sorted = skills.to_vec();
        sorted.sort_by(|a, b| {
            scope_rank(a)
                .cmp(&scope_rank(b))
                .then_with(|| locale_compare(a.name(), b.name()))
        });
        sorted.truncate(limit);
        return sorted;
    }

    let mut scored: Vec<(Skill, i64)> = Vec::new();
    for skill in skills {
        let name_hit = fuzzy_match(&needle, skill.name());
        let invocation_hit = if name_hit.is_some() {
            None
        } else {
            let mut words = vec![skill.invocation().to_string()];
            if let Skill::Native(native) = skill {
                words.extend(native.aliases.clone().unwrap_or_default());
            }
            fuzzy_match(&needle, &words.join(" "))
        };
        let desc_hit = if name_hit.is_some() || invocation_hit.is_some() {
            None
        } else {
            fuzzy_match(&needle, skill.description())
        };
        let named = name_hit.is_some() || invocation_hit.is_some();
        let Some(hit) = name_hit.or(invocation_hit).or(desc_hit) else {
            continue;
        };
        let score = if named { hit.score + 400 } else { hit.score };
        scored.push((skill.clone(), score));
    }
    scored.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| locale_compare(a.0.name(), b.0.name()))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(skill, _)| skill)
        .collect()
}

fn scope_rank(skill: &Skill) -> u8 {
    match skill {
        Skill::Builtin(_) => 0,
        Skill::Native(_) => 1,
        Skill::File(file) if file.scope == super::FileSkillScope::Project => 1,
        Skill::File(_) => 2,
    }
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\n' | '\t' | '\r')
}

/// `slashTokenAt`: the slash token that contains `cursor`, if the user is
/// typing `/skill`.
pub fn slash_token_at(text: &str, cursor: usize, native: bool) -> Option<SlashToken> {
    let mut i = cursor.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    let mut start = i;
    while let Some(previous) = text[..start].chars().next_back() {
        if is_space(previous) {
            break;
        }
        start -= previous.len_utf8();
    }
    if !text[start..].starts_with('/') {
        return None;
    }
    if text[..start].ends_with(':') {
        return None;
    }
    if is_markdown_blockquote_position(text, start) {
        return None;
    }

    let mut end = start + 1;
    while let Some(next) = text[end..].chars().next() {
        if is_space(next) {
            break;
        }
        end += next.len_utf8();
    }

    let typed = &text[(start + 1).min(i)..i];
    if typed.contains('/') || typed.contains('\\') {
        return None;
    }
    if native {
        if !typed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
        {
            return None;
        }
    } else {
        if typed.chars().any(|c| c.is_ascii_uppercase()) {
            return None;
        }
        if !is_skill_query(typed) {
            return None;
        }
    }

    Some(SlashToken {
        start,
        end,
        query: typed.to_string(),
    })
}

/// `/^(?:[a-z0-9-]+(?::[a-z0-9-]*)?)?$/`.
fn is_skill_query(typed: &str) -> bool {
    if typed.is_empty() {
        return true;
    }
    let word = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
    match typed.split_once(':') {
        None => typed.chars().all(word),
        Some((head, tail)) => !head.is_empty() && head.chars().all(word) && tail.chars().all(word),
    }
}

/// `replaceSlashToken`.
pub fn replace_slash_token(text: &str, token: &SlashToken, name: &str) -> String {
    let rest = &text[token.end.min(text.len())..];
    let spacer = if rest.starts_with(' ') { "" } else { " " };
    format!(
        "{}/{name}{spacer}{rest}",
        &text[..token.start.min(text.len())]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_skill(name: &str) -> Skill {
        Skill::Native(monocode_harness::core::native_commands::NativeCommand {
            name: name.into(),
            description: "fixture command".into(),
            invocation: name.into(),
            source: monocode_core::HarnessId::Codex,
            origin: None,
            aliases: None,
            input_hint: None,
            subcommands: None,
        })
    }

    #[test]
    fn matches_intl_skill_ranking_without_changing_scope_or_score() {
        for locale in ["en", "fr", "ja", "ar"] {
            monocode_locale::with_locale(locale, || {
                let input: Vec<_> = ["file.a", "file-a", "file_a"]
                    .into_iter()
                    .map(native_skill)
                    .collect();
                let ranked = rank_skills(&input, "file", usize::MAX);
                assert_eq!(
                    ranked.iter().map(Skill::name).collect::<Vec<_>>(),
                    ["file_a", "file-a", "file.a"]
                );
                let mut scopes = input.clone();
                scopes.push(Skill::Builtin(super::super::BuiltinSkill::new(
                    "zzz", "zzz", "fixture",
                )));
                let ranked = rank_skills(&scopes, "", usize::MAX);
                assert_eq!(
                    ranked.iter().map(Skill::name).collect::<Vec<_>>(),
                    ["zzz", "file_a", "file-a", "file.a"]
                );
            })
            .unwrap();
        }
    }

    fn token(start: usize, end: usize, query: &str) -> SlashToken {
        SlashToken {
            start,
            end,
            query: query.into(),
        }
    }

    // slashTokenAt
    #[test]
    fn reads_the_token_the_cursor_is_in() {
        assert_eq!(slash_token_at("/cre", 4, false), Some(token(0, 4, "cre")));
        assert_eq!(
            slash_token_at("please /rev", 11, false),
            Some(token(7, 11, "rev"))
        );
        assert_eq!(
            slash_token_at("/skill:arch", 11, false),
            Some(token(0, 11, "skill:arch"))
        );
    }

    #[test]
    fn ignores_urls_and_paths() {
        assert_eq!(slash_token_at("https://example.com", 12, false), None);
        assert_eq!(slash_token_at("/Users/me", 4, false), None);
        assert_eq!(slash_token_at("foo/bar", 4, false), None);
    }

    #[test]
    fn closes_after_a_space() {
        assert_eq!(slash_token_at("/review-pr now", 14, false), None);
    }

    // replaceSlashToken
    #[test]
    fn inserts_an_exact_invocation_and_a_trailing_space() {
        assert_eq!(
            replace_slash_token("/cre", &token(0, 4, "cre"), "create-skill"),
            "/create-skill "
        );
        assert_eq!(
            replace_slash_token("x /r y", &token(2, 4, "r"), "review-pr"),
            "x /review-pr y"
        );
        assert_eq!(
            replace_slash_token("/arch", &token(0, 5, "arch"), "skill:architect"),
            "/skill:architect "
        );
    }
}
