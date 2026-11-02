//! Moves the Mac onto a hotspot when Wi-Fi drops while agents work, and back
//! to its usual network once they finish.
//!
//! macOS hides the current network's name from apps without Location access,
//! so this never asks which network the Mac is on. It notices a drop by
//! failing to reach the internet, confirms a join by the Wi-Fi link (see
//! [`join`]), and saves a record of the switch so a later run can undo it.
//!
//! [`Hotspot`] decides; it is a pure state machine fed a clock, and reports
//! each decision as an [`Event`] for the log. [`join::join`] and
//! [`join::leave`] carry the decisions out against a [`join::Wifi`].
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

// Only `mac` calls it; other Unixes build it for its tests.
#[cfg(unix)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod cmd;
pub mod join;
#[cfg(target_os = "macos")]
pub mod mac;
mod probe;
pub mod record;

pub use join::{JoinError, JoinRecord, Joined, Left};
pub use probe::online;

/// Offline this long before joining, so a blip doesn't switch networks.
pub const OFFLINE_BEFORE_JOIN: Duration = Duration::from_secs(15);
/// Between reachability checks while agents work.
pub const PROBE_EVERY: Duration = Duration::from_secs(5);
/// The wait after the first failed join; it doubles each time after.
pub const FIRST_RETRY: Duration = Duration::from_secs(20);
/// The longest wait between joins.
pub const MAX_RETRY: Duration = Duration::from_secs(5 * 60);
/// Joins per outage before giving up, about half an hour of trying.
pub const MAX_ATTEMPTS: u32 = 10;
/// After a wake, Wi-Fi gets this long to come back by itself.
pub const WAKE_GRACE: Duration = Duration::from_secs(30);
/// After a launch that finds an earlier run's join, agents get this long to
/// show up before it goes back.
pub const RESTORE_GRACE: Duration = Duration::from_secs(10);
/// The wall clock ran this much further than the monotonic one between two
/// checks: the Mac slept.
pub const SLEPT: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Stay,
    Join,
    /// Back to the usual network.
    Leave,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaveWhy {
    AgentsDone,
    LidOpened,
}

/// A decision, or the outcome of one, for the log.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Offline,
    Online {
        offline_for: Duration,
    },
    Join {
        attempt: u32,
        offline_for: Duration,
    },
    Joined {
        ssid: String,
        router: Option<Ipv4Addr>,
        took: Duration,
    },
    Failed {
        error: JoinError,
        attempt: u32,
        retry_in: Duration,
    },
    GaveUp {
        error: JoinError,
        attempts: u32,
    },
    Leave {
        why: LeaveWhy,
    },
    Left {
        how: Left,
    },
    LeaveFailed {
        error: String,
    },
    Woke,
    Restored {
        ssid: String,
        at: u64,
    },
    /// The hotspot settings changed; failures so far are forgotten.
    Reset,
}
impl Event {
    /// One JSON line; `unix` is when it happened.
    pub fn json(&self, unix: u64) -> String {
        use serde_json::json;
        let secs = |d: &Duration| d.as_secs();
        let mut body = match self {
            Self::Offline => json!({"event": "offline"}),
            Self::Online { offline_for } => {
                json!({"event": "online", "offline_s": secs(offline_for)})
            }
            Self::Join {
                attempt,
                offline_for,
            } => json!({"event": "join", "attempt": attempt, "offline_s": secs(offline_for)}),
            Self::Joined { ssid, router, took } => json!({
                "event": "joined", "ssid": ssid,
                "router": router.map(|r| r.to_string()), "took_s": secs(took)
            }),
            Self::Failed {
                error,
                attempt,
                retry_in,
            } => json!({
                "event": "failed", "error": error.key(), "detail": error.message(),
                "attempt": attempt, "retry_in_s": secs(retry_in)
            }),
            Self::GaveUp { error, attempts } => json!({
                "event": "gave_up", "error": error.key(), "detail": error.message(),
                "attempts": attempts
            }),
            Self::Leave { why } => json!({"event": "leave", "why": match why {
                LeaveWhy::AgentsDone => "agents_done",
                LeaveWhy::LidOpened => "lid_opened",
            }}),
            Self::Left { how } => json!({"event": "left", "how": how.key()}),
            Self::LeaveFailed { error } => json!({"event": "leave_failed", "detail": error}),
            Self::Woke => json!({"event": "woke"}),
            Self::Restored { ssid, at } => {
                json!({"event": "restored", "ssid": ssid, "joined_at": at})
            }
            Self::Reset => json!({"event": "reset"}),
        };
        body["at"] = json!(unix);
        body.to_string()
    }
}

#[derive(Debug, Default)]
pub struct Hotspot {
    offline_since: Option<Instant>,
    /// No join before this.
    next_try: Option<Instant>,
    /// Joins tried in this outage.
    attempts: u32,
    error: Option<JoinError>,
    gave_up: bool,
    /// What this moved the Mac onto and hasn't moved it back from.
    joined: Option<JoinRecord>,
    /// After a wake, no join before this.
    hold_until: Option<Instant>,
    /// A record from an earlier run: no leave before this.
    restored_until: Option<Instant>,
    /// A join or leave is running.
    doing: Option<Step>,
    working: bool,
    events: Vec<Event>,
}
impl Hotspot {
    pub fn on_hotspot(&self) -> bool {
        self.joined.is_some()
    }
    pub fn record(&self) -> Option<&JoinRecord> {
        self.joined.as_ref()
    }
    pub fn error(&self) -> Option<&JoinError> {
        self.error.as_ref()
    }
    /// The decisions made since the last call, oldest first.
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Feeds the latest reachability check. `working`: agents work and a
    /// hotspot is set up.
    pub fn step(&mut self, online: bool, working: bool, now: Instant) -> Step {
        self.working = working;
        if self.doing.is_some() {
            return Step::Stay;
        }
        if !working {
            self.offline_since = None;
            self.hold_until = None;
            // The spacing between joins holds, so agents stopping and
            // starting can't hammer the hotspot; the count starts over, and
            // only a failure retrying can't fix stays given up.
            self.attempts = 0;
            if !self.error.as_ref().is_some_and(JoinError::permanent) {
                self.gave_up = false;
            }
            if self.joined.is_none() || self.restored_until.is_some_and(|until| now < until) {
                return Step::Stay;
            }
            self.events.push(Event::Leave {
                why: LeaveWhy::AgentsDone,
            });
            return Step::Leave;
        }
        if online {
            if let Some(since) = self.offline_since.take() {
                self.events.push(Event::Online {
                    offline_for: now.saturating_duration_since(since),
                });
            }
            self.clear_failures();
            return Step::Stay;
        }
        let since = match self.offline_since {
            Some(since) => since,
            None => {
                self.events.push(Event::Offline);
                *self.offline_since.insert(now)
            }
        };
        let offline_for = now.saturating_duration_since(since);
        let due = !self.gave_up
            && offline_for >= OFFLINE_BEFORE_JOIN
            && self.hold_until.is_none_or(|until| now >= until)
            && self.next_try.is_none_or(|at| now >= at);
        if !due {
            return Step::Stay;
        }
        self.attempts += 1;
        self.events.push(Event::Join {
            attempt: self.attempts,
            offline_for,
        });
        Step::Join
    }
    /// Someone is back at the Mac: let it return to its usual network.
    pub fn lid_opened(&mut self) -> Step {
        if self.joined.is_none() || self.doing.is_some() {
            return Step::Stay;
        }
        self.events.push(Event::Leave {
            why: LeaveWhy::LidOpened,
        });
        Step::Leave
    }
    /// A join or leave the last step asked for is under way.
    pub fn started(&mut self, step: Step) {
        if step != Step::Stay {
            self.doing = Some(step);
        }
    }
    pub fn busy(&self) -> Option<Step> {
        self.doing
    }

    pub fn joined(&mut self, joined: Joined) {
        self.doing = None;
        self.events.push(Event::Joined {
            ssid: joined.record.ssid.clone(),
            router: joined.record.router,
            took: joined.took,
        });
        self.joined = Some(joined.record);
        self.restored_until = None;
        self.offline_since = None;
        self.clear_failures();
    }
    pub fn join_failed(&mut self, error: JoinError, now: Instant) {
        self.doing = None;
        let attempt = self.attempts.max(1);
        if error.permanent() || attempt >= MAX_ATTEMPTS {
            self.gave_up = true;
            self.events.push(Event::GaveUp {
                error: error.clone(),
                attempts: attempt,
            });
        } else {
            let retry_in = backoff(attempt);
            self.next_try = Some(now + retry_in);
            self.events.push(Event::Failed {
                error: error.clone(),
                attempt,
                retry_in,
            });
        }
        self.error = Some(error);
    }
    /// The leave finished, or failed. Either way this stops counting the Mac
    /// as on the hotspot, so a failing leave can't repeat every tick.
    pub fn left(&mut self, outcome: Result<Left, String>) {
        self.events.push(match outcome {
            Ok(how) => Event::Left { how },
            Err(error) => Event::LeaveFailed { error },
        });
        *self = Self {
            events: std::mem::take(&mut self.events),
            // A lid-open leave while agents still work mustn't let the next
            // join fire before its spacing.
            next_try: self.next_try,
            working: self.working,
            ..Self::default()
        };
    }
    /// The Mac woke from sleep: Wi-Fi needs a moment before a drop counts.
    pub fn woke(&mut self, now: Instant) {
        self.offline_since = None;
        self.hold_until = Some(now + WAKE_GRACE);
        self.events.push(Event::Woke);
    }
    /// A record an earlier run saved: this run goes back for it.
    pub fn restore(&mut self, record: JoinRecord, now: Instant) {
        self.events.push(Event::Restored {
            ssid: record.ssid.clone(),
            at: record.at,
        });
        self.joined = Some(record);
        self.restored_until = Some(now + RESTORE_GRACE);
    }
    /// The hotspot's name or password changed: try again from scratch.
    pub fn reset(&mut self) {
        if self.gave_up || self.error.is_some() || self.next_try.is_some() {
            self.events.push(Event::Reset);
        }
        self.clear_failures();
    }
    fn clear_failures(&mut self) {
        self.attempts = 0;
        self.next_try = None;
        self.error = None;
        self.gave_up = false;
    }

    /// One line on what it's doing, for the popover and Settings; `None`
    /// when there's nothing to say.
    pub fn summary(&self, ssid: &str, now: Instant) -> Option<String> {
        let secs = |at: Instant| at.saturating_duration_since(now).as_secs().max(1);
        match self.doing {
            Some(Step::Join) => {
                return Some(format!(
                    "Wi-Fi dropped. Joining {ssid}… (try {})",
                    self.attempts.max(1)
                ));
            }
            Some(Step::Leave) => return Some("Going back to your usual Wi-Fi…".into()),
            _ => {}
        }
        if let Some(record) = &self.joined {
            return Some(match (&self.error, self.working) {
                (Some(error), true) => format!(
                    "On {}, but the internet is gone. {}",
                    record.ssid,
                    error.message()
                ),
                _ => format!(
                    "On {}. Back to your usual Wi-Fi when agents finish.",
                    record.ssid
                ),
            });
        }
        if !self.working {
            return None;
        }
        if let Some(error) = &self.error {
            let msg = error.message();
            return Some(match (self.gave_up, self.next_try) {
                (true, _) if error.permanent() => {
                    format!("{msg} Not trying again until the hotspot settings change.")
                }
                (true, _) => format!(
                    "{msg} Stopped after {MAX_ATTEMPTS} tries; tries again the next time the internet drops."
                ),
                (false, Some(at)) if self.offline_since.is_some() => {
                    format!("{msg} Trying again in {}s.", secs(at))
                }
                _ => msg,
            });
        }
        let since = self.offline_since?;
        if let Some(until) = self.hold_until.filter(|until| now < *until) {
            return Some(format!(
                "The internet is down after sleep. Giving Wi-Fi {}s before joining {ssid}.",
                secs(until)
            ));
        }
        let at = since + OFFLINE_BEFORE_JOIN;
        Some(if now < at {
            format!(
                "The internet is down. Joining {ssid} in {}s if it stays down.",
                secs(at)
            )
        } else {
            "The internet is down.".into()
        })
    }
}

/// The wait after `attempt` failed joins: 20s, 40s, 80s, 160s, then 5 min.
pub fn backoff(attempt: u32) -> Duration {
    let doublings = attempt.saturating_sub(1).min(16);
    (FIRST_RETRY * 2u32.pow(doublings)).min(MAX_RETRY)
}

/// Whether the Mac slept between two checks: the wall clock moved on further
/// than the monotonic clock, which stops while asleep.
pub fn slept(wall_gap: Duration, mono_gap: Duration) -> bool {
    wall_gap.saturating_sub(mono_gap) >= SLEPT
}

/// Whether a turn's error reads like the connection dropped, rather than the
/// provider refusing or failing on its own.
pub fn network_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    [
        "connection error",
        "connection reset",
        "connection refused",
        "connection closed",
        "network",
        "econnreset",
        "econnrefused",
        "enotfound",
        "etimedout",
        "enetunreach",
        "ehostunreach",
        "getaddrinfo",
        "fetch failed",
        "socket hang up",
        "stream disconnected",
        "dns error",
        "error sending request",
        "offline",
    ]
    .iter()
    .any(|needle| error.contains(needle))
}

/// The Wi-Fi device (`en0`) from `networksetup -listallhardwareports`.
pub fn parse_wifi_device(ports: &str) -> Option<String> {
    let mut lines = ports.lines();
    while let Some(line) = lines.next() {
        if matches!(
            line.trim(),
            "Hardware Port: Wi-Fi" | "Hardware Port: AirPort"
        ) {
            return lines
                .next()?
                .trim()
                .strip_prefix("Device: ")
                .map(|d| d.trim().to_string());
        }
    }
    None
}

/// Names from `networksetup -listpreferredwirelessnetworks`.
pub fn parse_saved_networks(list: &str) -> Vec<String> {
    list.lines()
        .filter(|line| line.starts_with('\t'))
        .map(|line| line.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect()
}

/// `networksetup -getairportpower`: "Wi-Fi Power (en0): On".
pub fn parse_wifi_power(out: &str) -> Option<bool> {
    match out.trim().rsplit(':').next()?.trim() {
        "On" => Some(true),
        "Off" => Some(false),
        _ => None,
    }
}

/// `networksetup -getairportnetwork`: the name, when macOS shows it. On macOS
/// 15 and later, without Location access, it says "You are not associated"
/// even while connected, so that reads as unknown, not as disconnected.
pub fn parse_current_network(out: &str) -> Option<String> {
    let name = out.trim().strip_prefix("Current Wi-Fi Network:")?.trim();
    (!name.is_empty() && name != "<redacted>").then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Duration = Duration::from_secs(1);

    fn joined(ssid: &str) -> Joined {
        Joined {
            record: JoinRecord {
                ssid: ssid.into(),
                ip: "172.20.10.3".parse().ok(),
                router: "172.20.10.1".parse().ok(),
                at: 0,
            },
            took: 4 * S,
        }
    }

    /// Runs a step and, like the driver, marks a join or leave as started.
    fn step(h: &mut Hotspot, online: bool, working: bool, at: Instant) -> Step {
        let step = h.step(online, working, at);
        h.started(step);
        step
    }

    #[test]
    fn joins_after_a_sustained_drop_and_retries_slowly() {
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        assert_eq!(step(&mut h, true, true, t0), Step::Stay);
        assert_eq!(step(&mut h, false, true, t0), Step::Stay);
        assert_eq!(step(&mut h, false, true, t0 + 10 * S), Step::Stay);
        assert_eq!(step(&mut h, false, true, t0 + 15 * S), Step::Join);
        // The join failed; the next try waits.
        h.join_failed(JoinError::NotFound, t0 + 16 * S);
        assert_eq!(step(&mut h, false, true, t0 + 30 * S), Step::Stay);
        assert_eq!(step(&mut h, false, true, t0 + 75 * S), Step::Join);
        h.joined(joined("My iPhone"));
        assert!(h.on_hotspot());
        assert_eq!(step(&mut h, true, true, t0 + 80 * S), Step::Stay);
    }

    #[test]
    fn a_blip_does_not_switch() {
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        step(&mut h, false, true, t0);
        step(&mut h, true, true, t0 + 10 * S);
        assert_eq!(step(&mut h, false, true, t0 + 20 * S), Step::Stay);
        assert_eq!(step(&mut h, false, true, t0 + 30 * S), Step::Stay);
    }

    #[test]
    fn goes_back_when_agents_finish_or_the_lid_opens() {
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        assert_eq!(step(&mut h, false, false, t0), Step::Stay);
        assert_eq!(h.lid_opened(), Step::Stay);
        h.joined(joined("My iPhone"));
        assert_eq!(h.lid_opened(), Step::Leave);
        assert_eq!(step(&mut h, true, false, t0), Step::Leave);
        h.left(Ok(Left::Cycled));
        assert!(!h.on_hotspot());
        assert_eq!(step(&mut h, true, false, t0), Step::Stay);
    }

    #[test]
    fn retry_waits_from_the_failure_not_the_attempt() {
        // A join started at 15s that takes 50s to fail must not retry 10s
        // later. The old code timed retries from the start of the attempt.
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        step(&mut h, false, true, t0);
        assert_eq!(step(&mut h, false, true, t0 + 15 * S), Step::Join);
        h.join_failed(JoinError::NotFound, t0 + 65 * S);
        assert_eq!(step(&mut h, false, true, t0 + 75 * S), Step::Stay);
        assert_eq!(step(&mut h, false, true, t0 + 85 * S), Step::Join);
    }

    #[test]
    fn first_retry_is_quicker_than_a_minute() {
        // Someone opening the Hotspot screen on their phone shouldn't wait a
        // minute for the next try. The old code always waited 60s.
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        step(&mut h, false, true, t0);
        assert_eq!(step(&mut h, false, true, t0 + 15 * S), Step::Join);
        h.join_failed(JoinError::NotFound, t0 + 15 * S);
        assert_eq!(step(&mut h, false, true, t0 + 35 * S), Step::Join);
    }

    #[test]
    fn restarting_agents_keeps_the_retry_spacing() {
        // The old code forgot the last try whenever agents stopped.
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        step(&mut h, false, true, t0);
        assert_eq!(step(&mut h, false, true, t0 + 15 * S), Step::Join);
        h.join_failed(JoinError::NotFound, t0 + 30 * S);
        step(&mut h, false, false, t0 + 31 * S);
        step(&mut h, false, true, t0 + 32 * S);
        assert_eq!(step(&mut h, false, true, t0 + 47 * S), Step::Stay);
        assert_eq!(step(&mut h, false, true, t0 + 50 * S), Step::Join);
    }

    #[test]
    fn backs_off_and_caps_attempts() {
        assert_eq!(
            (1..=7).map(backoff).collect::<Vec<_>>(),
            [20, 40, 80, 160, 300, 300, 300].map(|s| s * S)
        );
        assert_eq!(backoff(u32::MAX), MAX_RETRY);
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        let mut now = t0;
        step(&mut h, false, true, now);
        let mut joins = 0;
        // Two hours offline, checking every 5s.
        while now < t0 + 7200 * S {
            now += PROBE_EVERY;
            if step(&mut h, false, true, now) == Step::Join {
                joins += 1;
                h.join_failed(JoinError::NotFound, now);
            }
        }
        assert_eq!(joins, MAX_ATTEMPTS);
        assert!(matches!(
            h.take_events().last(),
            Some(Event::GaveUp { attempts: 10, .. })
        ));
        // The outage ends; the next one tries again.
        step(&mut h, true, true, now);
        step(&mut h, false, true, now + S);
        assert_eq!(step(&mut h, false, true, now + 16 * S), Step::Join);
    }

    #[test]
    fn gives_up_at_once_on_a_wrong_password() {
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        step(&mut h, false, true, t0);
        assert_eq!(step(&mut h, false, true, t0 + 15 * S), Step::Join);
        h.join_failed(JoinError::WrongPassword, t0 + 20 * S);
        for t in 21..2000 {
            assert_eq!(step(&mut h, false, true, t0 + t * S), Step::Stay);
        }
        // Agents finishing doesn't forget it either.
        step(&mut h, false, false, t0 + 2000 * S);
        step(&mut h, false, true, t0 + 2001 * S);
        assert_eq!(step(&mut h, false, true, t0 + 2100 * S), Step::Stay);
        assert!(h
            .summary("My iPhone", t0 + 2100 * S)
            .unwrap()
            .contains("password"));
    }

    #[test]
    fn settings_change_resets_the_backoff() {
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        step(&mut h, false, true, t0);
        step(&mut h, false, true, t0 + 15 * S);
        h.join_failed(JoinError::WrongPassword, t0 + 16 * S);
        h.reset();
        assert_eq!(step(&mut h, false, true, t0 + 17 * S), Step::Join);
        assert!(h.take_events().contains(&Event::Reset));
    }

    #[test]
    fn no_join_right_after_a_wake() {
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        step(&mut h, false, true, t0);
        h.woke(t0 + 5 * S);
        for t in 6..35 {
            assert_eq!(step(&mut h, false, true, t0 + t * S), Step::Stay, "at {t}s");
        }
        assert_eq!(step(&mut h, false, true, t0 + 36 * S), Step::Join);
        // Wi-Fi back within the grace: nothing happens at all.
        let mut h = Hotspot::default();
        h.woke(t0);
        step(&mut h, false, true, t0 + S);
        assert_eq!(step(&mut h, true, true, t0 + 12 * S), Step::Stay);
        assert_eq!(step(&mut h, false, true, t0 + 40 * S), Step::Stay);
    }

    #[test]
    fn detects_sleep_from_the_clocks() {
        assert!(!slept(5 * S, 5 * S));
        assert!(!slept(8 * S, S));
        assert!(slept(600 * S, S));
        assert!(!slept(S, 600 * S));
    }

    #[test]
    fn a_restored_join_leaves_after_the_grace() {
        // The old code kept the join in memory only, so a restart while on
        // the hotspot never went back.
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        h.restore(joined("My iPhone").record, t0);
        assert!(h.on_hotspot());
        assert_eq!(step(&mut h, true, false, t0 + 5 * S), Step::Stay);
        assert_eq!(step(&mut h, true, false, t0 + 10 * S), Step::Leave);
        // Agents resumed in time: it stays until they finish.
        let mut h = Hotspot::default();
        h.restore(joined("My iPhone").record, t0);
        assert_eq!(step(&mut h, true, true, t0 + 2 * S), Step::Stay);
        assert_eq!(step(&mut h, true, true, t0 + 60 * S), Step::Stay);
        assert_eq!(step(&mut h, true, false, t0 + 61 * S), Step::Leave);
    }

    #[test]
    fn nothing_new_starts_while_a_join_runs() {
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        step(&mut h, false, true, t0);
        assert_eq!(step(&mut h, false, true, t0 + 15 * S), Step::Join);
        assert_eq!(step(&mut h, false, false, t0 + 16 * S), Step::Stay);
        assert_eq!(h.lid_opened(), Step::Stay);
        h.joined(joined("My iPhone"));
        assert_eq!(step(&mut h, false, false, t0 + 17 * S), Step::Leave);
        // A failing leave doesn't repeat every tick.
        h.left(Err("Couldn't turn Wi-Fi off.".into()));
        assert_eq!(step(&mut h, false, false, t0 + 18 * S), Step::Stay);
    }

    #[test]
    fn every_decision_is_an_event() {
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        step(&mut h, false, true, t0);
        step(&mut h, false, true, t0 + 15 * S);
        h.join_failed(JoinError::NotFound, t0 + 20 * S);
        step(&mut h, false, true, t0 + 40 * S);
        h.joined(joined("My iPhone"));
        step(&mut h, true, true, t0 + 45 * S);
        step(&mut h, true, false, t0 + 50 * S);
        h.left(Ok(Left::Cycled));
        let keys: Vec<String> = h
            .take_events()
            .iter()
            .map(|e| {
                let v: serde_json::Value = serde_json::from_str(&e.json(7)).unwrap();
                assert_eq!(v["at"], 7);
                v["event"].as_str().unwrap().to_string()
            })
            .collect();
        assert_eq!(
            keys,
            ["offline", "join", "failed", "join", "joined", "leave", "left"]
        );
        assert!(h.take_events().is_empty());
        let failed = Event::Failed {
            error: JoinError::NotFound,
            attempt: 1,
            retry_in: 20 * S,
        }
        .json(1);
        assert!(failed.contains("\"error\":\"not_found\"") && failed.contains("\"retry_in_s\":20"));
    }

    #[test]
    fn summary_says_what_it_is_doing() {
        let t0 = Instant::now();
        let mut h = Hotspot::default();
        assert_eq!(h.summary("P", t0), None);
        step(&mut h, false, true, t0);
        assert_eq!(
            h.summary("P", t0 + 5 * S).unwrap(),
            "The internet is down. Joining P in 10s if it stays down."
        );
        step(&mut h, false, true, t0 + 15 * S);
        assert_eq!(
            h.summary("P", t0 + 15 * S).unwrap(),
            "Wi-Fi dropped. Joining P… (try 1)"
        );
        h.join_failed(JoinError::NotFound, t0 + 20 * S);
        assert!(h
            .summary("P", t0 + 25 * S)
            .unwrap()
            .ends_with("Trying again in 15s."));
        step(&mut h, false, true, t0 + 40 * S);
        h.joined(joined("P"));
        assert_eq!(
            h.summary("P", t0 + 45 * S).unwrap(),
            "On P. Back to your usual Wi-Fi when agents finish."
        );
    }

    #[test]
    fn tells_a_dropped_connection_from_other_failures() {
        assert!(network_error("API Error: Connection error."));
        assert!(network_error(
            "request failed: getaddrinfo ENOTFOUND api.anthropic.com"
        ));
        assert!(network_error("stream disconnected before completion"));
        assert!(network_error("TypeError: fetch failed"));
        assert!(!network_error("API Error: 529 Overloaded"));
        assert!(!network_error("Prompt is too long"));
        assert!(!network_error("Ready"));
    }

    #[test]
    fn reads_networksetup() {
        let ports = "\nHardware Port: Ethernet\nDevice: en1\n\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: aa\n";
        assert_eq!(parse_wifi_device(ports).as_deref(), Some("en0"));
        assert_eq!(
            parse_wifi_device("Hardware Port: Ethernet\nDevice: en1"),
            None
        );
        let list = "Preferred networks on en0:\n\tHome Wi-Fi\n\tMy iPhone\n";
        assert_eq!(parse_saved_networks(list), ["Home Wi-Fi", "My iPhone"]);
        assert_eq!(join::classify(""), None);
        assert_eq!(
            join::classify("Could not find network My iPhone."),
            Some(JoinError::NotFound)
        );
        assert_eq!(
            join::classify("Failed to join network My iPhone.\nError: -3900  The operation couldn't be completed.")
                .unwrap()
                .message(),
            "The hotspot refused the password. Check it in Settings."
        );
        assert_eq!(parse_wifi_power("Wi-Fi Power (en0): On\n"), Some(true));
        assert_eq!(parse_wifi_power("Wi-Fi Power (en0): Off"), Some(false));
        assert_eq!(parse_wifi_power("garbage"), None);
        // macOS 27 without Location access, while connected.
        assert_eq!(
            parse_current_network("You are not associated with an AirPort network.\n"),
            None
        );
        assert_eq!(
            parse_current_network("Current Wi-Fi Network: My iPhone\n").as_deref(),
            Some("My iPhone")
        );
    }
}
