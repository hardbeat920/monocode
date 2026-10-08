//! GitHub through the GitHub CLI, which owns sign-in and clone credentials.

use std::path::Path;

use serde::Deserialize;

use super::{GitHost, GitHostRepo, GitHostStatus};

pub(crate) struct GitHub;

/// Pages of 100 repositories listed, most recently pushed first. Anything
/// older can still be typed as `owner/name`.
const REPO_PAGES: u32 = 3;
const HOST: &str = "github.com";
const REPOS_ENDPOINT: &str =
    "user/repos?affiliation=owner,collaborator,organization_member&sort=pushed&per_page=100";
const REPO_FIELDS: &str = ".[] | {slug: .full_name, description, private, pushedAt: .pushed_at}";

impl GitHost for GitHub {
    fn id(&self) -> &'static str {
        "github"
    }

    fn domain(&self) -> &'static str {
        HOST
    }

    fn status(&self) -> GitHostStatus {
        let status = crate::fs::git_github_status_for();
        GitHostStatus {
            provider: self.id(),
            installed: status.installed,
            authenticated: status.authenticated,
        }
    }

    fn repos(&self) -> Result<Vec<GitHostRepo>, String> {
        let mut repos = Vec::new();
        for page in 1..=REPO_PAGES {
            let endpoint = format!("{REPOS_ENDPOINT}&page={page}");
            // Pinned to github.com so `GH_HOST` cannot point it at another server.
            let args = ["api", "--hostname", HOST, endpoint.as_str(), "--jq", REPO_FIELDS];
            let output = crate::fs::gh_run(Path::new("."), &args, true)?;
            let listed = parse_repos(&output)?;
            let last = listed.len() < 100;
            repos.extend(listed);
            if last {
                break;
            }
        }
        Ok(repos)
    }

    fn clone_into(&self, slug: &str, dest: &Path) -> Result<(), String> {
        let parent = dest.parent().unwrap_or(dest);
        let repo = format!("{HOST}/{slug}");
        let dest = dest.to_string_lossy();
        crate::fs::gh_run(parent, &["repo", "clone", &repo, &dest], true).map(|_| ())
    }
}

fn parse_repos(lines: &str) -> Result<Vec<GitHostRepo>, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Repo {
        slug: String,
        description: Option<String>,
        private: bool,
        pushed_at: Option<String>,
    }
    lines
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| -> Result<GitHostRepo, String> {
            let repo: Repo = serde_json::from_str(line).map_err(|error| error.to_string())?;
            Ok(GitHostRepo {
                provider: "github",
                slug: repo.slug,
                description: repo.description.filter(|text| !text.trim().is_empty()),
                private: repo.private,
                pushed_at: repo.pushed_at,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_repos_reads_jq_lines() {
        let repos = parse_repos(
            "{\"slug\":\"o/a\",\"description\":\"\",\"private\":true,\"pushedAt\":\"2026-01-01T00:00:00Z\"}\n\
             {\"slug\":\"o/b\",\"description\":\"B\",\"private\":false,\"pushedAt\":null}\n",
        )
        .unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].slug, "o/a");
        assert_eq!(repos[0].description, None);
        assert!(repos[0].private);
        assert_eq!(repos[1].description.as_deref(), Some("B"));
        assert_eq!(repos[1].pushed_at, None);
    }
}
