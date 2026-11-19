//! The React Mono habit schedule, claiming and run-history policy.
use super::files;
use chrono::{DateTime, Datelike, Duration, Local, Timelike};
use serde::{Deserialize, Serialize};
use serde_json::{
    value::{to_raw_value, RawValue},
    Value,
};
use std::collections::BTreeMap;
use std::path::Path;

type RawObject = BTreeMap<String, Box<RawValue>>;

// JSON maps used for native field lookup keep the last occurrence of a key.
// File edits need every member, including repeated extension keys.
struct ObjectMembers(Vec<(String, Box<RawValue>)>);

impl ObjectMembers {
    fn read(raw: &RawValue) -> serde_json::Result<Self> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = ObjectMembers;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut members = Vec::new();
                while let Some(member) = map.next_entry()? {
                    members.push(member);
                }
                Ok(ObjectMembers(members))
            }
        }
        serde::Deserializer::deserialize_map(
            &mut serde_json::Deserializer::from_str(raw.get()),
            Visitor,
        )
    }
}

impl Serialize for ObjectMembers {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.0.iter().map(|(key, value)| (key, value)))
    }
}

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedRow {
    index: usize,
    revision: String,
}

impl SavedRow {
    pub fn index(&self) -> usize {
        self.index
    }
    pub fn position(&self, habits: &[Habit]) -> Result<usize, String> {
        habits
            .iter()
            .position(|h| h.saved_row() == Some(self))
            .ok_or_else(|| "Habits changed. Review the refreshed list and try again.".into())
    }
    pub fn for_id(root: &Path, mono: &str, id: &str) -> Result<Self, String> {
        let file = files::read(root, mono, None, "habits.json")?;
        let parsed = parse(file.text.as_deref(), &file.hash)?;
        // Resolve against all raw objects, even unreadable or undisplayed habits.
        let mut matching = parsed.rows.iter().enumerate().filter(|(_, row)| {
            serde_json::from_str::<RawObject>(row.get())
                .ok()
                .and_then(|fields| serde_json::from_str::<String>(fields.get("id")?.get()).ok())
                .is_some_and(|found| found == id)
        });
        let (index, _) = matching.next().ok_or("Habit not found")?;
        if matching.next().is_some() {
            return Err("Multiple habits have the same ID".into());
        }
        parsed
            .habits
            .into_iter()
            .filter_map(|h| h.saved_row)
            .find(|row| row.index == index)
            .ok_or_else(|| "Habit not found".into())
    }
    pub fn edit(
        &self,
        root: &Path,
        mono: &str,
        change: impl Fn(&mut Habit) -> Result<(), String>,
    ) -> Result<Self, String> {
        let ((), revision) = update_with_revision(root, mono, |all| {
            let index = self.position(all)?;
            change(&mut all[index])
        })?;
        Ok(Self {
            index: self.index,
            revision,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Habit {
    #[serde(skip)]
    saved_row: Option<SavedRow>,
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
    runs: Vec<Run>,
    // File writes retain the original entries, including runs newer clients understand.
    #[serde(skip)]
    stored_history: Option<Box<RawValue>>,
    #[serde(default)]
    pub run_requested: bool,
    #[serde(default)]
    pub running_since: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_claim_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_card: Option<String>,
}
fn yes() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
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
            saved_row: None,
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
            stored_history: None,
            run_requested: false,
            running_since: None,
            native_claim_id: None,
            from_card: None,
        })
    }
    pub fn saved_row(&self) -> Option<&SavedRow> {
        self.saved_row.as_ref()
    }
    pub fn runs(&self) -> &[Run] {
        &self.runs
    }
    pub fn supports_run_history(&self) -> bool {
        self.stored_history
            .as_ref()
            .is_none_or(|v| v.get() == "null" || v.get().starts_with('['))
    }
    pub fn due(&self, now: u64) -> bool {
        self.supports_run_history()
            && self.running_since.is_none()
            && (self.run_requested
                || self.enabled && self.next_run_at <= now && now - self.next_run_at <= GRACE_MS)
    }
    pub fn finish(&mut self, run: Run) -> Result<(), String> {
        if !self.supports_run_history() {
            return Err(self.history_error());
        }
        if let Some(history) = &self.stored_history {
            let mut entries: Vec<Box<RawValue>> = serde_json::from_str::<Option<_>>(history.get())
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            entries.insert(0, to_raw_value(&run).map_err(|e| e.to_string())?);
            let mut readable = 0;
            entries.retain(|entry| {
                if parse_run(entry).is_none() {
                    return true;
                }
                readable += 1;
                readable <= 20
            });
            self.stored_history = Some(to_raw_value(&entries).map_err(|e| e.to_string())?);
        }
        self.last_run_at = Some(run.at);
        self.last_outcome = Some(run.outcome.clone());
        self.last_error = run.error.clone();
        self.running_since = None;
        self.native_claim_id = None;
        self.runs.insert(0, run);
        self.runs.truncate(20);
        Ok(())
    }
    fn history_error(&self) -> String {
        format!(
            "Cannot run habit '{}': unsupported run history format",
            self.name
        )
    }
    fn file_fields(&self) -> serde_json::Result<RawObject> {
        let mut fields: RawObject = serde_json::from_str(&serde_json::to_string(self)?)?;
        if let Some(history) = &self.stored_history {
            fields.insert("runs".into(), history.clone());
        }
        Ok(fields)
    }
}
pub fn load(root: &Path, id: &str) -> Result<Vec<Habit>, String> {
    let file = files::read(root, id, None, "habits.json")?;
    Ok(parse(file.text.as_deref(), &file.hash)?.habits)
}

struct HabitFile {
    rows: Vec<Box<RawValue>>,
    habits: Vec<Habit>,
}

fn parse_run(raw: &RawValue) -> Option<Run> {
    let run: Run = serde_json::from_str(raw.get()).ok()?;
    ["posted", "quiet", "failed", "interrupted"]
        .contains(&run.outcome.as_str())
        .then_some(run)
}

fn parse_habit(raw: &RawValue) -> Result<Habit, String> {
    let mut fields: RawObject = serde_json::from_str(raw.get()).map_err(|e| e.to_string())?;
    if !fields
        .get("schedule")
        .is_some_and(|v| v.get().starts_with('{'))
    {
        return Err("expected a habit with a schedule object".into());
    }
    // Decode only supported runs for the UI; keep their exact JSON for file writes.
    let stored_history = fields.remove("runs");
    let text = serde_json::to_string(&fields).map_err(|e| e.to_string())?;
    let mut habit: Habit = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if let Some(history) = stored_history.as_ref().filter(|v| v.get().starts_with('[')) {
        let entries: Vec<&RawValue> =
            serde_json::from_str(history.get()).map_err(|e| e.to_string())?;
        habit.runs = entries.into_iter().filter_map(parse_run).take(20).collect();
    }
    habit.stored_history = stored_history;
    if habit.id.trim().is_empty()
        || habit.name.trim().is_empty()
        || habit.instructions.trim().is_empty()
    {
        return Err("missing id, name or instructions".into());
    }
    habit.schedule.validate()?;
    Ok(habit)
}

fn parse(text: Option<&str>, revision: &str) -> Result<HabitFile, String> {
    let rows: Vec<Box<RawValue>> =
        serde_json::from_str(text.filter(|t| !t.trim().is_empty()).unwrap_or("[]"))
            .map_err(|e| format!("Cannot read habits: {e}"))?;
    let mut habits = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if habits.len() == MAX {
            break;
        }
        match parse_habit(row) {
            Ok(mut habit) => {
                if !habit.supports_run_history() {
                    eprintln!("{}", habit.history_error());
                }
                habit.saved_row = Some(SavedRow {
                    index,
                    revision: revision.into(),
                });
                habits.push(habit);
            }
            Err(e) => eprintln!("Skipping habits.json entry {}: {e}", index + 1),
        }
    }
    Ok(HabitFile { rows, habits })
}

// Apply only native edits to the original object. This also leaves legacy
// spellings, omitted defaults and partially readable history alone on edits.
fn apply_edit(
    row: &RawValue,
    before: &RawObject,
    after: RawObject,
) -> serde_json::Result<Box<RawValue>> {
    if before.len() == after.len()
        && after
            .iter()
            .all(|(key, value)| before.get(key).is_some_and(|v| v.get() == value.get()))
    {
        return Ok(row.to_owned());
    }
    let mut object = ObjectMembers::read(row)?;
    object
        .0
        .retain(|(key, _)| !before.contains_key(key) || after.contains_key(key));
    for (key, mut value) in after {
        if before.get(&key).is_some_and(|v| v.get() == value.get()) {
            continue;
        }
        // Match the last occurrence, just as parse_habit does.
        let member = object.0.iter_mut().rev().find(|(name, _)| name == &key);
        if let Some((_, original)) = member {
            if key == "schedule" {
                let mut schedule = ObjectMembers::read(original)?;
                let fields: RawObject = serde_json::from_str(value.get())?;
                schedule
                    .0
                    .retain(|(name, _)| name != "kind" && !fields.contains_key(name));
                schedule.0.extend(fields);
                value = to_raw_value(&schedule)?;
            }
            *original = value;
        } else {
            object.0.push((key, value));
        }
    }
    to_raw_value(&object)
}
pub fn update<T>(
    root: &Path,
    id: &str,
    change: impl Fn(&mut Vec<Habit>) -> Result<T, String>,
) -> Result<T, String> {
    update_with_revision(root, id, change).map(|(result, _)| result)
}

fn update_with_revision<T>(
    root: &Path,
    id: &str,
    change: impl Fn(&mut Vec<Habit>) -> Result<T, String>,
) -> Result<(T, String), String> {
    for _ in 0..5 {
        let file = files::read(root, id, None, "habits.json")?;
        let HabitFile { rows, mut habits } = parse(file.text.as_deref(), &file.hash)?;
        let mut originals = BTreeMap::new();
        for habit in &habits {
            originals.insert(
                habit.saved_row().unwrap().index,
                habit.file_fields().map_err(|e| e.to_string())?,
            );
        }
        let result = change(&mut habits)?;
        if habits.len() > MAX {
            return Err("At most 20 habits per Mono".into());
        }
        // Keep skipped entries in file order, including any beyond the native limit.
        let mut skipped = rows
            .iter()
            .enumerate()
            .filter(|(i, _)| !originals.contains_key(i))
            .peekable();
        let mut saved = Vec::new();
        for habit in habits {
            let index = habit.saved_row().map_or(rows.len(), |row| row.index);
            while let Some((_, row)) = skipped.next_if(|(i, _)| *i < index) {
                saved.push(row.clone());
            }
            let value = habit.file_fields().map_err(|e| e.to_string())?;
            if let Some(before) = originals.get(&index) {
                if !habit.supports_run_history()
                    && ((habit.run_requested
                        && before["runRequested"].get() != value["runRequested"].get())
                        || (habit.running_since.is_some()
                            && before["runningSince"].get() != value["runningSince"].get()))
                {
                    return Err(habit.history_error());
                }
                saved.push(apply_edit(&rows[index], before, value).map_err(|e| e.to_string())?);
            } else {
                saved.push(to_raw_value(&value).map_err(|e| e.to_string())?);
            }
        }
        saved.extend(skipped.map(|(_, row)| row.clone()));
        let text = serde_json::to_string_pretty(&saved).map_err(|e| e.to_string())? + "\n";
        match files::save(root, id, None, "habits.json", &text, Some(&file.hash)) {
            Ok(revision) => return Ok((result, revision)),
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
    fn repeated_extension_members_survive_habit_and_schedule_edits() {
        let temp = tempfile::tempdir().unwrap();
        let text = r#"[{"id":"a","name":"A","instructions":"Check","extension":1,"extension":2,"schedule":{"kind":"daily","extension":3,"extension":4}}]"#;
        files::save(temp.path(), "mono", None, "habits.json", text, None).unwrap();
        update(temp.path(), "mono", |all| {
            all[0].enabled = false;
            all[0].schedule.time = "10:00".into();
            Ok(())
        })
        .unwrap();
        let saved = files::read(temp.path(), "mono", None, "habits.json")
            .unwrap()
            .text
            .unwrap();
        for member in [
            r#""extension":1"#,
            r#""extension":2"#,
            r#""extension":3"#,
            r#""extension":4"#,
        ] {
            assert!(saved.contains(member), "lost {member} in {saved}");
        }
        let habit = load(temp.path(), "mono").unwrap().remove(0);
        assert!(!habit.enabled);
        assert_eq!(habit.schedule.time, "10:00");
    }

    #[test]
    fn habit_writes_preserve_foreign_fields_and_unreadable_entries() {
        use serde_json::{json, Value};
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let id = "test";
        let habit = json!({"id":"a","name":"A","instructions":"Check",
            "schedule":{"kind":"daily","timezone":"local"},"project":"/a",
            "projectRealPath":"/b","precheck":{"command":"true"},"precheckLast":null,
            "runs":[{"at":1,"outcome":"quiet","source":"desktop"}]});
        let unreadable = json!({"id":"a","schedule":{"future":true}});
        let mut neighbor = habit.clone();
        neighbor["id"] = json!("b");
        let initial = json!([habit, unreadable, neighbor]);
        files::save(root, id, None, "habits.json", &initial.to_string(), None).unwrap();
        let new = Habit::new("C".into(), "Check".into(), Schedule::default(), 0).unwrap();
        update(root, id, |all| {
            assert_eq!(all.len(), 2);
            all.push(new.clone());
            Ok(())
        })
        .unwrap();
        update(root, id, |all| {
            all[0].enabled = false;
            all[0].name = "Edited".into();
            all[0].schedule = Schedule {
                schedule_kind: "hourly".into(),
                ..Default::default()
            };
            all[0].finish(serde_json::from_value(json!({"at":2,"outcome":"posted"})).unwrap())?;
            all.retain(|h| h.id != new.id);
            Ok(())
        })
        .unwrap();
        let text = files::read(root, id, None, "habits.json")
            .unwrap()
            .text
            .unwrap();
        let saved: Value = serde_json::from_str(&text).unwrap();
        for key in ["project", "projectRealPath", "precheck", "precheckLast"] {
            assert_eq!(saved[0].get(key), habit.get(key), "{key}");
            assert_eq!(saved[2].get(key), neighbor.get(key), "{key}");
        }
        assert_eq!(saved[1], unreadable);
        assert_eq!(saved[0]["schedule"]["timezone"], "local");
        assert_eq!(saved[0]["runs"][1]["source"], "desktop");
        assert_eq!(saved[0]["enabled"], false);
        assert_eq!(saved[0]["name"], "Edited");
        assert_eq!(saved[0]["lastRunAt"], 2);
        assert_eq!(saved.as_array().unwrap().len(), 3);
    }

    #[test]
    fn editing_a_neighbor_preserves_large_numeric_identifiers() {
        let dir = tempfile::tempdir().unwrap();
        let text = r#"[{"id":"a","name":"A","instructions":"Check","schedule":{"kind":"daily"},
            "project":18446744073709551617},
            {"id":"b","name":"B","instructions":"Check","schedule":{"kind":"daily","zoneId":18446744073709551618},
             "runs":[{"at":1,"outcome":"quiet","sourceId":18446744073709551619}]},
            {"futureId":18446744073709551620}]"#;
        files::save(dir.path(), "test", None, "habits.json", text, None).unwrap();
        update(dir.path(), "test", |all| {
            all[1].schedule.time = "10:00".into();
            all[1].finish(
                serde_json::from_value(serde_json::json!({"at":2,"outcome":"posted"})).unwrap(),
            )
        })
        .unwrap();
        let saved = files::read(dir.path(), "test", None, "habits.json")
            .unwrap()
            .text
            .unwrap();
        for number in 18446744073709551617u128..=18446744073709551620 {
            assert!(saved.contains(&number.to_string()), "{saved}");
        }
    }

    fn file_with_history(history: Value) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let text = serde_json::json!([{"id":"a","name":"Digest","instructions":"Check",
            "schedule":{"kind":"daily"},"runs":history}])
        .to_string();
        files::save(dir.path(), "test", None, "habits.json", &text, None).unwrap();
        dir
    }

    #[test]
    fn unsupported_history_blocks_claim_and_completion_without_writing() {
        let dir = file_with_history(serde_json::json!({"version":2,"items":[1]}));
        let root = dir.path();
        let before = files::read(root, "test", None, "habits.json").unwrap();
        for claim in [false, true] {
            let result = update(root, "test", |all| {
                if claim {
                    all[0].running_since = Some(10);
                    Ok(())
                } else {
                    all[0].finish(
                        serde_json::from_value(serde_json::json!({"at":10,"outcome":"quiet"}))
                            .unwrap(),
                    )
                }
            });
            assert_eq!(
                files::read(root, "test", None, "habits.json").unwrap().text,
                before.text
            );
            assert!(result.unwrap_err().contains("history"));
        }
    }

    #[test]
    fn completion_preserves_interleaved_history_order_at_the_limit() {
        use serde_json::json;
        let mut history: Vec<_> = (1..=20)
            .rev()
            .map(|at| json!({"at":at,"outcome":"quiet"}))
            .collect();
        history.insert(0, json!({"at":22,"outcome":"future"}));
        history.insert(5, json!({"version":2}));
        let dir = file_with_history(json!(history));
        update(dir.path(), "test", |all| {
            all[0].finish(serde_json::from_value(json!({"at":23,"outcome":"posted"})).unwrap())
        })
        .unwrap();
        let text = files::read(dir.path(), "test", None, "habits.json")
            .unwrap()
            .text
            .unwrap();
        let saved: Value = serde_json::from_str(&text).unwrap();
        let runs = saved[0]["runs"].as_array().unwrap();
        assert_eq!(runs[0]["at"], 23);
        assert_eq!(&runs[1..], &history[..history.len() - 1]);
    }

    #[test]
    fn deleting_a_habit_keeps_entries_beyond_the_native_limit() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let rows: Vec<_> = (0..=MAX)
            .map(|i| {
                serde_json::json!({
                    "id":i.to_string(),"name":"Digest","instructions":"Check",
                    "schedule":{"scheduleKind":"daily"}
                })
            })
            .collect();
        files::save(
            root,
            "test",
            None,
            "habits.json",
            &serde_json::to_string(&rows).unwrap(),
            None,
        )
        .unwrap();
        update(root, "test", |all| {
            assert_eq!(all.len(), MAX);
            all.remove(0);
            Ok(())
        })
        .unwrap();
        let saved = load(root, "test").unwrap();
        assert_eq!(saved.len(), MAX);
        assert_eq!(saved.last().unwrap().id, MAX.to_string());
    }

    #[test]
    fn desktop_habit_optional_timestamps_and_partial_history_remain_editable() {
        let text = r#"[{"id":"desktop","name":"Digest","instructions":"Check the news",
            "schedule":{"scheduleKind":"daily"},"fromCard":"suggestion-1",
            "runs":[{"at":100,"outcome":"posted","report":"**News**"},
                    {"at":"bad","outcome":"failed"},
                    {"at":80,"outcome":"quiet"}]}]"#;
        let habits = parse(Some(text), "test").unwrap().habits;
        assert_eq!(habits.len(), 1);
        let h = &habits[0];
        assert_eq!(h.created_at, 0);
        assert_eq!(h.next_run_at, 0);
        assert_eq!(h.runs.len(), 2);
        assert_eq!(h.runs[0].duration_ms, 0);
        assert_eq!(h.runs[0].report.as_deref(), Some("**News**"));
        assert_eq!(h.from_card.as_deref(), Some("suggestion-1"));
        let serialized = serde_json::to_string(&habits).unwrap();
        assert_eq!(
            parse(Some(&serialized), "test").unwrap().habits[0].from_card,
            h.from_card
        );
    }

    #[test]
    fn malformed_history_does_not_hide_an_otherwise_valid_habit() {
        let text = r#"[{"id":"desktop","name":"Digest","instructions":"Check updates",
            "schedule":{"scheduleKind":"hourly","minute":15},"runs":null}]"#;
        let habits = parse(Some(text), "test").unwrap().habits;
        assert_eq!(habits.len(), 1);
        assert!(habits[0].runs.is_empty());
    }
}
