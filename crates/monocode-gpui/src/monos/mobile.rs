//! A Mono's details as the phone shows them (`mono.profile`): the Overview
//! tab's numbers without its drawing, its habits and documents. And the look
//! the phone edits (`mono.update`), saved as the desktop's own editor does,
//! and its soul and memory as Markdown (`mono.file`, `mono.saveFile`).
use super::dashboard::{Day, levels, local_day, streak};
use super::*;
use crate::shell::monos::MonoSession;
use chrono::{DateTime, Local};
use monocode_core::mono::files;
use monocode_core::project_appearance::MASCOTS;
use monocode_core::time::{now_ms, relative};
use serde_json::{Value, json};

impl Monos {
    /// The Overview as data, read from what the desktop already holds: the
    /// contributions as last loaded (a first look starts them loading), the
    /// Mono's sessions among `views`, and its habits, read from disk for a
    /// Mono that isn't the one open here.
    pub(crate) fn mobile_profile(
        &mut self,
        id: &str,
        views: &[MonoSession],
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let mono = self
            .roster
            .get(id)
            .cloned()
            .ok_or("Mono is no longer available")?;
        let now = now_ms();
        let today = Local::now().date_naive();
        let mine: Vec<&MonoSession> = views
            .iter()
            .filter(|v| mono.launches.iter().any(|l| l.session_id == v.id))
            .collect();
        let state = self
            .chats
            .get(&mono.id)
            .map(|chat| chat.read(cx).mono_status().to_string())
            .unwrap_or_else(|| "idle".into());
        let busy: Vec<_> = mine.iter().filter(|v| v.busy).collect();
        let state = match state.as_str() {
            "idle" if !busy.is_empty() => "working",
            s => s,
        }
        .to_string();
        // The dashboard's live line, word for word.
        let status = match (state.as_str(), busy.first()) {
            ("needs-you", _) => "Waiting for your input".to_string(),
            (_, Some(v)) if busy.len() > 1 => {
                format!("Working on {} +{} more", v.title, busy.len() - 1)
            }
            (_, Some(v)) => format!("Working on {}", v.title),
            ("working", None) => "Thinking…".to_string(),
            _ => match mine.iter().map(|v| v.updated_at).max().filter(|t| *t > 0) {
                Some(at) => format!("Idle · last active {} ago", relative(at, now)),
                None => "Idle".to_string(),
            },
        };

        let (days, merged_today) = self.contributions_for(&mono, today, cx);
        let today_stats = days.last().copied();
        let habits: Vec<Habit> = if self.active.as_deref() == Some(id) {
            self.habits.clone()
        } else {
            habits::load(&self.root, id).unwrap_or_default()
        };
        let ran_today: usize = habits
            .iter()
            .map(|h| {
                h.runs
                    .iter()
                    .filter(|r| local_day(r.at) == Some(today))
                    .count()
            })
            .sum();
        let still_today = habits
            .iter()
            .filter(|h| h.enabled && h.next_run_at > now && local_day(h.next_run_at) == Some(today))
            .count();
        let mut upcoming: Vec<_> = habits
            .iter()
            .filter(|h| h.enabled || self.habit_running_since(h).is_some())
            .map(|h| (self.habit_running_since(h), h))
            .collect();
        upcoming.sort_by_key(|(running, h)| (running.is_none(), h.next_run_at));
        let when = |running: Option<u64>, habit: &Habit| match running {
            Some(since) => format!("Running · {}", relative(since, now)),
            None => {
                let day = DateTime::from_timestamp_millis(habit.next_run_at as i64)
                    .map(|t| {
                        let format = if local_day(habit.next_run_at) == Some(today) {
                            "%H:%M"
                        } else {
                            "%a"
                        };
                        t.with_timezone(&Local).format(format).to_string()
                    })
                    .unwrap_or_default();
                match relative(now, habit.next_run_at.max(now)) {
                    soon if soon == "now" => format!("{day} · soon"),
                    later => format!("{day} · in {later}"),
                }
            }
        };
        let mut documents = mono.documents.clone();
        documents.sort_by_key(|d| std::cmp::Reverse(d.updated_at.max(d.created_at)));

        Ok(json!({
            "status": status,
            "state": state,
            "contributions": {
                "total": days.iter().map(Day::count).sum::<u32>(),
                "streak": streak(&days),
                // Day by day from a Sunday, 0 to 4, as the graph's columns.
                "levels": levels(&days),
            },
            "today": {
                "merged": merged_today,
                "additions": today_stats.map_or(0, |d| d.additions),
                "deletions": today_stats.map_or(0, |d| d.deletions),
                "sessions": mine.iter().filter(|v| local_day(v.updated_at) == Some(today)).count(),
                "habitsRan": ran_today,
                "habitsTotal": ran_today + still_today,
            },
            "upNext": upcoming.iter().take(3).map(|(running, h)| json!({
                "id": h.id,
                "name": h.name,
                "schedule": h.schedule.label(),
                "when": when(*running, h),
                "running": running.is_some(),
            })).collect::<Vec<_>>(),
            // Each habit whole for its page, its latest runs first.
            "habits": habits.iter().map(|h| json!({
                "id": h.id,
                "name": h.name,
                "instructions": h.instructions,
                "schedule": h.schedule.label(),
                "enabled": h.enabled,
                "when": when(self.habit_running_since(h), h),
                "running": self.habit_running_since(h).is_some(),
                "lastRunAt": h.last_run_at,
                "lastOutcome": h.last_outcome,
                "lastError": h.last_error,
                "runs": h.runs.iter().rev().take(10).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "documents": documents.iter().map(|d| json!({
                "id": d.id,
                "title": d.title,
                "kind": d.kind,
                "summary": d.summary,
                "updatedAt": d.updated_at.max(d.created_at),
            })).collect::<Vec<_>>(),
            "colors": monocode_core::mono::COLORS_MONO.iter().map(|c| format!("#{c:06x}")).collect::<Vec<_>>(),
            "mascots": MASCOTS,
        }))
    }

    /// A new Mono from the phone's drawer, with the roster's next mascot and
    /// colour and no projects yet, its chat ready to talk to.
    pub(crate) fn mobile_create(&mut self, cx: &mut Context<Self>) -> Result<Value, String> {
        let id = self.roster.create(Vec::new()).id.clone();
        self.save(cx);
        self.ensure_chat(&id, cx);
        cx.notify();
        Ok(json!({ "monoId": id }))
    }

    /// The phone's Reset conversation or Delete Mono, confirmed there.
    pub(crate) fn mobile_reset(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        self.roster.get(id).ok_or("Mono is no longer available")?;
        self.clear_conversation(id, cx);
        cx.notify();
        Ok(Value::Null)
    }
    pub(crate) fn mobile_delete(
        &mut self,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        self.roster.get(id).ok_or("Mono is no longer available")?;
        self.remove_mono(id, cx);
        cx.notify();
        Ok(Value::Null)
    }

    /// The name, mascot and colour from the phone, kept as the desktop's
    /// editor keeps them: a name of at most 40 characters (empty falls back
    /// to the mascot's), a known mascot, a colour as `#rrggbb`.
    pub(crate) fn mobile_update(
        &mut self,
        id: &str,
        params: &Value,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let mono = self
            .roster
            .get_mut(id)
            .ok_or("Mono is no longer available")?;
        if let Some(name) = params["name"].as_str() {
            mono.name = name.trim().chars().take(40).collect();
        }
        if let Some(mascot) = params["mascot"].as_str() {
            if !MASCOTS.contains(&mascot) {
                return Err("Unknown mascot".into());
            }
            mono.mascot = mascot.into();
        }
        if let Some(color) = params["color"].as_str() {
            mono.color = u32::from_str_radix(color.trim_start_matches('#'), 16)
                .ok()
                .filter(|c| *c <= 0xffffff)
                .ok_or("Unknown colour")?;
        }
        // Projects as the desktop's settings add and remove them.
        if let Some(path) = params["addPath"].as_str() {
            mono.projects.push(path.into());
        }
        if let Some(paths) = params["removePaths"].as_array() {
            mono.projects
                .retain(|p| !paths.iter().any(|r| r.as_str() == Some(p.as_str())));
        }
        mono.normalize();
        self.configure_chat(id, cx);
        self.save(cx);
        cx.notify();
        Ok(Value::Null)
    }

    /// The soul or memory whole, as the phone's editor opens it, or saved over
    /// the text it opened: a save whose `expectedHash` no longer matches is
    /// refused, so the phone never overwrites what the Mono wrote meanwhile.
    pub(crate) fn mobile_file(
        &mut self,
        id: &str,
        params: &Value,
        cx: &mut Context<Self>,
    ) -> gpui::Task<Result<Value, String>> {
        let path = match params["file"].as_str() {
            Some("soul") => "SOUL.md",
            Some("memory") => "MEMORY.md",
            _ => return gpui::Task::ready(Err("Unknown file".into())),
        };
        if self.roster.get(id).is_none() {
            return gpui::Task::ready(Err("Mono is no longer available".into()));
        }
        let root = self.root.clone();
        let target = id.to_owned();
        let save = params["text"].as_str().map(|text| {
            (
                text.to_owned(),
                params["expectedHash"].as_str().map(str::to_owned),
            )
        });
        let saving = save.is_some();
        let work = cx.background_executor().spawn(async move {
            match save {
                Some((text, expected)) => {
                    match files::save(&root, &target, None, path, &text, expected.as_deref()) {
                        Ok(hash) => Ok(json!({ "hash": hash })),
                        Err(e) if e == files::CONFLICT => Err(
                            "It changed on the desktop while you were editing. Reopen it to see the latest."
                                .into(),
                        ),
                        Err(e) => Err(e),
                    }
                }
                None => files::read(&root, &target, None, path).map(|file| {
                    json!({ "text": file.text.unwrap_or_default(), "hash": file.hash })
                }),
            }
        });
        let id = id.to_owned();
        cx.spawn(async move |this, cx| {
            let result = work.await;
            // The desktop's open editor and the next turn read the new text.
            if saving && result.is_ok() {
                let _ = this.update(cx, |this, cx| {
                    this.reload_files(&id, cx);
                    cx.notify();
                });
            }
            result
        })
    }
}
