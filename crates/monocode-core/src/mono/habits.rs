//! The React Mono habit schedule, claiming and run-history policy.
use super::files;
use chrono::{DateTime, Datelike, Duration, Local, Timelike};
use serde::{Deserialize, Serialize};
use std::path::Path;
pub const MAX: usize = 20;
pub const GRACE_MS: u64 = 2 * 60 * 60 * 1000;
pub const TIMEOUT_MS: u64 = 60 * 60 * 1000;
pub const APPROVAL_WAIT_MS: u64 = 12 * 60 * 60 * 1000;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    #[serde(alias = "kind")]
    pub schedule_kind: String,
    #[serde(default)]
    pub minute: u32,
    #[serde(default = "nine")]
    pub time: String,
    #[serde(default = "monday")]
    pub day_of_week: u32,
}
fn nine() -> String {
    "09:00".into()
}
fn monday() -> u32 {
    1
}
impl Default for Schedule {
    fn default() -> Self {
        Self {
            schedule_kind: "daily".into(),
            minute: 0,
            time: nine(),
            day_of_week: 1,
        }
    }
}
impl Schedule {
    pub fn validate(&self) -> Result<(), String> {
        let (h, m) = self.time.split_once(':').ok_or("Time must be HH:MM")?;
        if !["hourly", "daily", "weekdays", "weekly"].contains(&self.schedule_kind.as_str())
            || h.len() != 2
            || m.len() != 2
            || h.parse::<u32>().ok().is_none_or(|h| h > 23)
            || m.parse::<u32>().ok().is_none_or(|m| m > 59)
            || self.minute > 59
            || self.day_of_week > 6
        {
            return Err("Choose hourly, daily, weekdays or weekly, a valid 24-hour time, minute 0–59 and weekday 0–6".into());
        }
        Ok(())
    }
    pub fn label(&self) -> String {
        match self.schedule_kind.as_str() {
            "hourly" => format!("Hourly at :{:02}", self.minute),
            "weekdays" => format!("Weekdays at {}", self.time),
            "weekly" => format!(
                "{} at {}",
                [
                    "Sunday",
                    "Monday",
                    "Tuesday",
                    "Wednesday",
                    "Thursday",
                    "Friday",
                    "Saturday"
                ][self.day_of_week.min(6) as usize],
                self.time
            ),
            _ => format!("Daily at {}", self.time),
        }
    }
    pub fn next(&self, after: u64) -> Result<u64, String> {
        self.validate()?;
        let start = DateTime::from_timestamp_millis(after as i64)
            .ok_or("Invalid schedule date")?
            .with_timezone(&Local);
        let hour = self.time[..2].parse::<u32>().unwrap();
        let minute = self.time[3..].parse::<u32>().unwrap();
        // Iterate real minutes across DST transitions. Wall-clock predicates
        // avoid invalid or duplicated local-time arithmetic.
        let mut at =
            start.with_second(0).unwrap().with_nanosecond(0).unwrap() + Duration::minutes(1);
        for _ in 0..8 * 24 * 60 {
            let day = at.weekday().num_days_from_sunday();
            let matches = if self.schedule_kind == "hourly" {
                at.minute() == self.minute
            } else {
                at.hour() == hour
                    && at.minute() == minute
                    && match self.schedule_kind.as_str() {
                        "weekdays" => (1..=5).contains(&day),
                        "weekly" => day == self.day_of_week,
                        _ => true,
                    }
            };
            if matches {
                return Ok(at.timestamp_millis().max(0) as u64);
            }
            at += Duration::minutes(1);
        }
        Err("No next habit run found".into())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Habit {
    pub id: String,
    pub name: String,
    pub instructions: String,
    pub schedule: Schedule,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub next_run_at: u64,
    pub last_run_at: Option<u64>,
    pub last_outcome: Option<String>,
    pub last_error: Option<String>,
    #[serde(default)]
    pub runs: Vec<Run>,
    #[serde(default)]
    pub run_requested: bool,
    #[serde(default)]
    pub running_since: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_card: Option<String>,
}
fn yes() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub at: u64,
    #[serde(default)]
    pub duration_ms: u64,
    pub outcome: String,
    pub report: Option<String>,
    pub error: Option<String>,
}
impl Habit {
    pub fn new(
        name: String,
        instructions: String,
        schedule: Schedule,
        now: u64,
    ) -> Result<Self, String> {
        if name.trim().is_empty()
            || name.chars().count() > 120
            || instructions.trim().is_empty()
            || instructions.len() > 32000
        {
            return Err("A habit needs a name and instructions".into());
        }
        let next_run_at = schedule.next(now)?;
        Ok(Self {
            id: super::new_id(),
            name: name.trim().into(),
            instructions: instructions.trim().into(),
            schedule,
            enabled: true,
            created_at: now,
            next_run_at,
            last_run_at: None,
            last_outcome: None,
            last_error: None,
            runs: Vec::new(),
            run_requested: false,
            running_since: None,
            from_card: None,
        })
    }
    pub fn due(&self, now: u64) -> bool {
        self.running_since.is_none()
            && (self.run_requested
                || self.enabled && self.next_run_at <= now && now - self.next_run_at <= GRACE_MS)
    }
    pub fn finish(&mut self, run: Run) -> Result<(), String> {
        self.last_run_at = Some(run.at);
        self.last_outcome = Some(run.outcome.clone());
        self.last_error = run.error.clone();
        self.running_since = None;
        self.runs.insert(0, run);
        self.runs.truncate(20);
        Ok(())
    }
}
pub fn load(root: &Path, id: &str) -> Result<Vec<Habit>, String> {
    let file = files::read(root, id, None, "habits.json")?;
    parse(file.text.as_deref())
}
fn parse(text: Option<&str>) -> Result<Vec<Habit>, String> {
    let Some(text) = text.filter(|t| !t.trim().is_empty()) else {
        return Ok(Vec::new());
    };
    let values: Vec<serde_json::Value> =
        serde_json::from_str(text).map_err(|e| format!("Cannot read habits: {e}"))?;
    Ok(values
        .into_iter()
        .filter_map(|mut value| {
            // The desktop parser accepts each recent run independently. A bad
            // history item must not make the habit (and its controls) disappear.
            if let Some(items) = value.get_mut("runs") {
                let runs: Vec<Run> = items
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|run| serde_json::from_value::<Run>(run.clone()).ok())
                    .filter(|run| {
                        ["posted", "quiet", "failed", "interrupted"].contains(&run.outcome.as_str())
                    })
                    .take(20)
                    .collect();
                *items = serde_json::to_value(runs).ok()?;
            }
            serde_json::from_value::<Habit>(value).ok()
        })
        .filter(|h| {
            !h.id.trim().is_empty()
                && !h.name.trim().is_empty()
                && !h.instructions.trim().is_empty()
                && h.schedule.validate().is_ok()
        })
        .take(MAX)
        .collect())
}
pub fn update<T>(
    root: &Path,
    id: &str,
    change: impl Fn(&mut Vec<Habit>) -> Result<T, String>,
) -> Result<T, String> {
    for _ in 0..5 {
        let file = files::read(root, id, None, "habits.json")?;
        let mut habits = parse(file.text.as_deref())?;
        let result = change(&mut habits)?;
        if habits.len() > MAX {
            return Err("At most 20 habits per Mono".into());
        }
        let text = serde_json::to_string_pretty(&habits).map_err(|e| e.to_string())? + "\n";
        match files::save(root, id, None, "habits.json", &text, Some(&file.hash)) {
            Ok(_) => return Ok(result),
            Err(e) if e == files::CONFLICT => continue,
            Err(e) => return Err(e),
        }
    }
    Err("Habits kept changing; try again".into())
}
pub fn report(text: &str) -> Option<String> {
    let t = text.trim();
    let simple = t
        .chars()
        .filter(|c| !c.is_whitespace() && !"`*.".contains(*c))
        .collect::<String>();
    if simple == "NOTHING_TO_REPORT" {
        return None;
    }
    let text = t.trim_end_matches("NOTHING_TO_REPORT").trim();
    (!text.is_empty()).then(|| text.chars().take(4000).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_habit_optional_timestamps_and_partial_history_remain_editable() {
        let text = r#"[{"id":"desktop","name":"Digest","instructions":"Check the news",
            "schedule":{"scheduleKind":"daily"},"fromCard":"suggestion-1",
            "runs":[{"at":100,"outcome":"posted","report":"**News**"},
                    {"at":"bad","outcome":"failed"},
                    {"at":80,"outcome":"quiet"}]}]"#;
        let habits = parse(Some(text)).unwrap();
        assert_eq!(habits.len(), 1);
        let h = &habits[0];
        assert_eq!(h.created_at, 0);
        assert_eq!(h.next_run_at, 0);
        assert_eq!(h.runs.len(), 2);
        assert_eq!(h.runs[0].duration_ms, 0);
        assert_eq!(h.runs[0].report.as_deref(), Some("**News**"));
        assert_eq!(h.from_card.as_deref(), Some("suggestion-1"));
        let serialized = serde_json::to_string(&habits).unwrap();
        assert_eq!(parse(Some(&serialized)).unwrap()[0].from_card, h.from_card);
    }

    #[test]
    fn malformed_history_does_not_hide_an_otherwise_valid_habit() {
        let text = r#"[{"id":"desktop","name":"Digest","instructions":"Check updates",
            "schedule":{"scheduleKind":"hourly","minute":15},"runs":null}]"#;
        let habits = parse(Some(text)).unwrap();
        assert_eq!(habits.len(), 1);
        assert!(habits[0].runs.is_empty());
    }
}
