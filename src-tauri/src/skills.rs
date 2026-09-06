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
#[tauri::command(async)]
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
    }

    for (dir, source) in [
        (".claude/skills", "claude"),
        (".cursor/skills", "cursor"),
        (".codex/skills", "codex"),
        (".opencode/skills", "opencode"),
        (".pi/skills", "pi"),
        (".omp/skills", "omp"),
        (".fx/skills", "fx"),
        (".grok/skills", "grok"),
    ] {
        add_root(project.join(dir), "project", source);
        if let Some(home) = home {
            add_root(home.join(dir), "user", source);
        }
    }
    if let Some(home) = home {
        add_root(home.join(".pi/agent/skills"), "user", "pi");
        add_root(home.join(".omp/agent/skills"), "user", "omp");
        for (root, scope, namespace) in claude_plugin_skill_roots(home, project) {
            add_namespaced_root(&mut by_name, root, scope, "claude", &namespace);
        }
    }

    let mut out: Vec<DiscoveredSkill> = by_name.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn add_namespaced_root(
    by_name: &mut HashMap<String, DiscoveredSkill>,
    root: PathBuf,
    scope: &str,
    source: &str,
    namespace: &str,
) {
    if by_name.len() >= MAX_SKILLS {
        return;
    }
    for mut skill in scan_root(&root, scope, source) {
        if by_name.len() >= MAX_SKILLS {
            break;
        }
        skill.name = format!("{namespace}:{}", skill.name);
        by_name.entry(skill.name.clone()).or_insert(skill);
    }
}

fn claude_plugin_skill_roots(home: &Path, project: &Path) -> Vec<(PathBuf, &'static str, String)> {
    let registry = home.join(".claude/plugins/installed_plugins.json");
    let Ok(raw) = std::fs::read_to_string(registry) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    let Some(plugins) = value.get("plugins").and_then(|value| value.as_object()) else {
        return Vec::new();
    };

    let mut roots = Vec::new();
    for (plugin_id, installed) in plugins {
        if !claude_plugin_enabled(home, project, plugin_id) {
            continue;
        }
        let namespace = plugin_id
            .rsplit_once('@')
            .map(|(name, _)| name)
            .unwrap_or(plugin_id);
        if !is_valid_skill_name(namespace) {
            continue;
        }
        let entries: Vec<&serde_json::Value> = match installed.as_array() {
            Some(entries) => entries.iter().collect(),
            None if installed.is_object() => vec![installed],
            None => continue,
        };
        for entry in entries {
            let Some(install_path) = entry.get("installPath").and_then(|value| value.as_str())
            else {
                continue;
            };
            let scope = match entry.get("scope").and_then(|value| value.as_str()) {
                Some("project" | "local") => {
                    let Some(project_path) =
                        entry.get("projectPath").and_then(|value| value.as_str())
                    else {
                        continue;
                    };
                    if !path_is_within(project, &resolve_home_path(project_path, home)) {
                        continue;
                    }
                    "project"
                }
                Some("user") | None => "user",
                Some(_) => continue,
            };
            roots.push((
                resolve_home_path(install_path, home).join("skills"),
                scope,
                namespace.to_string(),
            ));
        }
    }
    roots.sort_by_key(|(_, scope, _)| if *scope == "project" { 0 } else { 1 });
    roots
}

fn resolve_home_path(raw: &str, home: &Path) -> PathBuf {
    if raw == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.join(rest);
    }
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        home.join(path)
    }
}

fn claude_plugin_enabled(home: &Path, project: &Path, plugin_id: &str) -> bool {
    if let Some(enabled) = managed_plugin_setting(plugin_id) {
        return enabled;
    }
    let project_root = claude_settings_project_root(project);
    for settings in [
        project_root.join(".claude/settings.local.json"),
        project_root.join(".claude/settings.json"),
        home.join(".claude/settings.json"),
    ] {
        if let Some(enabled) = plugin_setting(&settings, plugin_id) {
            return enabled;
        }
    }
    true
}

fn claude_settings_project_root(project: &Path) -> PathBuf {
    for candidate in project.ancestors() {
        let claude = candidate.join(".claude");
        if claude.join("settings.local.json").is_file() || claude.join("settings.json").is_file() {
            return candidate.to_path_buf();
        }
    }
    project.to_path_buf()
}

fn plugin_setting(path: &Path, plugin_id: &str) -> Option<bool> {
    let raw = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    value.get("enabledPlugins")?.get(plugin_id)?.as_bool()
}

fn managed_plugin_setting(plugin_id: &str) -> Option<bool> {
    let root = managed_settings_root()?;
    managed_plugin_setting_from_root(&root, plugin_id)
}

fn managed_plugin_setting_from_root(root: &Path, plugin_id: &str) -> Option<bool> {
    let mut value = plugin_setting(&root.join("managed-settings.json"), plugin_id);
    let dir = root.join("managed-settings.d");
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|ext| ext.to_str()) == Some("json")
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| !name.starts_with('.'))
        })
        .collect();
    files.sort();
    for file in files {
        if let Some(enabled) = plugin_setting(&file, plugin_id) {
            value = Some(enabled);
        }
    }
    value
}

fn managed_settings_root() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        return Some(PathBuf::from("/Library/Application Support/ClaudeCode"));
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        return Some(PathBuf::from("/etc/claude-code"));
    }
    #[cfg(target_os = "windows")]
    {
        return Some(PathBuf::from(r"C:\Program Files\ClaudeCode"));
    }
    #[allow(unreachable_code)]
    None
}

fn path_is_within(path: &Path, root: &Path) -> bool {
    let Ok(path) = std::fs::canonicalize(path) else {
        return false;
    };
    let Ok(root) = std::fs::canonicalize(root) else {
        return false;
    };
    path == root || path.starts_with(root)
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
            path: crate::fs::path_to_js(&skill_md),
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

    fn write_plugin_setting(root: &Path, file: &str, plugin_id: &str, enabled: bool) {
        let dir = root.join(".claude");
        std::fs::create_dir_all(&dir).unwrap();
        let settings = serde_json::json!({
            "enabledPlugins": { plugin_id: enabled }
        });
        std::fs::write(dir.join(file), serde_json::to_vec(&settings).unwrap()).unwrap();
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
    fn discovers_grok_project_and_user_skills() {
        let project = tmp("proj-grok");
        let home = tmp("home-grok");
        write_skill(
            &project.0.join(".grok/skills"),
            "grok-review",
            "---\nname: grok-review\ndescription: grok project skill\n---\n",
        );
        write_skill(
            &home.0.join(".grok/skills"),
            "grok-global",
            "---\nname: grok-global\ndescription: grok user skill\n---\n",
        );

        let skills = list_skills_from(&project.0, Some(&home.0));
        let project_skill = skills.iter().find(|s| s.name == "grok-review").unwrap();
        assert_eq!(project_skill.source, "grok");
        assert_eq!(project_skill.scope, "project");
        let user_skill = skills.iter().find(|s| s.name == "grok-global").unwrap();
        assert_eq!(user_skill.source, "grok");
        assert_eq!(user_skill.scope, "user");
    }

    #[test]
    fn discovers_installed_claude_plugin_skills() {
        let project = tmp("proj-claude-plugin");
        let home = tmp("home-claude-plugin");
        let plugin = home
            .0
            .join(".claude/plugins/cache/community/workflow-kit/1.2.3");
        write_skill(
            &plugin.join("skills"),
            "quick-plan",
            "---\nname: quick-plan\ndescription: Plan from plugin\n---\n",
        );
        write_skill(
            &home.0.join(".claude/skills"),
            "quick-plan",
            "---\nname: quick-plan\ndescription: Personal plan\n---\n",
        );
        std::fs::create_dir_all(home.0.join(".claude/plugins")).unwrap();
        std::fs::write(
            home.0.join(".claude/plugins/installed_plugins.json"),
            r#"{"version":2,"plugins":{"workflow-kit@community":[{"scope":"user","installPath":"~/.claude/plugins/cache/community/workflow-kit/1.2.3","version":"1.2.3"}]}}"#,
        )
        .unwrap();

        let skills = list_skills_from(&project.0, Some(&home.0));
        let skill = skills
            .iter()
            .find(|skill| skill.name == "workflow-kit:quick-plan")
            .unwrap();
        assert_eq!(skill.description, "Plan from plugin");
        assert_eq!(skill.source, "claude");
        assert_eq!(skill.scope, "user");
        assert!(skill
            .path
            .ends_with("workflow-kit/1.2.3/skills/quick-plan/SKILL.md"));
        assert!(skills.iter().any(|skill| skill.name == "quick-plan"));
    }

    #[test]
    fn claude_project_plugins_only_apply_to_their_project() {
        let project = tmp("proj-claude-scoped");
        let other = tmp("other-claude-scoped");
        let home = tmp("home-claude-scoped");
        let plugin = home
            .0
            .join(".claude/plugins/cache/community/workflow-kit/2.0.0");
        write_skill(
            &plugin.join("skills"),
            "feature-delivery",
            "---\nname: feature-delivery\ndescription: Deliver feature\n---\n",
        );
        std::fs::create_dir_all(home.0.join(".claude/plugins")).unwrap();
        let registry = serde_json::json!({
            "version": 2,
            "plugins": {
                "workflow-kit@community": [{
                    "scope": "project",
                    "projectPath": project.0.to_string_lossy(),
                    "installPath": plugin.to_string_lossy(),
                    "version": "2.0.0"
                }]
            }
        });
        std::fs::write(
            home.0.join(".claude/plugins/installed_plugins.json"),
            serde_json::to_vec(&registry).unwrap(),
        )
        .unwrap();

        let nested = project.0.join("src");
        std::fs::create_dir_all(&nested).unwrap();
        let matching = list_skills_from(&nested, Some(&home.0));
        let skill = matching
            .iter()
            .find(|skill| skill.name == "workflow-kit:feature-delivery")
            .unwrap();
        assert_eq!(skill.scope, "project");

        let unrelated = list_skills_from(&other.0, Some(&home.0));
        assert!(!unrelated
            .iter()
            .any(|skill| skill.name == "workflow-kit:feature-delivery"));
    }

    #[test]
    fn disabled_claude_plugin_skills_are_hidden() {
        let project = tmp("proj-claude-disabled");
        let home = tmp("home-claude-disabled");
        let plugin = home
            .0
            .join(".claude/plugins/cache/community/workflow-kit/1.2.3");
        write_skill(
            &plugin.join("skills"),
            "quick-plan",
            "---\nname: quick-plan\ndescription: Plan from plugin\n---\n",
        );
        std::fs::create_dir_all(home.0.join(".claude/plugins")).unwrap();
        std::fs::write(
            home.0.join(".claude/plugins/installed_plugins.json"),
            r#"{"version":2,"plugins":{"workflow-kit@community":[{"scope":"user","installPath":"~/.claude/plugins/cache/community/workflow-kit/1.2.3","version":"1.2.3"}]}}"#,
        )
        .unwrap();
        write_plugin_setting(&home.0, "settings.json", "workflow-kit@community", false);

        let skills = list_skills_from(&project.0, Some(&home.0));
        assert!(!skills
            .iter()
            .any(|skill| skill.name == "workflow-kit:quick-plan"));
    }

    #[test]
    fn claude_plugin_enablement_uses_local_project_user_precedence() {
        let project = tmp("proj-claude-precedence");
        let nested = project.0.join("src");
        let home = tmp("home-claude-precedence");
        std::fs::create_dir_all(&nested).unwrap();
        write_plugin_setting(&home.0, "settings.json", "workflow-kit@community", false);
        write_plugin_setting(&project.0, "settings.json", "workflow-kit@community", true);
        assert!(claude_plugin_enabled(
            &home.0,
            &nested,
            "workflow-kit@community"
        ));
        write_plugin_setting(
            &project.0,
            "settings.local.json",
            "workflow-kit@community",
            false,
        );
        assert!(!claude_plugin_enabled(
            &home.0,
            &nested,
            "workflow-kit@community"
        ));
    }

    #[test]
    fn managed_plugin_settings_apply_dropins_in_order() {
        let root = tmp("managed-settings");
        let plugin_id = "workflow-kit@community";
        let base = serde_json::json!({ "enabledPlugins": { plugin_id: true } });
        std::fs::write(
            root.0.join("managed-settings.json"),
            serde_json::to_vec(&base).unwrap(),
        )
        .unwrap();
        let dropins = root.0.join("managed-settings.d");
        std::fs::create_dir_all(&dropins).unwrap();
        let earlier = serde_json::json!({ "enabledPlugins": { plugin_id: true } });
        let later = serde_json::json!({ "enabledPlugins": { plugin_id: false } });
        std::fs::write(
            dropins.join("10-enable.json"),
            serde_json::to_vec(&earlier).unwrap(),
        )
        .unwrap();
        std::fs::write(
            dropins.join("20-disable.json"),
            serde_json::to_vec(&later).unwrap(),
        )
        .unwrap();

        assert_eq!(
            managed_plugin_setting_from_root(&root.0, plugin_id),
            Some(false)
        );
    }

    #[test]
    fn path_is_within_fails_closed_when_either_path_is_missing() {
        let root = tmp("path-root");
        let nested = root.0.join("src");
        std::fs::create_dir_all(&nested).unwrap();
        assert!(path_is_within(&nested, &root.0));
        assert!(!path_is_within(&root.0.join("missing"), &root.0));
        assert!(!path_is_within(&nested, &root.0.join("missing")));
    }

    #[test]
    fn ignores_unregistered_claude_plugin_cache_versions() {
        let project = tmp("proj-claude-stale");
        let home = tmp("home-claude-stale");
        let stale = home
            .0
            .join(".claude/plugins/cache/community/workflow-kit/0.9.0");
        write_skill(
            &stale.join("skills"),
            "stale-skill",
            "---\nname: stale-skill\ndescription: Old cached skill\n---\n",
        );
        std::fs::create_dir_all(home.0.join(".claude/plugins")).unwrap();
        std::fs::write(
            home.0.join(".claude/plugins/installed_plugins.json"),
            r#"{"version":2,"plugins":{}}"#,
        )
        .unwrap();

        let skills = list_skills_from(&project.0, Some(&home.0));
        assert!(!skills.iter().any(|skill| skill.name == "stale-skill"));
    }

    #[test]
    fn skips_dirs_without_skill_md() {
        let project = tmp("empty");
        std::fs::create_dir_all(project.0.join(".agents/skills/nope")).unwrap();
        let skills = list_skills_from(&project.0, None);
        assert!(skills.is_empty());
    }
}
