use super::*;
use std::sync::{Arc, Barrier};

struct Fixture {
    temp: tempfile::TempDir,
    data: PathBuf,
    home: PathBuf,
    source: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let canonical_root = fs::canonicalize(temp.path()).unwrap();
        let data = canonical_root.join("app data");
        let home = canonical_root.join("isolated home");
        let source = canonical_root.join("original skill");
        fs::create_dir(&home).unwrap();
        fs::create_dir(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            instructions("Use references/info.txt."),
        )
        .unwrap();
        Self {
            temp,
            data,
            home,
            source,
        }
    }

    fn manager(&self) -> SkillManager {
        SkillManager::open(&self.data, &self.home).unwrap()
    }
    fn shared(&self) -> PathBuf {
        self.home.join(".agents/skills/example-skill")
    }

    fn root(&self) -> PathBuf {
        fs::canonicalize(self.temp.path()).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fn writable(path: &Path) {
            let Ok(metadata) = fs::symlink_metadata(path) else {
                return;
            };
            if metadata.file_type().is_symlink() {
                return;
            }
            let mut permissions = metadata.permissions();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                permissions.set_mode(if metadata.is_dir() { 0o755 } else { 0o644 });
            }
            #[cfg(not(unix))]
            #[allow(clippy::permissions_set_readonly_false)]
            {
                // This branch clears the Windows read-only flag. Unix uses explicit modes.
                permissions.set_readonly(false);
            }
            fs::set_permissions(path, permissions).unwrap();
            if metadata.is_dir() {
                for child in fs::read_dir(path).unwrap() {
                    writable(&child.unwrap().path());
                }
            }
        }
        writable(self.temp.path());
    }
}

fn instructions(body: &str) -> String {
    format!("---\nname: example-skill\ndescription: |\n  A skill with a multiline description.\n  It preserves supporting resources.\nprovider-extension:\n  arbitrary: [one, two]\n---\n{body}\n")
}

fn change_source(manager: &SkillManager, id: &str, text: &str) {
    fs::write(
        manager.source_path(id).unwrap().join("SKILL.md"),
        instructions(text),
    )
    .unwrap();
}

#[test]
fn snapshot_reports_the_remembered_plan_and_catalog_with_one_generation() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let empty = manager.snapshot().unwrap();
    assert_eq!(empty.generation, 0);
    assert!(empty.entries.is_empty());
    assert!(empty
        .targets
        .iter()
        .flat_map(|target| &target.providers)
        .all(|provider| provider != "droid"));

    let imported = manager.import(&fixture.source).unwrap();
    let account = ExportTarget::new(
        "claude-account",
        fixture.root().join("account/skills"),
        vec!["claude".into()],
    );
    let report = manager.reconcile(std::slice::from_ref(&account)).unwrap();
    let snapshot = manager.snapshot().unwrap();
    assert_eq!(snapshot.generation, report.generation);
    assert_eq!(snapshot.entries.len(), 1);
    assert_eq!(snapshot.entries[0].id, imported.entry.id);
    assert!(snapshot.targets.contains(&account));
    assert!(snapshot.entries[0]
        .statuses
        .iter()
        .any(|status| status.target_key == account.key && status.state == ExportState::Exported));

    let json = serde_json::to_value(&snapshot).unwrap();
    assert!(json["entries"][0]["source_path"].is_string());
    assert!(json["entries"][0]["applied_path"].is_string());
    assert_eq!(
        snapshot.entries[0].applied_path,
        manager.applied_path(&imported.entry.id).unwrap()
    );
    assert!(json["entries"][0].get("sourcePath").is_none());
    assert_eq!(json["entries"][0]["statuses"][0]["state"], "exported");
    assert_eq!(json["targets"][0]["key"], "shared");
    assert_eq!(manager.snapshot().unwrap().generation, snapshot.generation);

    let applied = snapshot.entries[0].applied_path.join("SKILL.md");
    let original_bytes = fs::read(&applied).unwrap();
    change_source(&manager, &imported.entry.id, "New applied instructions.");
    manager.apply(&imported.entry.id).unwrap();
    let newer = manager.snapshot().unwrap();
    assert!(newer.generation > snapshot.generation);
    assert_ne!(
        newer.entries[0].applied_path,
        snapshot.entries[0].applied_path
    );
    assert_eq!(fs::read(applied).unwrap(), original_bytes);
}

#[test]
fn imports_full_bundle_and_preserves_bytes_executable_modes_and_unknown_yaml() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.source.join("references")).unwrap();
    fs::create_dir(fixture.source.join("empty-directory")).unwrap();
    fs::write(
        fixture.source.join("references/info.txt"),
        b"resource\0bytes\n",
    )
    .unwrap();
    fs::write(fixture.source.join("run.sh"), b"#!/bin/sh\necho skill\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            fixture.source.join("run.sh"),
            fs::Permissions::from_mode(0o751),
        )
        .unwrap();
    }
    let manager = fixture.manager();
    let imported = manager.import(&fixture.source).unwrap();
    assert_eq!(imported.entry.revision, 1);
    assert!(imported.entry.description.contains('\n'));
    assert!(!imported.entry.warnings.is_empty());
    for root in default_targets(&fixture.home) {
        let copied = root.root.join("example-skill");
        assert_eq!(
            fs::read(copied.join("SKILL.md")).unwrap(),
            fs::read(fixture.source.join("SKILL.md")).unwrap()
        );
        assert_eq!(
            fs::read(copied.join("references/info.txt")).unwrap(),
            b"resource\0bytes\n"
        );
        assert!(copied.join("empty-directory").is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(copied.join("run.sh"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o111,
                0o111
            );
        }
    }
    assert_eq!(imported.report.changed_paths.len(), 4);
    assert!(imported
        .entry
        .statuses
        .iter()
        .filter(|s| s.state == ExportState::Exported)
        .all(|s| s.detail.contains("not been verified")));
    let unsupported = imported
        .entry
        .statuses
        .iter()
        .find(|s| s.state == ExportState::Unsupported)
        .unwrap();
    assert!(unsupported.path.as_os_str().is_empty());
}

#[test]
fn unchanged_reconcile_performs_no_manifest_or_export_writes() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    manager.import(&fixture.source).unwrap();
    let generation = manager.generation().unwrap();
    let registry_before = fs::metadata(manager.registry_path())
        .unwrap()
        .modified()
        .unwrap();
    let export_before = fs::metadata(fixture.shared().join("SKILL.md"))
        .unwrap()
        .modified()
        .unwrap();
    let report = manager.reconcile(&[]).unwrap();
    assert!(report.changed_paths.is_empty());
    assert_eq!(report.generation, generation);
    assert_eq!(
        fs::metadata(manager.registry_path())
            .unwrap()
            .modified()
            .unwrap(),
        registry_before
    );
    assert_eq!(
        fs::metadata(fixture.shared().join("SKILL.md"))
            .unwrap()
            .modified()
            .unwrap(),
        export_before
    );
}

#[test]
fn same_name_same_content_is_idempotent_and_different_content_is_conflict() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let first = manager.import(&fixture.source).unwrap();
    let generation = manager.generation().unwrap();
    let second = manager.import(&fixture.source).unwrap();
    assert!(second.already_present);
    assert_eq!(first.entry.id, second.entry.id);
    assert_eq!(manager.generation().unwrap(), generation);
    fs::write(
        fixture.source.join("SKILL.md"),
        instructions("Different instructions."),
    )
    .unwrap();
    assert!(matches!(
        manager.import(&fixture.source),
        Err(Error::NameConflict { .. })
    ));
    assert_eq!(manager.entries().unwrap().len(), 1);
}

#[test]
fn unmanaged_destination_is_preserved_even_if_content_matches() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.shared()).unwrap();
    let bytes = fs::read(fixture.source.join("SKILL.md")).unwrap();
    fs::write(fixture.shared().join("SKILL.md"), &bytes).unwrap();
    let manager = fixture.manager();
    let imported = manager.import(&fixture.source).unwrap();
    assert_eq!(fs::read(fixture.shared().join("SKILL.md")).unwrap(), bytes);
    assert_eq!(
        imported
            .entry
            .statuses
            .iter()
            .find(|s| s.target_key == "shared")
            .unwrap()
            .state,
        ExportState::Conflict
    );
    manager.set_shared(&imported.entry.id, false).unwrap();
    assert!(fixture.shared().is_dir());
}

#[test]
fn external_export_edits_block_apply_and_survive_stop_sharing() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let imported = manager.import(&fixture.source).unwrap();
    fs::write(fixture.shared().join("personal.txt"), b"preserve this edit").unwrap();
    change_source(&manager, &imported.entry.id, "Applied new instructions.");
    let report = manager.apply(&imported.entry.id).unwrap();
    assert_eq!(
        report
            .statuses
            .iter()
            .find(|s| s.export.target_key == "shared")
            .unwrap()
            .export
            .state,
        ExportState::Conflict
    );
    assert_eq!(
        fs::read(fixture.shared().join("personal.txt")).unwrap(),
        b"preserve this edit"
    );
    let disabled = manager.set_shared(&imported.entry.id, false).unwrap();
    assert_eq!(disabled.changed_paths.len(), 3);
    assert!(fixture.shared().exists());
    let entry = manager.entries().unwrap().remove(0);
    assert!(!entry.shared);
    assert!(entry
        .statuses
        .iter()
        .any(|s| s.state == ExportState::Conflict));
    assert_eq!(manager.reconcile(&[]).unwrap().changed_paths.len(), 0);
}

#[test]
fn apply_creates_new_snapshot_and_does_not_change_previous_snapshot() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let imported = manager.import(&fixture.source).unwrap();
    let old = manager.applied_path(&imported.entry.id).unwrap();
    let old_bytes = fs::read(old.join("SKILL.md")).unwrap();
    change_source(&manager, &imported.entry.id, "New revision.");
    assert!(manager.entries().unwrap()[0]
        .warnings
        .iter()
        .any(|w| w.contains("unapplied")));
    manager.apply(&imported.entry.id).unwrap();
    let new = manager.applied_path(&imported.entry.id).unwrap();
    assert_ne!(old, new);
    assert_eq!(fs::read(old.join("SKILL.md")).unwrap(), old_bytes);
    assert_eq!(manager.entries().unwrap()[0].revision, 2);
    assert!(fs::metadata(old.join("SKILL.md"))
        .unwrap()
        .permissions()
        .readonly());
}

#[test]
fn malformed_yaml_and_path_traversal_names_do_not_enter_registry() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    for text in [
        "---\nname: ../escape\ndescription: Escape\n---\n",
        "---\nname: example-skill\ndescription: [unfinished\n---\n",
        "---\nname: example-skill\ndescription: false\n---\n",
        "---\nname: example-skill\ndescription: Missing closing marker\n",
    ] {
        fs::write(fixture.source.join("SKILL.md"), text).unwrap();
        assert!(matches!(
            manager.import(&fixture.source),
            Err(Error::InvalidBundle(_))
        ));
        assert!(manager.entries().unwrap().is_empty());
    }
}

#[test]
fn windows_reserved_public_skill_names_cannot_enter_the_library() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    for name in ["con", "prn", "aux", "nul", "com1", "lpt9"] {
        fs::write(
            fixture.source.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Reserved name\n---\nInstructions\n"),
        )
        .unwrap();
        let error = manager.import(&fixture.source).unwrap_err();
        assert!(matches!(error, Error::InvalidBundle(_)));
        assert!(error.to_string().contains("reserved on Windows"));
    }
    assert!(manager.entries().unwrap().is_empty());
}

#[test]
fn large_unknown_yaml_aliases_are_preserved_without_materializing_their_values() {
    let fixture = Fixture::new();
    let payload = "x".repeat(512 * 1024);
    let aliases = std::iter::repeat_n("*payload", 10_000)
        .collect::<Vec<_>>()
        .join(", ");
    let text = format!("---\nname: example-skill\ndescription: Bounded metadata parsing\nprovider-text: &payload {payload}\nprovider-copies: [{aliases}]\nmetadata:\n  repeated: [{aliases}]\n---\nInstructions\n");
    assert!(text.len() < 1024 * 1024);
    fs::write(fixture.source.join("SKILL.md"), &text).unwrap();
    let imported = fixture.manager().import(&fixture.source).unwrap();
    assert_eq!(imported.entry.name, "example-skill");
    assert_eq!(
        fs::read_to_string(fixture.shared().join("SKILL.md")).unwrap(),
        text
    );
}

#[test]
fn known_yaml_aliases_obey_catalog_string_limits_and_metadata_mapping_type() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let valid = "---\nprovider-name: &skill-name example-skill\nprovider-description: &description A bounded description\nprovider-metadata: &metadata {resource: references/info.txt}\nname: *skill-name\ndescription: *description\nmetadata: *metadata\n---\nInstructions\n";
    fs::write(fixture.source.join("SKILL.md"), valid).unwrap();
    assert_eq!(
        manager.import(&fixture.source).unwrap().entry.description,
        "A bounded description"
    );
    let payload = "x".repeat(2048);
    fs::write(fixture.source.join("SKILL.md"), format!("---\nprovider-text: &payload {payload}\nname: example-skill\ndescription: *payload\n---\nInstructions\n")).unwrap();
    let error = manager.import(&fixture.source).unwrap_err();
    assert!(error.to_string().contains("exceeds 1024 characters"));
    fs::write(fixture.source.join("SKILL.md"), "---\nprovider-sequence: &sequence [one, two]\nname: example-skill\ndescription: Valid description\nmetadata: *sequence\n---\nInstructions\n").unwrap();
    assert!(manager
        .import(&fixture.source)
        .unwrap_err()
        .to_string()
        .contains("metadata mapping"));
}

#[test]
fn unknown_yaml_values_still_require_valid_syntax_and_frontmatter_fields_are_unique() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    for text in [
        "---\nname: example-skill\ndescription: Description\nprovider-data: [unfinished\n---\nInstructions\n",
        "---\nname: example-skill\nname: second-name\ndescription: Description\n---\nInstructions\n",
        "---\nname: example-skill\ndescription: Description\nprovider-data: *unknown-anchor\n---\nInstructions\n",
    ] {
        fs::write(fixture.source.join("SKILL.md"), text).unwrap();
        assert!(matches!(manager.import(&fixture.source), Err(Error::InvalidBundle(_))));
    }
    assert!(manager.entries().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn rejects_external_symlinks_and_materializes_internal_resources() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let manager = fixture.manager();
    fs::write(fixture.root().join("private.txt"), b"outside").unwrap();
    symlink(
        fixture.root().join("private.txt"),
        fixture.source.join("bad-link"),
    )
    .unwrap();
    assert!(matches!(
        manager.import(&fixture.source),
        Err(Error::InvalidBundle(_))
    ));
    fs::remove_file(fixture.source.join("bad-link")).unwrap();
    fs::write(fixture.source.join("resource.txt"), b"inside").unwrap();
    symlink("resource.txt", fixture.source.join("good-link")).unwrap();
    manager.import(&fixture.source).unwrap();
    assert_eq!(
        fs::read(fixture.shared().join("good-link")).unwrap(),
        b"inside"
    );
    assert!(!fs::symlink_metadata(fixture.shared().join("good-link"))
        .unwrap()
        .file_type()
        .is_symlink());
}

#[cfg(unix)]
#[test]
fn rejects_symlink_cycles_and_preserves_destination_symlinks() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let manager = fixture.manager();
    symlink(".", fixture.source.join("cycle")).unwrap();
    assert!(matches!(
        manager.import(&fixture.source),
        Err(Error::InvalidBundle(_))
    ));
    fs::remove_file(fixture.source.join("cycle")).unwrap();
    fs::create_dir_all(fixture.shared().parent().unwrap()).unwrap();
    symlink(&fixture.source, fixture.shared()).unwrap();
    let imported = manager.import(&fixture.source).unwrap();
    assert_eq!(
        imported
            .entry
            .statuses
            .iter()
            .find(|s| s.target_key == "shared")
            .unwrap()
            .state,
        ExportState::Conflict
    );
    manager.set_shared(&imported.entry.id, false).unwrap();
    assert!(fs::symlink_metadata(fixture.shared())
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn resolved_account_targets_persist_across_apply_and_stop_sharing() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let imported = manager.import(&fixture.source).unwrap();
    let root = fixture.root().join("account profile/skills");
    let target = ExportTarget::new("claude-account-work", &root, vec!["claude".into()]);
    manager.reconcile(&[target]).unwrap();
    drop(manager);
    let manager = fixture.manager();
    change_source(&manager, &imported.entry.id, "Updated for all accounts.");
    manager.apply(&imported.entry.id).unwrap();
    assert!(fs::read_to_string(root.join("example-skill/SKILL.md"))
        .unwrap()
        .contains("Updated for all accounts"));
    manager.set_shared(&imported.entry.id, false).unwrap();
    assert!(!root.join("example-skill").exists());
}

#[test]
fn retired_account_targets_are_not_recreated_by_later_apply() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let imported = manager.import(&fixture.source).unwrap();
    let profile = fixture.root().join("provider accounts/claude/work");
    manager
        .reconcile(&[ExportTarget::new(
            "claude-work",
            profile.join("skills"),
            vec!["claude".into()],
        )])
        .unwrap();
    let default_contents = fs::read(fixture.shared().join("SKILL.md")).unwrap();
    fs::remove_dir_all(&profile).unwrap();
    let report = manager.retire_targets_under(&profile).unwrap();
    assert!(report.changed_paths.is_empty());
    assert!(!profile.exists());
    assert_eq!(
        fs::read(fixture.shared().join("SKILL.md")).unwrap(),
        default_contents
    );
    let generation = manager.generation().unwrap();
    assert_eq!(
        manager.retire_targets_under(&profile).unwrap().generation,
        generation
    );
    change_source(
        &manager,
        &imported.entry.id,
        "Apply after account deletion.",
    );
    manager.apply(&imported.entry.id).unwrap();
    assert!(!profile.exists());
    assert!(manager
        .load()
        .unwrap()
        .targets
        .iter()
        .all(|target| !target.root.starts_with(&profile)));
}

#[cfg(unix)]
#[test]
fn absent_home_under_symlinked_parent_has_stable_registry_paths_after_import() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let real_parent = fixture.root().join("real-parent");
    let alias_parent = fixture.root().join("alias-parent");
    fs::create_dir(&real_parent).unwrap();
    symlink(&real_parent, &alias_parent).unwrap();
    let home = alias_parent.join("new-home");
    let manager = SkillManager::open(&fixture.data, &home).unwrap();
    assert!(!home.exists());
    assert_eq!(manager.skill_home, real_parent.join("new-home"));
    manager.import(&fixture.source).unwrap();
    assert!(home.is_dir());
    drop(manager);
    let manager = SkillManager::open(&fixture.data, &home).unwrap();
    let entries = manager.entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0]
        .statuses
        .iter()
        .filter(|status| status.state != ExportState::Unsupported)
        .all(|status| status.state == ExportState::Exported));
    assert!(manager.reconcile(&[]).unwrap().changed_paths.is_empty());
}

#[test]
fn changed_account_roots_remove_only_unchanged_old_owned_exports() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    manager.import(&fixture.source).unwrap();
    let first = fixture.root().join("first/skills");
    manager
        .reconcile(&[ExportTarget::new("account", &first, vec!["codex".into()])])
        .unwrap();
    let second = fixture.root().join("second/skills");
    manager
        .reconcile(&[ExportTarget::new("account", &second, vec!["codex".into()])])
        .unwrap();
    assert!(!first.join("example-skill").exists());
    fs::write(second.join("example-skill/user-edit.txt"), b"keep").unwrap();
    let third = fixture.root().join("third/skills");
    manager
        .reconcile(&[ExportTarget::new("account", &third, vec!["codex".into()])])
        .unwrap();
    assert_eq!(
        fs::read(second.join("example-skill/user-edit.txt")).unwrap(),
        b"keep"
    );
    assert!(manager.entries().unwrap()[0]
        .statuses
        .iter()
        .any(|s| s.path == second.join("example-skill") && s.state == ExportState::Conflict));
}

#[test]
fn two_managers_import_concurrently_without_duplicate_entries_or_overwrites() {
    let fixture = Fixture::new();
    let barrier = Arc::new(Barrier::new(2));
    let mut threads = Vec::new();
    for _ in 0..2 {
        let data = fixture.data.clone();
        let home = fixture.home.clone();
        let source = fixture.source.clone();
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            let manager = SkillManager::open(data, home).unwrap();
            barrier.wait();
            manager.import(source).unwrap().entry.id
        }));
    }
    assert_eq!(
        threads.remove(0).join().unwrap(),
        threads.remove(0).join().unwrap()
    );
    assert_eq!(fixture.manager().entries().unwrap().len(), 1);
}

#[test]
fn malformed_registry_is_preserved_and_not_reset() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let path = manager.registry_path();
    fs::write(&path, b"{broken json").unwrap();
    assert!(matches!(
        SkillManager::open(&fixture.data, &fixture.home),
        Err(Error::InvalidRegistry { .. })
    ));
    assert_eq!(fs::read(&path).unwrap(), b"{broken json");
}

#[test]
fn oversized_bundle_is_rejected_before_resource_contents_are_read() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let file = File::create(fixture.source.join("large-resource.bin")).unwrap();
    file.set_len(64 * 1024 * 1024 + 1).unwrap();
    assert!(matches!(
        manager.import(&fixture.source),
        Err(Error::InvalidBundle(_))
    ));
    assert!(manager.entries().unwrap().is_empty());
}

#[test]
fn malformed_operation_journal_is_preserved_and_does_not_change_exports() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    manager.import(&fixture.source).unwrap();
    let bytes = fs::read(fixture.shared().join("SKILL.md")).unwrap();
    fs::write(manager.journal_path(), b"{invalid operation").unwrap();
    assert!(matches!(
        SkillManager::open(&fixture.data, &fixture.home),
        Err(Error::InvalidJournal { .. })
    ));
    assert_eq!(
        fs::read(manager.journal_path()).unwrap(),
        b"{invalid operation"
    );
    assert_eq!(fs::read(fixture.shared().join("SKILL.md")).unwrap(), bytes);
}

#[test]
fn retirement_survives_crash_before_operation_journal_cleanup() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let imported = manager.import(&fixture.source).unwrap();
    let profile = fixture.root().join("profile");
    let target = ExportTarget::new("profile", profile.join("skills"), vec!["claude".into()]);
    manager.reconcile(&[target]).unwrap();
    let destination = profile.join("skills/example-skill");
    let registry = manager.load().unwrap();
    let owned = registry.owned.get(&destination).unwrap().clone();
    let token = uuid::Uuid::new_v4();
    let journal = Journal {
        schema_version: SCHEMA_VERSION,
        destination,
        stage: profile.join(format!("skills/.monocode-skill-{token}.stage")),
        backup: profile.join(format!("skills/.monocode-skill-{token}.backup")),
        previous: Some(owned.clone()),
        next: Some(owned),
    };
    fs::remove_dir_all(&profile).unwrap();
    manager.retire_targets_under(&profile).unwrap();
    // A crash after retirement commits but before journal removal leaves this file.
    persistence::atomic_json(&manager.journal_path(), &journal).unwrap();
    drop(manager);
    let manager = fixture.manager();
    assert!(!manager.journal_path().exists());
    change_source(
        &manager,
        &imported.entry.id,
        "After interrupted retirement.",
    );
    manager.apply(&imported.entry.id).unwrap();
    assert!(!profile.exists());
}

#[test]
fn recovery_finishes_interrupted_replace_after_previous_directory_was_moved() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let imported = manager.import(&fixture.source).unwrap();
    change_source(&manager, &imported.entry.id, "Recover this revision.");
    let bundle = Bundle::read(&manager.source_path(&imported.entry.id).unwrap(), true).unwrap();
    manager.store_snapshot(&bundle).unwrap();
    let mut registry = manager.load().unwrap();
    registry.entries[0].digest = bundle.digest.clone();
    registry.entries[0].revision += 1;
    registry.generation += 1;
    manager.save(&registry).unwrap();
    let destination = fixture.shared();
    let token = uuid::Uuid::new_v4();
    let stage = destination
        .parent()
        .unwrap()
        .join(format!(".monocode-skill-{token}.stage"));
    let backup = destination
        .parent()
        .unwrap()
        .join(format!(".monocode-skill-{token}.backup"));
    bundle.write_new(&stage).unwrap();
    let previous = registry.owned.get(&destination).unwrap().clone();
    let next = OwnedExport {
        digest: bundle.digest,
        generation: registry.generation + 1,
        ..previous.clone()
    };
    let journal = Journal {
        schema_version: SCHEMA_VERSION,
        destination: destination.clone(),
        stage: stage.clone(),
        backup: backup.clone(),
        previous: Some(previous),
        next: Some(next),
    };
    persistence::atomic_json(&manager.journal_path(), &journal).unwrap();
    fs::rename(&destination, &backup).unwrap();
    drop(manager);
    let manager = fixture.manager();
    assert!(fs::read_to_string(destination.join("SKILL.md"))
        .unwrap()
        .contains("Recover this revision"));
    assert!(!backup.exists());
    assert!(!stage.exists());
    assert!(!manager.journal_path().exists());
    manager.reconcile(&[]).unwrap();
    assert!(manager.reconcile(&[]).unwrap().changed_paths.is_empty());
}

#[test]
fn recovery_restores_complete_previous_export_when_staged_copy_is_missing() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let imported = manager.import(&fixture.source).unwrap();
    let mut registry = manager.load().unwrap();
    let destination = fixture.shared();
    let previous = registry.owned.get(&destination).unwrap().clone();
    let token = uuid::Uuid::new_v4();
    let stage = destination
        .parent()
        .unwrap()
        .join(format!(".monocode-skill-{token}.stage"));
    let backup = destination
        .parent()
        .unwrap()
        .join(format!(".monocode-skill-{token}.backup"));
    let next = OwnedExport {
        digest: "f".repeat(64),
        ..previous.clone()
    };
    let journal = Journal {
        schema_version: SCHEMA_VERSION,
        destination: destination.clone(),
        stage,
        backup: backup.clone(),
        previous: Some(previous),
        next: Some(next),
    };
    persistence::atomic_json(&manager.journal_path(), &journal).unwrap();
    fs::rename(&destination, &backup).unwrap();
    manager.recover(&mut registry).unwrap();
    assert!(destination.join("SKILL.md").exists());
    assert!(!backup.exists());
    assert_eq!(
        registry.owned.get(&destination).unwrap().skill_id,
        imported.entry.id
    );
}

#[test]
fn recovery_restores_edited_backup_as_visible_conflict_and_preserves_new_stage() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let imported = manager.import(&fixture.source).unwrap();
    change_source(&manager, &imported.entry.id, "New staged revision.");
    let bundle = Bundle::read(&manager.source_path(&imported.entry.id).unwrap(), true).unwrap();
    manager.store_snapshot(&bundle).unwrap();
    let mut registry = manager.load().unwrap();
    registry.entries[0].digest = bundle.digest.clone();
    registry.entries[0].revision += 1;
    registry.generation += 1;
    manager.save(&registry).unwrap();
    let destination = fixture.shared();
    let previous = registry.owned.get(&destination).unwrap().clone();
    let token = uuid::Uuid::new_v4();
    let stage = destination
        .parent()
        .unwrap()
        .join(format!(".monocode-skill-{token}.stage"));
    let backup = destination
        .parent()
        .unwrap()
        .join(format!(".monocode-skill-{token}.backup"));
    bundle.write_new(&stage).unwrap();
    let next = OwnedExport {
        digest: bundle.digest,
        generation: registry.generation + 1,
        ..previous.clone()
    };
    let journal = Journal {
        schema_version: SCHEMA_VERSION,
        destination: destination.clone(),
        stage: stage.clone(),
        backup: backup.clone(),
        previous: Some(previous),
        next: Some(next),
    };
    persistence::atomic_json(&manager.journal_path(), &journal).unwrap();
    fs::rename(&destination, &backup).unwrap();
    fs::write(backup.join("personal.txt"), b"preserved backup edit").unwrap();
    drop(manager);
    let manager = fixture.manager();
    assert_eq!(
        fs::read(destination.join("personal.txt")).unwrap(),
        b"preserved backup edit"
    );
    assert!(stage.join("SKILL.md").exists());
    assert!(!backup.exists());
    let entry = manager.entries().unwrap().remove(0);
    assert_eq!(
        entry
            .statuses
            .iter()
            .find(|status| status.target_key == "shared")
            .unwrap()
            .state,
        ExportState::Conflict
    );
    manager.reconcile(&[]).unwrap();
    assert_eq!(
        fs::read(destination.join("personal.txt")).unwrap(),
        b"preserved backup edit"
    );
}

#[test]
fn recovery_preserves_external_destination_created_during_install() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let imported = manager.import(&fixture.source).unwrap();
    let mut registry = manager.load().unwrap();
    let destination = fixture.root().join("raced/skills/example-skill");
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    let bundle = Bundle::read(&fixture.source, true).unwrap();
    let token = uuid::Uuid::new_v4();
    let stage = destination
        .parent()
        .unwrap()
        .join(format!(".monocode-skill-{token}.stage"));
    let backup = destination
        .parent()
        .unwrap()
        .join(format!(".monocode-skill-{token}.backup"));
    bundle.write_new(&stage).unwrap();
    let next = OwnedExport {
        skill_id: imported.entry.id,
        target: ExportTarget::new("race", destination.parent().unwrap(), vec!["codex".into()]),
        digest: bundle.digest.clone(),
        generation: registry.generation + 1,
    };
    registry.targets.push(next.target.clone());
    manager.save(&registry).unwrap();
    let journal = Journal {
        schema_version: SCHEMA_VERSION,
        destination: destination.clone(),
        stage: stage.clone(),
        backup,
        previous: None,
        next: Some(next),
    };
    persistence::atomic_json(&manager.journal_path(), &journal).unwrap();
    bundle.write_new(&destination).unwrap();
    manager.recover(&mut registry).unwrap();
    assert!(!registry.owned.contains_key(&destination));
    assert!(destination.join("SKILL.md").exists());
    assert!(stage.exists());
}
