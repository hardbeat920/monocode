//! Native inbox presentation of Sonar data. Remote rule HTML is converted to text/Markdown.
use super::sonar::*;
use super::{Diff, DiffFile, Item, Label, Person};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn branch_key(project: &str, branch: &str) -> String {
    serde_json::to_string(&(project, branch)).unwrap_or_default()
}

pub fn label(code: &str) -> String {
    match code {
        "BLOCKER" => "Blocker",
        "CRITICAL" => "Critical",
        "MAJOR" => "Major",
        "MINOR" => "Minor",
        "INFO" => "Info",
        "BUG" => "Bug",
        "VULNERABILITY" => "Vulnerability",
        "CODE_SMELL" => "Code smell",
        "OPEN" => "Open",
        "CONFIRMED" => "Confirmed",
        "REOPENED" => "Reopened",
        "RESOLVED" => "Resolved",
        "CLOSED" => "Closed",
        "ACCEPTED" => "Accepted",
        "FALSE_POSITIVE" | "FALSEPOSITIVE" => "False positive",
        "FIXED" => "Fixed",
        "WONTFIX" => "Won’t fix",
        "confirm" => "Confirm",
        "unconfirm" => "Unconfirm",
        "reopen" => "Reopen",
        "resolve" => "Resolve",
        "falsepositive" => "False positive",
        "wontfix" => "Won’t fix",
        "accept" => "Accept",
        _ => code,
    }
    .to_string()
}

fn remote_name(remote: &str) -> &str {
    remote
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("")
        .trim_end_matches(".git")
}

pub fn project(projects: &[PathBuf], cwd: &Path, name: &str) -> PathBuf {
    project_with_names(projects, cwd, name, &std::collections::HashMap::new())
}

pub fn project_with_names(
    projects: &[PathBuf],
    cwd: &Path,
    name: &str,
    appearances: &std::collections::HashMap<String, crate::project_appearance::ProjectAppearance>,
) -> PathBuf {
    let matches =
        |candidate: &str| !name.trim().is_empty() && candidate.eq_ignore_ascii_case(name.trim());
    let mut ordered: Vec<_> = projects.iter().collect();
    ordered.sort_by_key(|path| path.as_path() != cwd);
    ordered
        .iter()
        .find(|path| {
            super::run_git(path, &["config", "--get-regexp", "remote\\..*\\.url"])
                .unwrap_or_default()
                .lines()
                .filter_map(|line| line.split_whitespace().nth(1))
                .any(|remote| matches(remote_name(remote)))
        })
        .or_else(|| {
            ordered.iter().find(|path| {
                matches(&path.file_name().unwrap_or_default().to_string_lossy())
                    || appearances
                        .get(&path.to_string_lossy().into_owned())
                        .is_some_and(|appearance| matches(&appearance.name))
            })
        })
        .copied()
        .cloned()
        .unwrap_or_else(|| {
            if projects.iter().any(|path| path == cwd) {
                cwd.to_path_buf()
            } else {
                projects
                    .first()
                    .cloned()
                    .unwrap_or_else(|| cwd.to_path_buf())
            }
        })
}

pub fn item(issue: SonarIssue, projects: &[PathBuf], cwd: &Path) -> Item {
    let path = project(projects, cwd, &issue.repository_name);
    Item {
        provider: "sonar".into(),
        kind: "sonar".into(),
        id: issue.key.clone(),
        identifier: if issue.rule.is_empty() {
            issue.key
        } else {
            issue.rule.clone()
        },
        title: issue.message,
        url: issue.url,
        state: issue.status,
        state_type: if issue.resolved { "completed" } else { "open" }.into(),
        updated_at: issue.updated_at,
        labels: [
            issue.severity,
            issue.issue_type,
            issue.rule,
            issue.branch.clone(),
            if let Some(line) = issue.line {
                format!("{}:{line}", issue.file)
            } else {
                issue.file
            },
        ]
        .into_iter()
        .filter(|value| !value.is_empty())
        .map(|name| Label {
            name: label(&name),
            color: String::new(),
        })
        .collect(),
        assignees: if issue.assignee.is_empty() {
            vec![]
        } else {
            vec![Person {
                login: issue.assignee,
                ..Default::default()
            }]
        },
        repo: issue.repository_name,
        project_path: path.to_string_lossy().into_owned(),
        sonar_project: issue.project_key,
        sonar_branch: issue.branch,
        ..Default::default()
    }
}

fn decode(value: &str) -> String {
    let entity = regex::Regex::new(r"&(#x[0-9a-fA-F]+|#[0-9]+|amp|lt|gt|quot|apos|nbsp);").unwrap();
    entity
        .replace_all(value, |captures: &regex::Captures<'_>| match &captures[1] {
            "amp" => "&".into(),
            "lt" => "<".into(),
            "gt" => ">".into(),
            "quot" => "\"".into(),
            "apos" => "'".into(),
            "nbsp" => " ".into(),
            code => code
                .strip_prefix("#x")
                .and_then(|n| u32::from_str_radix(n, 16).ok())
                .or_else(|| code.strip_prefix('#').and_then(|n| n.parse().ok()))
                .and_then(char::from_u32)
                .map(|c| c.to_string())
                .unwrap_or_default(),
        })
        .into_owned()
}
fn fenced(code: &str) -> String {
    let size = code
        .split(|c| c != '`')
        .map(str::len)
        .max()
        .unwrap_or(0)
        .max(2)
        + 1;
    let fence = "`".repeat(size);
    format!("\n\n{fence}\n{}\n{fence}\n\n", code.trim_matches('\n'))
}

pub fn html_markdown(html: &str) -> String {
    let tokens = regex::Regex::new(r"(?s)<!--.*?-->|<[^>]*>|[^<]+|<").unwrap();
    let mut out = String::new();
    let mut pre = None::<String>;
    let mut dropped = None::<String>;
    for token in tokens.find_iter(html).map(|m| m.as_str()) {
        if token.starts_with("<!--") {
            continue;
        }
        if token.starts_with('<') && token.ends_with('>') {
            let inner = token[1..token.len() - 1].trim();
            let closing = inner.starts_with('/');
            let tag = inner
                .trim_start_matches('/')
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches('/')
                .to_ascii_lowercase();
            if let Some(blocked) = &dropped {
                if closing && &tag == blocked {
                    dropped = None;
                }
                continue;
            }
            if matches!(
                tag.as_str(),
                "script" | "style" | "iframe" | "object" | "svg" | "form"
            ) {
                if !closing {
                    dropped = Some(tag);
                }
                continue;
            }
            if tag == "pre" {
                if closing {
                    if let Some(code) = pre.take() {
                        out.push_str(&fenced(&code));
                    }
                } else {
                    pre = Some(String::new());
                }
                continue;
            }
            if pre.is_some() {
                continue;
            }
            match tag.as_str() {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    out.push_str("\n\n");
                    if !closing {
                        out.push_str(&"#".repeat(tag[1..].parse().unwrap_or(2)));
                        out.push(' ');
                    }
                }
                "p" | "div" | "section" | "ul" | "ol" | "table" => out.push_str("\n\n"),
                "br" | "tr" => out.push('\n'),
                "li" if !closing => out.push_str("\n- "),
                "td" | "th" if closing => out.push_str(" | "),
                "b" | "strong" => out.push_str("**"),
                "i" | "em" => out.push('*'),
                "code" => out.push('`'),
                _ => {}
            }
        } else if dropped.is_none() {
            let text = decode(token);
            if let Some(code) = &mut pre {
                code.push_str(&text);
            } else {
                // Escape raw HTML and image syntax before passing untrusted descriptions to Markdown.
                out.push_str(
                    &text
                        .replace('<', "&lt;")
                        .replace('>', "&gt;")
                        .replace("![", "\\!["),
                );
            }
        }
    }
    if let Some(code) = pre {
        out.push_str(&fenced(&code));
    }
    out.trim().into()
}
fn field(value: &Value, name: &str) -> String {
    value
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .into()
}

pub fn body(content: &SonarIssueContent) -> String {
    let issue = &content.issue;
    let mut text = format!(
        "**{}** · {} · {}\n\nProject: {} · Branch: {}\n\nFile: {}{}\n\nRule: {}\n\n",
        label(&field(issue, "severity")),
        label(&field(issue, "type")),
        label(
            &issue
                .get("issueStatus")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| field(issue, "status"))
        ),
        field(issue, "project"),
        field(issue, "branch"),
        field(issue, "component"),
        issue
            .get("line")
            .map(|line| format!(":{line}"))
            .unwrap_or_default(),
        field(issue, "rule")
    );
    for name in [
        "effort",
        "debt",
        "cleanCodeAttribute",
        "cleanCodeAttributeCategory",
        "resolution",
    ] {
        let value = field(issue, name);
        if !value.is_empty() {
            text.push_str(&format!("{name}: {value}\n\n"));
        }
    }
    for name in ["impacts", "flows"] {
        if let Some(value) = issue
            .get(name)
            .filter(|v| v.as_array().is_some_and(|v| !v.is_empty()))
        {
            text.push_str(&format!(
                "### {}\n{}\n",
                if name == "flows" {
                    "Related code locations"
                } else {
                    "Quality impacts"
                },
                fenced(&serde_json::to_string_pretty(value).unwrap_or_default())
            ));
        }
    }
    if !content.source.is_empty() {
        text.push_str("### Analyzed source context\n");
        let start = issue
            .pointer("/textRange/startLine")
            .and_then(Value::as_u64)
            .or_else(|| issue.get("line").and_then(Value::as_u64));
        let end = issue
            .pointer("/textRange/endLine")
            .and_then(Value::as_u64)
            .or(start);
        text.push_str(&fenced(
            &content
                .source
                .iter()
                .map(|line| {
                    format!(
                        "{} {:>5} | {}",
                        if start
                            .zip(end)
                            .is_some_and(|(start, end)| (start..=end).contains(&line.line))
                        {
                            ">"
                        } else {
                            " "
                        },
                        line.line,
                        line.code
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }
    text.push_str(if content.reference_documentation {
        "### Latest reference documentation\n\nMay differ from the installed analyzer.\n\n"
    } else {
        "### Rule guidance\n\n"
    });
    text.push_str(&html_markdown(&content.rule_html));
    if let Some(sections) = content
        .rule
        .as_ref()
        .and_then(|rule| rule.get("descriptionSections"))
        .and_then(Value::as_array)
    {
        for section in sections {
            text.push_str(&format!(
                "\n\n#### {}\n\n{}",
                field(section, "key"),
                html_markdown(&field(section, "content"))
            ));
        }
    }
    text.push_str(&format!("\n\n[Rule documentation]({})\n", content.rule_url));
    if content.comparison.is_some() {
        text.push_str("\nThe Code tab compares the latest analyzed file on this branch with the latest analyzed main branch source.\n");
    }
    if let Some(comments) = issue.get("comments").and_then(Value::as_array) {
        text.push_str("\n### Comments\n");
        for comment in comments {
            text.push_str(&format!(
                "\n**{}** · {}\n\n{}\n",
                field(comment, "login"),
                field(comment, "createdAt"),
                if let Some(html) = comment.get("htmlText").and_then(Value::as_str) {
                    html_markdown(html)
                } else {
                    field(comment, "markdown")
                }
            ));
        }
    }
    for error in &content.errors {
        text.push_str(&format!("\n\n{error}"));
    }
    text
}

fn examples(html: &str) -> Vec<(String, String)> {
    let blocks = regex::Regex::new(r"(?is)<pre\b([^>]*)>(.*?)</pre\s*>").unwrap();
    let attributes =
        regex::Regex::new(r#"(?i)(data-diff-id|data-diff-type)\s*=\s*["']([^"']*)["']"#).unwrap();
    let tags = regex::Regex::new(r"<[^>]*>").unwrap();
    let mut pairs = std::collections::BTreeMap::<String, (Option<String>, Option<String>)>::new();
    for block in blocks.captures_iter(html) {
        let mut id = None;
        let mut kind = None;
        for attr in attributes.captures_iter(&block[1]) {
            if attr[1].eq_ignore_ascii_case("data-diff-id") {
                id = Some(attr[2].to_string());
            } else {
                kind = Some(attr[2].to_string());
            }
        }
        if let (Some(id), Some(kind)) = (id, kind) {
            let pair = pairs.entry(id).or_default();
            let code = decode(&tags.replace_all(&block[2], ""))
                .trim_start_matches('\n')
                .to_string();
            if kind == "noncompliant" {
                pair.0 = Some(code);
            } else if kind == "compliant" {
                pair.1 = Some(code);
            }
        }
    }
    pairs
        .into_values()
        .filter_map(|(original, current)| original.zip(current))
        .collect()
}

pub fn diff(content: &SonarIssueContent) -> Diff {
    let mut comparisons = vec![];
    if let Some(comparison) = &content.comparison {
        let component = field(&content.issue, "component");
        let path = component
            .split_once(':')
            .map(|(_, path)| path.to_string())
            .unwrap_or_else(|| "analyzed-source".into());
        comparisons.push((
            path,
            comparison.original.clone(),
            comparison.current.clone(),
        ));
    }
    let mut html = vec![content.rule_html.clone()];
    if let Some(sections) = content
        .rule
        .as_ref()
        .and_then(|rule| rule.get("descriptionSections"))
        .and_then(Value::as_array)
    {
        html.extend(sections.iter().map(|section| field(section, "content")));
    }
    for (ix, (original, current)) in html.iter().flat_map(|html| examples(html)).enumerate() {
        comparisons.push((
            format!("Rule example {} (reference only)", ix + 1),
            original,
            current,
        ));
    }
    let mut result = Diff::default();
    for (path, original, current) in comparisons {
        let changes = similar::TextDiff::from_lines(&original, &current);
        let additions = changes
            .iter_all_changes()
            .filter(|c| c.tag() == similar::ChangeTag::Insert)
            .count() as i64;
        let deletions = changes
            .iter_all_changes()
            .filter(|c| c.tag() == similar::ChangeTag::Delete)
            .count() as i64;
        result.additions += additions;
        result.deletions += deletions;
        result.patch.push_str(&format!(
            "diff --git a/{path} b/{path}\n{}",
            changes
                .unified_diff()
                .header(&format!("a/{path}"), &format!("b/{path}"))
        ));
        result.files.push(DiffFile {
            path,
            additions,
            deletions,
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn content() -> SonarIssueContent {
        SonarIssueContent {
            repository_name: "app".into(),
            issue: json!({"key":"one", "project":"acme_app", "component":"acme_app:src/app.rs", "rule":"rust:S1", "line":2, "textRange":{"startLine":2,"endLine":3}, "status":"OPEN"}),
            rule: None,
            rule_html: "<p>Use a guard.</p>".into(),
            rule_url: "https://sonar.example/rules".into(),
            reference_documentation: false,
            source: vec![
                SonarSourceLine {
                    line: 1,
                    code: "fn main() {".into(),
                },
                SonarSourceLine {
                    line: 2,
                    code: "  work();".into(),
                },
            ],
            comparison: Some(SonarSourceComparison {
                base_branch: "main".into(),
                original: "old\n".into(),
                current: "new\n".into(),
            }),
            errors: vec![],
        }
    }

    #[test]
    fn sonar_keys_preserve_case_and_distinguish_projects_and_branches() {
        let first = Item {
            provider: "sonar".into(),
            id: "one".into(),
            sonar_project: "acme_app".into(),
            sonar_branch: "Main".into(),
            ..Default::default()
        };
        let mut second = first.clone();
        second.sonar_branch = "main".into();
        assert_ne!(first.key(), second.key());
        second.sonar_branch = first.sonar_branch.clone();
        second.sonar_project = "other".into();
        assert_ne!(first.key(), second.key());
        assert_ne!(branch_key("a:b", "c"), branch_key("a", "b:c"));
    }

    #[test]
    fn native_branch_filter_ignores_checkout_scope_and_filters_locally() {
        let item = Item {
            provider: "sonar".into(),
            sonar_project: "app".into(),
            sonar_branch: "main".into(),
            project_path: "/other/checkout".into(),
            ..Default::default()
        };
        let mut filters = super::super::Filters {
            hidden_projects: vec!["/other/checkout".into()],
            ..Default::default()
        };
        assert!(filters.matches(&item, "sonar", ""));
        filters
            .hidden_sonar_branches
            .push(branch_key("app", "main"));
        assert!(!filters.matches(&item, "sonar", ""));
        filters.hidden_sonar_branches.clear();
        filters.status.closed = true;
        assert!(!filters.matches(&item, "sonar", ""));
        assert!(filters.matches(
            &Item {
                state: "ACCEPTED".into(),
                ..item
            },
            "sonar",
            ""
        ));
    }

    #[test]
    fn guidance_drops_active_html_preserves_code_and_uses_safe_fences() {
        let text = html_markdown("<h2>Why</h2><script>secret()</script><style>hidden</style><iframe>remote</iframe><img src='https://evil.test/pixel'><p>Use <strong>a guard</strong> &amp; retry.</p><pre><code>if (x &lt; 3) {\n```\n}</code></pre>");
        assert!(text.contains("## Why"));
        assert!(text.contains("**a guard** & retry"));
        assert!(text.contains("````\nif (x < 3) {\n```\n}\n````"));
        for forbidden in ["secret", "hidden", "remote", "evil.test", "<script>"] {
            assert!(!text.contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn details_include_guidance_comments_source_ranges_and_partial_failures() {
        let mut content = content();
        content.issue["comments"] =
            json!([{"login":"author","htmlText":"<p>Review the guard.</p>"}]);
        content.errors.push("Source comparison unavailable".into());
        content.rule = Some(
            json!({"descriptionSections":[{"key":"how_to_fix","content":"<pre>guard();</pre>"}]}),
        );
        content.reference_documentation = true;
        let body = body(&content);
        for expected in [
            ">     2 |   work();",
            "Use a guard.",
            "how_to_fix",
            "guard();",
            "Latest reference documentation",
            "Review the guard.",
            "Source comparison unavailable",
        ] {
            assert!(body.contains(expected), "{expected}");
        }
    }

    #[test]
    fn source_and_example_diffs_are_separate_and_correct() {
        let mut content = content();
        content.rule_html = r#"<pre data-diff-type="noncompliant" data-diff-id="one">bad();</pre><pre data-diff-id='one' data-diff-type='compliant'>good();</pre><pre data-diff-id="incomplete" data-diff-type="compliant">ignored</pre>"#.into();
        let diff = diff(&content);
        assert_eq!(diff.files.len(), 2);
        assert_eq!(diff.files[0].path, "src/app.rs");
        assert_eq!(diff.files[1].path, "Rule example 1 (reference only)");
        assert!(diff.patch.contains("-bad();"));
        assert!(diff.patch.contains("+good();"));
        assert_eq!((diff.additions, diff.deletions), (2, 2));
    }

    #[test]
    fn git_remote_name_wins_over_checkout_folder_and_current_project() {
        let dir = tempfile::tempdir().unwrap();
        let alias = dir.path().join("alias");
        let named = dir.path().join("app");
        std::fs::create_dir_all(&alias).unwrap();
        std::fs::create_dir_all(&named).unwrap();
        super::super::run_git(&alias, &["init"]).unwrap();
        super::super::run_git(
            &alias,
            &[
                "config",
                "remote.origin.url",
                "git@example.test:acme/app.git",
            ],
        )
        .unwrap();
        assert_eq!(
            project(&[named.clone(), alias.clone()], &named, "app"),
            alias
        );
        assert_eq!(
            project(std::slice::from_ref(&named), &named, "missing"),
            named
        );
    }
    #[test]
    fn display_names_match_before_falling_back_to_the_current_project() {
        let current = PathBuf::from("/work/current");
        let alias = PathBuf::from("/work/alias");
        let appearances = std::collections::HashMap::from([(
            alias.to_string_lossy().into_owned(),
            crate::project_appearance::ProjectAppearance {
                name: "Application".into(),
                ..Default::default()
            },
        )]);
        assert_eq!(
            project_with_names(
                &[current.clone(), alias.clone()],
                &current,
                "application",
                &appearances
            ),
            alias
        );
    }
}
