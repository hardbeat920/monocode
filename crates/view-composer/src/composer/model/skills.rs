//! The composer's side of src/features/skills/model/skills.ts and
//! slashCommands.ts: the skill row type, `/token` parsing, ranking, and the
//! highlight split. Discovery and loading stay with the engine, which hands
//! the catalog over through [`crate::composer::host::ComposerHost`].
//!
//! The parsing and ranking logic is copied from monocode-engine's
//! `submit::skills` with a flat [`Skill`] struct in place of the engine's
//! enum. Delete the copy once the view crates depend on the engine.
//!
//! Positions are byte offsets.

use std::cmp::Ordering;
use std::collections::HashSet;

use monocode_core::js;

use super::fuzzy::fuzzy_match;
use super::quote_draft::is_markdown_blockquote_position;

/// `Skill.kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SkillKind {
    /// A SKILL.md on disk.
    File,
    /// A MonoCode command or the bundled create-skill skill.
    Builtin,
    /// A provider-owned command.
    Native,
}

/// One row of the slash picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub kind: SkillKind,
    pub name: String,
    pub description: String,
    /// What `/` inserts.
    pub invocation: String,
    /// `project`, `user`, or `builtin`. Empty for native commands.
    pub scope: String,
    /// `agents`, `monocode`, or a harness id.
    pub source: String,
    /// The SKILL.md path for file skills.
    pub path: Option<String>,
    /// Native command aliases the picker also matches.
    pub aliases: Vec<String>,
}

impl Skill {
    /// A MonoCode built-in command.
    pub fn builtin(name: &str, invocation: &str, description: &str) -> Self {
        Self {
            kind: SkillKind::Builtin,
            name: name.into(),
            description: description.into(),
            invocation: invocation.into(),
            scope: "builtin".into(),
            source: "monocode".into(),
            path: None,
            aliases: Vec::new(),
        }
    }

    /// A SKILL.md skill.
    pub fn file(name: &str, description: &str, path: &str, scope: &str, source: &str) -> Self {
        Self {
            kind: SkillKind::File,
            name: name.into(),
            description: description.into(),
            invocation: name.into(),
            scope: scope.into(),
            source: source.into(),
            path: Some(path.into()),
            aliases: Vec::new(),
        }
    }

    /// A provider-owned command.
    pub fn native(name: &str, invocation: &str, description: &str, source: &str) -> Self {
        Self {
            kind: SkillKind::Native,
            name: name.into(),
            description: description.into(),
            invocation: invocation.into(),
            scope: String::new(),
            source: source.into(),
            path: None,
            aliases: Vec::new(),
        }
    }

    pub fn is_builtin(&self, name: &str) -> bool {
        self.kind == SkillKind::Builtin && self.name == name
    }
}

/// `CREATE_SKILL_NAME`.
pub const CREATE_SKILL_NAME: &str = "create-skill";

/// `BUILTIN_CREATE_SKILL`.
pub fn builtin_create_skill() -> Skill {
    Skill::builtin(
        CREATE_SKILL_NAME,
        CREATE_SKILL_NAME,
        "Create a MonoCode skill as a SKILL.md in .agents/skills. Use when the user wants to author, write, save, or scaffold a skill, or asks about skill format.",
    )
}

/// `SlashToken`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashToken {
    pub start: usize,
    pub end: usize,
    pub query: String,
}

/// `MAX_PICKER` in slashCommands.ts.
pub const MAX_PICKER: usize = 50;

/// `localeCompare` for skill names with the OS default locale.
fn locale_compare(a: &str, b: &str) -> Ordering {
    monocode_locale::compare(a, b)
}

fn scope_rank(skill: &Skill) -> u8 {
    match skill.kind {
        SkillKind::Builtin => 0,
        SkillKind::Native => 1,
        SkillKind::File if skill.scope == "project" => 1,
        SkillKind::File => 2,
    }
}

/// `rankSkills`. Pass `usize::MAX` for `Number.POSITIVE_INFINITY`.
pub fn rank_skills(skills: &[Skill], query: &str, limit: usize) -> Vec<Skill> {
    let needle = js::trim(query).to_lowercase();
    if needle.is_empty() {
        let mut sorted = skills.to_vec();
        sorted.sort_by(|a, b| {
            scope_rank(a)
                .cmp(&scope_rank(b))
                .then_with(|| locale_compare(&a.name, &b.name))
        });
        sorted.truncate(limit);
        return sorted;
    }

    let mut scored: Vec<(Skill, i64)> = Vec::new();
    for skill in skills {
        let name_hit = fuzzy_match(&needle, &skill.name);
        let invocation_hit = if name_hit.is_some() {
            None
        } else {
            let mut words = vec![skill.invocation.clone()];
            if skill.kind == SkillKind::Native {
                words.extend(skill.aliases.iter().cloned());
            }
            fuzzy_match(&needle, &words.join(" "))
        };
        let desc_hit = if name_hit.is_some() || invocation_hit.is_some() {
            None
        } else {
            fuzzy_match(&needle, &skill.description)
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
            .then_with(|| locale_compare(&a.0.name, &b.0.name))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(skill, _)| skill)
        .collect()
}

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\n' | '\t' | '\r')
}

/// `slashTokenAt`: the slash token that contains `cursor`, if the user is
/// typing `/skill`. `native` loosens the query for provider commands.
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

/// `isNativeCommandPrompt` for a harness whose commands take raw slash
/// arguments: a leading `/command` followed by whitespace or the end.
pub fn leading_native_command(text: &str) -> bool {
    let trimmed = text.trim_start_matches(js::is_space);
    let Some(rest) = trimmed.strip_prefix('/') else {
        return false;
    };
    let end = rest
        .char_indices()
        .find(|(_, c)| js::is_space(*c) || *c == '/' || *c == '\\')
        .map_or(rest.len(), |(index, _)| index);
    if end == 0 {
        return false;
    }
    rest[end..].chars().next().is_none_or(js::is_space)
}

/// One `SKILL_TOKEN_RE` match: the name and the byte range of the token.
struct SkillToken<'a> {
    name: &'a str,
    start: usize,
    end: usize,
}

/// `[a-z0-9]+(?:-[a-z0-9]+)*`, returning the length matched.
fn skill_segment(text: &str) -> usize {
    let bytes = text.as_bytes();
    let word = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    let mut index = 0;
    while index < bytes.len() && word(bytes[index]) {
        index += 1;
    }
    if index == 0 {
        return 0;
    }
    while index + 1 < bytes.len() && bytes[index] == b'-' && word(bytes[index + 1]) {
        index += 1;
        while index < bytes.len() && word(bytes[index]) {
            index += 1;
        }
    }
    index
}

/// Every `SKILL_TOKEN_RE` match in order:
/// `/(^|\s)\/(name(?::name)?)(?=\s|$)/g`.
fn skill_tokens(text: &str) -> Vec<SkillToken<'_>> {
    let mut tokens = Vec::new();
    let mut search = 0;
    while search < text.len() {
        let Some(offset) = text[search..].find('/') else {
            break;
        };
        let slash = search + offset;
        let lead_ok = slash == 0 || text[..slash].chars().next_back().is_some_and(js::is_space);
        let rest = &text[slash + 1..];
        let mut len = skill_segment(rest);
        if len > 0 && rest[len..].starts_with(':') {
            let tail = skill_segment(&rest[len + 1..]);
            if tail > 0 {
                len += 1 + tail;
            }
        }
        let end = slash + 1 + len;
        let boundary = text[end..].chars().next().is_none_or(js::is_space);
        if lead_ok && len > 0 && boundary {
            tokens.push(SkillToken {
                name: &text[slash + 1..end],
                start: slash,
                end,
            });
            search = end;
        } else {
            search = slash + 1;
        }
    }
    tokens
}

/// `skillNamesInText`.
pub fn skill_names_in_text(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for token in skill_tokens(text) {
        if names.iter().any(|name| name == token.name)
            || is_markdown_blockquote_position(text, token.start)
        {
            continue;
        }
        names.push(token.name.to_string());
    }
    names
}

/// `SkillTextPart`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillTextPart {
    pub text: String,
    pub skill: bool,
}

/// Known `/skill` token ranges in `text`, for highlighting.
pub fn skill_ranges(text: &str, names: &HashSet<String>) -> Vec<std::ops::Range<usize>> {
    if names.is_empty() {
        return Vec::new();
    }
    skill_tokens(text)
        .into_iter()
        .filter(|token| {
            names.contains(token.name) && !is_markdown_blockquote_position(text, token.start)
        })
        .map(|token| token.start..token.end)
        .collect()
}

/// `skillTextParts`: split composer text so known `/skill` tokens can be
/// highlighted.
pub fn skill_text_parts(text: &str, names: &HashSet<String>) -> Vec<SkillTextPart> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut parts: Vec<SkillTextPart> = Vec::new();
    let mut push = |value: &str, skill: bool| {
        if value.is_empty() {
            return;
        }
        if let Some(last) = parts.last_mut()
            && last.skill == skill
        {
            last.text.push_str(value);
            return;
        }
        parts.push(SkillTextPart {
            text: value.to_string(),
            skill,
        });
    };
    let mut cursor = 0;
    for range in skill_ranges(text, names) {
        push(&text[cursor..range.start], false);
        push(&text[range.clone()], true);
        cursor = range.end;
    }
    push(&text[cursor..], false);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_intl_composer_skill_order_without_changing_scope_or_score() {
        let mut skills: Vec<_> = [
            "filez",
            "file.a",
            "fileé",
            "filee\u{301}",
            "file-a",
            "filee",
            "file_a",
        ]
        .into_iter()
        .map(|name| Skill::native(name, name, "lookup", "codex"))
        .collect();
        skills.push(Skill::builtin("z-builtin", "z-builtin", "lookup"));
        let expected = [
            "file_a",
            "file-a",
            "file.a",
            "filee",
            "fileé",
            "filee\u{301}",
            "filez",
        ];
        let ranked = rank_skills(&skills, "", usize::MAX);
        assert_eq!(ranked[0].name, "z-builtin");
        assert_eq!(
            ranked[1..]
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            expected
        );
        skills.pop();
        skills.push(Skill::native("lookup", "lookup", "lookup", "codex"));
        let ranked = rank_skills(&skills, "lookup", usize::MAX);
        assert_eq!(ranked[0].name, "lookup");
        assert_eq!(
            ranked[1..]
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            expected
        );
    }

    fn token(start: usize, end: usize, query: &str) -> SlashToken {
        SlashToken {
            start,
            end,
            query: query.into(),
        }
    }

    fn review() -> Skill {
        Skill::file(
            "review-pr",
            "Review pull requests against team standards.",
            "/tmp/.agents/skills/review-pr/SKILL.md",
            "project",
            "agents",
        )
    }

    fn cursor_only() -> Skill {
        Skill::file(
            "cursor-only",
            "Cursor native helper",
            "/tmp/.cursor/skills/cursor-only/SKILL.md",
            "project",
            "cursor",
        )
    }

    fn pi_native() -> Skill {
        Skill::native(
            "architect",
            "skill:architect",
            "Design before implementation.",
            "pi",
        )
    }

    fn pi_file() -> Skill {
        Skill::file(
            "pi-file",
            "A file discovered by the existing scanner.",
            "/tmp/.pi/skills/pi-file/SKILL.md",
            "project",
            "pi",
        )
    }

    fn names() -> HashSet<String> {
        ["review-pr", "create-skill", "skill:architect"]
            .into_iter()
            .map(String::from)
            .collect()
    }

    fn part(text: &str, skill: bool) -> SkillTextPart {
        SkillTextPart {
            text: text.into(),
            skill,
        }
    }

    // native command composer behavior
    #[test]
    fn filters_commands_by_alias_and_inserts_their_invocation_with_arguments_intact() {
        let mut workflow = Skill::native("orchestrate", "orchestrate", "Choose agents", "omp");
        workflow.aliases = vec!["review".into()];
        assert_eq!(
            rank_skills(std::slice::from_ref(&workflow), "review", MAX_PICKER),
            vec![workflow.clone()]
        );
        let text = "/rev foo";
        assert_eq!(
            replace_slash_token(
                text,
                &slash_token_at(text, 4, true).unwrap(),
                &workflow.invocation
            ),
            "/orchestrate foo"
        );
        assert_eq!(
            slash_token_at("/Review_Code", 12, true).map(|t| t.query),
            Some("Review_Code".into())
        );
        assert_eq!(slash_token_at("/Review_Code", 12, false), None);
    }

    #[test]
    fn only_treats_leading_command_tokens_as_native_invocations() {
        assert!(leading_native_command("/workflow foo @README.md"));
        assert!(leading_native_command("/omp:plan investigate"));
        for text in [
            "Explain /workflow",
            "> /workflow",
            "/tmp/file.ts",
            "/tmp\\file.ts",
            "hello",
        ] {
            assert!(!leading_native_command(text), "{text}");
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

    // skillNamesInText
    #[test]
    fn collects_unique_skill_tokens() {
        assert_eq!(
            skill_names_in_text("/review-pr and /review-pr then /create-skill"),
            vec!["review-pr".to_string(), "create-skill".to_string()]
        );
    }

    // skillTextParts
    #[test]
    fn marks_known_skill_tokens() {
        assert_eq!(
            skill_text_parts("/review-pr look at auth", &names()),
            vec![part("/review-pr", true), part(" look at auth", false)]
        );
    }

    #[test]
    fn marks_a_known_namespaced_invocation() {
        assert_eq!(
            skill_text_parts("/skill:architect inspect this", &names()),
            vec![part("/skill:architect", true), part(" inspect this", false)]
        );
    }

    #[test]
    fn leaves_unknown_tokens_as_plain_text() {
        assert_eq!(
            skill_text_parts("see /not-a-skill please", &names()),
            vec![part("see /not-a-skill please", false)]
        );
    }

    #[test]
    fn splits_multiple_skills() {
        assert_eq!(
            skill_text_parts("/review-pr then /create-skill", &names()),
            vec![
                part("/review-pr", true),
                part(" then ", false),
                part("/create-skill", true)
            ]
        );
    }

    #[test]
    fn ignores_skill_tokens_inside_markdown_blockquotes() {
        let text = "/review-pr\n> /create-skill\n  > /review-pr";
        let at = text.find("/create-skill").unwrap() + 3;
        assert_eq!(slash_token_at(text, at, false), None);
        assert_eq!(skill_names_in_text(text), vec!["review-pr".to_string()]);
        let skills: Vec<String> = skill_text_parts(text, &names())
            .into_iter()
            .filter(|part| part.skill)
            .map(|part| part.text)
            .collect();
        assert_eq!(skills, vec!["/review-pr".to_string()]);
    }

    // rankSkills
    #[test]
    fn puts_create_skill_first_when_the_query_is_empty() {
        let ranked = rank_skills(
            &[cursor_only(), review(), builtin_create_skill()],
            "",
            MAX_PICKER,
        );
        let names: Vec<&str> = ranked.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["create-skill", "cursor-only", "review-pr"]);
    }

    #[test]
    fn fuzzy_matches_names_ahead_of_descriptions() {
        let ranked = rank_skills(
            &[cursor_only(), review(), builtin_create_skill()],
            "rev",
            MAX_PICKER,
        );
        assert_eq!(ranked[0].name, "review-pr");
    }

    #[test]
    fn ranks_native_pi_rows_with_project_skills() {
        let ranked = rank_skills(&[review(), pi_native(), pi_file()], "", MAX_PICKER);
        let names: Vec<&str> = ranked.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["architect", "pi-file", "review-pr"]);
    }

    #[test]
    fn matches_the_displayed_invocation() {
        assert_eq!(
            rank_skills(&[review(), pi_native()], "skill", MAX_PICKER),
            vec![pi_native()]
        );
        assert_eq!(
            rank_skills(&[review(), pi_native()], "skill:arch", MAX_PICKER),
            vec![pi_native()]
        );
    }

    #[test]
    fn gives_the_built_in_row_its_exact_invocation() {
        assert_eq!(builtin_create_skill().invocation, "create-skill");
    }

    #[test]
    fn keeps_every_pi_result_when_the_composer_removes_the_default_cap() {
        let rows: Vec<Skill> = (0..75)
            .map(|index| {
                let name = format!("skill-{index:02}");
                Skill::native(&name, &format!("skill:{name}"), "Pi skill", "pi")
            })
            .collect();
        assert_eq!(rank_skills(&rows, "", MAX_PICKER).len(), 50);
        assert_eq!(rank_skills(&rows, "", usize::MAX).len(), 75);
        assert_eq!(
            rank_skills(&rows, "skill-74", usize::MAX)[0].name,
            "skill-74"
        );
    }
}
