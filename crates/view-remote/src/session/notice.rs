//! The status line over a remote session and the path mapping for files it
//! opens. Ports of the `notice` and `hostFilePath` code in RemoteSession.tsx.

use monocode_layout::paths::{parse_remote_path, remote_path};

/// What the engine reports about a host session's requests: the inputs of
/// the `notice` expression in RemoteSession.tsx.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteSessionStatus {
    /// A command waits for the host to confirm it (`pending`). It may have
    /// reached the host, so it is retried with its own command ID.
    pub pending: bool,
    /// A command is being dispatched now (`sending`).
    pub sending: bool,
    /// The first message or a draft did not reach the host
    /// (`starting.failed`).
    pub failed_turn: Option<FailedTurn>,
    /// The last request's error, without its `Error: ` prefix.
    pub error: String,
    /// Why the host's model list did not load:
    /// `catalog.errors[harness] ?? catalogError`.
    pub catalog_problem: String,
}

/// A turn that failed before the host accepted it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FailedTurn {
    /// It was a draft, not a message.
    pub draft: bool,
}

/// What the notice's button does. The engine acts on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeAction {
    /// `retryPending`: dispatch the pending command again with its ID.
    RetryPending,
    /// Send the failed first message or draft again.
    TryAgain,
    /// Clear the error.
    Dismiss,
    /// Load the host's model list again.
    RetryCatalog,
}

impl NoticeAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::RetryPending | Self::RetryCatalog => "Retry",
            Self::TryAgain => "Try again",
            Self::Dismiss => "Dismiss",
        }
    }
}

/// The line over the transcript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteNotice {
    pub text: String,
    /// Shown dimmer after the text, and as its tooltip.
    pub detail: String,
    pub action: NoticeAction,
    /// `role="alert"` when an error is set, else `role="status"`.
    pub alert: bool,
}

impl RemoteNotice {
    /// The button works only while the machine answers, except Dismiss.
    pub fn action_enabled(&self, online: bool) -> bool {
        online || self.action == NoticeAction::Dismiss
    }
}

/// `notice`: the request waiting on the host comes first, then a failed
/// first turn, then any error, then a model list that did not load.
pub fn remote_notice(status: &RemoteSessionStatus, machine_name: &str) -> Option<RemoteNotice> {
    let alert = !status.error.is_empty();
    if status.pending && !status.sending {
        return Some(RemoteNotice {
            text: "Waiting for the host to confirm your request.".into(),
            detail: status.error.clone(),
            action: NoticeAction::RetryPending,
            alert,
        });
    }
    if let Some(turn) = status.failed_turn {
        let what = if turn.draft {
            "save the draft"
        } else {
            "send the message"
        };
        return Some(RemoteNotice {
            text: format!("Couldn’t {what} on {machine_name}."),
            detail: status.error.clone(),
            action: NoticeAction::TryAgain,
            alert,
        });
    }
    if alert {
        return Some(RemoteNotice {
            text: status.error.clone(),
            detail: String::new(),
            action: NoticeAction::Dismiss,
            alert,
        });
    }
    if !status.catalog_problem.is_empty() {
        return Some(RemoteNotice {
            text: format!("Couldn’t load models from {machine_name}."),
            detail: status.catalog_problem.clone(),
            action: NoticeAction::RetryCatalog,
            alert,
        });
    }
    None
}

/// `hostFilePath`: the `remote://` path for a file the transcript or the
/// composer names. Relative paths are under the session's host checkout.
pub fn host_file_path(environment_id: &str, execution_cwd: &str, path: &str) -> String {
    if parse_remote_path(path).is_some() {
        return path.to_string();
    }
    let bytes = path.as_bytes();
    let windows_absolute = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');
    let absolute = if path.starts_with('/') || path.starts_with("\\\\") || windows_absolute {
        path.to_string()
    } else {
        format!(
            "{}/{}",
            execution_cwd.trim_end_matches(['/', '\\']),
            path.strip_prefix("./").unwrap_or(path)
        )
    };
    remote_path(environment_id, &absolute)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> RemoteSessionStatus {
        RemoteSessionStatus::default()
    }

    #[test]
    fn does_not_flash_a_status_banner_while_an_ordinary_message_is_in_flight() {
        let sending = RemoteSessionStatus {
            pending: true,
            sending: true,
            ..status()
        };
        assert_eq!(remote_notice(&sending, "Home server"), None);
        assert_eq!(remote_notice(&status(), "Home server"), None);
    }

    #[test]
    fn waits_for_the_host_to_confirm_an_uncertain_request() {
        let pending = RemoteSessionStatus {
            pending: true,
            error: "Machine is unreachable".into(),
            ..status()
        };
        let notice = remote_notice(&pending, "Home server").unwrap();
        assert_eq!(notice.text, "Waiting for the host to confirm your request.");
        assert_eq!(notice.detail, "Machine is unreachable");
        assert_eq!(notice.action, NoticeAction::RetryPending);
        assert_eq!(notice.action.label(), "Retry");
        assert!(notice.alert);
        assert!(!notice.action_enabled(false));
        assert!(notice.action_enabled(true));
    }

    #[test]
    fn offers_to_try_a_failed_first_turn_again() {
        let failed = RemoteSessionStatus {
            failed_turn: Some(FailedTurn { draft: false }),
            ..status()
        };
        let notice = remote_notice(&failed, "Home server").unwrap();
        assert_eq!(notice.text, "Couldn’t send the message on Home server.");
        assert_eq!(notice.action.label(), "Try again");
        assert!(!notice.alert);
        let draft = RemoteSessionStatus {
            failed_turn: Some(FailedTurn { draft: true }),
            ..status()
        };
        assert_eq!(
            remote_notice(&draft, "Home server").unwrap().text,
            "Couldn’t save the draft on Home server."
        );
    }

    #[test]
    fn an_error_can_be_dismissed_while_offline() {
        let failed = RemoteSessionStatus {
            error: "Select that model in the composer before building this remote plan.".into(),
            catalog_problem: "codex is not installed".into(),
            ..status()
        };
        let notice = remote_notice(&failed, "Home server").unwrap();
        assert_eq!(notice.action, NoticeAction::Dismiss);
        assert_eq!(notice.detail, "");
        assert!(notice.action_enabled(false));
    }

    #[test]
    fn reports_a_model_list_that_did_not_load() {
        let catalog = RemoteSessionStatus {
            catalog_problem: "codex is not installed".into(),
            ..status()
        };
        let notice = remote_notice(&catalog, "Home server").unwrap();
        assert_eq!(notice.text, "Couldn’t load models from Home server.");
        assert_eq!(notice.detail, "codex is not installed");
        assert_eq!(notice.action, NoticeAction::RetryCatalog);
        assert!(!notice.alert);
    }

    #[test]
    fn maps_transcript_paths_into_the_host_checkout() {
        let map = |path: &str| host_file_path("env", "/home/me/repo/", path);
        assert_eq!(map("src/app.ts"), "remote://env/home/me/repo/src/app.ts");
        assert_eq!(map("./src/app.ts"), "remote://env/home/me/repo/src/app.ts");
        assert_eq!(map("/etc/hosts"), "remote://env/etc/hosts");
        assert_eq!(map("C:\\repo\\a.ts"), "remote://env/C:/repo/a.ts");
        assert_eq!(
            map("remote://env/home/me/repo/README.md"),
            "remote://env/home/me/repo/README.md"
        );
    }
}
