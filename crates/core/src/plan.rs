//! Port of src/features/sessions/model/plan.ts.
//!
//! The TypeScript uses regular expressions. This crate has no regex engine,
//! so each pattern is matched by hand. Every matcher names the pattern it
//! replaces and keeps its JavaScript semantics (`\s`, `^` and `$` with the `m`
//! flag, global match counting).

use crate::js;
use crate::task_list::split_lines;

/// The `/plan` builtin skill, as listed in the composer.
pub const PLAN_COMMAND_NAME: &str = "plan";
pub const PLAN_COMMAND_DESCRIPTION: &str =
    "Create a reviewable implementation plan before changing files.";

/// Result of `consumePlanCommand`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanCommand {
    pub text: String,
    pub planning: bool,
}

/// `consumePlanCommand`: consume `/plan` when it leads the composer text.
pub fn consume_plan_command(text: &str) -> PlanCommand {
    // /^\s*\/plan(?=\s|$)\s*/i
    let rest = text.trim_start_matches(js::is_space);
    let matched = rest
        .get(..5)
        .filter(|head| head.eq_ignore_ascii_case("/plan"))
        .map(|_| &rest[5..])
        .filter(|after| after.is_empty() || after.starts_with(js::is_space));
    match matched {
        Some(after) => PlanCommand {
            text: after.trim_start_matches(js::is_space).to_string(),
            planning: true,
        },
        None => PlanCommand {
            text: text.to_string(),
            planning: false,
        },
    }
}

/// `planTurnKey`: names the plan block a turn owns. The turn counter restarts
/// with the app while the key is saved, so the caller passes a fresh UUID.
pub fn plan_turn_key(generation: i64, uuid: &str) -> String {
    format!("turn:{generation}:{uuid}")
}

/// `planTurnPrompt`.
pub fn plan_turn_prompt(request: &str) -> String {
    [
        "You are in plan mode. Investigate the request and the repository, but do not modify files, run destructive commands, or start implementing.",
        "Resolve important implementation details and finish with one self-contained Markdown plan. The plan must be specific enough to build after explicit user approval.",
        "Structure the final plan with a Markdown heading and concrete implementation steps.",
        "Do not ask the user to approve inside the response; the application provides a separate Build action.",
        "",
        "## Request",
        "",
        js::trim(request),
    ]
    .join("\n")
}

/// `buildPlanPrompt`.
pub fn build_plan_prompt(plan: &str) -> String {
    [
        "The user reviewed and explicitly approved the following implementation plan. Implement it now, using this exact edited version as the source of truth.",
        "",
        "<approved_plan>",
        js::trim(plan),
        "</approved_plan>",
    ]
    .join("\n")
}

/// `isProviderFailureText`: provider messages that can arrive as ordinary
/// assistant text although no work happened.
pub fn is_provider_failure_text(text: &str) -> bool {
    let value: Vec<char> = js::trim(text).chars().collect();
    if value.is_empty() {
        return false;
    }
    let mut limit_phrases = Vec::new();
    for you in ["you've ", "you have ", ""] {
        for your in ["your ", ""] {
            for kind in ["usage", "request", "spend"] {
                limit_phrases.push(format!("{you}reached {your}{kind} limit"));
            }
        }
    }
    let mut reached_phrases = Vec::new();
    for kind in ["usage", "rate", "request"] {
        for verb in ["reached", "exceeded"] {
            reached_phrases.push(format!("{kind} limit {verb}"));
        }
    }
    let auth_phrases = [
        "authentication required",
        "please sign in to continue",
        "please log in to continue",
    ];
    // Each pattern starts with (?:^|\n)\s*, so try every start of input and
    // every position after a newline, then skip whitespace.
    let starts = std::iter::once(0).chain(
        value
            .iter()
            .enumerate()
            .filter(|(_, c)| **c == '\n')
            .map(|(i, _)| i + 1),
    );
    for start in starts {
        let at = skip_space(&value, start);
        // upgrade your plan to continue[.!]?\s*(?:$|\n)
        if let Some(end) = phrase_at(&value, at, "upgrade your plan to continue") {
            let end = if matches!(value.get(end), Some('.' | '!')) {
                end + 1
            } else {
                end
            };
            let run_end = skip_space(&value, end);
            if run_end == value.len() || value[end..run_end].contains(&'\n') {
                return true;
            }
        }
        let any = |phrases: &[String]| phrases.iter().any(|p| phrase_at(&value, at, p).is_some());
        if any(&limit_phrases)
            || any(&reached_phrases)
            || auth_phrases
                .iter()
                .any(|p| phrase_at(&value, at, p).is_some())
        {
            return true;
        }
    }
    false
}

/// `isReviewablePlan`: a fallback assistant response must look like an
/// authored Markdown plan.
pub fn is_reviewable_plan(text: &str) -> bool {
    let value = js::trim(text);
    if value.is_empty() || is_provider_failure_text(value) {
        return false;
    }
    let chars: Vec<char> = value.chars().collect();
    let has_heading = has_heading_with_text(&chars);
    let steps = count_list_steps(&chars);
    has_heading || steps >= 2
}

/// `planTitle`: the first markdown heading, or the first prose line.
pub fn plan_title(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if let Some((start, end)) = first_heading_capture(&chars) {
        let heading: String = chars[start..end].iter().collect();
        return unwrap_markdown(&heading);
    }
    for line in split_lines(text) {
        let trimmed = js::trim(line);
        if trimmed.is_empty() || trimmed.starts_with("```") || trimmed == "---" {
            continue;
        }
        let title = unwrap_markdown(trimmed);
        let short = js::slice_prefix(&title, 80);
        return if short.is_empty() {
            "Plan".into()
        } else {
            short.to_string()
        };
    }
    "Plan".into()
}

/// `planSummary`: the first prose paragraph that is not the title heading.
pub fn plan_summary(text: &str) -> String {
    let title = plan_title(text);
    let mut parts: Vec<String> = Vec::new();
    let mut in_fence = false;
    for line in split_lines(text) {
        if line.trim_start_matches(js::is_space).starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        let line_chars: Vec<char> = line.chars().collect();
        if in_fence || heading_prefix_end(&line_chars, 0).is_some() {
            continue;
        }
        let trimmed = js::trim(line);
        if trimmed.is_empty() || trimmed == "---" {
            continue;
        }
        let next = unwrap_markdown(strip_bullet(trimmed));
        if next.is_empty() || next == title {
            continue;
        }
        parts.push(next);
        if js::len(&parts.join(" ")) >= 140 {
            break;
        }
    }
    let summary = parts.join(" ");
    if summary.is_empty() {
        return String::new();
    }
    if js::len(&summary) > 140 {
        return format!("{}…", js::slice_prefix(&summary, 139));
    }
    summary
}

/// `planMeta`: chips such as "Plan", "3 sections", "diagram", "120 words".
pub fn plan_meta(text: &str) -> Vec<String> {
    let mut parts = vec!["Plan".to_string()];
    let chars: Vec<char> = text.chars().collect();
    let headings = count_heading_prefixes(&chars);
    if headings > 1 {
        parts.push(format!("{headings} sections"));
    }
    if has_mermaid_fence(&chars) {
        parts.push("diagram".into());
    }
    let words = js::trim(text)
        .split(js::is_space)
        .filter(|word| !word.is_empty())
        .count();
    if words > 0 {
        parts.push(format!("{words} words"));
    }
    parts
}

fn skip_space(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && js::is_space(chars[i]) {
        i += 1;
    }
    i
}

/// Case-insensitive (ASCII, as JavaScript's `i` flag without `u`) literal
/// match at `at`. Returns the end index.
fn phrase_at(chars: &[char], at: usize, phrase: &str) -> Option<usize> {
    let mut i = at;
    for p in phrase.chars() {
        let c = *chars.get(i)?;
        if !c.eq_ignore_ascii_case(&p) {
            return None;
        }
        i += 1;
    }
    Some(i)
}

/// Positions where `^` matches with the `m` flag.
fn is_line_start(chars: &[char], i: usize) -> bool {
    i == 0 || js::is_line_terminator(chars[i - 1])
}

/// From a line start, `\s{0,3}#{1,6}` followed by at least one `\s`.
/// Returns the index right after the hashes.
fn heading_prefix_end(chars: &[char], start: usize) -> Option<usize> {
    let q = skip_space(chars, start);
    if q - start > 3 || chars.get(q) != Some(&'#') {
        return None;
    }
    let mut end = q;
    while chars.get(end) == Some(&'#') {
        end += 1;
    }
    let hashes = end - q;
    if !(1..=6).contains(&hashes) {
        return None;
    }
    chars.get(end).filter(|c| js::is_space(**c))?;
    Some(end)
}

/// `/^\s{0,3}#{1,6}\s+\S/m.test(value)`.
fn has_heading_with_text(chars: &[char]) -> bool {
    (0..chars.len()).any(|i| {
        is_line_start(chars, i)
            && heading_prefix_end(chars, i).is_some_and(|end| skip_space(chars, end) < chars.len())
    })
}

/// The capture of `/^\s{0,3}#{1,6}\s+(.+)$/m`, as a char range.
fn first_heading_capture(chars: &[char]) -> Option<(usize, usize)> {
    for i in 0..chars.len() {
        if !is_line_start(chars, i) {
            continue;
        }
        let Some(after_hashes) = heading_prefix_end(chars, i) else {
            continue;
        };
        let space_end = skip_space(chars, after_hashes);
        // \s+ is greedy, so try the longest run first and give back one
        // character at a time until (.+)$ can match.
        for capture_start in (after_hashes + 1..=space_end).rev() {
            let mut end = capture_start;
            while end < chars.len() && !js::is_line_terminator(chars[end]) {
                end += 1;
            }
            if end > capture_start {
                return Some((capture_start, end));
            }
        }
    }
    None
}

/// Number of matches of `/^\s{0,3}#{1,6}\s+/gm`.
fn count_heading_prefixes(chars: &[char]) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i < chars.len() {
        if is_line_start(chars, i)
            && let Some(end) = heading_prefix_end(chars, i)
        {
            count += 1;
            i = skip_space(chars, end);
            continue;
        }
        i += 1;
    }
    count
}

/// Number of matches of `/^\s*(?:[-*+] |\d+[.)] )\S/gm`.
fn count_list_steps(chars: &[char]) -> usize {
    let item_end = |q: usize| -> Option<usize> {
        let marker_end = match chars.get(q)? {
            '-' | '*' | '+' => q + 1,
            c if c.is_ascii_digit() => {
                let mut d = q;
                while chars.get(d).is_some_and(char::is_ascii_digit) {
                    d += 1;
                }
                if !matches!(chars.get(d), Some('.' | ')')) {
                    return None;
                }
                d + 1
            }
            _ => return None,
        };
        if chars.get(marker_end) != Some(&' ') {
            return None;
        }
        let next = *chars.get(marker_end + 1)?;
        (!js::is_space(next)).then_some(marker_end + 2)
    };
    let mut count = 0;
    let mut i = 0;
    while i < chars.len() {
        if is_line_start(chars, i)
            && let Some(end) = item_end(skip_space(chars, i))
        {
            count += 1;
            i = end;
            continue;
        }
        i += 1;
    }
    count
}

/// `/```\s*mermaid\b/i.test(text)`.
fn has_mermaid_fence(chars: &[char]) -> bool {
    (0..chars.len()).any(|i| {
        if phrase_at(chars, i, "```").is_none() {
            return false;
        }
        let at = skip_space(chars, i + 3);
        match phrase_at(chars, at, "mermaid") {
            Some(end) => !chars
                .get(end)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_'),
            None => false,
        }
    })
}

/// `value.replace(/^[-*+]\s+/, "")`.
fn strip_bullet(value: &str) -> &str {
    match value.strip_prefix(['-', '*', '+']) {
        Some(rest) if rest.starts_with(js::is_space) => rest.trim_start_matches(js::is_space),
        _ => value,
    }
}

/// `unwrapMarkdown`: drop images, keep link text, drop emphasis and code
/// marks, then trim.
pub fn unwrap_markdown(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let find = |from: usize, target: char| (from..chars.len()).find(|&i| chars[i] == target);

    // /!\[[^\]]*]\([^)]*\)/g -> ""
    let mut no_images = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '!'
            && chars.get(i + 1) == Some(&'[')
            && let Some(close) = find(i + 2, ']')
            && chars.get(close + 1) == Some(&'(')
            && let Some(paren) = find(close + 2, ')')
        {
            i = paren + 1;
            continue;
        }
        no_images.push(chars[i]);
        i += 1;
    }

    // /\[([^\]]+)]\([^)]*\)/g -> "$1"
    let chars = no_images;
    let find = |from: usize, target: char| (from..chars.len()).find(|&i| chars[i] == target);
    let mut linked = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '['
            && let Some(close) = find(i + 1, ']')
            && close > i + 1
            && chars.get(close + 1) == Some(&'(')
            && let Some(paren) = find(close + 2, ')')
        {
            linked.extend_from_slice(&chars[i + 1..close]);
            i = paren + 1;
            continue;
        }
        linked.push(chars[i]);
        i += 1;
    }

    // /[*_`]+/g -> ""
    let stripped: String = linked
        .into_iter()
        .filter(|c| !matches!(c, '*' | '_' | '`'))
        .collect();
    js::trim(&stripped).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // plan mode prompts
    #[test]
    fn consumes_only_a_leading_plan_command() {
        assert_eq!(
            consume_plan_command("/plan build a settings page"),
            PlanCommand {
                text: "build a settings page".into(),
                planning: true
            }
        );
        assert_eq!(
            consume_plan_command("  /PLAN\ninspect this"),
            PlanCommand {
                text: "inspect this".into(),
                planning: true
            }
        );
        assert_eq!(
            consume_plan_command("mention /plan in docs"),
            PlanCommand {
                text: "mention /plan in docs".into(),
                planning: false
            }
        );
        assert!(!consume_plan_command("/planner").planning);
        assert_eq!(
            consume_plan_command("/plan"),
            PlanCommand {
                text: "".into(),
                planning: true
            }
        );
    }

    #[test]
    fn separates_investigation_from_explicit_approved_plan_execution() {
        assert!(plan_turn_prompt("Add search").contains("do not modify files"));
        let build = build_plan_prompt("# Plan\n\n1. Add search");
        assert!(build.contains("explicitly approved"));
        assert!(build.contains("# Plan\n\n1. Add search"));
    }

    #[test]
    fn rejects_provider_blockers_and_ordinary_commentary_as_fallback_plans() {
        assert!(is_provider_failure_text("Upgrade your plan to continue"));
        assert!(!is_reviewable_plan("Upgrade your plan to continue"));
        assert!(!is_reviewable_plan(
            "I checked the repository and found the issue."
        ));
    }

    #[test]
    fn accepts_structured_markdown_fallback_plans() {
        assert!(is_reviewable_plan(
            "# Plan\n\nInspect the flow and update the adapter."
        ));
        assert!(is_reviewable_plan(
            "1. Inspect the flow\n2. Update the adapter"
        ));
    }

    #[test]
    fn recognizes_each_provider_failure_phrase() {
        assert!(is_provider_failure_text(
            "Done.\n  You've reached your usage limit."
        ));
        assert!(is_provider_failure_text("Rate limit exceeded"));
        assert!(is_provider_failure_text("please LOG in to continue"));
        assert!(is_provider_failure_text(
            "Upgrade your plan to continue!\nmore"
        ));
        assert!(!is_provider_failure_text(
            "Upgrade your plan to continue now"
        ));
        assert!(!is_provider_failure_text(
            "We hit the usage limit reached state"
        ));
    }

    #[test]
    fn titles_and_summarizes_plans() {
        let text = "Intro line\n\n## **Add** [search](http://x)\n\n- First step here\n```\ncode\n```\nSecond para";
        assert_eq!(plan_title(text), "Add search");
        assert_eq!(plan_summary(text), "Intro line First step here Second para");
        assert_eq!(
            plan_title("```\n---\n![img](a.png) Real title"),
            "Real title"
        );
        assert_eq!(plan_title(""), "Plan");
        assert_eq!(plan_title("#\n\nnext"), "next");
    }

    #[test]
    fn describes_plan_shape() {
        let text = "# One\n## Two\n```mermaid\ngraph\n```\n";
        assert_eq!(
            plan_meta(text),
            ["Plan", "2 sections", "diagram", "7 words"]
        );
        assert_eq!(plan_meta("#\n  # a"), ["Plan", "3 words"]);
        assert_eq!(plan_meta("```mermaidx"), ["Plan", "1 words"]);
    }

    #[test]
    fn counts_list_steps_like_the_global_regex() {
        let chars: Vec<char> = "1. a\n\n  - b\n-c\n2) d".chars().collect();
        assert_eq!(count_list_steps(&chars), 3);
    }

    #[test]
    fn truncates_long_summaries() {
        let long = "word ".repeat(40);
        let summary = plan_summary(&format!("# T\n{long}"));
        assert_eq!(js::len(&summary), 140);
        assert!(summary.ends_with('…'));
    }
}
