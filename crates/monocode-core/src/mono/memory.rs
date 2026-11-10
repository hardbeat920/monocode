//! Dated memory edits, secret redaction and budgeted prompt context.
use super::files;
use chrono::{Local, NaiveDate};
use regex::Regex;
use serde::Serialize;
use std::{path::Path, sync::LazyLock};
pub const MAX_LINES: usize = 200;
pub const MAX_BYTES: usize = 24 * 1024;
static SECRETS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----",
        r"\b(?:sk|pk|rk)-[A-Za-z0-9_-]{16,}",
        r"\bgh[pousr]_[A-Za-z0-9]{20,}",
        r"\bgithub_pat_[A-Za-z0-9_]{20,}",
        r"\bxox[abprs]-[A-Za-z0-9-]{10,}",
        r"\bAKIA[0-9A-Z]{16}\b",
        r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
        r"(?i)\b(?:password|passwd|secret|token|api[_-]?key)\s*[:=]\s*\S+",
    ]
    .iter()
    .map(|p| Regex::new(p).unwrap())
    .collect()
});
pub fn redact(text: &str) -> String {
    SECRETS.iter().fold(text.into(), |s, re| {
        re.replace_all(&s, |c: &regex::Captures<'_>| {
            format!("«redacted {} chars»", c[0].chars().count())
        })
        .into_owned()
    })
}
pub fn date() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}
pub fn entry(fact: &str, day: &str, until: Option<&str>) -> Result<String, String> {
    let fact = redact(fact)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if fact.is_empty() || fact.chars().count() > 1000 {
        return Err("A memory fact must contain 1–1000 characters".into());
    }
    if until.is_some_and(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err()) {
        return Err("until must be a date like 2026-10-31".into());
    }
    Ok(format!(
        "- {day} · {fact}{}",
        until.map(|d| format!(" · until {d}")).unwrap_or_default()
    ))
}
pub fn topic(value: &str) -> Result<String, String> {
    let name = value.trim().trim_end_matches(".md");
    if name.is_empty()
        || name.len() > 80
        || name.starts_with('.')
        || name.eq_ignore_ascii_case("archive")
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " ._-".contains(c))
    {
        return Err("Use a topic name containing letters, numbers, spaces, dots or dashes".into());
    }
    Ok(name.into())
}
pub fn within_budget(text: &str) -> String {
    let mut bytes = 0;
    text.lines()
        .take(MAX_LINES)
        .take_while(|line| {
            bytes += line.len() + 1;
            bytes <= MAX_BYTES
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn dated(line: &str) -> bool {
    let line = line
        .strip_prefix("- ")
        .unwrap_or(line)
        .trim_start_matches("~~");
    line.get(..10)
        .is_some_and(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok())
        && line.get(10..).is_some_and(|s| s.starts_with(" · "))
}
pub fn fact(line: &str) -> String {
    let line = line.trim_start_matches("- ").trim_start_matches("~~");
    let line = if dated(&format!("- {line}")) {
        line.split_once(" · ").map(|(_, rest)| rest).unwrap_or(line)
    } else {
        line
    };
    line.split(" · until ")
        .next()
        .unwrap_or(line)
        .split(" · superseded ")
        .next()
        .unwrap_or(line)
        .replace("~~", "")
        .trim()
        .into()
}
pub fn edit_line(text: &str, original: &str, replacement: Option<&str>) -> Result<String, String> {
    let mut rows = lines(text);
    let found = rows
        .iter()
        .enumerate()
        .filter(|(_, l)| l.as_str() == original)
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    let [index] = found.as_slice() else {
        return Err("Memory changed; reload before editing this fact".into());
    };
    if let Some(replacement) = replacement {
        rows[*index] = entry(replacement, &date(), None)?;
    } else {
        rows.remove(*index);
    }
    Ok(join(&rows))
}
fn lines(text: &str) -> Vec<String> {
    if text.trim().is_empty() {
        Vec::new()
    } else {
        text.trim_end().lines().map(str::to_string).collect()
    }
}
fn join(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    }
}
pub fn add(text: &str, entry: &str) -> String {
    let mut rows = lines(text);
    let wanted = fact(entry);
    if !rows
        .iter()
        .any(|line| !line.contains("~~") && fact(line) == wanted)
    {
        rows.push(entry.into());
    }
    join(&rows)
}
pub fn replace(text: &str, find: &str, entry: Option<&str>, day: &str) -> Result<String, String> {
    if find.trim().is_empty() {
        return Err("find must not be empty".into());
    }
    let mut rows = lines(text);
    let found: Vec<_> = rows
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains(find.trim()) && !line.contains("~~"))
        .map(|(i, _)| i)
        .collect();
    let [index] = found.as_slice() else {
        return Err("Read memory and quote text matching exactly one live entry".into());
    };
    if let Some(entry) = entry {
        let old = rows[*index].trim_start_matches("- ");
        rows.splice(
            *index..=*index,
            [format!("- ~~{old}~~ · superseded {day}"), entry.into()],
        );
    } else {
        rows.remove(*index);
    }
    Ok(join(&rows))
}
pub fn fit(text: &str, keep: Option<&str>, day: &str) -> (String, Vec<String>) {
    let mut rows = lines(text);
    let mut moved = Vec::new();
    for pass in 0..3 {
        loop {
            if rows.len() <= MAX_LINES && join(&rows).len() <= MAX_BYTES {
                break;
            }
            let pick = rows.iter().position(|line| {
                dated(line)
                    && Some(line.as_str()) != keep
                    && match pass {
                        0 => line.contains("~~"),
                        1 => line
                            .split(" · until ")
                            .nth(1)
                            .and_then(|s| s.get(..10))
                            .is_some_and(|d| d < day),
                        _ => true,
                    }
            });
            let Some(index) = pick else {
                break;
            };
            moved.push(rows.remove(index));
        }
    }
    (join(&rows), moved)
}
pub fn read(root: &Path, id: &str, topic_name: Option<&str>) -> Result<files::AgentFile, String> {
    if topic_name == Some("archive") {
        return files::read(root, id, None, "memory/archive.md");
    }
    let path = topic_name
        .map(|t| topic(t).map(|t| format!("memory/{t}.md")))
        .transpose()?
        .unwrap_or_else(|| "MEMORY.md".into());
    files::read(root, id, None, &path)
}
pub fn edit(
    root: &Path,
    id: &str,
    topic_name: Option<&str>,
    change: impl Fn(&str) -> Result<String, String>,
) -> Result<String, String> {
    let path = topic_name
        .map(|t| topic(t).map(|t| format!("memory/{t}.md")))
        .transpose()?
        .unwrap_or_else(|| "MEMORY.md".into());
    for _ in 0..5 {
        let file = files::read(root, id, None, &path)?;
        let changed = change(file.text.as_deref().unwrap_or_default())?;
        let (text, moved) = if topic_name.is_none() {
            fit(&changed, changed.lines().last(), &date())
        } else {
            (changed, Vec::new())
        };
        if !moved.is_empty() {
            let archive = files::read(root, id, None, "memory/archive.md")?;
            let mut rows = lines(archive.text.as_deref().unwrap_or("# Archive\n"));
            for line in moved {
                let line = format!("{line} · moved {}", date());
                if !rows.contains(&line) {
                    rows.push(line);
                }
            }
            files::save(
                root,
                id,
                None,
                "memory/archive.md",
                &join(&rows),
                Some(&archive.hash),
            )?;
        }
        match files::save(root, id, None, &path, &text, Some(&file.hash)) {
            Ok(hash) => return Ok(hash),
            Err(e) if e == files::CONFLICT => continue,
            Err(e) => return Err(e),
        }
    }
    Err("Memory kept changing; try again".into())
}
#[derive(Serialize)]
pub struct Hit {
    pub file: String,
    pub line: String,
    pub date: Option<String>,
}
pub fn search(root: &Path, id: &str, query: &str, since: Option<&str>) -> Result<Vec<Hit>, String> {
    let words:Vec<_>=query.to_lowercase().split(|c:char|!c.is_alphanumeric()&&!"_.-".contains(c)).filter(|w|w.len()>=3&&!"the and for are was were with that this what when where which who how why did does have has had you your our not but can will from into about there their them then than just any all its".split_whitespace().any(|stop|stop==*w)).map(str::to_owned).collect();
    if words.is_empty() {
        return Err("Use a few specific words to search memory".into());
    }
    let since = since
        .map(|value| {
            if NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok() {
                Ok(value.to_string())
            } else {
                let (digits, unit) = value.split_at(
                    value
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0),
                );
                let count = digits
                    .parse::<i64>()
                    .map_err(|_| "since must be YYYY-MM-DD, 7d, 24h or 2w")?;
                if !(0..=876_000).contains(&count) {
                    return Err("since must be a recent positive duration");
                }
                let hours = match unit {
                    "h" => count,
                    "d" => count * 24,
                    "w" => count * 168,
                    _ => return Err("since must be YYYY-MM-DD, 7d, 24h or 2w"),
                };
                Ok((Local::now() - chrono::Duration::hours(hours))
                    .format("%Y-%m-%d")
                    .to_string())
            }
        })
        .transpose()?;
    let loaded = files::load(root, id, None)?;
    let mut paths = vec!["MEMORY.md".into(), "memory/archive.md".into()];
    paths.extend(loaded.topics.iter().map(|t| format!("memory/{t}.md")));
    let mut hits = Vec::new();
    for file in paths {
        let text = files::read(root, id, None, &file)?.text.unwrap_or_default();
        for line in text.lines() {
            let lower = line.to_lowercase();
            if line.contains("~~") || !words.iter().all(|w| lower.contains(w)) {
                continue;
            }
            let at = dated(line).then(|| line.trim_start_matches("- ")[..10].to_string());
            if since
                .as_ref()
                .is_some_and(|since| at.as_ref().is_none_or(|at| at < since))
            {
                continue;
            }
            hits.push(Hit {
                file: file.clone(),
                line: line.into(),
                date: at,
            });
            if hits.len() == 50 {
                return Ok(hits);
            }
        }
    }
    Ok(hits)
}
