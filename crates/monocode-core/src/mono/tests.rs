use super::*;
use habits::{Habit, Schedule};
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn identity_projects_order_and_hidden_sessions_survive_restart() {
    let temp = tempfile::tempdir().unwrap();
    let db = Store::open(&temp.path().join("native.sqlite")).unwrap();
    let mut roster = Roster::default();
    let a = roster
        .create(vec!["/one/app".into(), "/two/app".into()])
        .id
        .clone();
    let other = roster.create(vec!["/else".into()]).id.clone();
    let b = roster.create(vec!["/one/app".into()]).id.clone();
    assert!(roster.get(&a).unwrap().project(Some("app")).is_err());
    assert!(roster.get(&a).unwrap().project(Some("/outside")).is_err());
    assert!(roster.get(&a).unwrap().project(None).is_err());
    assert_eq!(roster.get(&b).unwrap().project(None).unwrap(), "/one/app");
    roster.reorder(&[b.clone(), a.clone()]);
    roster.hidden_sessions.push(17);
    roster.save(&db).unwrap();
    let saved = Roster::load(&db).unwrap();
    assert_eq!(
        saved.monos.iter().map(|m| &m.id).collect::<Vec<_>>(),
        vec![&b, &other, &a]
    );
    assert_eq!(saved.hidden_sessions, vec![17]);
    assert_ne!(saved.monos[0].mascot, saved.monos[1].mascot);
}
#[test]
fn memory_supersedes_uniquely_redacts_and_archives_before_prompt_budget() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let id = "memory-test";
    let secret = "sk-1234567890abcdefghijklmnop";
    let entry = memory::entry(&format!("Use Rust. token={secret}"), "2026-10-19", None).unwrap();
    assert!(!entry.contains(secret));
    let first = memory::add("# User notes\n", &entry);
    assert_eq!(memory::add(&first, &entry), first);
    let new = memory::entry("Use GPUI", "2026-10-19", None).unwrap();
    let replaced = memory::replace(&first, "Use Rust", Some(&new), "2026-10-19").unwrap();
    assert!(replaced.contains("~~"));
    assert!(memory::replace(&replaced, "not present", None, "2026-10-19").is_err());
    let mut many = "# User notes\nDo not drop this user-written line.\n".to_string();
    for n in 0..205 {
        many.push_str(&memory::entry(&format!("fact number {n}"), "2026-10-18", None).unwrap());
        many.push('\n');
    }
    memory::edit(root, id, None, |_| Ok(many.clone())).unwrap();
    let file = memory::read(root, id, None).unwrap().text.unwrap();
    assert!(file.lines().count() <= 200);
    assert!(file.contains("Do not drop"));
    assert!(file.contains("fact number 204"));
    assert!(files::read(root, id, None, "memory/archive.md")
        .unwrap()
        .text
        .unwrap()
        .contains("fact number 0"));
    assert_eq!(
        memory::search(root, id, "fact number 204", None)
            .unwrap()
            .len(),
        1
    );
    assert!(memory::topic("../secret").is_err());
}
#[test]
fn habit_updates_retry_conflicts_without_losing_other_edits() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let id = "habit-test";
    let now = 1_792_411_200_000;
    let a = Habit::new(
        "A".into(),
        "Report changes".into(),
        Schedule::default(),
        now,
    )
    .unwrap();
    let b = Habit::new("B".into(), "Check builds".into(), Schedule::default(), now).unwrap();
    let inject = AtomicBool::new(true);
    habits::update(root, id, |all| {
        if inject.swap(false, Ordering::SeqCst) {
            habits::update(root, id, |other| {
                other.push(b.clone());
                Ok(())
            })?;
        }
        all.push(a.clone());
        Ok(())
    })
    .unwrap();
    let all = habits::load(root, id).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, b.id);
    assert_eq!(all[1].id, a.id);
    let mut h = a;
    h.enabled = false;
    h.next_run_at = now;
    assert!(!h.due(now));
    h.run_requested = true;
    assert!(h.due(now));
    h.running_since = Some(now);
    assert!(!h.due(now));
    assert!(habits::report("`NOTHING_TO_REPORT`").is_none());
    assert!(habits::report("The build failed.\nNOTHING_TO_REPORT")
        .unwrap()
        .contains("build failed"));
}
#[test]
fn schedules_are_strictly_future_and_reject_invalid_fields() {
    let now = 1_792_411_200_000;
    for kind in ["hourly", "daily", "weekdays", "weekly"] {
        let s = Schedule {
            schedule_kind: kind.into(),
            ..Default::default()
        };
        let next = s.next(now).unwrap();
        assert!(next > now);
        assert!(next - now <= 8 * 86400000);
        assert!(s.next(next).unwrap() > next);
    }
    assert!(Schedule {
        time: "25:00".into(),
        ..Default::default()
    }
    .validate()
    .is_err());
    assert!(Schedule {
        minute: 60,
        ..Default::default()
    }
    .validate()
    .is_err());
    let s: Schedule = serde_json::from_str(r#"{"kind":"hourly","minute":15}"#).unwrap();
    assert_eq!(s.minute, 15);
}
#[test]
fn delivery_receipt_is_saved_with_the_user_message() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("native.sqlite");
    let mut db = Store::open(&path).unwrap();
    let session = db.create("codex", "/project").unwrap();
    let row = Row {
        role: "user".into(),
        text: "hello".into(),
        first: true,
        sent_at: None,
    };
    db.write_rows_with_message(session.id, 0, std::slice::from_ref(&row), Some("message-1"))
        .unwrap();
    drop(db);
    let db = Store::open(&path).unwrap();
    assert_eq!(db.load(session.id).unwrap().rows, vec![row]);
    assert_eq!(
        db.setting(&format!("mono-delivered:{}:message-1", session.id))
            .unwrap()
            .as_deref(),
        Some("1")
    );
}

#[test]
fn context_shows_how_to_delegate_without_running_help() {
    let mono = Mono {
        projects: vec!["/repo".into()],
        ..Default::default()
    };
    let text = context(
        &mono,
        &files::AgentFiles {
            id: mono.id.clone(),
            dir: String::new(),
            soul: Some(DEFAULT_SOUL.into()),
            soul_hash: String::new(),
            memory: String::new(),
            memory_hash: String::new(),
            memory_path: String::new(),
            topics: Vec::new(),
        },
        std::path::Path::new("/Applications/Monocode.app/monocode"),
    );
    assert!(text.contains(
        r#"/Applications/Monocode.app/monocode app sessions.start --json '{"project":"<assigned path>","title":"<short title>","prompt":"<complete instructions>","notifyOnComplete":true}'"#
    ));
}

fn launch(session_id: i64, adopted: bool) -> Launch {
    Launch {
        session_id,
        cwd: "/repo".into(),
        title: "Work".into(),
        harness: "claude".into(),
        model: None,
        origin_turn: (5 << 32) | 3,
        notify_on_complete: true,
        completed: false,
        dismissed: false,
        adopted,
    }
}

#[test]
fn a_session_knows_the_mono_that_started_it_but_not_one_that_only_adopted_it() {
    let mut roster = Roster::default();
    let first = roster.create(vec!["/repo".into()]).id.clone();
    let second = roster.create(vec!["/repo".into()]).id.clone();
    roster.get_mut(&first).unwrap().name = "TaskGiver".into();
    roster.get_mut(&first).unwrap().launches = vec![launch(10, false), launch(11, true)];
    roster.get_mut(&second).unwrap().launches = vec![launch(10, true), launch(12, false)];
    let origin = roster.origin_of(10).unwrap();
    assert_eq!(
        (origin.mono.as_str(), origin.name.as_str()),
        (first.as_str(), "TaskGiver")
    );
    assert!(!origin.deleted);
    assert_eq!(roster.origin_of(12).unwrap().mono, second);
    assert!(roster.origin_of(11).is_none(), "adopted, never started");
    assert!(roster.origin_of(99).is_none());
}

#[test]
fn a_deleted_mono_keeps_its_look_for_the_sessions_it_started() {
    let temp = tempfile::tempdir().unwrap();
    let db = Store::open(&temp.path().join("native.sqlite")).unwrap();
    let mut roster = Roster::default();
    let id = roster.create(vec!["/repo".into()]).id.clone();
    let mono = roster.get_mut(&id).unwrap();
    mono.name = "Scout".into();
    mono.launches = vec![launch(20, false), launch(21, true)];
    let (mascot, color) = (mono.mascot.clone(), mono.color);
    assert!(roster.remove(&id));
    assert!(!roster.remove(&id));
    let origin = roster.origin_of(20).unwrap();
    assert!(origin.deleted);
    assert_eq!(
        (origin.name.as_str(), origin.mascot.as_str(), origin.color),
        ("Scout", mascot.as_str(), color)
    );
    assert!(roster.origin_of(21).is_none());
    roster.save(&db).unwrap();
    let saved = Roster::load(&db).unwrap();
    assert!(saved.origin_of(20).unwrap().deleted);
    // A Mono that started nothing leaves no trace behind.
    let other = roster.create(vec![]).id.clone();
    roster.remove(&other);
    assert_eq!(roster.departed.len(), 1);
}

#[test]
fn launches_saved_before_adoption_was_tracked_read_as_started() {
    let launch: Launch = serde_json::from_str(
        r#"{"sessionId":1,"cwd":"/r","title":"t","harness":"claude","model":null,"originTurn":4,"notifyOnComplete":true}"#,
    )
    .unwrap();
    assert!(!launch.adopted);
}
