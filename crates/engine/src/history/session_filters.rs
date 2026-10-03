//! Port of src/features/sessions/model/sessionFilters.ts: the sidebar's
//! archive, provider, time, and status filters and their storage.

use std::collections::HashSet;

use chrono::{Local, LocalResult, TimeZone};
use monocode_core::HarnessId;
use monocode_core::harness::HARNESSES;
use monocode_settings::Kv;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::session_store::SessionSummary;

/// `FILTERS_KEY`.
pub const SESSION_SIDEBAR_FILTERS_KEY: &str = "monocode.sessionSidebarFilters";
/// The key the archive toggle used before the filters existed.
pub const LEGACY_SHOW_ARCHIVED_KEY: &str = "monocode.sessionsShowArchived";

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// `SessionTimeFilter`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SessionTimeFilter {
    #[default]
    #[serde(rename = "all")]
    All,
    #[serde(rename = "today")]
    Today,
    #[serde(rename = "7d")]
    SevenDays,
    #[serde(rename = "30d")]
    ThirtyDays,
}

impl SessionTimeFilter {
    fn parse(value: &Value) -> Option<Self> {
        match value.as_str()? {
            "all" => Some(Self::All),
            "today" => Some(Self::Today),
            "7d" => Some(Self::SevenDays),
            "30d" => Some(Self::ThirtyDays),
            _ => None,
        }
    }
}

/// `SessionStatusFilter`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatusFilter {
    pub working: bool,
    pub needs_approval: bool,
    pub done: bool,
}

/// `SessionSidebarFilters`. `Default` is `DEFAULT_SESSION_SIDEBAR_FILTERS`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSidebarFilters {
    pub show_archived: bool,
    pub hidden_harnesses: Vec<HarnessId>,
    pub time: SessionTimeFilter,
    pub status: SessionStatusFilter,
}

/// `harnessesInSessions`: the providers present, in `HARNESSES` order.
pub fn harnesses_in_sessions(rows: &[SessionSummary]) -> Vec<HarnessId> {
    let seen: HashSet<HarnessId> = rows.iter().map(|row| row.harness).collect();
    HARNESSES
        .iter()
        .copied()
        .filter(|harness| seen.contains(harness))
        .collect()
}

/// `loadSessionSidebarFilters`.
pub fn load_session_sidebar_filters(kv: &Kv) -> SessionSidebarFilters {
    let Some(raw) = kv
        .get_item(SESSION_SIDEBAR_FILTERS_KEY)
        .filter(|raw| !raw.is_empty())
    else {
        let legacy_archived = kv.get_item(LEGACY_SHOW_ARCHIVED_KEY).as_deref() == Some("1");
        return SessionSidebarFilters {
            show_archived: legacy_archived,
            ..SessionSidebarFilters::default()
        };
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw) else {
        return SessionSidebarFilters::default();
    };
    // `JSON.parse("null")` succeeds and the field reads then throw, which the
    // TypeScript catch turned into the defaults.
    if parsed.is_null() {
        return SessionSidebarFilters::default();
    }
    let status = &parsed["status"];
    SessionSidebarFilters {
        show_archived: parsed["showArchived"] == Value::Bool(true),
        hidden_harnesses: parsed["hiddenHarnesses"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .filter_map(|id| id.parse::<HarnessId>().ok())
                    .collect()
            })
            .unwrap_or_default(),
        time: SessionTimeFilter::parse(&parsed["time"]).unwrap_or_default(),
        status: SessionStatusFilter {
            working: status["working"] == Value::Bool(true),
            needs_approval: status["needsApproval"] == Value::Bool(true),
            done: status["done"] == Value::Bool(true),
        },
    }
}

/// `saveSessionSidebarFilters`.
pub fn save_session_sidebar_filters(kv: &Kv, filters: &SessionSidebarFilters) {
    if let Ok(raw) = serde_json::to_string(filters) {
        kv.set_item(SESSION_SIDEBAR_FILTERS_KEY, &raw);
    }
}

/// `hasActiveSessionFilters`.
pub fn has_active_session_filters(filters: &SessionSidebarFilters) -> bool {
    filters.show_archived
        || !filters.hidden_harnesses.is_empty()
        || filters.time != SessionTimeFilter::All
        || filters.status.working
        || filters.status.needs_approval
        || filters.status.done
}

/// `filterSessionsByHarness`.
pub fn filter_sessions_by_harness(
    rows: &[SessionSummary],
    hidden_harnesses: &[HarnessId],
) -> Vec<SessionSummary> {
    if hidden_harnesses.is_empty() {
        return rows.to_vec();
    }
    rows.iter()
        .filter(|row| !hidden_harnesses.contains(&row.harness))
        .cloned()
        .collect()
}

/// `filterSessionsByTime`.
pub fn filter_sessions_by_time(
    rows: &[SessionSummary],
    time: SessionTimeFilter,
    now: i64,
) -> Vec<SessionSummary> {
    if time == SessionTimeFilter::All {
        return rows.to_vec();
    }
    let start = time_filter_start(time, now);
    rows.iter()
        .filter(|row| row.updated_at >= start)
        .cloned()
        .collect()
}

/// `filterSessionsByStatus`: any selected status matches.
pub fn filter_sessions_by_status(
    rows: &[SessionSummary],
    status: SessionStatusFilter,
    busy_ids: &HashSet<String>,
    approval_ids: &HashSet<String>,
    done_ids: &HashSet<String>,
) -> Vec<SessionSummary> {
    if !(status.working || status.needs_approval || status.done) {
        return rows.to_vec();
    }
    rows.iter()
        .filter(|row| {
            (status.working && busy_ids.contains(&row.id))
                || (status.needs_approval && approval_ids.contains(&row.id))
                || (status.done && done_ids.contains(&row.id))
        })
        .cloned()
        .collect()
}

/// `timeFilterStart`: today starts at local midnight.
pub fn time_filter_start(time: SessionTimeFilter, now: i64) -> i64 {
    match time {
        SessionTimeFilter::Today => local_midnight(now),
        SessionTimeFilter::SevenDays => now - 7 * DAY_MS,
        SessionTimeFilter::ThirtyDays => now - 30 * DAY_MS,
        SessionTimeFilter::All => 0,
    }
}

/// `date.setHours(0, 0, 0, 0)` in the local time zone.
fn local_midnight(now: i64) -> i64 {
    let LocalResult::Single(at) = Local.timestamp_millis_opt(now) else {
        return now;
    };
    let Some(midnight) = at.date_naive().and_hms_opt(0, 0, 0) else {
        return now;
    };
    // A day that starts inside a DST gap has no local midnight; JavaScript
    // moves forward to the first valid time.
    match Local.from_local_datetime(&midnight) {
        LocalResult::Single(start) | LocalResult::Ambiguous(start, _) => start.timestamp_millis(),
        LocalResult::None => Local
            .from_local_datetime(&(midnight + chrono::Duration::hours(1)))
            .earliest()
            .map(|start| start.timestamp_millis())
            .unwrap_or(now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::harness::RuntimeMode;

    fn summary(id: &str) -> SessionSummary {
        SessionSummary {
            model: "gpt-5".into(),
            runtime_mode: RuntimeMode::Supervised,
            title: format!("cursor · {id}"),
            created_at: 1,
            updated_at: 1,
            additions: Some(0),
            deletions: Some(0),
            ..SessionSummary::new(id, "/tmp/project", HarnessId::Cursor)
        }
    }

    fn ids(rows: &[SessionSummary]) -> Vec<&str> {
        rows.iter().map(|row| row.id.as_str()).collect()
    }

    fn set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    /// `new Date("2026-08-25T15:00:00").getTime()`: local time.
    fn local(y: i32, m: u32, d: u32, h: u32) -> i64 {
        Local
            .with_ymd_and_hms(y, m, d, h, 0, 0)
            .earliest()
            .unwrap()
            .timestamp_millis()
    }

    #[test]
    fn hides_selected_providers() {
        let rows = vec![
            summary("a1"),
            SessionSummary {
                harness: HarnessId::Claude,
                ..summary("a2")
            },
        ];
        assert_eq!(
            ids(&filter_sessions_by_harness(&rows, &[HarnessId::Cursor])),
            vec!["a2"]
        );
    }

    #[test]
    fn keeps_sessions_updated_today() {
        let now = local(2026, 8, 25, 15);
        let rows = vec![
            SessionSummary {
                updated_at: now - 60_000,
                ..summary("a1")
            },
            SessionSummary {
                updated_at: now - 8 * DAY_MS,
                ..summary("a2")
            },
        ];
        assert_eq!(
            ids(&filter_sessions_by_time(
                &rows,
                SessionTimeFilter::Today,
                now
            )),
            vec!["a1"]
        );
    }

    #[test]
    fn keeps_sessions_from_the_last_7_days() {
        let now = local(2026, 8, 25, 15);
        let rows = vec![
            SessionSummary {
                updated_at: now - 6 * DAY_MS,
                ..summary("a1")
            },
            SessionSummary {
                updated_at: now - 10 * DAY_MS,
                ..summary("a2")
            },
        ];
        assert_eq!(
            ids(&filter_sessions_by_time(
                &rows,
                SessionTimeFilter::SevenDays,
                now
            )),
            vec!["a1"]
        );
    }

    #[test]
    fn matches_any_selected_live_status() {
        let rows = vec![summary("a1"), summary("a2"), summary("a3")];
        let filtered = filter_sessions_by_status(
            &rows,
            SessionStatusFilter {
                working: true,
                ..Default::default()
            },
            &set(&["a1"]),
            &set(&["a2"]),
            &set(&["a3"]),
        );
        assert_eq!(ids(&filtered), vec!["a1"]);
    }

    #[test]
    fn combines_status_filters_with_or_semantics() {
        let rows = vec![summary("a1"), summary("a2"), summary("a3")];
        let filtered = filter_sessions_by_status(
            &rows,
            SessionStatusFilter {
                working: true,
                needs_approval: true,
                done: false,
            },
            &set(&["a1"]),
            &set(&["a2"]),
            &set(&["a3"]),
        );
        assert_eq!(ids(&filtered), vec!["a1", "a2"]);
    }

    #[test]
    fn has_no_active_filters_by_default() {
        assert!(!has_active_session_filters(
            &SessionSidebarFilters::default()
        ));
    }

    #[test]
    fn is_active_when_any_filter_is_set() {
        assert!(has_active_session_filters(&SessionSidebarFilters {
            time: SessionTimeFilter::SevenDays,
            ..Default::default()
        }));
    }

    #[test]
    fn starts_today_at_local_midnight() {
        assert_eq!(
            time_filter_start(SessionTimeFilter::Today, local(2026, 8, 25, 15)),
            local(2026, 8, 25, 0)
        );
    }

    #[test]
    fn round_trips_filters_and_reads_the_legacy_archive_flag() {
        let kv = Kv::in_memory();
        kv.set_item(LEGACY_SHOW_ARCHIVED_KEY, "1");
        assert!(load_session_sidebar_filters(&kv).show_archived);
        let filters = SessionSidebarFilters {
            show_archived: false,
            hidden_harnesses: vec![HarnessId::Claude],
            time: SessionTimeFilter::ThirtyDays,
            status: SessionStatusFilter {
                done: true,
                ..Default::default()
            },
        };
        save_session_sidebar_filters(&kv, &filters);
        assert_eq!(
            kv.get_item(SESSION_SIDEBAR_FILTERS_KEY).as_deref(),
            Some(
                r#"{"showArchived":false,"hiddenHarnesses":["claude"],"time":"30d","status":{"working":false,"needsApproval":false,"done":true}}"#
            )
        );
        assert_eq!(load_session_sidebar_filters(&kv), filters);
        kv.set_item(
            SESSION_SIDEBAR_FILTERS_KEY,
            r#"{"hiddenHarnesses":["nope","codex"],"time":"1y"}"#,
        );
        assert_eq!(
            load_session_sidebar_filters(&kv),
            SessionSidebarFilters {
                hidden_harnesses: vec![HarnessId::Codex],
                ..Default::default()
            }
        );
        kv.set_item(SESSION_SIDEBAR_FILTERS_KEY, "{oops");
        assert_eq!(
            load_session_sidebar_filters(&kv),
            SessionSidebarFilters::default()
        );
    }

    #[test]
    fn lists_providers_in_catalog_order() {
        let rows = vec![
            SessionSummary {
                harness: HarnessId::Codex,
                ..summary("a")
            },
            summary("b"),
            SessionSummary {
                harness: HarnessId::Claude,
                ..summary("c")
            },
        ];
        let expected: Vec<HarnessId> = HARNESSES
            .iter()
            .copied()
            .filter(|h| matches!(h, HarnessId::Codex | HarnessId::Cursor | HarnessId::Claude))
            .collect();
        assert_eq!(harnesses_in_sessions(&rows), expected);
    }
}
