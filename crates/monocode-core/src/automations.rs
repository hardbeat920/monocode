//! Persisted automations, scheduling, event deduplication, and run history.
use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use chrono::{Datelike, Local, TimeZone, Timelike};
use std::path::Path;

fn now_millis() -> i64 {
    crate::time::now_ms().min(i64::MAX as u64) as i64
}

pub fn new_id() -> String {
    format!("{:032x}", rand::random::<u128>())
}

fn validate_id(id: &str, kind: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 200
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':'))
    {
        return Err(format!("Invalid {kind} id."));
    }
    Ok(())
}

const MAX_NAME: usize = 200;
const MAX_PROMPT: usize = 1_000_000;
const MAX_TRIGGERS: usize = 20;
const MAX_RUNS_PER_AUTOMATION: i64 = 100;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Automation {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub harness: String,
    pub model: String,
    #[serde(default)]
    pub model_settings: HashMap<String, String>,
    pub cwd: String,
    pub workspace_mode: String,
    #[serde(default)]
    pub worktree_cwd: String,
    #[serde(default)]
    pub session_folder_id: String,
    pub reuse_session: bool,
    pub runtime_mode: String,
    #[serde(default = "default_trigger_kind")]
    pub trigger_kind: String,
    #[serde(default)]
    pub trigger_event: String,
    pub schedule_kind: String,
    pub minute: i64,
    pub time: String,
    pub day_of_week: i64,
    #[serde(default)]
    pub triggers: Option<Vec<AutomationTrigger>>,
    pub missed_run_grace_minutes: i64,
    pub enabled: bool,
    pub next_run_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_session_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationUpsert {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub harness: String,
    pub model: String,
    #[serde(default)]
    pub model_settings: HashMap<String, String>,
    pub cwd: String,
    pub workspace_mode: String,
    #[serde(default)]
    pub worktree_cwd: String,
    #[serde(default)]
    pub session_folder_id: String,
    #[serde(default)]
    pub reuse_session: bool,
    pub runtime_mode: String,
    #[serde(default = "default_trigger_kind")]
    pub trigger_kind: String,
    #[serde(default)]
    pub trigger_event: String,
    pub schedule_kind: String,
    pub minute: i64,
    pub time: String,
    pub day_of_week: i64,
    #[serde(default)]
    pub triggers: Option<Vec<AutomationTrigger>>,
    pub missed_run_grace_minutes: i64,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub next_run_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRun {
    pub id: String,
    pub automation_id: String,
    pub trigger: String,
    pub scheduled_for: i64,
    pub created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationTrigger {
    #[serde(default)]
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub event: String,
    #[serde(default = "default_schedule_kind")]
    pub schedule_kind: String,
    #[serde(default)]
    pub minute: i64,
    #[serde(default = "default_time")]
    pub time: String,
    #[serde(default = "default_day_of_week")]
    pub day_of_week: i64,
    #[serde(default)]
    pub repos: Vec<String>,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default = "default_actor")]
    pub actor: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DueAutomationRun {
    pub automation: Automation,
    pub run: AutomationRun,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationEventClaim {
    pub event_key: String,
    pub event_kind: String,
    pub event: String,
    pub scheduled_for: i64,
    pub prompt: String,
}

fn default_true() -> bool {
    true
}

fn default_trigger_kind() -> String {
    "time".into()
}

fn default_schedule_kind() -> String {
    "weekdays".into()
}

fn default_time() -> String {
    "09:00".into()
}

fn default_day_of_week() -> i64 {
    1
}

fn default_actor() -> String {
    "anyone".into()
}

pub(crate) fn ensure_tables(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS automations (
           id TEXT PRIMARY KEY,
           definition_json TEXT NOT NULL,
           enabled INTEGER NOT NULL,
           next_run_at INTEGER NOT NULL,
           updated_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS automations_due_idx
           ON automations (enabled, next_run_at);
         CREATE TABLE IF NOT EXISTS automation_runs (
           id TEXT PRIMARY KEY,
           automation_id TEXT NOT NULL REFERENCES automations(id) ON DELETE CASCADE,
           created_at INTEGER NOT NULL,
           run_json TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS automation_runs_history_idx
           ON automation_runs (automation_id, created_at DESC);
         CREATE TABLE IF NOT EXISTS automation_event_claims (
           automation_id TEXT NOT NULL REFERENCES automations(id) ON DELETE CASCADE,
           event_key TEXT NOT NULL,
           created_at INTEGER NOT NULL,
           PRIMARY KEY (automation_id, event_key)
         );",
    )?;

    // Backfill claims from pre-ledger run history. Invalid legacy rows should not
    // prevent the session store from opening.
    let mut statement =
        conn.prepare("SELECT automation_id, created_at, run_json FROM automation_runs")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut claims = Vec::new();
    for row in rows {
        let (automation_id, created_at, raw) = row?;
        if let Ok(run) = serde_json::from_str::<AutomationRun>(&raw) {
            if let Some(event_key) = run.event_key {
                claims.push((automation_id, event_key, created_at));
            }
        }
    }
    drop(statement);
    for (automation_id, event_key, created_at) in claims {
        conn.execute(
            "INSERT OR IGNORE INTO automation_event_claims
             (automation_id, event_key, created_at) VALUES (?1, ?2, ?3)",
            params![automation_id, event_key, created_at],
        )?;
    }
    Ok(())
}

fn validate_upsert(input: &AutomationUpsert, now: i64) -> Result<(), String> {
    validate_id(&input.id, "automation")?;
    if input.name.trim().is_empty() || input.name.len() > MAX_NAME {
        return Err("Automation name is required and must be under 200 characters.".into());
    }
    if input.prompt.trim().is_empty() || input.prompt.len() > MAX_PROMPT {
        return Err("Automation prompt is required.".into());
    }
    if input.cwd.trim().is_empty() {
        return Err("Choose a project for this automation.".into());
    }
    if !matches!(
        input.workspace_mode.as_str(),
        "current" | "worktree" | "existing"
    ) {
        return Err("Invalid automation workspace mode.".into());
    }
    if input.workspace_mode == "existing" {
        let path = input.worktree_cwd.trim();
        if path.is_empty() || path.len() > 1_000 {
            return Err("Choose an existing worktree for this automation.".into());
        }
    }
    if input.session_folder_id.len() > 80 {
        return Err("Invalid automation session folder.".into());
    }
    if !matches!(
        input.runtime_mode.as_str(),
        "supervised" | "auto-accept-edits" | "auto" | "full-access"
    ) {
        return Err("Invalid automation run mode.".into());
    }
    if !matches!(
        input.trigger_kind.as_str(),
        "time" | "github" | "linear" | "jira" | "gitlab" | "azuredevops"
    ) {
        return Err("Invalid automation trigger.".into());
    }
    if input.trigger_event.len() > 200 {
        return Err("Invalid automation trigger event.".into());
    }
    if !matches!(
        input.schedule_kind.as_str(),
        "hourly" | "daily" | "weekdays" | "weekly"
    ) {
        return Err("Invalid automation schedule.".into());
    }
    if !(0..=59).contains(&input.minute)
        || !(0..=6).contains(&input.day_of_week)
        || !valid_time(&input.time)
    {
        return Err("Invalid automation time.".into());
    }
    if !(0..=43_200).contains(&input.missed_run_grace_minutes) {
        return Err("Invalid missed-run grace period.".into());
    }
    if input.next_run_at <= now {
        return Err("The next automation run must be in the future.".into());
    }
    if let Some(triggers) = &input.triggers {
        if triggers.len() > MAX_TRIGGERS {
            return Err("Too many automation triggers.".into());
        }
        for trigger in triggers {
            validate_trigger(trigger)?;
        }
    }
    Ok(())
}

fn validate_trigger(trigger: &AutomationTrigger) -> Result<(), String> {
    if !matches!(
        trigger.kind.as_str(),
        "time" | "github" | "linear" | "jira" | "gitlab" | "azuredevops"
    ) {
        return Err("Invalid automation trigger.".into());
    }
    if trigger.event.len() > 200
        || trigger.repo.len() > 400
        || trigger.branch.len() > 400
        || trigger.actor.len() > 200
        || trigger.repos.len() > 50
        || trigger.repos.iter().any(|repo| repo.len() > 400)
    {
        return Err("Invalid automation trigger event.".into());
    }
    if !matches!(
        trigger.schedule_kind.as_str(),
        "hourly" | "daily" | "weekdays" | "weekly"
    ) {
        return Err("Invalid automation schedule.".into());
    }
    if !(0..=59).contains(&trigger.minute)
        || !(0..=6).contains(&trigger.day_of_week)
        || !valid_time(&trigger.time)
    {
        return Err("Invalid automation time.".into());
    }
    Ok(())
}

fn hydrate_triggers(automation: &mut Automation) {
    if automation.triggers.is_some() {
        return;
    }
    automation.triggers = Some(vec![legacy_trigger(automation)]);
}

fn legacy_trigger(automation: &Automation) -> AutomationTrigger {
    trigger_from_fields(
        &automation.id,
        &automation.trigger_kind,
        &automation.trigger_event,
        &automation.schedule_kind,
        automation.minute,
        &automation.time,
        automation.day_of_week,
    )
}

fn has_time_trigger(automation: &Automation) -> bool {
    match &automation.triggers {
        Some(triggers) => triggers.iter().any(|trigger| trigger.kind == "time"),
        None => automation.trigger_kind == "time",
    }
}

fn normalize_triggers(input: &AutomationUpsert) -> Vec<AutomationTrigger> {
    let mut triggers = input.triggers.clone().unwrap_or_else(|| {
        vec![trigger_from_fields(
            &input.id,
            &input.trigger_kind,
            &input.trigger_event,
            &input.schedule_kind,
            input.minute,
            &input.time,
            input.day_of_week,
        )]
    });
    for trigger in &mut triggers {
        if trigger.id.trim().is_empty() {
            trigger.id = new_id();
        }
        trigger.event = trigger.event.trim().to_string();
        trigger.repo = trigger.repo.trim().to_string();
        trigger.branch = trigger.branch.trim().to_string();
        trigger.actor = trigger.actor.trim().to_string();
        if trigger.actor.is_empty() {
            trigger.actor = default_actor();
        }
        trigger.repos = trigger
            .repos
            .iter()
            .map(|repo| repo.trim().to_string())
            .filter(|repo| !repo.is_empty())
            .collect();
    }
    triggers
}

fn trigger_from_fields(
    id: &str,
    kind: &str,
    event: &str,
    schedule_kind: &str,
    minute: i64,
    time: &str,
    day_of_week: i64,
) -> AutomationTrigger {
    AutomationTrigger {
        id: format!("{id}:legacy"),
        kind: kind.to_string(),
        event: if event.is_empty() {
            schedule_kind.to_string()
        } else {
            event.to_string()
        },
        schedule_kind: schedule_kind.to_string(),
        minute,
        time: time.to_string(),
        day_of_week,
        repos: Vec::new(),
        repo: String::new(),
        branch: String::new(),
        actor: default_actor(),
    }
}

fn parse_automation(value: String) -> Result<Automation, String> {
    let mut automation: Automation =
        serde_json::from_str(&value).map_err(|error| error.to_string())?;
    hydrate_triggers(&mut automation);
    Ok(automation)
}

fn valid_time(value: &str) -> bool {
    let Some((hour, minute)) = value.split_once(':') else {
        return false;
    };
    hour.len() == 2
        && minute.len() == 2
        && hour.parse::<u8>().is_ok_and(|value| value < 24)
        && minute.parse::<u8>().is_ok_and(|value| value < 60)
}

fn list(conn: &Connection) -> Result<Vec<Automation>, String> {
    let mut statement = conn
        .prepare("SELECT definition_json FROM automations ORDER BY updated_at DESC, id")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    rows.map(|row| {
        let value = row.map_err(|error| error.to_string())?;
        parse_automation(value)
    })
    .collect()
}

fn get(conn: &Connection, id: &str) -> Result<Option<Automation>, String> {
    let raw = conn
        .query_row(
            "SELECT definition_json FROM automations WHERE id = ?1",
            [id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    raw.map(parse_automation).transpose()
}

fn write_automation(conn: &Connection, automation: &Automation) -> Result<(), String> {
    let raw = serde_json::to_string(automation).map_err(|error| error.to_string())?;
    conn.execute(
        "INSERT INTO automations (id, definition_json, enabled, next_run_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
           definition_json = excluded.definition_json,
           enabled = excluded.enabled,
           next_run_at = excluded.next_run_at,
           updated_at = excluded.updated_at",
        params![
            automation.id,
            raw,
            automation.enabled,
            automation.next_run_at,
            automation.updated_at
        ],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn write_run(conn: &Connection, run: &AutomationRun) -> Result<(), String> {
    let raw = serde_json::to_string(run).map_err(|error| error.to_string())?;
    conn.execute(
        "INSERT INTO automation_runs (id, automation_id, created_at, run_json)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET run_json = excluded.run_json",
        params![run.id, run.automation_id, run.created_at, raw],
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

fn new_run(
    automation_id: &str,
    trigger: &str,
    scheduled_for: i64,
    now: i64,
    prompt: &str,
) -> AutomationRun {
    AutomationRun {
        id: new_id(),
        automation_id: automation_id.to_string(),
        trigger: trigger.to_string(),
        scheduled_for,
        created_at: now,
        started_at: None,
        completed_at: None,
        status: "pending".into(),
        session_id: None,
        error: None,
        event_key: None,
        event_kind: None,
        event: None,
        prompt: Some(prompt.to_string()),
    }
}

fn new_event_run(
    automation_id: &str,
    event_key: &str,
    event_kind: &str,
    event: &str,
    scheduled_for: i64,
    now: i64,
    prompt: &str,
) -> AutomationRun {
    let mut run = new_run(automation_id, "event", scheduled_for, now, prompt);
    run.event_key = Some(event_key.to_string());
    run.event_kind = Some(event_kind.to_string());
    run.event = Some(event.to_string());
    run
}

fn validate_event_key(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 400
        || !value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'/' | b'.')
        })
    {
        return Err("Invalid automation event key.".into());
    }
    Ok(())
}

fn has_event_trigger(automation: &Automation, kind: &str, event: &str) -> bool {
    match &automation.triggers {
        Some(triggers) => triggers
            .iter()
            .any(|trigger| trigger.kind == kind && trigger.event == event),
        None => automation.trigger_kind == kind && automation.trigger_event == event,
    }
}

fn list_runs(conn: &Connection, automation_id: &str) -> Result<Vec<AutomationRun>, String> {
    let mut statement = conn
        .prepare(
            "SELECT run_json FROM automation_runs WHERE automation_id = ?1
             ORDER BY created_at DESC LIMIT 100",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([automation_id], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;
    rows.map(|row| {
        let value = row.map_err(|error| error.to_string())?;
        serde_json::from_str(&value).map_err(|error| error.to_string())
    })
    .collect()
}

fn trim_history(conn: &Connection, automation_id: &str) -> Result<(), String> {
    let mut statement = conn
        .prepare(
            "SELECT id, run_json FROM automation_runs WHERE automation_id = ?1
             ORDER BY created_at DESC, id DESC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([automation_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|error| error.to_string())?;
    let mut delete_ids = Vec::new();
    for (index, row) in rows.enumerate() {
        let (id, raw) = row.map_err(|error| error.to_string())?;
        if index < MAX_RUNS_PER_AUTOMATION as usize {
            continue;
        }
        let run: AutomationRun = serde_json::from_str(&raw).map_err(|error| error.to_string())?;
        if matches!(
            run.status.as_str(),
            "succeeded" | "failed" | "skipped" | "cancelled"
        ) {
            delete_ids.push(id);
        }
    }
    drop(statement);
    for id in delete_ids {
        conn.execute("DELETE FROM automation_runs WHERE id = ?1", [id])
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn apply_run_summary(automation: &mut Automation, run: &AutomationRun) {
    if automation
        .last_run_at
        .is_none_or(|last_run_at| run.created_at >= last_run_at)
    {
        automation.last_run_at = Some(run.created_at);
        automation.last_run_status = Some(run.status.clone());
        automation.last_run_error = run.error.clone();
        if run.session_id.is_some() {
            automation.last_session_id = run.session_id.clone();
        }
    }
}

/// Access to the same tables and JSON used by the Tauri application.
/// Open with the session database path to preserve existing definitions and history.
pub struct Service {
    conn: Connection,
}
impl Service {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
        }
        let conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| e.to_string())?;
        ensure_tables(&conn).map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }
    fn lock_conn(&mut self) -> Result<&mut Connection, String> {
        Ok(&mut self.conn)
    }
    pub fn list(&mut self) -> Result<Vec<Automation>, String> {
        let conn = self.lock_conn()?;
        list(conn)
    }

    pub fn upsert(&mut self, automation: AutomationUpsert) -> Result<Automation, String> {
        let now = now_millis();
        validate_upsert(&automation, now)?;
        let triggers = Some(normalize_triggers(&automation));
        let conn = self.lock_conn()?;
        let previous = get(conn, &automation.id)?;
        let created_at = previous
            .as_ref()
            .map(|value| value.created_at)
            .unwrap_or(now);
        let worktree_cwd = if automation.workspace_mode == "existing" {
            automation.worktree_cwd.trim().to_string()
        } else {
            String::new()
        };
        let saved = Automation {
            id: automation.id,
            name: automation.name.trim().to_string(),
            prompt: automation.prompt,
            harness: automation.harness,
            model: automation.model,
            model_settings: automation.model_settings,
            cwd: automation.cwd,
            workspace_mode: automation.workspace_mode,
            worktree_cwd,
            session_folder_id: automation.session_folder_id.trim().to_string(),
            reuse_session: automation.reuse_session,
            runtime_mode: automation.runtime_mode,
            trigger_kind: automation.trigger_kind,
            trigger_event: automation.trigger_event.trim().to_string(),
            schedule_kind: automation.schedule_kind,
            minute: automation.minute,
            time: automation.time,
            day_of_week: automation.day_of_week,
            triggers,
            missed_run_grace_minutes: automation.missed_run_grace_minutes,
            enabled: automation.enabled,
            next_run_at: automation.next_run_at,
            last_run_at: previous.as_ref().and_then(|value| value.last_run_at),
            last_run_status: previous
                .as_ref()
                .and_then(|value| value.last_run_status.clone()),
            last_run_error: previous
                .as_ref()
                .and_then(|value| value.last_run_error.clone()),
            last_session_id: previous.and_then(|value| value.last_session_id),
            created_at,
            updated_at: now,
        };
        write_automation(conn, &saved)?;
        Ok(saved)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        validate_id(id, "automation")?;
        let conn = self.lock_conn()?;
        conn.execute("DELETE FROM automations WHERE id = ?1", [id])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn runs(&mut self, automation_id: &str) -> Result<Vec<AutomationRun>, String> {
        validate_id(automation_id, "automation")?;
        let conn = self.lock_conn()?;
        let mut runs = list_runs(conn, automation_id)?;
        // Launch prompts can be large and are only needed by restart recovery, not
        // by the run-history surface.
        for run in &mut runs {
            run.prompt = None;
        }
        Ok(runs)
    }

    pub fn recover(
        &mut self,
        started_before: i64,
        now: i64,
    ) -> Result<Vec<DueAutomationRun>, String> {
        let conn = self.lock_conn()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let mut statement = tx
            .prepare(
                "SELECT run_json FROM automation_runs WHERE created_at <= ?1
             ORDER BY created_at, id",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([started_before], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        let mut runs = Vec::new();
        for row in rows {
            let raw = row.map_err(|error| error.to_string())?;
            runs.push(
                serde_json::from_str::<AutomationRun>(&raw).map_err(|error| error.to_string())?,
            );
        }
        drop(statement);

        let mut due = Vec::new();
        let mut changed_automation_ids = Vec::new();
        for mut run in runs {
            if run.status != "pending" && run.status != "running" {
                continue;
            }
            let Some(mut automation) = get(&tx, &run.automation_id)? else {
                continue;
            };
            if run.status == "running" {
                run.status = "cancelled".into();
                run.completed_at = Some(now);
                run.error = Some("Interrupted when MonoCode last stopped.".into());
                apply_run_summary(&mut automation, &run);
                write_run(&tx, &run)?;
                write_automation(&tx, &automation)?;
                changed_automation_ids.push(run.automation_id.clone());
                continue;
            }
            if run.trigger == "event"
                && run
                    .prompt
                    .as_deref()
                    .is_none_or(|prompt| prompt.trim().is_empty())
            {
                run.status = "failed".into();
                run.completed_at = Some(now);
                run.error = Some("The Inbox event prompt was not available after restart.".into());
                apply_run_summary(&mut automation, &run);
                write_run(&tx, &run)?;
                write_automation(&tx, &automation)?;
                changed_automation_ids.push(run.automation_id.clone());
                continue;
            }
            due.push(DueAutomationRun { automation, run });
        }
        changed_automation_ids.sort();
        changed_automation_ids.dedup();
        for automation_id in changed_automation_ids {
            trim_history(&tx, &automation_id)?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok(due)
    }

    pub fn run_now(&mut self, automation_id: &str, now: i64) -> Result<AutomationRun, String> {
        validate_id(automation_id, "automation")?;
        let conn = self.lock_conn()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let mut automation =
            get(&tx, automation_id)?.ok_or_else(|| "Automation not found.".to_string())?;
        let run = new_run(automation_id, "manual", now, now, &automation.prompt);
        automation.last_run_at = Some(now);
        automation.last_run_status = Some(run.status.clone());
        automation.last_run_error = None;
        write_run(&tx, &run)?;
        write_automation(&tx, &automation)?;
        trim_history(&tx, automation_id)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(run)
    }

    pub fn claim_occurrence(
        &mut self,
        automation_id: &str,
        expected_next_run_at: i64,
        next_run_at: i64,
        now: i64,
    ) -> Result<Option<DueAutomationRun>, String> {
        validate_id(automation_id, "automation")?;
        if next_run_at <= expected_next_run_at {
            return Err("The following automation occurrence must advance the schedule.".into());
        }
        let conn = self.lock_conn()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let current =
            get(&tx, automation_id)?.ok_or_else(|| "Automation not found.".to_string())?;
        if !has_time_trigger(&current) {
            tx.rollback().map_err(|error| error.to_string())?;
            return Ok(None);
        }
        let claimed = tx
            .execute(
                "UPDATE automations SET next_run_at = ?1
             WHERE id = ?2 AND enabled = 1 AND next_run_at = ?3 AND next_run_at <= ?4",
                params![next_run_at, automation_id, expected_next_run_at, now],
            )
            .map_err(|error| error.to_string())?;
        if claimed == 0 {
            tx.rollback().map_err(|error| error.to_string())?;
            return Ok(None);
        }
        let mut automation =
            get(&tx, automation_id)?.ok_or_else(|| "Automation not found.".to_string())?;
        automation.next_run_at = next_run_at;
        let mut run = new_run(
            automation_id,
            "scheduled",
            expected_next_run_at,
            now,
            &automation.prompt,
        );
        let grace_ms = automation.missed_run_grace_minutes.saturating_mul(60_000);
        let missed = now.saturating_sub(expected_next_run_at);
        let dispatch = missed <= grace_ms;
        if !dispatch {
            run.status = "skipped".into();
            run.completed_at = Some(now);
            run.error = Some("Missed the scheduled run beyond its grace period.".into());
        }
        automation.last_run_at = Some(now);
        automation.last_run_status = Some(run.status.clone());
        automation.last_run_error = run.error.clone();
        write_run(&tx, &run)?;
        write_automation(&tx, &automation)?;
        trim_history(&tx, automation_id)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(Some(DueAutomationRun { automation, run }))
    }

    pub fn claim_event(
        &mut self,
        automation_id: &str,
        claim: AutomationEventClaim,
        now: i64,
    ) -> Result<Option<DueAutomationRun>, String> {
        validate_id(automation_id, "automation")?;
        validate_event_key(&claim.event_key)?;
        if !matches!(
            claim.event_kind.as_str(),
            "github" | "linear" | "jira" | "gitlab" | "azuredevops"
        ) {
            return Err("Invalid automation trigger.".into());
        }
        if claim.event.is_empty() || claim.event.len() > 200 {
            return Err("Invalid automation trigger event.".into());
        }
        if claim.prompt.trim().is_empty() || claim.prompt.len() > MAX_PROMPT + 10_000 {
            return Err("Invalid automation run prompt.".into());
        }
        let scheduled_for = if claim.scheduled_for > 0 {
            claim.scheduled_for
        } else {
            now
        };
        let conn = self.lock_conn()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let mut automation =
            get(&tx, automation_id)?.ok_or_else(|| "Automation not found.".to_string())?;
        if !automation.enabled || !has_event_trigger(&automation, &claim.event_kind, &claim.event) {
            tx.rollback().map_err(|error| error.to_string())?;
            return Ok(None);
        }
        let claimed = tx
            .execute(
                "INSERT OR IGNORE INTO automation_event_claims
             (automation_id, event_key, created_at) VALUES (?1, ?2, ?3)",
                params![automation_id, claim.event_key, now],
            )
            .map_err(|error| error.to_string())?;
        if claimed == 0 {
            tx.rollback().map_err(|error| error.to_string())?;
            return Ok(None);
        }
        let run = new_event_run(
            automation_id,
            &claim.event_key,
            &claim.event_kind,
            &claim.event,
            scheduled_for,
            now,
            &claim.prompt,
        );
        automation.last_run_at = Some(now);
        automation.last_run_status = Some(run.status.clone());
        automation.last_run_error = None;
        write_run(&tx, &run)?;
        write_automation(&tx, &automation)?;
        trim_history(&tx, automation_id)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(Some(DueAutomationRun { automation, run }))
    }

    pub fn update_run(
        &mut self,
        run_id: &str,
        status: &str,
        session_id: Option<String>,
        error: Option<String>,
        now: i64,
    ) -> Result<AutomationRun, String> {
        validate_id(run_id, "automation run")?;
        if let Some(session_id) = session_id.as_deref() {
            validate_id(session_id, "session")?;
        }
        if !matches!(
            status,
            "pending" | "running" | "succeeded" | "failed" | "skipped" | "cancelled"
        ) {
            return Err("Invalid automation run status.".into());
        }
        let conn = self.lock_conn()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let raw = tx
            .query_row(
                "SELECT run_json FROM automation_runs WHERE id = ?1",
                [&run_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Automation run not found.".to_string())?;
        let mut run: AutomationRun =
            serde_json::from_str(&raw).map_err(|error| error.to_string())?;
        run.status = status.to_owned();
        if run.status == "running" && run.started_at.is_none() {
            run.started_at = Some(now);
        }
        if matches!(
            run.status.as_str(),
            "succeeded" | "failed" | "skipped" | "cancelled"
        ) {
            run.completed_at = Some(now);
        }
        if session_id.is_some() {
            run.session_id = session_id;
        }
        run.error = error.filter(|value| !value.trim().is_empty());
        let mut automation =
            get(&tx, &run.automation_id)?.ok_or_else(|| "Automation not found.".to_string())?;
        apply_run_summary(&mut automation, &run);
        write_run(&tx, &run)?;
        write_automation(&tx, &automation)?;
        trim_history(&tx, &run.automation_id)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(run)
    }
}

pub const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

impl AutomationTrigger {
    pub fn new(kind: &str, event: &str) -> Self {
        Self {
            id: new_id(),
            kind: kind.into(),
            event: event.into(),
            schedule_kind: if matches!(event, "hourly" | "daily" | "weekdays" | "weekly") {
                event.into()
            } else {
                "weekdays".into()
            },
            minute: 0,
            time: "09:00".into(),
            day_of_week: 1,
            repos: Vec::new(),
            repo: String::new(),
            branch: String::new(),
            actor: "anyone".into(),
        }
    }
    pub fn label(&self) -> String {
        if self.kind != "time" {
            return event_label(&self.kind, &self.event);
        }
        match self.schedule_kind.as_str() {
            "hourly" => format!("Hourly at :{:02}", self.minute),
            "daily" => format!("Daily at {}", self.time),
            "weekly" => format!(
                "{} at {}",
                WEEKDAYS.get(self.day_of_week as usize).unwrap_or(&"Weekly"),
                self.time
            ),
            _ => format!("Weekdays at {}", self.time),
        }
    }
    /// Calendar arithmetic in the user's time zone, including daylight-saving changes.
    pub fn next_at(&self, after: i64) -> i64 {
        let Some(start) = Local.timestamp_millis_opt(after).single() else {
            return after.saturating_add(86_400_000);
        };
        if self.schedule_kind == "hourly" {
            // Enumerating actual clock minutes also handles repeated/nonexistent DST hours.
            let mut candidate = (after / 60_000 + 1) * 60_000;
            for _ in 0..180 {
                if Local
                    .timestamp_millis_opt(candidate)
                    .single()
                    .is_some_and(|date| date.minute() == self.minute.clamp(0, 59) as u32)
                {
                    return candidate;
                }
                candidate += 60_000;
            }
            return candidate;
        }
        let (hour, minute) = self
            .time
            .split_once(':')
            .and_then(|(h, m)| Some((h.parse::<u32>().ok()?, m.parse::<u32>().ok()?)))
            .unwrap_or((9, 0));
        for offset in 0..15 {
            let date = start.date_naive() + chrono::Duration::days(offset);
            let weekday = date.weekday().num_days_from_sunday();
            if self.schedule_kind == "weekdays" && (weekday == 0 || weekday == 6) {
                continue;
            }
            if self.schedule_kind == "weekly" && weekday != self.day_of_week.clamp(0, 6) as u32 {
                continue;
            }
            if let Some(naive) = date.and_hms_opt(hour.min(23), minute.min(59), 0) {
                // A repeated clock time fires once. For a spring-forward gap use the first
                // valid clock minute after the requested wall time, as Date.setHours does.
                for gap in 0..=180 {
                    let target = naive + chrono::Duration::minutes(gap);
                    if let Some(candidate) = Local.from_local_datetime(&target).earliest() {
                        let at = candidate.timestamp_millis();
                        if at > after {
                            return at;
                        }
                        break;
                    }
                }
            }
        }
        after.saturating_add(7 * 86_400_000)
    }
}

pub fn event_label(kind: &str, event: &str) -> String {
    match (kind, event) {
        ("jira", "issue_created") => "Issue appeared",
        (_, "draft_opened") => "Draft opened",
        (_, "pull_request_opened") => "Pull request opened",
        (_, "issue_opened") => "Issue opened",
        (_, "merge_request_opened") => "Merge request opened",
        (_, "issue_created") => "Issue created",
        (_, "pull_request_appeared") => "Pull request appeared",
        (_, "work_item_appeared") => "Work item appeared",
        (_, "pull_request_merged") => "Pull request merged",
        (_, "pull_request_review_requested") => "Review requested",
        (_, "issue_assigned") => "Issue assigned",
        (_, "check_failed") => "Check failed",
        _ => event,
    }
    .to_owned()
}

pub fn next_run_at(triggers: &[AutomationTrigger], after: i64) -> i64 {
    triggers
        .iter()
        .filter(|t| t.kind == "time")
        .map(|t| t.next_at(after))
        .min()
        .unwrap_or_else(|| after.saturating_add(365 * 86_400_000))
}

impl Automation {
    pub fn triggers(&self) -> Vec<AutomationTrigger> {
        self.triggers
            .clone()
            .unwrap_or_else(|| vec![legacy_trigger(self)])
    }
    pub fn trigger_label(&self) -> String {
        let triggers = self.triggers();
        match triggers.len() {
            0 => "Manual only".into(),
            1 => triggers[0].label(),
            count => format!("{} + {} more", triggers[0].label(), count - 1),
        }
    }
}

impl AutomationUpsert {
    pub fn new(cwd: String, harness: String, model: String) -> Self {
        Self {
            id: new_id(),
            name: String::new(),
            prompt: String::new(),
            harness,
            model,
            model_settings: HashMap::new(),
            cwd,
            workspace_mode: "worktree".into(),
            worktree_cwd: String::new(),
            session_folder_id: String::new(),
            reuse_session: false,
            runtime_mode: "auto".into(),
            trigger_kind: "time".into(),
            trigger_event: String::new(),
            schedule_kind: "weekdays".into(),
            minute: 0,
            time: "09:00".into(),
            day_of_week: 1,
            triggers: Some(Vec::new()),
            missed_run_grace_minutes: 720,
            enabled: true,
            next_run_at: now_millis().saturating_add(365 * 86_400_000),
        }
    }
    pub fn from_automation(automation: &Automation) -> Self {
        Self {
            id: automation.id.clone(),
            name: automation.name.clone(),
            prompt: automation.prompt.clone(),
            harness: automation.harness.clone(),
            model: automation.model.clone(),
            model_settings: automation.model_settings.clone(),
            cwd: automation.cwd.clone(),
            workspace_mode: automation.workspace_mode.clone(),
            worktree_cwd: automation.worktree_cwd.clone(),
            session_folder_id: automation.session_folder_id.clone(),
            reuse_session: automation.reuse_session,
            runtime_mode: automation.runtime_mode.clone(),
            trigger_kind: automation.trigger_kind.clone(),
            trigger_event: automation.trigger_event.clone(),
            schedule_kind: automation.schedule_kind.clone(),
            minute: automation.minute,
            time: automation.time.clone(),
            day_of_week: automation.day_of_week,
            triggers: Some(automation.triggers()),
            missed_run_grace_minutes: automation.missed_run_grace_minutes,
            enabled: automation.enabled,
            next_run_at: automation.next_run_at,
        }
    }
    pub fn sync_triggers(&mut self, now: i64) {
        let triggers = self.triggers.get_or_insert_with(Vec::new);
        if let Some(primary) = triggers
            .iter()
            .find(|t| t.kind == "time")
            .or_else(|| triggers.first())
        {
            self.trigger_kind = primary.kind.clone();
            self.trigger_event = primary.event.clone();
            self.schedule_kind = primary.schedule_kind.clone();
            self.minute = primary.minute;
            self.time = primary.time.clone();
            self.day_of_week = primary.day_of_week;
        } else {
            self.trigger_kind = "time".into();
            self.trigger_event.clear();
        }
        self.next_run_at = next_run_at(triggers, now);
    }
}

impl Service {
    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<Automation, String> {
        let automation = get(&self.conn, id)?.ok_or("Automation not found.")?;
        let mut draft = AutomationUpsert::from_automation(&automation);
        draft.enabled = enabled;
        draft.sync_triggers(now_millis());
        self.upsert(draft)
    }
    /// Atomically claims each due occurrence, including skipped occurrences outside
    /// the grace period. Bounded work prevents an offline period monopolizing UI polling.
    pub fn claim_due(&mut self, now: i64) -> Result<Vec<DueAutomationRun>, String> {
        let mut due = Vec::new();
        for mut automation in self.list()? {
            if !automation.enabled || !has_time_trigger(&automation) {
                continue;
            }
            for _ in 0..100 {
                if automation.next_run_at > now {
                    break;
                }
                let next = next_run_at(&automation.triggers(), automation.next_run_at);
                if next <= automation.next_run_at {
                    break;
                }
                let claim = match self.claim_occurrence(
                    &automation.id,
                    automation.next_run_at,
                    next,
                    now,
                ) {
                    Ok(Some(claim)) => claim,
                    // A failed transaction leaves this occurrence due for the
                    // next poll. Preserve already committed claims so callers
                    // can dispatch them instead of stranding pending runs.
                    Ok(None) | Err(_) => break,
                };
                automation = claim.automation.clone();
                if claim.run.status == "pending" {
                    due.push(claim);
                }
            }
        }
        Ok(due)
    }
}

/// Matches the original Inbox appeared-event filters before using the durable
/// claim ledger. Polling, restarting and overlapping refreshes cannot rerun an item.
pub fn claim_inbox_events(
    service: &mut Service,
    items: &[crate::inbox::Item],
    now: i64,
) -> Result<Vec<DueAutomationRun>, String> {
    claim_inbox_events_retry(service, items, now).map(|(runs, _)| runs)
}

/// A failed match leaves its Inbox item retryable while successful claims are
/// still returned for dispatch. The ledger makes retries of partial batches safe.
pub fn claim_inbox_events_retry(
    service: &mut Service,
    items: &[crate::inbox::Item],
    now: i64,
) -> Result<(Vec<DueAutomationRun>, Vec<crate::inbox::Item>), String> {
    let automations = service.list()?;
    let mut claimed = Vec::new();
    let mut retry = Vec::new();
    for item in items {
        let event = match (item.provider.as_str(), item.kind.as_str(), item.draft) {
            ("github", "pr", true) => "draft_opened",
            ("github", "pr", false) => "pull_request_opened",
            ("github" | "gitlab", "issue", _) => "issue_opened",
            ("gitlab", "pr", _) => "merge_request_opened",
            ("linear" | "jira", _, _) => "issue_created",
            ("azuredevops", "pr", _) => "pull_request_appeared",
            ("azuredevops", "issue", _) => "work_item_appeared",
            _ => continue,
        };
        let identity = if crate::inbox::is_tracker(&item.provider) {
            let id = if !item.id.is_empty() {
                item.id.clone()
            } else if !item.identifier.is_empty() {
                item.identifier.clone()
            } else {
                item.number.to_string()
            };
            format!("{}:issue:{id}", item.provider)
        } else {
            format!(
                "{}:{}:{}:{}",
                item.provider, item.kind, item.repo, item.number
            )
        };
        let event_key = identity
            .trim()
            .to_lowercase()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.' | '/') {
                    c
                } else {
                    '_'
                }
            })
            .take(400)
            .collect::<String>();
        for automation in &automations {
            if !automation.enabled {
                continue;
            }
            let tracker_without_project =
                crate::inbox::is_tracker(&item.provider) && item.project_path.trim().is_empty();
            let path_key = |path: &str| {
                let normalized = path.trim().replace('\\', "/");
                let normalized = normalized.trim_end_matches('/');
                let normalized = if normalized.is_empty() {
                    "/"
                } else {
                    normalized
                };
                if normalized.as_bytes().get(1) == Some(&b':') || normalized.starts_with("//") {
                    normalized.to_lowercase()
                } else {
                    normalized.to_string()
                }
            };
            if !tracker_without_project && path_key(&item.project_path) != path_key(&automation.cwd)
            {
                continue;
            }
            let trigger = automation.triggers().into_iter().find(|trigger| {
                if trigger.kind != item.provider || trigger.event != event {
                    return false;
                }
                let actor = trigger.actor.trim();
                if !actor.is_empty() && !actor.eq_ignore_ascii_case("anyone") {
                    return false;
                }
                let repos: Vec<_> = trigger
                    .repos
                    .iter()
                    .chain(std::iter::once(&trigger.repo))
                    .map(|repo| repo.trim())
                    .filter(|repo| !repo.is_empty())
                    .collect();
                repos.is_empty()
                    || repos
                        .iter()
                        .any(|repo| repo.eq_ignore_ascii_case(item.repo.trim()))
            });
            let Some(trigger) = trigger else {
                continue;
            };
            let provider = crate::inbox::provider_label(&item.provider);
            let kind = if crate::inbox::is_tracker(&item.provider) || item.kind != "pr" {
                "issue"
            } else if item.provider == "gitlab" {
                "merge request"
            } else {
                "pull request"
            };
            let reference = if crate::inbox::is_tracker(&item.provider) {
                if item.identifier.trim().is_empty() {
                    format!("{provider} #{}", item.number)
                } else {
                    item.identifier.trim().to_string()
                }
            } else {
                format!("#{}", item.number)
            };
            let title = if item.title.trim().is_empty() {
                if crate::inbox::is_tracker(&item.provider) {
                    reference.clone()
                } else {
                    format!("{provider} {kind} #{}", item.number)
                }
            } else {
                item.title.trim().to_string()
            };
            let mut prompt = format!(
                "{}\n\nWork on this {provider} {kind}:\n\n{reference} {title}",
                automation.prompt.trim()
            );
            if !item.url.trim().is_empty() {
                prompt.push('\n');
                prompt.push_str(item.url.trim());
            }
            let created = crate::inbox::timestamp_ms(&item.created_at);
            let occurred_at = if created != 0 {
                created
            } else {
                item.updated_ms()
            };
            match service.claim_event(
                &automation.id,
                AutomationEventClaim {
                    event_key: event_key.clone(),
                    event_kind: trigger.kind,
                    event: trigger.event,
                    scheduled_for: if occurred_at == 0 { now } else { occurred_at },
                    prompt,
                },
                now,
            ) {
                Ok(Some(run)) => claimed.push(run),
                Ok(None) => {}
                Err(_) => {
                    if !retry
                        .iter()
                        .any(|existing: &crate::inbox::Item| existing == item)
                    {
                        retry.push(item.clone());
                    }
                }
            }
        }
    }
    Ok((claimed, retry))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local_at(value: &str) -> i64 {
        let naive = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S").unwrap();
        Local
            .from_local_datetime(&naive)
            .earliest()
            .unwrap()
            .timestamp_millis()
    }
    fn save(service: &mut Service, triggers: Vec<AutomationTrigger>) -> Automation {
        let mut draft = AutomationUpsert::new("/repo".into(), "codex".into(), "model".into());
        draft.name = "Review work".into();
        draft.prompt = "Review the change and implement safe fixes.".into();
        draft.triggers = Some(triggers);
        draft.sync_triggers(now_millis());
        service.upsert(draft).unwrap()
    }
    fn item(number: i64) -> crate::inbox::Item {
        crate::inbox::Item {
            provider: "github".into(),
            kind: "pr".into(),
            repo: "acme/app".into(),
            project_path: "/repo".into(),
            number,
            title: format!("Change {number}"),
            url: format!("https://github.com/acme/app/pull/{number}"),
            created_at: "2026-09-19T10:00:00Z".into(),
            ..Default::default()
        }
    }

    #[test]
    fn schedules_skip_weekends_and_advance_exact_occurrence_boundaries() {
        let hourly = AutomationTrigger {
            minute: 15,
            ..AutomationTrigger::new("time", "hourly")
        };
        assert_eq!(
            hourly.next_at(local_at("2026-09-19T10:20:00")),
            local_at("2026-09-19T11:15:00")
        );
        assert_eq!(
            hourly.next_at(local_at("2026-09-19T10:15:00")),
            local_at("2026-09-19T11:15:00")
        );
        let weekdays = AutomationTrigger::new("time", "weekdays");
        assert_eq!(
            weekdays.next_at(local_at("2026-09-18T10:00:00")),
            local_at("2026-09-21T09:00:00")
        );
        let daily = AutomationTrigger {
            time: "18:00".into(),
            ..AutomationTrigger::new("time", "daily")
        };
        assert_eq!(
            daily.next_at(local_at("2026-09-19T10:00:00")),
            local_at("2026-09-19T18:00:00")
        );
        assert_eq!(
            daily.next_at(local_at("2026-09-19T18:00:00")),
            local_at("2026-09-20T18:00:00")
        );
        assert_eq!(
            next_run_at(&[weekdays, daily], local_at("2026-09-19T10:00:00")),
            local_at("2026-09-19T18:00:00")
        );
    }

    #[test]
    fn inbox_claims_filter_projects_repositories_and_actors_and_dedupe_across_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sessions.sqlite");
        let mut service = Service::open(&path).unwrap();
        let trigger = AutomationTrigger {
            repos: vec!["ACME/App".into()],
            ..AutomationTrigger::new("github", "pull_request_opened")
        };
        let automation = save(&mut service, vec![trigger]);
        let mut wrong = item(1);
        wrong.project_path = "/another".into();
        assert!(claim_inbox_events(&mut service, &[wrong], now_millis())
            .unwrap()
            .is_empty());
        let mut wrong = item(2);
        wrong.repo = "acme/other".into();
        assert!(claim_inbox_events(&mut service, &[wrong], now_millis())
            .unwrap()
            .is_empty());
        let claims = claim_inbox_events(&mut service, &[item(3), item(3)], now_millis()).unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(
            claims[0].run.event_key.as_deref(),
            Some("github:pr:acme/app:3")
        );
        assert!(claims[0]
            .run
            .prompt
            .as_deref()
            .unwrap()
            .contains("Work on this GitHub pull request:"));
        drop(service);
        let mut service = Service::open(&path).unwrap();
        assert!(claim_inbox_events(&mut service, &[item(3)], now_millis())
            .unwrap()
            .is_empty());
        let mut draft = AutomationUpsert::from_automation(&automation);
        draft.triggers.as_mut().unwrap()[0].actor = "someone".into();
        draft.sync_triggers(now_millis());
        service.upsert(draft).unwrap();
        assert!(claim_inbox_events(&mut service, &[item(4)], now_millis())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn failed_event_in_batch_keeps_successes_dispatchable_and_failed_item_retryable() {
        let directory = tempfile::tempdir().unwrap();
        let mut service = Service::open(&directory.path().join("sessions.sqlite")).unwrap();
        save(
            &mut service,
            vec![AutomationTrigger::new("github", "pull_request_opened")],
        );
        let mut invalid = item(2);
        invalid.url = "x".repeat(MAX_PROMPT + 10_001);
        let (claims, retry) =
            claim_inbox_events_retry(&mut service, &[item(1), invalid], now_millis()).unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(retry.len(), 1);
        assert_eq!(retry[0].number, 2);
        let (claims, retry) =
            claim_inbox_events_retry(&mut service, &[item(1), item(2)], now_millis()).unwrap();
        assert_eq!(claims.len(), 1);
        assert!(retry.is_empty());
        assert_eq!(
            claims[0].run.event_key.as_deref(),
            Some("github:pr:acme/app:2")
        );
    }

    #[test]
    fn recovery_returns_pending_prompts_and_cancels_interrupted_running_runs() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sessions.sqlite");
        let mut service = Service::open(&path).unwrap();
        let automation = save(&mut service, vec![]);
        let now = now_millis();
        let pending = service.run_now(&automation.id, now).unwrap();
        let running = service.run_now(&automation.id, now + 1).unwrap();
        service
            .update_run(
                &running.id,
                "running",
                Some("session-1".into()),
                None,
                now + 2,
            )
            .unwrap();
        drop(service);
        let mut service = Service::open(&path).unwrap();
        let recovery = service.recover(now + 10, now + 20).unwrap();
        assert_eq!(recovery.len(), 1);
        assert_eq!(recovery[0].run.id, pending.id);
        assert_eq!(
            recovery[0].run.prompt.as_deref(),
            Some(automation.prompt.as_str())
        );
        let history = service.runs(&automation.id).unwrap();
        let interrupted = history.iter().find(|run| run.id == running.id).unwrap();
        assert_eq!(interrupted.status, "cancelled");
        assert_eq!(interrupted.session_id.as_deref(), Some("session-1"));
        assert!(interrupted
            .error
            .as_deref()
            .unwrap()
            .contains("Interrupted"));
    }

    #[test]
    fn due_poll_preserves_successful_claims_when_a_later_transaction_fails() {
        let directory = tempfile::tempdir().unwrap();
        let mut service = Service::open(&directory.path().join("sessions.sqlite")).unwrap();
        let mut healthy = save(&mut service, vec![AutomationTrigger::new("time", "daily")]);
        let mut failing = save(&mut service, vec![AutomationTrigger::new("time", "daily")]);
        let now = now_millis();
        healthy.next_run_at = now - 1;
        failing.next_run_at = now - 1;
        healthy.updated_at = now + 1;
        failing.updated_at = now;
        write_automation(&service.conn, &healthy).unwrap();
        write_automation(&service.conn, &failing).unwrap();
        service.conn.execute_batch(&format!(
            "CREATE TRIGGER reject_one_automation_run BEFORE INSERT ON automation_runs
             WHEN NEW.automation_id = '{}' BEGIN SELECT RAISE(ABORT, 'temporary write failure'); END;",
            failing.id
        )).unwrap();

        let claimed = service.claim_due(now).unwrap();
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].automation.id, healthy.id);
        assert_eq!(claimed[0].run.status, "pending");
        assert_eq!(
            get(&service.conn, &failing.id)
                .unwrap()
                .unwrap()
                .next_run_at,
            now - 1
        );
        assert!(service.runs(&failing.id).unwrap().is_empty());

        service
            .conn
            .execute_batch("DROP TRIGGER reject_one_automation_run;")
            .unwrap();
        let retried = service.claim_due(now).unwrap();
        assert_eq!(retried.len(), 1);
        assert_eq!(retried[0].automation.id, failing.id);
        assert_eq!(service.runs(&healthy.id).unwrap().len(), 1);
    }
}
