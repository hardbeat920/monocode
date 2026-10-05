//! Port of the pure half of src/features/connections/model/remoteTurns.ts.
//! `RemoteSessions` runs the `useRemoteTurnUpdates` effect.

use monocode_remote::host::protocol::SessionChange;

/// `RemoteChangesDetail`: one batch of session writes from a watched
/// machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteChangesDetail {
    pub machine_id: String,
    pub sessions: Vec<SessionChange>,
    /// The host restarted or the desktop fell behind; reload what is shown.
    pub reset: bool,
}

/// `RemoteTurnTarget`: an open tab that shows a host session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTurnTarget {
    pub shell_id: String,
    pub machine_id: String,
    pub host_session_id: String,
    pub busy: bool,
}

/// What `remote_turn_updates` found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RemoteTurnUpdates {
    /// Tabs whose turn started elsewhere.
    pub started: Vec<String>,
    /// Tabs whose turn finished; each loads its final snapshot.
    pub finished: Vec<RemoteTurnTarget>,
}

/// `remoteTurnUpdates`: what a batch of host writes means for open tabs: a
/// turn started in a tab that still looks idle, or a turn finished in a tab
/// that still looks busy. After a host restart every busy tab is rechecked.
pub fn remote_turn_updates(
    targets: &[RemoteTurnTarget],
    detail: &RemoteChangesDetail,
) -> RemoteTurnUpdates {
    let mut updates = RemoteTurnUpdates::default();
    for target in targets {
        if target.machine_id != detail.machine_id {
            continue;
        }
        if detail.reset {
            if target.busy {
                updates.finished.push(target.clone());
            }
            continue;
        }
        // The host sends each session's latest write once per batch.
        let Some(busy) = detail
            .sessions
            .iter()
            .find(|entry| entry.id == target.host_session_id)
            .and_then(|change| change.busy)
        else {
            continue;
        };
        if busy && !target.busy {
            updates.started.push(target.shell_id.clone());
        } else if !busy && target.busy {
            updates.finished.push(target.clone());
        }
    }
    updates
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_remote::host::protocol::HostSessionStatus;

    fn tab(shell_id: &str, busy: bool, machine_id: &str) -> RemoteTurnTarget {
        RemoteTurnTarget {
            shell_id: shell_id.into(),
            machine_id: machine_id.into(),
            host_session_id: format!("host-{shell_id}"),
            busy,
        }
    }

    fn change(id: &str, revision: i64, busy: Option<bool>) -> SessionChange {
        SessionChange {
            id: id.into(),
            project_id: "p".into(),
            revision,
            deleted: None,
            status: busy.map(|busy| {
                if busy {
                    HostSessionStatus::Running
                } else {
                    HostSessionStatus::Idle
                }
            }),
            busy,
        }
    }

    // remoteTurns.test.ts
    #[test]
    fn marks_a_turn_that_started_elsewhere_and_reloads_one_that_finished() {
        let targets = [
            tab("a", false, "mini"),
            tab("b", true, "mini"),
            tab("c", true, "mini"),
            tab("d", true, "other"),
        ];
        let detail = RemoteChangesDetail {
            machine_id: "mini".into(),
            reset: false,
            sessions: vec![
                change("host-a", 2, Some(true)),
                change("host-b", 9, Some(false)),
                // Still streaming: nothing to do.
                change("host-c", 4, Some(true)),
                // Another machine's session with the same host ID is not this tab's.
                change("host-d", 1, Some(false)),
            ],
        };
        assert_eq!(
            remote_turn_updates(&targets, &detail),
            RemoteTurnUpdates {
                started: vec!["a".into()],
                finished: vec![tab("b", true, "mini")],
            }
        );
    }

    // remoteTurns.test.ts
    #[test]
    fn ignores_writes_from_hosts_that_do_not_report_turn_state() {
        let detail = RemoteChangesDetail {
            machine_id: "mini".into(),
            reset: false,
            sessions: vec![change("host-a", 3, None)],
        };
        assert_eq!(
            remote_turn_updates(&[tab("a", true, "mini")], &detail),
            RemoteTurnUpdates::default()
        );
    }

    // remoteTurns.test.ts
    #[test]
    fn rechecks_every_busy_tab_after_the_host_restarts() {
        let detail = RemoteChangesDetail {
            machine_id: "mini".into(),
            reset: true,
            sessions: Vec::new(),
        };
        assert_eq!(
            remote_turn_updates(&[tab("a", true, "mini"), tab("b", false, "mini")], &detail),
            RemoteTurnUpdates {
                started: Vec::new(),
                finished: vec![tab("a", true, "mini")],
            }
        );
    }
}
