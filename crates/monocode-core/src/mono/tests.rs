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
#[test]
fn a_duplicate_keeps_its_identity_but_not_its_chat_memory_or_history() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let now = 1_792_411_200_000;
    let mut roster = Roster::default();
    let source_id = roster.create(vec!["/code/app".into()]).id.clone();
    let after = roster.create(vec!["/else".into()]).id.clone();
    let source = roster.get_mut(&source_id).unwrap();
    source.name = "Scout".into();
    source.session_id = Some(9);
    source.seen_rows = 4;
    source.habit_sessions = vec![3];
    let source = source.clone();

    let copy = source.duplicate("/code/web");
    assert_ne!(copy.id, source.id);
    assert_eq!(copy.name, "Scout · web");
    assert_eq!((&copy.mascot, copy.color), (&source.mascot, source.color));
    assert_eq!(copy.projects, vec!["/code/web".to_string()]);
    assert_eq!(copy.session_id, None);
    assert_eq!(copy.seen_rows, 0);
    assert!(copy.habit_sessions.is_empty());
    let long = Mono {
        name: "x".repeat(40),
        ..source.clone()
    }
    .duplicate("/code/web");
    assert_eq!(long.name.chars().count(), 40);
    assert!(long.name.ends_with(" · web"));

    files::save(root, &source.id, None, "SOUL.md", "# Soul\nBe brief.", None).unwrap();
    files::save(root, &source.id, None, "MEMORY.md", "- private fact", None).unwrap();
    let mut habit = Habit::new(
        "Digest".into(),
        "Summarise /code/app, not /code/app-web.".into(),
        Schedule::default(),
        now - 86_400_000,
    )
    .unwrap();
    habit.enabled = false;
    habit.last_outcome = Some("posted".into());
    habit.running_since = Some(now);
    habits::update(root, &source.id, |all| {
        all.push(habit.clone());
        Ok(())
    })
    .unwrap();
    copy_identity(root, &source, &copy, now).unwrap();
    let files = load_files(root, &copy).unwrap();
    assert_eq!(files.soul.as_deref(), Some("# Soul\nBe brief."));
    assert_eq!(files.memory, "");
    let copied = habits::load(root, &copy.id).unwrap();
    assert_eq!(copied.len(), 1);
    let h = &copied[0];
    assert_ne!(h.id, habit.id);
    assert!(!h.enabled);
    assert_eq!(h.instructions, "Summarise /code/web, not /code/app-web.");
    assert_eq!((h.last_outcome.as_ref(), h.running_since), (None, None));
    assert!(h.next_run_at > now);

    roster.insert_after(&source.id, copy.clone());
    assert_eq!(
        roster.monos.iter().map(|m| &m.id).collect::<Vec<_>>(),
        vec![&source.id, &copy.id, &after]
    );
}
