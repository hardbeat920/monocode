//! Whether the active GitHub CLI account has starred MonoCode, and starring it.
use std::path::PathBuf;

pub const REPOSITORY_URL: &str = "https://github.com/hardbeat920/monocode";
const STAR_ENDPOINT: &str = "/user/starred/hardbeat920/monocode";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StarStatus {
    Starred,
    NotStarred,
    /// `gh` is missing, signed out or offline; the prompt stays out of the way.
    Unavailable,
}

/// Blocks on `gh`; call from a background task.
pub fn status() -> StarStatus {
    status_from_result(crate::inbox::run_gh(
        &root(),
        &["api", "--silent", STAR_ENDPOINT],
        true,
    ))
}

/// Blocks on `gh`; call from a background task.
pub fn star() -> Result<(), String> {
    crate::inbox::run_gh(
        &root(),
        &["api", "--silent", "--method", "PUT", STAR_ENDPOINT],
        true,
    )
    .map(|_| ())
}

fn root() -> PathBuf {
    crate::scm::shell::dirs_home()
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

fn status_from_result(result: Result<String, String>) -> StarStatus {
    match result {
        Ok(_) => StarStatus::Starred,
        Err(error) if error.contains("HTTP 404") => StarStatus::NotStarred,
        Err(_) => StarStatus::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_a_missing_star_from_an_unavailable_check() {
        assert_eq!(status_from_result(Ok(String::new())), StarStatus::Starred);
        assert_eq!(
            status_from_result(Err("gh: Not Found (HTTP 404)".into())),
            StarStatus::NotStarred
        );
        assert_eq!(
            status_from_result(Err("GitHub CLI is not installed".into())),
            StarStatus::Unavailable
        );
    }
}
