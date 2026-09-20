//! Read-only discovery for the connection picker. Draft targets are never
//! persisted until the user opens a folder. SSH remains the config resolver.
use crate::{
    connections::{validate_profile, ConnectionProfile},
    remote,
};
use serde::Serialize;
use std::{collections::BTreeSet, fs, path::Path, time::Duration};
use tauri::AppHandle;

fn words(value: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    for ch in value.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            (None, c) if c.is_whitespace() => {
                if !current.is_empty() {
                    result.push(std::mem::take(&mut current));
                }
            }
            (_, c) => current.push(c),
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

// Only enumerate literal aliases; do not run ssh -G (Match exec can execute
// commands). Include patterns are expanded relative to ~/.ssh, as OpenSSH does.
fn ssh_hosts(
    path: &Path,
    ssh_dir: &Path,
    depth: usize,
    hosts: &mut BTreeSet<String>,
) -> Result<(), String> {
    if depth > 16 {
        return Ok(());
    }
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("Could not read SSH config: {e}")),
    };
    for line in raw.lines() {
        let line = line.trim();
        let split = line.find(|c: char| c.is_whitespace() || c == '=');
        let Some(at) = split else { continue };
        let key = &line[..at];
        let rest = line[at..].trim_start().trim_start_matches('=').trim_start();
        let words = words(rest);
        if key.eq_ignore_ascii_case("host") {
            for host in words.into_iter().take_while(|word| !word.starts_with('#')) {
                if !host.is_empty()
                    && !host.starts_with(['!', '-'])
                    && !host.contains(['*', '?', '[', ']'])
                    && !host.chars().any(char::is_whitespace)
                {
                    hosts.insert(host);
                }
            }
        } else if key.eq_ignore_ascii_case("include") {
            for include in words.into_iter().take_while(|word| !word.starts_with('#')) {
                let expanded = if let Some(rest) = include.strip_prefix("~/") {
                    ssh_dir.parent().unwrap_or(ssh_dir).join(rest)
                } else if Path::new(&include).is_absolute() {
                    include.clone().into()
                } else {
                    ssh_dir.join(&include)
                };
                if include.contains('*') || include.contains('?') {
                    let Some(parent) = expanded.parent() else {
                        continue;
                    };
                    let pattern = expanded
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("");
                    if let Ok(entries) = fs::read_dir(parent) {
                        for entry in entries.flatten() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            let matched = if pattern == "*.conf" {
                                name.ends_with(".conf")
                            } else {
                                name == pattern
                            };
                            if matched {
                                ssh_hosts(&entry.path(), ssh_dir, depth + 1, hosts)?;
                            }
                        }
                    }
                } else {
                    ssh_hosts(&expanded, ssh_dir, depth + 1, hosts)?;
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn connections_ssh_hosts() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let home = crate::fs::expand_home("~");
        let dir = home.join(".ssh");
        let mut hosts = BTreeSet::new();
        ssh_hosts(&dir.join("config"), &dir, 0, &mut hosts)?;
        Ok(hosts.into_iter().collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Container {
    id: String,
    name: String,
    image: String,
    status: String,
}

fn parse_containers(raw: &str) -> Result<Vec<Container>, String> {
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: serde_json::Value =
                serde_json::from_str(line).map_err(|e| format!("Invalid Docker response: {e}"))?;
            let field = |key| {
                value
                    .get(key)
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .ok_or_else(|| format!("Docker response is missing {key}"))
            };
            Ok(Container {
                id: field("ID")?,
                name: field("Names")?,
                image: field("Image")?,
                status: field("Status")?,
            })
        })
        .collect()
}

#[tauri::command]
pub async fn connections_containers(
    app: AppHandle,
    mut profile: ConnectionProfile,
) -> Result<Vec<Container>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        profile.container = None; // Docker is queried on the host, never nested.
        validate_profile(&profile)?;
        let argv = ["docker", "ps", "--format", "{{json .}}"].map(str::to_string);
        let mut command = remote::remote_exec(&app, &profile, None, false, &argv)?;
        let output = remote::capture_timeout(&mut command, Duration::from_secs(20))?;
        if !output.success {
            return Err(remote::ssh_failure(&profile, &output));
        }
        parse_containers(&output.stdout)
    })
    .await
    .map_err(|e| e.to_string())?
}

// Python is already required by the remote filesystem helper. JSON preserves
// spaces, quotes and Unicode names. Paths are argv, never shell interpolation.
const BROWSE: &str = r#"import json, os, sys
p = os.path.abspath(os.path.expanduser(sys.argv[1] or '~'))
with os.scandir(p) as entries:
    names = sorted([e.name for e in entries if e.is_dir()], key=str.casefold)
print(json.dumps({'path': p, 'home': os.path.expanduser('~'), 'directories': names}))
"#;

#[derive(Serialize, serde::Deserialize)]
pub struct DirectoryListing {
    path: String,
    home: String,
    directories: Vec<String>,
}

#[tauri::command]
pub async fn connections_browse(
    app: AppHandle,
    profile: ConnectionProfile,
    path: String,
) -> Result<DirectoryListing, String> {
    tauri::async_runtime::spawn_blocking(move || {
        validate_profile(&profile)?;
        if !path.is_empty() && !path.starts_with('/') && path != "~" && !path.starts_with("~/") {
            return Err("Choose an absolute path or a path beginning with ~/".into());
        }
        let argv = vec!["python3".into(), "-c".into(), BROWSE.into(), path];
        let mut command = remote::remote_exec(&app, &profile, None, false, &argv)?;
        let output = remote::capture_timeout(&mut command, Duration::from_secs(20))?;
        if !output.success {
            return Err(remote::ssh_failure(&profile, &output));
        }
        serde_json::from_str(&output.stdout)
            .map_err(|e| format!("Could not read directory list: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ssh_aliases_include_globs_quotes_and_cycles_without_patterns() {
        let root = std::env::temp_dir().join(format!("mono-ssh-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("conf.d")).unwrap();
        fs::write(root.join("config"), "Host dev-4090 other !excluded *.internal\nHostName ignored\ninclude conf.d/*.conf\nHost=\"quoted\"\n").unwrap();
        fs::write(
            root.join("conf.d/a.conf"),
            "Host container-host\nInclude config\nHost dev-4090 # comment\n",
        )
        .unwrap();
        let mut hosts = BTreeSet::new();
        ssh_hosts(&root.join("config"), &root, 0, &mut hosts).unwrap();
        assert_eq!(
            hosts.into_iter().collect::<Vec<_>>(),
            ["container-host", "dev-4090", "other", "quoted"]
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn docker_rows_preserve_names_and_reject_errors() {
        let rows = parse_containers(
            r#"{"ID":"abc","Names":"kineai-dev","Image":"image:latest","Status":"Up 3 hours"}"#,
        )
        .unwrap();
        assert_eq!(rows[0].name, "kineai-dev");
        assert!(parse_containers("").unwrap().is_empty());
        assert!(parse_containers("permission denied").is_err());
    }
}
