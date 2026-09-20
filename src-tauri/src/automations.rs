use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::session_store::{now_millis, validate_id, SessionStore};

pub(crate) const CHANGED: &str = "monocode:automations-changed";

const MAX_NAME: usize = 200;
const MAX_PROMPT: usize = 1_000_000;
const MAX_TRIGGERS: usize = 20;
const MAX_RUNS_PER_AUTOMATION: i64 = 100;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Automation {
    id: String,
    name: String,
    prompt: String,
    harness: String,
    model: String,
    #[serde(default)]
    model_settings: HashMap<String, String>,
    cwd: String,
    workspace_mode: String,
    #[serde(default)]
    worktree_cwd: String,
    #[serde(default)]
    session_folder_id: String,
    reuse_session: bool,
    runtime_mode: String,
    #[serde(default = "default_trigger_kind")]
    trigger_kind: String,
    #[serde(default)]
    trigger_event: String,
    schedule_kind: String,
    minute: i64,
    time: String,
    day_of_week: i64,
    #[serde(default)]
    triggers: Option<Vec<AutomationTrigger>>,
    missed_run_grace_minutes: i64,
    enabled: bool,
    next_run_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_run_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_run_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_run_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_session_id: Option<String>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationUpsert {
    id: String,
    name: String,
    prompt: String,
    harness: String,
    model: String,
    #[serde(default)]
    model_settings: HashMap<String, String>,
    cwd: String,
    workspace_mode: String,
    #[serde(default)]
    worktree_cwd: String,
    #[serde(default)]
    session_folder_id: String,
    #[serde(default)]
    reuse_session: bool,
    runtime_mode: String,
    #[serde(default = "default_trigger_kind")]
    trigger_kind: String,
    #[serde(default)]
    trigger_event: String,
    schedule_kind: String,
    minute: i64,
    time: String,
    day_of_week: i64,
    #[serde(default)]
    triggers: Option<Vec<AutomationTrigger>>,
    missed_run_grace_minutes: i64,
    #[serde(default = "default_true")]
    enabled: bool,
    next_run_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRun {
    id: String,
    automation_id: String,
    trigger: String,
    scheduled_for: i64,
    created_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    started_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completed_at: Option<i64>,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    event_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    event_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prompt: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationTrigger {
    #[serde(default)]
    id: String,
    kind: String,
    #[serde(default)]
    event: String,
    #[serde(default = "default_schedule_kind")]
    schedule_kind: String,
    #[serde(default)]
    minute: i64,
    #[serde(default = "default_time")]
    time: String,
    #[serde(default = "default_day_of_week")]
    day_of_week: i64,
    #[serde(default)]
    repos: Vec<String>,
    #[serde(default)]
    repo: String,
    #[serde(default)]
    branch: String,
    #[serde(default = "default_actor")]
    actor: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DueAutomationRun {
    automation: Automation,
    run: AutomationRun,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomationEventClaim {
    event_key: String,
    event_kind: String,
    event: String,
    scheduled_for: i64,
    prompt: String,
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
        "time" | "github" | "linear" | "gitlab"
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
        "time" | "github" | "linear" | "gitlab"
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
            trigger.id = Uuid::new_v4().to_string();
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
        id: Uuid::new_v4().to_string(),
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

#[tauri::command(async)]
pub fn automations_list(store: State<'_, SessionStore>) -> Result<Vec<Automation>, String> {
    let conn = store.lock_conn()?;
    list(&conn)
}

#[tauri::command(async)]
pub fn automations_upsert(
    app: AppHandle,
    store: State<'_, SessionStore>,
    automation: AutomationUpsert,
) -> Result<Automation, String> {
    let now = now_millis();
    validate_upsert(&automation, now)?;
    let triggers = Some(normalize_triggers(&automation));
    let conn = store.lock_conn()?;
    let previous = get(&conn, &automation.id)?;
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
    write_automation(&conn, &saved)?;
    drop(conn);
    let _ = app.emit(CHANGED, ());
    Ok(saved)
}

#[tauri::command(async)]
pub fn automations_delete(
    app: AppHandle,
    store: State<'_, SessionStore>,
    id: String,
) -> Result<(), String> {
    validate_id(&id, "automation")?;
    let conn = store.lock_conn()?;
    conn.execute("DELETE FROM automations WHERE id = ?1", [id])
        .map_err(|error| error.to_string())?;
    drop(conn);
    let _ = app.emit(CHANGED, ());
    Ok(())
}

#[tauri::command(async)]
pub fn automation_runs_list(
    store: State<'_, SessionStore>,
    automation_id: String,
) -> Result<Vec<AutomationRun>, String> {
    validate_id(&automation_id, "automation")?;
    let conn = store.lock_conn()?;
    let mut runs = list_runs(&conn, &automation_id)?;
    // Launch prompts can be large and are only needed by restart recovery, not
    // by the run-history surface.
    for run in &mut runs {
        run.prompt = None;
    }
    Ok(runs)
}

#[tauri::command(async)]
pub fn automation_runs_recover(
    app: AppHandle,
    store: State<'_, SessionStore>,
    started_before: i64,
    now: i64,
) -> Result<Vec<DueAutomationRun>, String> {
    let mut conn = store.lock_conn()?;
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
        runs.push(serde_json::from_str::<AutomationRun>(&raw).map_err(|error| error.to_string())?);
    }
    drop(statement);

    let mut due = Vec::new();
    let mut changed = false;
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
            changed = true;
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
            changed = true;
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
    drop(conn);
    if changed || !due.is_empty() {
        let _ = app.emit(CHANGED, ());
    }
    Ok(due)
}

#[tauri::command(async)]
pub fn automation_run_now(
    app: AppHandle,
    store: State<'_, SessionStore>,
    automation_id: String,
    now: i64,
) -> Result<AutomationRun, String> {
    validate_id(&automation_id, "automation")?;
    let mut conn = store.lock_conn()?;
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    let mut automation =
        get(&tx, &automation_id)?.ok_or_else(|| "Automation not found.".to_string())?;
    let run = new_run(&automation_id, "manual", now, now, &automation.prompt);
    automation.last_run_at = Some(now);
    automation.last_run_status = Some(run.status.clone());
    automation.last_run_error = None;
    write_run(&tx, &run)?;
    write_automation(&tx, &automation)?;
    trim_history(&tx, &automation_id)?;
    tx.commit().map_err(|error| error.to_string())?;
    drop(conn);
    let _ = app.emit(CHANGED, ());
    Ok(run)
}

#[tauri::command(async)]
pub fn automations_claim_due(
    app: AppHandle,
    store: State<'_, SessionStore>,
    automation_id: String,
    expected_next_run_at: i64,
    next_run_at: i64,
    now: i64,
) -> Result<Option<DueAutomationRun>, String> {
    validate_id(&automation_id, "automation")?;
    if next_run_at <= expected_next_run_at {
        return Err("The following automation occurrence must advance the schedule.".into());
    }
    let mut conn = store.lock_conn()?;
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    let current = get(&tx, &automation_id)?.ok_or_else(|| "Automation not found.".to_string())?;
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
        get(&tx, &automation_id)?.ok_or_else(|| "Automation not found.".to_string())?;
    automation.next_run_at = next_run_at;
    let mut run = new_run(
        &automation_id,
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
    trim_history(&tx, &automation_id)?;
    tx.commit().map_err(|error| error.to_string())?;
    drop(conn);
    let _ = app.emit(CHANGED, ());
    Ok(Some(DueAutomationRun { automation, run }))
}

#[tauri::command(async)]
pub fn automations_claim_event(
    app: AppHandle,
    store: State<'_, SessionStore>,
    automation_id: String,
    claim: AutomationEventClaim,
    now: i64,
) -> Result<Option<DueAutomationRun>, String> {
    validate_id(&automation_id, "automation")?;
    validate_event_key(&claim.event_key)?;
    if !matches!(claim.event_kind.as_str(), "github" | "linear" | "gitlab") {
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
    let mut conn = store.lock_conn()?;
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    let mut automation =
        get(&tx, &automation_id)?.ok_or_else(|| "Automation not found.".to_string())?;
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
        &automation_id,
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
    trim_history(&tx, &automation_id)?;
    tx.commit().map_err(|error| error.to_string())?;
    drop(conn);
    let _ = app.emit(CHANGED, ());
    Ok(Some(DueAutomationRun { automation, run }))
}

#[tauri::command(async)]
pub fn automation_run_update(
    app: AppHandle,
    store: State<'_, SessionStore>,
    run_id: String,
    status: String,
    session_id: Option<String>,
    error: Option<String>,
    now: i64,
) -> Result<AutomationRun, String> {
    validate_id(&run_id, "automation run")?;
    if let Some(session_id) = session_id.as_deref() {
        validate_id(session_id, "session")?;
    }
    if !matches!(
        status.as_str(),
        "pending" | "running" | "succeeded" | "failed" | "skipped" | "cancelled"
    ) {
        return Err("Invalid automation run status.".into());
    }
    let mut conn = store.lock_conn()?;
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
    let mut run: AutomationRun = serde_json::from_str(&raw).map_err(|error| error.to_string())?;
    run.status = status;
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
    drop(conn);
    let _ = app.emit(CHANGED, ());
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_time_fields() {
        assert!(valid_time("09:30"));
        assert!(valid_time("23:59"));
        assert!(!valid_time("24:00"));
        assert!(!valid_time("9:30"));
    }

    #[test]
    fn existing_automations_default_to_time_triggers() {
        let automation = Automation {
            id: "automation-id".into(),
            name: "Audit".into(),
            prompt: "Review the repository".into(),
            harness: "codex".into(),
            model: "model".into(),
            model_settings: HashMap::new(),
            cwd: "/tmp".into(),
            workspace_mode: "worktree".into(),
            worktree_cwd: String::new(),
            session_folder_id: String::new(),
            reuse_session: false,
            runtime_mode: "auto".into(),
            trigger_kind: "github".into(),
            trigger_event: "pull_request_opened".into(),
            schedule_kind: "weekdays".into(),
            minute: 0,
            time: "09:00".into(),
            day_of_week: 1,
            triggers: None,
            missed_run_grace_minutes: 720,
            enabled: true,
            next_run_at: 1,
            last_run_at: None,
            last_run_status: None,
            last_run_error: None,
            last_session_id: None,
            created_at: 1,
            updated_at: 1,
        };
        let mut stored = serde_json::to_value(automation).unwrap();
        stored
            .as_object_mut()
            .unwrap()
            .remove("triggerKind")
            .unwrap();
        stored
            .as_object_mut()
            .unwrap()
            .remove("triggerEvent")
            .unwrap();

        let restored: Automation = serde_json::from_value(stored).unwrap();
        assert_eq!(restored.trigger_kind, "time");
        assert!(restored.trigger_event.is_empty());
    }

    #[test]
    fn missing_triggers_hydrate_from_legacy_fields() {
        let automation = Automation {
            id: "automation-id".into(),
            name: "Audit".into(),
            prompt: "Review the repository".into(),
            harness: "codex".into(),
            model: "model".into(),
            model_settings: HashMap::new(),
            cwd: "/tmp".into(),
            workspace_mode: "worktree".into(),
            worktree_cwd: String::new(),
            session_folder_id: String::new(),
            reuse_session: false,
            runtime_mode: "auto".into(),
            trigger_kind: "gitlab".into(),
            trigger_event: "merge_request_opened".into(),
            schedule_kind: "weekdays".into(),
            minute: 0,
            time: "09:00".into(),
            day_of_week: 1,
            triggers: None,
            missed_run_grace_minutes: 720,
            enabled: true,
            next_run_at: 1,
            last_run_at: None,
            last_run_status: None,
            last_run_error: None,
            last_session_id: None,
            created_at: 1,
            updated_at: 1,
        };
        let mut stored = serde_json::to_value(&automation).unwrap();
        stored.as_object_mut().unwrap().remove("triggers");
        let mut restored: Automation = serde_json::from_value(stored).unwrap();
        assert!(restored.triggers.is_none());
        hydrate_triggers(&mut restored);
        let triggers = restored.triggers.as_ref().unwrap();
        assert_eq!(triggers.len(), 1);
        assert_eq!(triggers[0].kind, "gitlab");
        assert_eq!(triggers[0].event, "merge_request_opened");
    }

    #[test]
    fn empty_triggers_stay_empty() {
        let mut restored: Automation = serde_json::from_value(serde_json::json!({
            "id": "automation-id",
            "name": "Audit",
            "prompt": "Review the repository",
            "harness": "codex",
            "model": "model",
            "cwd": "/tmp",
            "workspaceMode": "worktree",
            "reuseSession": false,
            "runtimeMode": "auto",
            "triggerKind": "time",
            "triggerEvent": "",
            "scheduleKind": "weekdays",
            "minute": 0,
            "time": "09:00",
            "dayOfWeek": 1,
            "triggers": [],
            "missedRunGraceMinutes": 720,
            "enabled": true,
            "nextRunAt": 1,
            "createdAt": 1,
            "updatedAt": 1
        }))
        .unwrap();
        hydrate_triggers(&mut restored);
        assert!(restored.triggers.as_ref().unwrap().is_empty());
    }

    #[test]
    fn trims_run_history_to_the_configured_limit() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_tables(&conn).unwrap();
        conn.execute(
            "INSERT INTO automations (id, definition_json, enabled, next_run_at, updated_at)
             VALUES ('automation-id', '{}', 1, 1, 1)",
            [],
        )
        .unwrap();
        for index in 0..=MAX_RUNS_PER_AUTOMATION {
            let run = AutomationRun {
                id: format!("run-{index}"),
                automation_id: "automation-id".into(),
                trigger: "manual".into(),
                scheduled_for: index,
                created_at: index,
                started_at: None,
                completed_at: Some(index),
                status: "succeeded".into(),
                session_id: None,
                error: None,
                event_key: None,
                event_kind: None,
                event: None,
                prompt: Some("Run the automation".into()),
            };
            write_run(&conn, &run).unwrap();
        }

        trim_history(&conn, "automation-id").unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM automation_runs WHERE automation_id = 'automation-id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, MAX_RUNS_PER_AUTOMATION);
    }

    #[test]
    fn trim_history_preserves_nonterminal_runs() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_tables(&conn).unwrap();
        conn.execute(
            "INSERT INTO automations (id, definition_json, enabled, next_run_at, updated_at)
             VALUES ('automation-id', '{}', 1, 1, 1)",
            [],
        )
        .unwrap();
        for index in 0..=MAX_RUNS_PER_AUTOMATION {
            let run = AutomationRun {
                id: format!("run-{index}"),
                automation_id: "automation-id".into(),
                trigger: "manual".into(),
                scheduled_for: index,
                created_at: index,
                started_at: None,
                completed_at: None,
                status: "pending".into(),
                session_id: None,
                error: None,
                event_key: None,
                event_kind: None,
                event: None,
                prompt: Some("Run the automation".into()),
            };
            write_run(&conn, &run).unwrap();
        }

        trim_history(&conn, "automation-id").unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM automation_runs WHERE automation_id = 'automation-id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, MAX_RUNS_PER_AUTOMATION + 1);
    }

    #[test]
    fn accepts_inbox_event_keys() {
        assert!(validate_event_key("github:pr:acme/web:12").is_ok());
        assert!(validate_event_key("linear:issue:eng-12").is_ok());
        assert!(validate_event_key("").is_err());
        assert!(validate_event_key("github:pr:acme web:12").is_err());
    }

    #[test]
    fn claims_each_event_key_once() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_tables(&conn).unwrap();
        conn.execute(
            "INSERT INTO automations (id, definition_json, enabled, next_run_at, updated_at)
             VALUES ('automation-id', '{}', 1, 1, 1)",
            [],
        )
        .unwrap();
        let claim = || {
            conn.execute(
                "INSERT OR IGNORE INTO automation_event_claims
                 (automation_id, event_key, created_at) VALUES (?1, ?2, ?3)",
                params!["automation-id", "github:pr:acme/web:12", 1],
            )
            .unwrap()
        };

        assert_eq!(claim(), 1);
        assert_eq!(claim(), 0);
    }
}
