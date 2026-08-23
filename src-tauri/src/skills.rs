use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::dirs_home;
use crate::fs::expand_home;

const MAX_SKILLS: usize = 300;
const MAX_FRONTMATTER_BYTES: usize = 16 * 1024;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredSkill {
    pub name: String,
    pub description: String,
    pub path: String,
    pub scope: String,
    pub source: String,
}

/// Skills visible for the open project: `.agents/skills` first, then native
/// harness folders. Same name: earlier roots win.
#[tauri::command]
pub fn list_skills(cwd: String) -> Result<Vec<DiscoveredSkill>, String> {
    let project = expand_home(&cwd);
    let home = dirs_home().map(PathBuf::from);
    Ok(list_skills_from(&project, home.as_deref()))
}

pub(crate) fn list_skills_from(project: &Path, home: Option<&Path>) -> Vec<DiscoveredSkill> {
    let mut by_name: HashMap<String, DiscoveredSkill> = HashMap::new();
    let mut seen_roots: HashSet<PathBuf> = HashSet::new();

    let mut add_root = |root: PathBuf, scope: &str, source: &str| {
        if by_name.len() >= MAX_SKILLS {
            return;
        }
        let key = std::fs::canonicalize(&root).unwrap_or(root.clone());
        if !seen_roots.insert(key) {
            return;
        }
        for skill in scan_root(&root, scope, source) {
            if by_name.len() >= MAX_SKILLS {
                break;
            }
            by_name.entry(skill.name.clone()).or_insert(skill);
        }
    };

    // Highest priority first so later roots cannot replace a name.
    add_root(project.join(".agents/skills"), "project", "agents");
    if let Some(home) = home {
        add_root(home.join(".agents/skills"), "user", "agents");
        // Amp uses ~/.config/agents/skills and ~/.config/amp/skills for personal skills.
        add_root(home.join(".config/agents/skills"), "user", "agents");
        add_root(home.join(".config/amp/skills"), "user", "amp");
    }

    for (dir, source) in [
        (".claude/skills", "claude"),
        (".cursor/skills", "cursor"),
        (".codex/skills", "codex"),
        (".opencode/skills", "opencode"),
        (".pi/skills", "pi"),
        (".fx/skills", "fx"),
    ] {
        add_root(project.join(dir), "project", source);
        if let Some(home) = home {
            add_root(home.join(dir), "user", source);
        }
    }
    if let Some(home) = home {
        add_root(home.join(".pi/agent/skills"), "user", "pi");
    }

    let mut out: Vec<DiscoveredSkill> = by_name.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn scan_root(root: &Path, scope: &str, source: &str) -> Vec<DiscoveredSkill> {
    let Ok(reader) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ent in reader.flatten() {
        let dir = ent.path();
        if !dir.is_dir() {
            continue;
        }
        let Some(folder) = dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if folder.starts_with('.') || folder == "skills-cursor" {
            continue;
        }
        let skill_md = skill_md_path(&dir);
        let Some(skill_md) = skill_md else { continue };
        let Ok(bytes) = read_prefix(&skill_md, MAX_FRONTMATTER_BYTES) else {
            continue;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let fallback = slug_name(folder);
        if fallback.is_empty() {
            continue;
        }
        let (name, description) = parse_frontmatter(&text, &fallback);
        if name.is_empty() {
            continue;
        }
        out.push(DiscoveredSkill {
            name,
            description,
            path: skill_md.to_string_lossy().into_owned(),
            scope: scope.to_string(),
            source: source.to_string(),
        });
    }
    out
}

fn skill_md_path(dir: &Path) -> Option<PathBuf> {
    let upper = dir.join("SKILL.md");
    if upper.is_file() {
        return Some(upper);
    }
    let lower = dir.join("skill.md");
    if lower.is_file() {
        return Some(lower);
    }
    None
}

fn read_prefix(path: &Path, max: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut buf = Vec::with_capacity(max.min(4096));
    file.take(max as u64).read_to_end(&mut buf)?;
    Ok(buf)
}

fn parse_frontmatter(text: &str, fallback: &str) -> (String, String) {
    let trimmed = text.trim_start_matches('\u{feff}');
    let Some(rest) = trimmed.strip_prefix("---") else {
        return (fallback.to_string(), String::new());
    };
    let rest = rest.strip_prefix('\r').unwrap_or(rest);
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let end = rest
        .find("\n---")
        .or_else(|| rest.find("\r\n---"))
        .unwrap_or(rest.len());
    let yaml = &rest[..end];

    let mut name: Option<String> = None;
    let mut description = String::new();
    let mut in_desc = false;
    let mut fold_desc = false;

    for raw in yaml.lines() {
        if in_desc {
            if is_yaml_indent(raw) {
                let piece = raw.trim();
                if piece.is_empty() {
                    continue;
                }
                if !description.is_empty() {
                    description.push(if fold_desc { ' ' } else { '\n' });
                }
                description.push_str(piece);
                continue;
            }
            in_desc = false;
        }

        let line = raw.trim_end();
        if let Some(value) = yaml_value(line, "name") {
            name = Some(unquote(&value));
        } else if let Some(value) = yaml_value(line, "description") {
            let value = value.trim();
            if is_folded_scalar(value) {
                in_desc = true;
                fold_desc = value.starts_with('>');
                description.clear();
            } else {
                description = unquote(value);
            }
        }
    }

    let folder = fallback.to_string();
    let name = name.filter(|n| is_valid_skill_name(n)).unwrap_or(folder);
    (name, description.trim().to_string())
}

fn yaml_value(line: &str, key: &str) -> Option<String> {
    let line = line.trim_start();
    let prefix = format!("{key}:");
    line.strip_prefix(&prefix)
        .map(|rest| rest.trim().to_string())
}

fn is_folded_scalar(value: &str) -> bool {
    value.starts_with('>') || value.starts_with('|')
}

fn is_yaml_indent(line: &str) -> bool {
    line.starts_with(' ') || line.starts_with('\t')
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

fn is_valid_skill_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }
    let mut prev_dash = true;
    for (i, ch) in name.chars().enumerate() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            prev_dash = false;
            continue;
        }
        if ch == '-' && i > 0 && !prev_dash {
            prev_dash = true;
            continue;
        }
        return false;
    }
    !prev_dash
}

fn slug_name(raw: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.len() > 64 {
        out.truncate(64);
        while out.ends_with('-') {
            out.pop();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tmp(label: &str) -> Tmp {
        loop {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "monocode-skills-{label}-{}-{stamp}-{seq}",
                std::process::id()
            ));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Tmp(dir),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("{}", error),
            }
        }
    }

    fn write_skill(root: &Path, folder: &str, body: &str) {
        let dir = root.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), body).unwrap();
    }

    #[test]
    fn parse_frontmatter_reads_name_and_folded_description() {
        let (name, desc) = parse_frontmatter(
            "---\nname: review-pr\ndescription: >\n  Review pull requests.\n  Use when asked to review.\n---\n\n# hi\n",
            "fallback",
        );
        assert_eq!(name, "review-pr");
        assert_eq!(desc, "Review pull requests. Use when asked to review.");
    }

    #[test]
    fn parse_frontmatter_falls_back_to_folder_name() {
        let (name, desc) = parse_frontmatter("# no yaml\n", "create-skill");
        assert_eq!(name, "create-skill");
        assert_eq!(desc, "");
    }

    #[test]
    fn agents_skills_win_over_provider_dirs() {
        let project = tmp("proj");
        let home = tmp("home");
        write_skill(
            &project.0.join(".agents/skills"),
            "ship",
            "---\nname: ship\ndescription: MonoCode ship\n---\n",
        );
        write_skill(
            &project.0.join(".claude/skills"),
            "ship",
            "---\nname: ship\ndescription: Claude ship\n---\n",
        );
        write_skill(
            &home.0.join(".agents/skills"),
            "greet",
            "---\nname: greet\ndescription: Hello\n---\n",
        );
        write_skill(
            &project.0.join(".cursor/skills"),
            "cursor-only",
            "---\nname: cursor-only\ndescription: Cursor native\n---\n",
        );

        let skills = list_skills_from(&project.0, Some(&home.0));
        let ship = skills.iter().find(|s| s.name == "ship").unwrap();
        assert_eq!(ship.description, "MonoCode ship");
        assert_eq!(ship.source, "agents");
        assert_eq!(ship.scope, "project");

        let greet = skills.iter().find(|s| s.name == "greet").unwrap();
        assert_eq!(greet.scope, "user");
        assert_eq!(greet.source, "agents");

        let native = skills.iter().find(|s| s.name == "cursor-only").unwrap();
        assert_eq!(native.source, "cursor");
        assert_eq!(native.scope, "project");
    }

    #[test]
    fn discovers_pi_project_and_user_skills() {
        let project = tmp("proj-pi");
        let home = tmp("home-pi");
        write_skill(
            &project.0.join(".pi/skills"),
            "pi-review",
            "---\nname: pi-review\ndescription: Pi project skill\n---\n",
        );
        write_skill(
            &home.0.join(".pi/agent/skills"),
            "pi-global",
            "---\nname: pi-global\ndescription: Pi user skill\n---\n",
        );

        let skills = list_skills_from(&project.0, Some(&home.0));
        let project_skill = skills.iter().find(|s| s.name == "pi-review").unwrap();
        assert_eq!(project_skill.source, "pi");
        assert_eq!(project_skill.scope, "project");
        let user_skill = skills.iter().find(|s| s.name == "pi-global").unwrap();
        assert_eq!(user_skill.source, "pi");
        assert_eq!(user_skill.scope, "user");
    }

    #[test]
    fn discovers_fx_project_and_user_skills() {
        let project = tmp("proj-fx");
        let home = tmp("home-fx");
        write_skill(
            &project.0.join(".fx/skills"),
            "fx-review",
            "---\nname: fx-review\ndescription: fx project skill\n---\n",
        );
        write_skill(
            &home.0.join(".fx/skills"),
            "fx-global",
            "---\nname: fx-global\ndescription: fx user skill\n---\n",
        );

        let skills = list_skills_from(&project.0, Some(&home.0));
        let project_skill = skills.iter().find(|s| s.name == "fx-review").unwrap();
        assert_eq!(project_skill.source, "fx");
        assert_eq!(project_skill.scope, "project");
        let user_skill = skills.iter().find(|s| s.name == "fx-global").unwrap();
        assert_eq!(user_skill.source, "fx");
        assert_eq!(user_skill.scope, "user");
    }

    #[test]
    fn skips_dirs_without_skill_md() {
        let project = tmp("empty");
        std::fs::create_dir_all(project.0.join(".agents/skills/nope")).unwrap();
        let skills = list_skills_from(&project.0, None);
        assert!(skills.is_empty());
    }
}
