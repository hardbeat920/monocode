//! Cloning projects from Git hosting services. Each service implements
//! [`GitHost`]; the reuse-or-clone rules below are shared by all of them and
//! mirror `src/features/git-hosts/model/remoteUrl.ts` and `host/git-hosts.ts`
//! so connected machines decide the same way.

mod github;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use serde::Serialize;

use crate::fs::expand_home;

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitHostStatus {
    pub provider: &'static str,
    pub installed: bool,
    pub authenticated: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitHostRepo {
    pub provider: &'static str,
    pub slug: String,
    pub description: Option<String>,
    pub private: bool,
    pub pushed_at: Option<String>,
    /// The signed-in account's own namespace owns this repository, as
    /// opposed to an organization or another collaborator's.
    pub mine: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutPlan {
    pub path: String,
    pub reuse: bool,
}

/// A Git hosting service projects can be cloned from.
pub(crate) trait GitHost: Sync {
    fn id(&self) -> &'static str;
    /// The host name its remotes point at, used to recognize existing checkouts.
    fn domain(&self) -> &'static str;
    fn status(&self) -> GitHostStatus;
    /// Repositories the signed-in account can reach, most recently pushed first.
    fn repos(&self) -> Result<Vec<GitHostRepo>, String>;
    /// Public repositories anywhere on the service matching `query`, most
    /// relevant first. Not limited to the signed-in account's own reach.
    fn search(&self, query: &str) -> Result<Vec<GitHostRepo>, String>;
    /// Clones `slug` into `dest`, which is missing or empty.
    fn clone_into(&self, slug: &str, dest: &Path) -> Result<(), String>;
}

static PROVIDERS: &[&dyn GitHost] = &[&github::GitHub];

fn provider(id: &str) -> Result<&'static dyn GitHost, String> {
    PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id() == id)
        .ok_or_else(|| format!("Unknown Git host: {id}"))
}

/// Which services are installed and signed in on this computer.
#[tauri::command]
pub async fn git_host_statuses() -> Result<Vec<GitHostStatus>, String> {
    tauri::async_runtime::spawn_blocking(|| PROVIDERS.iter().map(|p| p.status()).collect())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn git_host_repos(provider: String) -> Result<Vec<GitHostRepo>, String> {
    tauri::async_runtime::spawn_blocking(move || self::provider(&provider)?.repos())
        .await
        .map_err(|error| error.to_string())?
}

/// Public repositories matching `query`, searched live instead of listed up
/// front since the signed-in account's own reach does not bound them.
#[tauri::command]
pub async fn git_host_search_repos(
    provider: String,
    query: String,
) -> Result<Vec<GitHostRepo>, String> {
    tauri::async_runtime::spawn_blocking(move || self::provider(&provider)?.search(&query))
        .await
        .map_err(|error| error.to_string())?
}

/// The usual code folder in the home directory, or home itself.
#[tauri::command]
pub async fn git_host_default_parent() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(default_parent)
        .await
        .map_err(|error| error.to_string())?
}

fn default_parent() -> Result<String, String> {
    let home = PathBuf::from(crate::dirs_home().ok_or("Could not find the home folder")?);
    // Exact names, so a case-insensitive disk does not report `code` as `Code`.
    let names: Vec<String> = match std::fs::read_dir(&home) {
        Ok(entries) => entries
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .collect(),
        Err(_) => Vec::new(),
    };
    let parent = ["Code", "code", "src", "Projects", "projects", "dev"]
        .into_iter()
        .find(|name| names.iter().any(|entry| entry == name) && home.join(name).is_dir())
        .map(|name| home.join(name))
        .unwrap_or(home);
    Ok(parent.to_string_lossy().into_owned())
}

/// Where `slug` would be checked out under `parent`, without cloning.
#[tauri::command]
pub async fn git_host_checkout_plan(
    provider: String,
    slug: String,
    parent: String,
) -> Result<CheckoutPlan, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = self::provider(&provider)?;
        plan_checkout(host.domain(), &slug, &expand_home(&parent))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Opens an existing checkout of `slug` under `parent`, or clones it there.
#[tauri::command]
pub async fn git_host_checkout(
    provider: String,
    slug: String,
    parent: String,
) -> Result<CheckoutPlan, String> {
    tauri::async_runtime::spawn_blocking(move || {
        checkout(self::provider(&provider)?, &slug, &expand_home(&parent))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Folders being cloned into. A clone writes its remote before fetching, so
/// without this a second request could open, or clone beside, a half-written one.
static CLONING: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

struct CloneClaim(PathBuf);

impl CloneClaim {
    fn take(path: &Path) -> Result<Self, String> {
        let mut cloning = CLONING.lock().map_err(|error| error.to_string())?;
        if cloning.iter().any(|claimed| claimed == path) {
            return Err(format!("Already cloning into {}", path.display()));
        }
        cloning.push(path.to_path_buf());
        Ok(Self(path.to_path_buf()))
    }
}

impl Drop for CloneClaim {
    fn drop(&mut self) {
        if let Ok(mut cloning) = CLONING.lock() {
            cloning.retain(|claimed| claimed != &self.0);
        }
    }
}

fn being_cloned(path: &Path) -> bool {
    let Ok(cloning) = CLONING.lock() else {
        return false;
    };
    cloning.iter().any(|claimed| claimed == path)
}

fn checkout(host: &dyn GitHost, slug: &str, parent: &Path) -> Result<CheckoutPlan, String> {
    let plan = plan_checkout(host.domain(), slug, parent)?;
    if !plan.reuse {
        let dest = PathBuf::from(&plan.path);
        let _claim = CloneClaim::take(&dest)?;
        host.clone_into(slug, &dest)?;
    }
    Ok(plan)
}

const MAX_CHECKOUT_CANDIDATES: usize = 100;

#[derive(Debug, PartialEq, Eq)]
enum FolderState {
    /// Missing or empty, so it can be cloned into.
    Free,
    /// A complete checkout of the repository.
    Match,
    Taken,
}

/// The folder for `slug` under `parent`: its name, or `name-2`, `name-3`…
/// when that name holds something else. Matches `planCheckoutFolder` in
/// `src/features/git-hosts/model/remoteUrl.ts`.
fn plan_checkout(domain: &str, slug: &str, parent: &Path) -> Result<CheckoutPlan, String> {
    let base = repo_folder_name(slug)
        .filter(|_| valid_repo_slug(slug))
        .ok_or("Enter a repository as owner/name")?;
    if !parent.is_absolute() {
        return Err("Choose an absolute folder to clone into".into());
    }
    if !parent.is_dir() {
        return Err(format!("{} is not a folder", parent.display()));
    }
    for index in 1..=MAX_CHECKOUT_CANDIDATES {
        let name = if index == 1 {
            base.to_string()
        } else {
            format!("{base}-{index}")
        };
        let path = parent.join(&name);
        if being_cloned(&path) {
            return Err(format!("Already cloning into {}", path.display()));
        }
        let reuse = match folder_state(&path, domain, slug) {
            FolderState::Taken => continue,
            FolderState::Match => true,
            FolderState::Free => false,
        };
        return Ok(CheckoutPlan {
            path: path.to_string_lossy().into_owned(),
            reuse,
        });
    }
    Err(format!(
        "Too many folders named {base}; choose another location"
    ))
}

fn folder_state(path: &Path, domain: &str, slug: &str) -> FolderState {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return FolderState::Free;
    };
    if !metadata.is_dir() {
        return FolderState::Taken;
    }
    let Ok(mut entries) = std::fs::read_dir(path) else {
        return FolderState::Taken;
    };
    if entries.next().is_none() {
        return FolderState::Free;
    }
    // Only the checkout's own root counts, not a folder inside another repository.
    if !path.join(".git").exists() {
        return FolderState::Taken;
    }
    let remotes = git_stdout(path, &["remote", "-v"]).unwrap_or_default();
    let tracks = remotes
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .any(|url| remote_matches(url, domain, slug));
    if !tracks {
        return FolderState::Taken;
    }
    // An interrupted clone has the remote but no commit checked out.
    let head = git_stdout(path, &["rev-parse", "--verify", "-q", "HEAD"]);
    if head.is_some() {
        FolderState::Match
    } else {
        FolderState::Taken
    }
}

fn git_stdout(path: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new("git");
    crate::hide_window_console(&mut cmd);
    let output = cmd
        .arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn strip_git_suffix(name: &str) -> &str {
    let cut = name.len().saturating_sub(4);
    match name.get(cut..) {
        Some(suffix) if suffix.eq_ignore_ascii_case(".git") => &name[..cut],
        _ => name,
    }
}

/// The folder a repository clones into, or `None` when its name cannot be
/// one on every platform: `.`, `..`, a trailing dot, or a Windows device name.
fn repo_folder_name(slug: &str) -> Option<&str> {
    let name = strip_git_suffix(slug.split_once('/')?.1);
    let stem = name.split('.').next().unwrap_or(name).to_ascii_lowercase();
    let numbered = stem.len() == 4
        && (stem.starts_with("com") || stem.starts_with("lpt"))
        && stem.as_bytes()[3].is_ascii_digit();
    let reserved = numbered || matches!(stem.as_str(), "con" | "prn" | "aux" | "nul");
    if name.is_empty() || name.ends_with('.') || reserved {
        None
    } else {
        Some(name)
    }
}

fn valid_repo_slug(value: &str) -> bool {
    let parts: Vec<&str> = value.split('/').collect();
    parts.len() == 2
        && repo_folder_name(value).is_some()
        && parts.iter().all(|part| {
            part.len() <= 100
                && !part.chars().all(|c| c == '.')
                && !part.starts_with('-')
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
        })
}

/// `(host, path)` of a Git remote URL: the host lowercased, the path as
/// written without slashes or `.git`. Reads `https://`, `ssh://`, `git://`
/// and scp-like `git@host:owner/repo` forms.
fn parse_remote_url(url: &str) -> Option<(String, String)> {
    let value = url.trim();
    let (host, path) = if let Some((_, rest)) = value.split_once("://") {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = without_user(authority);
        let host = host.split_once(':').map_or(host, |(host, _)| host);
        (host, path)
    } else {
        let (authority, path) = value.split_once(':')?;
        let host = without_user(authority);
        if host.len() < 2 || authority.contains('/') {
            return None;
        }
        (host, path)
    };
    let path = strip_git_suffix(path.trim_matches('/'));
    if host.is_empty() || path.is_empty() {
        return None;
    }
    Some((host.to_ascii_lowercase(), path.to_string()))
}

fn without_user(authority: &str) -> &str {
    authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host)
}

fn remote_matches(url: &str, domain: &str, slug: &str) -> bool {
    let slug = strip_git_suffix(slug);
    parse_remote_url(url).is_some_and(|(host, path)| {
        host.eq_ignore_ascii_case(domain) && path.eq_ignore_ascii_case(slug)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_parent(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "monocode-git-hosts-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn git(path: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }

    /// A checkout with one commit, like a finished clone.
    fn git_repo(path: &Path, remote: &str) {
        std::fs::create_dir_all(path).unwrap();
        git(path, &["init", "-q"]);
        git(path, &["remote", "add", "origin", remote]);
        git(
            path,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "initial",
            ],
        );
    }

    struct FakeHost {
        clones: Mutex<Vec<PathBuf>>,
    }

    impl FakeHost {
        fn new() -> Self {
            Self {
                clones: Mutex::new(Vec::new()),
            }
        }
    }

    impl GitHost for FakeHost {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn domain(&self) -> &'static str {
            "github.com"
        }
        fn status(&self) -> GitHostStatus {
            GitHostStatus {
                provider: "fake",
                installed: true,
                authenticated: true,
            }
        }
        fn repos(&self) -> Result<Vec<GitHostRepo>, String> {
            Ok(Vec::new())
        }
        fn search(&self, _query: &str) -> Result<Vec<GitHostRepo>, String> {
            Ok(Vec::new())
        }
        fn clone_into(&self, _slug: &str, dest: &Path) -> Result<(), String> {
            // A second request for the same folder is refused while this runs.
            let parent = dest.parent().unwrap();
            assert!(plan_checkout("github.com", "owner/repo", parent).is_err());
            self.clones.lock().unwrap().push(dest.to_path_buf());
            Ok(())
        }
    }

    #[test]
    fn parse_remote_url_reads_common_forms() {
        let expected = Some(("github.com".to_string(), "owner/repo".to_string()));
        for url in [
            "https://GitHub.com/owner/repo.git",
            "https://token@github.com/owner/repo/",
            "ssh://git@github.com:22/owner/repo.git",
            "git@github.com:owner/repo.GIT",
        ] {
            assert_eq!(parse_remote_url(url), expected, "{url}");
        }
        assert_eq!(parse_remote_url("/srv/git/repo.git"), None);
        assert_eq!(parse_remote_url("C:\\repos\\repo"), None);
    }

    #[test]
    fn remote_matches_ignores_case() {
        let url = "git@github.com:Owner/Repo.git";
        assert!(remote_matches(url, "github.com", "owner/repo"));
        assert!(remote_matches(url, "github.com", "owner/repo.GIT"));
        assert!(!remote_matches(url, "github.com", "owner/other"));
        assert!(!remote_matches(url, "gitlab.com", "owner/repo"));
    }

    #[test]
    fn valid_repo_slug_accepts_owner_and_name_only() {
        assert!(valid_repo_slug("hardbeat920/monocode"));
        assert!(valid_repo_slug("o/.github"));
        for slug in [
            "monocode",
            "a/b/c",
            "a/..",
            "-o/x",
            "o/x y",
            "o/.git",
            "o/..git",
            "o/repo.",
            "o/con",
            "o/NUL.txt",
            "o/com1",
        ] {
            assert!(!valid_repo_slug(slug), "{slug}");
        }
        assert_eq!(repo_folder_name("o/Repo.GIT"), Some("Repo"));
    }

    #[test]
    fn checkout_clones_into_a_free_folder() {
        let parent = temp_parent("free");
        let host = FakeHost::new();
        let plan = checkout(&host, "owner/repo", &parent).unwrap();
        assert!(!plan.reuse);
        assert_eq!(plan.path, parent.join("repo").to_string_lossy());
        assert_eq!(*host.clones.lock().unwrap(), vec![parent.join("repo")]);
        // The claim is released once the clone returns.
        assert!(!being_cloned(&parent.join("repo")));
        std::fs::create_dir(parent.join("repo")).unwrap();
        let plan = plan_checkout("github.com", "owner/repo", &parent).unwrap();
        assert!(!plan.reuse);
        std::fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn checkout_reuses_a_matching_folder() {
        let parent = temp_parent("match");
        git_repo(&parent.join("repo"), "git@github.com:Owner/Repo.git");
        let host = FakeHost::new();
        let plan = checkout(&host, "owner/repo", &parent).unwrap();
        assert!(plan.reuse);
        assert_eq!(plan.path, parent.join("repo").to_string_lossy());
        assert!(host.clones.lock().unwrap().is_empty());
        std::fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn checkout_skips_an_unfinished_clone() {
        let parent = temp_parent("partial");
        let path = parent.join("repo");
        let remote = "https://github.com/owner/repo";
        std::fs::create_dir_all(&path).unwrap();
        git(&path, &["init", "-q"]);
        git(&path, &["remote", "add", "origin", remote]);
        let plan = plan_checkout("github.com", "owner/repo", &parent).unwrap();
        assert_eq!(plan.path, parent.join("repo-2").to_string_lossy());
        assert!(!plan.reuse);
        std::fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn checkout_steps_past_folders_that_hold_something_else() {
        let parent = temp_parent("taken");
        git_repo(&parent.join("repo"), "https://github.com/someone/else.git");
        std::fs::create_dir(parent.join("repo-2")).unwrap();
        std::fs::write(parent.join("repo-2").join("notes.txt"), "x").unwrap();
        let plan = plan_checkout("github.com", "owner/repo", &parent).unwrap();
        assert_eq!(plan.path, parent.join("repo-3").to_string_lossy());
        assert!(!plan.reuse);

        git_repo(&parent.join("repo-3"), "https://github.com/owner/repo");
        let plan = plan_checkout("github.com", "owner/repo", &parent).unwrap();
        assert_eq!(plan.path, parent.join("repo-3").to_string_lossy());
        assert!(plan.reuse);
        std::fs::remove_dir_all(parent).unwrap();
    }

    #[test]
    fn plan_checkout_rejects_bad_input() {
        let parent = temp_parent("bad");
        let plan = |slug, parent: &Path| plan_checkout("github.com", slug, parent);
        assert!(plan("../etc", &parent).is_err());
        assert!(plan("owner/..git", &parent).is_err());
        assert!(plan("owner/repo", Path::new("relative")).is_err());
        assert!(plan("owner/repo", &parent.join("missing")).is_err());
        std::fs::remove_dir_all(parent).unwrap();
    }
}
