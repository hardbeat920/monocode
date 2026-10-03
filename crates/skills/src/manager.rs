use crate::bundle::Bundle;
use crate::persistence::{self, Journal, OwnedExport, Registry, StoredSkill, SCHEMA_VERSION};
use crate::{
    io, Error, ExportState, ExportStatus, ExportTarget, ImportResult, ReconcileReport, Result,
    SkillEntry, SkillExportStatus, SkillSnapshot,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct SkillManager {
    root: PathBuf,
    skill_home: PathBuf,
}

/// These targets describe documented local filesystem roots, not runtime checks.
pub fn default_targets(skill_home: impl AsRef<Path>) -> Vec<ExportTarget> {
    let home = skill_home.as_ref();
    vec![
        ExportTarget::new(
            "shared",
            home.join(".agents/skills"),
            vec![
                "codex".into(),
                "cursor".into(),
                "grok".into(),
                "opencode".into(),
                "pi".into(),
                "fx".into(),
            ],
        ),
        ExportTarget::new("claude", home.join(".claude/skills"), vec!["claude".into()]),
        ExportTarget::new("omp", home.join(".omp/agent/skills"), vec!["omp".into()]),
        ExportTarget::new("hermes", home.join(".hermes/skills"), vec!["hermes".into()]),
    ]
}

impl SkillManager {
    /// All paths are explicit so tests and previews never touch personal skills.
    /// Each operation reloads the manifest under an exclusive OS file lock.
    pub fn open(data_dir: impl AsRef<Path>, skill_home: impl AsRef<Path>) -> Result<Self> {
        let data_dir = absolute(data_dir.as_ref())?;
        let skill_home = absolute(skill_home.as_ref())?;
        fs::create_dir_all(&data_dir).map_err(|e| io("Create data directory", &data_dir, e))?;
        let root = data_dir.join("skills");
        fs::create_dir_all(&root).map_err(|e| io("Create skill library", &root, e))?;
        let root = fs::canonicalize(&root).map_err(|e| io("Resolve skill library", &root, e))?;
        let skill_home = resolve_missing_path(&skill_home)?;
        let manager = Self { root, skill_home };
        let _lock = manager.lock()?;
        for directory in ["sources", "objects"] {
            let path = manager.root.join(directory);
            fs::create_dir_all(&path).map_err(|e| io("Create library directory", &path, e))?;
            if fs::symlink_metadata(&path)
                .map_err(|e| io("Read library directory", &path, e))?
                .file_type()
                .is_symlink()
            {
                return Err(Error::InvalidTarget(format!(
                    "Library directory is a symbolic link: {}",
                    path.display()
                )));
            }
        }
        if !exists(&manager.registry_path())? {
            let mut targets = default_targets(&manager.skill_home);
            for target in &mut targets {
                target.root = resolve_missing_path(&target.root)?;
            }
            let registry = Registry {
                schema_version: SCHEMA_VERSION,
                generation: 0,
                skill_home: manager.skill_home.clone(),
                entries: Vec::new(),
                targets,
                owned: BTreeMap::new(),
                retired_roots: Vec::new(),
            };
            manager.save(&registry)?;
        }
        let mut registry = manager.load()?;
        manager.recover(&mut registry)?;
        Ok(manager)
    }

    pub fn generation(&self) -> Result<u64> {
        let _lock = self.lock()?;
        let mut registry = self.load()?;
        self.recover(&mut registry)?;
        Ok(registry.generation)
    }

    pub fn entries(&self) -> Result<Vec<SkillEntry>> {
        Ok(self.snapshot()?.entries)
    }

    /// Read generation, entries, and remembered export targets under one lock.
    pub fn snapshot(&self) -> Result<SkillSnapshot> {
        let _lock = self.lock()?;
        let mut registry = self.load()?;
        self.recover(&mut registry)?;
        let entries = registry
            .entries
            .iter()
            .map(|entry| self.public_entry(&registry, entry))
            .collect();
        Ok(SkillSnapshot {
            generation: registry.generation,
            entries,
            targets: registry.targets,
        })
    }

    pub fn source_path(&self, id: &str) -> Result<PathBuf> {
        let _lock = self.lock()?;
        let registry = self.load()?;
        self.entry(&registry, id)?;
        Ok(self.source_directory(id))
    }

    /// Return the applied resource directory. Ordinary editing uses source_path.
    pub fn applied_path(&self, id: &str) -> Result<PathBuf> {
        let _lock = self.lock()?;
        let registry = self.load()?;
        Ok(self.object_directory(&self.entry(&registry, id)?.digest))
    }

    pub fn import(&self, source: impl AsRef<Path>) -> Result<ImportResult> {
        let _lock = self.lock()?;
        let mut registry = self.load()?;
        self.recover(&mut registry)?;
        let source = fs::canonicalize(source.as_ref())
            .map_err(|e| io("Resolve import source", source.as_ref(), e))?;
        let bundle = Bundle::read(&source, true)?;
        if let Some(index) = registry
            .entries
            .iter()
            .position(|entry| entry.name == bundle.name)
        {
            let entry = &mut registry.entries[index];
            if entry.digest != bundle.digest {
                return Err(Error::NameConflict {
                    name: bundle.name,
                    existing_id: entry.id.clone(),
                });
            }
            if !entry.origins.contains(&source) {
                entry.origins.push(source);
                registry.generation = increment(registry.generation, "generation")?;
                self.save(&registry)?;
            }
            let id = registry.entries[index].id.clone();
            let report = self.reconcile_locked(&mut registry)?;
            return Ok(ImportResult {
                entry: self.public_entry(&registry, self.entry(&registry, &id)?),
                already_present: true,
                report,
            });
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.store_snapshot(&bundle)?;
        let working_stage = self.root.join("sources").join(format!(".import-{id}"));
        bundle.write_new(&working_stage)?;
        let working = self.source_directory(&id);
        fs::rename(&working_stage, &working)
            .map_err(|e| io("Install editable source", &working, e))?;
        persistence::sync_directory(&self.root.join("sources"))?;
        registry.entries.push(StoredSkill {
            id: id.clone(),
            name: bundle.name,
            description: bundle.description,
            digest: bundle.digest,
            revision: 1,
            shared: true,
            origins: vec![source],
            warnings: bundle.warnings,
        });
        registry.generation = increment(registry.generation, "generation")?;
        self.save(&registry)?;
        let report = self.reconcile_locked(&mut registry)?;
        Ok(ImportResult {
            entry: self.public_entry(&registry, self.entry(&registry, &id)?),
            already_present: false,
            report,
        })
    }

    pub fn apply(&self, id: &str) -> Result<ReconcileReport> {
        let _lock = self.lock()?;
        let mut registry = self.load()?;
        self.recover(&mut registry)?;
        let index = registry
            .entries
            .iter()
            .position(|entry| entry.id == id)
            .ok_or_else(|| Error::UnknownSkill(id.into()))?;
        let bundle = self.editable_bundle(id)?;
        if bundle.name != registry.entries[index].name {
            return Err(Error::InvalidBundle("An applied edit cannot rename a skill. Import the renamed bundle as a separate skill.".into()));
        }
        if bundle.digest != registry.entries[index].digest {
            self.store_snapshot(&bundle)?;
            let entry = &mut registry.entries[index];
            entry.digest = bundle.digest;
            entry.description = bundle.description;
            entry.warnings = bundle.warnings;
            entry.revision = increment(entry.revision, "revision")?;
            registry.generation = increment(registry.generation, "generation")?;
            self.save(&registry)?;
        }
        self.reconcile_locked(&mut registry)
    }

    pub fn set_shared(&self, id: &str, shared: bool) -> Result<ReconcileReport> {
        let _lock = self.lock()?;
        let mut registry = self.load()?;
        self.recover(&mut registry)?;
        let entry = registry
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or_else(|| Error::UnknownSkill(id.into()))?;
        if entry.shared != shared {
            entry.shared = shared;
            registry.generation = increment(registry.generation, "generation")?;
            self.save(&registry)?;
        }
        self.reconcile_locked(&mut registry)
    }

    /// Caller-resolved targets persist so later edits reach the same accounts.
    /// Reusing a target key with a new root retires unchanged copies at its old root.
    pub fn reconcile(&self, extra_targets: &[ExportTarget]) -> Result<ReconcileReport> {
        let _lock = self.lock()?;
        let mut registry = self.load()?;
        self.recover(&mut registry)?;
        let mut changed = false;
        for configured in extra_targets {
            persistence::validate_target(configured)?;
            let mut target = configured.clone();
            target.root = resolve_missing_path(&target.root)?;
            if target.root.starts_with(&self.root) {
                return Err(Error::InvalidTarget(
                    "Export target cannot be inside the managed library".into(),
                ));
            }
            let previous_retired = registry.retired_roots.len();
            registry
                .retired_roots
                .retain(|root| !target.root.starts_with(root));
            changed |= previous_retired != registry.retired_roots.len();
            match registry
                .targets
                .iter_mut()
                .find(|existing| existing.key == target.key)
            {
                Some(existing) if *existing != target => {
                    *existing = target.clone();
                    changed = true;
                }
                Some(_) => {}
                None => {
                    registry.targets.push(target.clone());
                    changed = true;
                }
            }
        }
        if changed {
            registry.generation = increment(registry.generation, "generation")?;
            self.save(&registry)?;
        }
        self.reconcile_locked(&mut registry)
    }

    /// Forget an account before the caller deletes its profile directory.
    /// This changes library metadata only. It never changes provider files.
    pub fn retire_targets_under(&self, root: &Path) -> Result<ReconcileReport> {
        let root = absolute(root)?;
        let resolved = resolve_missing_path(&root)?;
        let _lock = self.lock()?;
        let mut registry = self.load()?;
        let under_root = |path: &Path| path.starts_with(&root) || path.starts_with(&resolved);
        let previous_targets = registry.targets.len();
        let previous_owned = registry.owned.len();
        registry.targets.retain(|target| !under_root(&target.root));
        registry
            .owned
            .retain(|path, owned| !under_root(path) && !under_root(&owned.target.root));
        let journal_path = self.journal_path();
        let mut retire_journal = false;
        if exists(&journal_path)? {
            let journal: Journal =
                persistence::read_json(&journal_path).map_err(|error| Error::InvalidJournal {
                    path: journal_path.clone(),
                    detail: error.to_string(),
                })?;
            retire_journal = under_root(&journal.destination);
        }
        if previous_targets != registry.targets.len()
            || previous_owned != registry.owned.len()
            || retire_journal
        {
            for path in [&root, &resolved] {
                if !registry.retired_roots.contains(path) {
                    registry.retired_roots.push(path.clone());
                }
            }
            registry.generation = increment(registry.generation, "generation")?;
            self.save(&registry)?;
        }
        if retire_journal {
            self.clear_journal()?;
        }
        Ok(ReconcileReport {
            generation: registry.generation,
            statuses: registry
                .entries
                .iter()
                .flat_map(|entry| {
                    self.public_entry(&registry, entry)
                        .statuses
                        .into_iter()
                        .map(|export| SkillExportStatus {
                            skill_id: entry.id.clone(),
                            export,
                        })
                })
                .collect(),
            changed_paths: Vec::new(),
        })
    }

    fn lock(&self) -> Result<File> {
        let path = self.root.join("manager.lock");
        if let Ok(metadata) = fs::symlink_metadata(&path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(Error::InvalidTarget(
                    "Manager lock must be a regular file".into(),
                ));
            }
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| io("Open manager lock", &path, e))?;
        file.lock()
            .map_err(|e| io("Lock skill library", &path, e))?;
        Ok(file)
    }

    fn registry_path(&self) -> PathBuf {
        self.root.join("registry.json")
    }
    fn journal_path(&self) -> PathBuf {
        self.root.join("operation.json")
    }
    fn source_directory(&self, id: &str) -> PathBuf {
        self.root.join("sources").join(id)
    }
    fn editable_bundle(&self, id: &str) -> Result<Bundle> {
        let source = self.source_directory(id);
        let metadata = fs::symlink_metadata(&source)
            .map_err(|error| io("Inspect editable source", &source, error))?;
        if metadata.file_type().is_symlink() {
            return Err(Error::InvalidBundle(
                "The managed editable source directory is a symbolic link".into(),
            ));
        }
        Bundle::read(&source, true)
    }
    fn object_directory(&self, digest: &str) -> PathBuf {
        self.root.join("objects").join(digest)
    }

    fn load(&self) -> Result<Registry> {
        let path = self.registry_path();
        let registry = persistence::read_json(&path)?;
        persistence::validate_registry(&registry, &path, &self.skill_home)?;
        Ok(registry)
    }

    fn save(&self, registry: &Registry) -> Result<()> {
        persistence::atomic_json(&self.registry_path(), registry)
    }

    fn entry<'a>(&self, registry: &'a Registry, id: &str) -> Result<&'a StoredSkill> {
        registry
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| Error::UnknownSkill(id.into()))
    }

    fn store_snapshot(&self, bundle: &Bundle) -> Result<()> {
        let destination = self.object_directory(&bundle.digest);
        if exists(&destination)? {
            if Bundle::read(&destination, false)?.digest != bundle.digest {
                return Err(Error::InvalidBundle(
                    "An immutable applied snapshot has been modified".into(),
                ));
            }
            return Ok(());
        }
        let stage = self
            .root
            .join("objects")
            .join(format!(".snapshot-{}", uuid::Uuid::new_v4()));
        bundle.write_new(&stage)?;
        make_read_only(&stage)?;
        fs::rename(&stage, &destination)
            .map_err(|e| io("Install applied snapshot", &destination, e))?;
        persistence::sync_directory(&self.root.join("objects"))
    }

    fn public_entry(&self, registry: &Registry, entry: &StoredSkill) -> SkillEntry {
        let mut warnings = entry.warnings.clone();
        match self.editable_bundle(&entry.id) {
            Ok(bundle) if bundle.digest != entry.digest => {
                warnings.push("The editable source has unapplied changes.".into())
            }
            Err(error) => warnings.push(format!("The editable source cannot be applied: {error}")),
            _ => {}
        }
        let mut statuses = registry
            .targets
            .iter()
            .map(|target| self.status(registry, entry, target))
            .collect::<Vec<_>>();
        // Edited residual exports remain visible even after a target changes roots.
        for (destination, owned) in &registry.owned {
            if owned.skill_id == entry.id
                && !statuses.iter().any(|status| status.path == *destination)
            {
                statuses.push(self.status_at(registry, entry, &owned.target, destination));
            }
        }
        statuses.push(unsupported_status());
        SkillEntry {
            id: entry.id.clone(),
            name: entry.name.clone(),
            description: entry.description.clone(),
            digest: entry.digest.clone(),
            revision: entry.revision,
            shared: entry.shared,
            source_path: self.source_directory(&entry.id),
            applied_path: self.object_directory(&entry.digest),
            origins: entry.origins.clone(),
            statuses,
            warnings,
        }
    }

    fn status(
        &self,
        registry: &Registry,
        entry: &StoredSkill,
        target: &ExportTarget,
    ) -> ExportStatus {
        let root = fs::canonicalize(&target.root).unwrap_or_else(|_| target.root.clone());
        self.status_at(registry, entry, target, &root.join(&entry.name))
    }

    fn status_at(
        &self,
        registry: &Registry,
        entry: &StoredSkill,
        target: &ExportTarget,
        path: &Path,
    ) -> ExportStatus {
        let desired_here = entry.shared
            && registry.targets.iter().any(|current| {
                current.key == target.key
                    && fs::canonicalize(&current.root)
                        .unwrap_or_else(|_| current.root.clone())
                        .join(&entry.name)
                        == path
            });
        let (state, detail) = match fingerprint(path) {
            Fingerprint::Missing if !desired_here => (ExportState::Disabled, "Managed sharing is off. Unmanaged copies and existing conversations are unaffected.".into()),
            Fingerprint::Missing => (ExportState::Pending, "The managed copy has not been exported.".into()),
            Fingerprint::Present(digest) => match registry.owned.get(path) {
                Some(owned) if owned.skill_id == entry.id && owned.digest == digest => {
                    if !desired_here { (ExportState::Pending, "An unchanged managed copy is pending removal.".into()) }
                    else if digest == entry.digest { (ExportState::Exported, "Exported on this host. Provider loading has not been verified. Start or refresh the provider session.".into()) }
                    else { (ExportState::Pending, "The managed copy is pending an applied revision.".into()) }
                }
                Some(_) => (ExportState::Conflict, "The exported directory changed outside MonoCode. Its files were preserved.".into()),
                None => (ExportState::Conflict, "An unmanaged directory already has this name. Its files were preserved.".into()),
            },
            Fingerprint::Invalid(detail) => (ExportState::Conflict, format!("The existing destination was preserved: {detail}")),
            Fingerprint::Unavailable(detail) => (ExportState::Pending, detail),
        };
        ExportStatus {
            target_key: target.key.clone(),
            providers: target.providers.clone(),
            path: path.into(),
            state,
            detail,
        }
    }

    fn reconcile_locked(&self, registry: &mut Registry) -> Result<ReconcileReport> {
        let mut report = ReconcileReport::default();
        let mut desired_paths = BTreeSet::new();
        let entries = registry.entries.clone();
        let targets = registry.targets.clone();
        for entry in &entries {
            for configured in &targets {
                let mut target = configured.clone();
                let root_result = if entry.shared {
                    fs::create_dir_all(&target.root).and_then(|_| fs::canonicalize(&target.root))
                } else {
                    fs::canonicalize(&target.root)
                };
                match root_result {
                    Ok(root) => target.root = root,
                    Err(error) if !entry.shared && error.kind() == std::io::ErrorKind::NotFound => {
                    }
                    Err(error) => {
                        report.statuses.push(SkillExportStatus {
                            skill_id: entry.id.clone(),
                            export: ExportStatus {
                                target_key: target.key,
                                providers: target.providers,
                                path: target.root.join(&entry.name),
                                state: ExportState::Pending,
                                detail: format!("Cannot prepare export directory: {error}"),
                            },
                        });
                        continue;
                    }
                }
                if target.root.starts_with(&self.root) {
                    return Err(Error::InvalidTarget(
                        "Resolved export target is inside the managed library".into(),
                    ));
                }
                let destination = target.root.join(&entry.name);
                if entry.shared {
                    desired_paths.insert(destination.clone());
                }
                let previous = registry.owned.get(&destination).cloned();
                let current = fingerprint(&destination);
                let unchanged_owned = match (&current, &previous) {
                    (Fingerprint::Present(digest), Some(owned)) => {
                        owned.skill_id == entry.id && owned.digest == *digest
                    }
                    _ => false,
                };
                let should_install = entry.shared && matches!(current, Fingerprint::Missing);
                let should_replace = entry.shared
                    && unchanged_owned
                    && previous.as_ref().is_some_and(|p| p.digest != entry.digest);
                let should_remove = !entry.shared && unchanged_owned;
                if should_install || should_replace || should_remove {
                    let next = if entry.shared {
                        Some(OwnedExport {
                            skill_id: entry.id.clone(),
                            target: target.clone(),
                            digest: entry.digest.clone(),
                            generation: increment(registry.generation, "generation")?,
                        })
                    } else {
                        None
                    };
                    match self.change_export(registry, &destination, previous, next) {
                        Ok(true) => report.changed_paths.push(destination.clone()),
                        Ok(false) => {}
                        Err(error) => {
                            report.statuses.push(SkillExportStatus {
                                skill_id: entry.id.clone(),
                                export: ExportStatus {
                                    target_key: target.key,
                                    providers: target.providers,
                                    path: destination,
                                    state: ExportState::Pending,
                                    detail: format!("Export needs another reconciliation: {error}"),
                                },
                            });
                            // Only one journal can be active. Recover it before another operation.
                            self.recover(registry)?;
                            continue;
                        }
                    }
                } else if !entry.shared
                    && matches!(current, Fingerprint::Missing)
                    && previous.is_some()
                {
                    registry.owned.remove(&destination);
                    registry.generation = increment(registry.generation, "generation")?;
                    self.save(registry)?;
                }
                report.statuses.push(SkillExportStatus {
                    skill_id: entry.id.clone(),
                    export: self.status_at(registry, entry, &target, &destination),
                });
            }
        }
        // Retire old account roots only when their owned bytes still match.
        let retired = registry.owned.clone();
        for (destination, owned) in retired {
            if desired_paths.contains(&destination) {
                continue;
            }
            let Some(entry) = entries.iter().find(|entry| entry.id == owned.skill_id) else {
                continue;
            };
            if report
                .statuses
                .iter()
                .any(|status| status.export.path == destination)
            {
                continue;
            }
            if matches!(fingerprint(&destination), Fingerprint::Present(ref digest) if digest == &owned.digest)
            {
                if self.change_export(registry, &destination, Some(owned.clone()), None)? {
                    report.changed_paths.push(destination.clone());
                }
            } else if matches!(fingerprint(&destination), Fingerprint::Missing) {
                registry.owned.remove(&destination);
                registry.generation = increment(registry.generation, "generation")?;
                self.save(registry)?;
            }
            report.statuses.push(SkillExportStatus {
                skill_id: entry.id.clone(),
                export: self.status_at(registry, entry, &owned.target, &destination),
            });
        }
        for entry in entries {
            report.statuses.push(SkillExportStatus {
                skill_id: entry.id,
                export: unsupported_status(),
            });
        }
        report.generation = registry.generation;
        Ok(report)
    }

    fn change_export(
        &self,
        registry: &mut Registry,
        destination: &Path,
        previous: Option<OwnedExport>,
        next: Option<OwnedExport>,
    ) -> Result<bool> {
        let root = destination
            .parent()
            .ok_or_else(|| Error::InvalidTarget("Export has no parent".into()))?;
        let token = uuid::Uuid::new_v4();
        let stage = root.join(format!(".monocode-skill-{token}.stage"));
        let backup = root.join(format!(".monocode-skill-{token}.backup"));
        if let Some(next) = &next {
            let object = self.object_directory(&next.digest);
            let bundle = Bundle::read(&object, false)?;
            if bundle.digest != next.digest {
                return Err(Error::InvalidBundle(
                    "An immutable applied snapshot has been modified".into(),
                ));
            }
            bundle.write_new(&stage)?;
        }
        let journal = Journal {
            schema_version: SCHEMA_VERSION,
            destination: destination.into(),
            stage,
            backup,
            previous,
            next,
        };
        persistence::atomic_json(&self.journal_path(), &journal)?;
        self.execute_journal(registry, &journal)
    }

    fn recover(&self, registry: &mut Registry) -> Result<()> {
        let path = self.journal_path();
        if !exists(&path)? {
            return Ok(());
        }
        let journal: Journal =
            persistence::read_json(&path).map_err(|error| Error::InvalidJournal {
                path: path.clone(),
                detail: error.to_string(),
            })?;
        if registry
            .retired_roots
            .iter()
            .any(|root| journal.destination.starts_with(root))
        {
            self.clear_journal()?;
            return Ok(());
        }
        self.validate_journal(registry, &journal)?;
        self.execute_journal(registry, &journal)?;
        Ok(())
    }

    fn validate_journal(&self, registry: &Registry, journal: &Journal) -> Result<()> {
        let invalid = |detail: &str| Error::InvalidJournal {
            path: self.journal_path(),
            detail: detail.into(),
        };
        if journal.schema_version != SCHEMA_VERSION
            || (journal.previous.is_none() && journal.next.is_none())
        {
            return Err(invalid("Unsupported or empty operation"));
        }
        let parent = journal
            .destination
            .parent()
            .ok_or_else(|| invalid("Destination has no parent"))?;
        let stage_name = journal
            .stage
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid("Invalid staging path"))?;
        let token = stage_name
            .strip_prefix(".monocode-skill-")
            .and_then(|name| name.strip_suffix(".stage"))
            .ok_or_else(|| invalid("Invalid staging path"))?;
        if uuid::Uuid::parse_str(token).is_err()
            || journal.stage.parent() != Some(parent)
            || journal.backup != parent.join(format!(".monocode-skill-{token}.backup"))
            || !journal.destination.is_absolute()
        {
            return Err(invalid("Operation paths are not owned sibling directories"));
        }
        for owned in [journal.previous.as_ref(), journal.next.as_ref()]
            .into_iter()
            .flatten()
        {
            persistence::validate_target(&owned.target)
                .map_err(|_| invalid("Invalid operation target"))?;
            let entry = registry
                .entries
                .iter()
                .find(|entry| entry.id == owned.skill_id)
                .ok_or_else(|| invalid("Operation refers to an unknown skill"))?;
            if owned.target.root != parent
                || journal
                    .destination
                    .file_name()
                    .and_then(|name| name.to_str())
                    != Some(&entry.name)
                || !persistence::valid_digest(&owned.digest)
            {
                return Err(invalid("Operation destination does not match its skill"));
            }
        }
        let same_content = |left: &OwnedExport, right: &OwnedExport| {
            left.skill_id == right.skill_id
                && left.target == right.target
                && left.digest == right.digest
        };
        if let Some(current) = registry.owned.get(&journal.destination) {
            if !journal
                .previous
                .as_ref()
                .is_some_and(|owned| same_content(owned, current))
                && !journal
                    .next
                    .as_ref()
                    .is_some_and(|owned| same_content(owned, current))
            {
                return Err(invalid(
                    "Operation does not match registered export ownership",
                ));
            }
        } else if journal.previous.is_some() && journal.next.is_some() {
            return Err(invalid("Replacement has no registered prior ownership"));
        }
        if let Some(next) = &journal.next {
            if !registry.targets.iter().any(|target| {
                target.key == next.target.key
                    && target.providers == next.target.providers
                    && fs::canonicalize(&target.root).unwrap_or_else(|_| target.root.clone())
                        == next.target.root
            }) {
                return Err(invalid(
                    "Operation target is absent from the registered export plan",
                ));
            }
        }
        Ok(())
    }

    fn execute_journal(&self, registry: &mut Registry, journal: &Journal) -> Result<bool> {
        let parent = journal
            .destination
            .parent()
            .ok_or_else(|| Error::InvalidJournal {
                path: self.journal_path(),
                detail: "Destination has no parent directory".into(),
            })?;
        let before = journal.previous.as_ref().map(|owned| owned.digest.as_str());
        let after = journal.next.as_ref().map(|owned| owned.digest.as_str());
        let mut current = fingerprint(&journal.destination);
        let backup = fingerprint(&journal.backup);
        let stage = fingerprint(&journal.stage);
        let matches =
            |fingerprint: &Fingerprint, expected: Option<&str>| match (fingerprint, expected) {
                (Fingerprint::Missing, None) => true,
                (Fingerprint::Present(actual), Some(expected)) => actual == expected,
                _ => false,
            };
        if before.is_some() && !matches!(backup, Fingerprint::Missing) && !matches(&backup, before)
        {
            // An edit to the saved prior copy must remain visible at its active path.
            if matches!(current, Fingerprint::Missing) {
                fs::rename(&journal.backup, &journal.destination).map_err(|error| {
                    io(
                        "Restore edited previous export",
                        &journal.destination,
                        error,
                    )
                })?;
                persistence::sync_directory(parent)?;
            } else if matches(&current, after) && matches!(stage, Fingerprint::Missing) {
                fs::rename(&journal.destination, &journal.stage)
                    .map_err(|error| io("Preserve staged revision", &journal.stage, error))?;
                fs::rename(&journal.backup, &journal.destination).map_err(|error| {
                    io(
                        "Restore edited previous export",
                        &journal.destination,
                        error,
                    )
                })?;
                persistence::sync_directory(parent)?;
            }
            self.clear_journal()?;
            return Ok(false);
        }
        let already_completed = matches(&current, after)
            && (after.is_none() || matches!(stage, Fingerprint::Missing))
            && (after.is_some() || !matches!(backup, Fingerprint::Missing));
        if !already_completed {
            if !matches(&current, before) && !matches!(current, Fingerprint::Missing) {
                // An external edit wins. Preserve destination and transaction copies.
                self.clear_journal()?;
                return Ok(false);
            }
            if before.is_some() && matches(&current, before) {
                if !matches!(backup, Fingerprint::Missing) {
                    self.clear_journal()?;
                    return Ok(false);
                }
                fs::rename(&journal.destination, &journal.backup)
                    .map_err(|e| io("Stage previous export", &journal.destination, e))?;
                persistence::sync_directory(parent)?;
                if !matches(&fingerprint(&journal.backup), before) {
                    fs::rename(&journal.backup, &journal.destination)
                        .map_err(|e| io("Preserve changed export", &journal.destination, e))?;
                    self.clear_journal()?;
                    return Ok(false);
                }
                current = Fingerprint::Missing;
            }
            if after.is_some() {
                if !matches(&stage, after) {
                    // Restore the previous complete export if a stage is unavailable.
                    if matches!(current, Fingerprint::Missing)
                        && matches(&fingerprint(&journal.backup), before)
                        && before.is_some()
                    {
                        fs::rename(&journal.backup, &journal.destination)
                            .map_err(|e| io("Restore previous export", &journal.destination, e))?;
                        persistence::sync_directory(parent)?;
                    }
                    self.clear_journal()?;
                    return Ok(false);
                }
                fs::rename(&journal.stage, &journal.destination)
                    .map_err(|e| io("Install complete export", &journal.destination, e))?;
                persistence::sync_directory(parent)?;
            } else if !matches!(current, Fingerprint::Missing) {
                self.clear_journal()?;
                return Ok(false);
            }
        }
        let ownership_changed = match &journal.next {
            Some(next) => registry.owned.get(&journal.destination) != Some(next),
            None => registry.owned.contains_key(&journal.destination),
        };
        if ownership_changed {
            registry.generation = increment(registry.generation, "generation")?;
            if let Some(next) = &journal.next {
                let mut next = next.clone();
                next.generation = registry.generation;
                registry.owned.insert(journal.destination.clone(), next);
            } else {
                registry.owned.remove(&journal.destination);
            }
            self.save(registry)?;
        }
        // Delete transaction copies only if their bytes still match ownership.
        for (path, expected) in [(&journal.backup, before), (&journal.stage, after)] {
            if expected.is_some() && matches(&fingerprint(path), expected) {
                fs::remove_dir_all(path)
                    .map_err(|e| io("Remove completed transaction copy", path, e))?;
            }
        }
        self.clear_journal()?;
        Ok(true)
    }

    fn clear_journal(&self) -> Result<()> {
        let path = self.journal_path();
        fs::remove_file(&path).map_err(|e| io("Complete operation journal", &path, e))?;
        persistence::sync_directory(&self.root)
    }
}

enum Fingerprint {
    Missing,
    Present(String),
    Invalid(String),
    Unavailable(String),
}

fn fingerprint(path: &Path) -> Fingerprint {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Fingerprint::Missing,
        Err(error) => Fingerprint::Unavailable(format!("Cannot inspect destination: {error}")),
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            Fingerprint::Invalid("Destination is not a regular directory".into())
        }
        Ok(_) => match Bundle::read(path, false) {
            Ok(bundle) => Fingerprint::Present(bundle.digest),
            Err(Error::Io { source, .. }) => {
                Fingerprint::Unavailable(format!("Cannot inspect existing bundle: {source}"))
            }
            Err(error) => Fingerprint::Invalid(error.to_string()),
        },
    }
}

fn absolute(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(Error::InvalidTarget(
            "Data directory and skill home must be absolute paths without '..'".into(),
        ));
    }
    Ok(path.into())
}

fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io("Inspect library file", path, error)),
    }
}

fn increment(value: u64, limit: &'static str) -> Result<u64> {
    value.checked_add(1).ok_or(Error::LimitReached(limit))
}

fn resolve_missing_path(path: &Path) -> Result<PathBuf> {
    let mut ancestor = path;
    let mut missing = Vec::new();
    loop {
        match fs::canonicalize(ancestor) {
            Ok(mut resolved) => {
                for component in missing.iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = ancestor
                    .file_name()
                    .ok_or_else(|| Error::InvalidTarget("Cannot resolve target root".into()))?;
                missing.push(name.to_os_string());
                ancestor = ancestor
                    .parent()
                    .ok_or_else(|| Error::InvalidTarget("Cannot resolve target root".into()))?;
            }
            Err(error) => return Err(io("Resolve removed account root", ancestor, error)),
        }
    }
}

fn unsupported_status() -> ExportStatus {
    ExportStatus {
        target_key: "antigravity".into(), providers: vec!["antigravity".into()], path: PathBuf::new(),
        state: ExportState::Unsupported,
        detail: "Antigravity's active personal runtime root has not been established. No managed copy was exported.".into(),
    }
}

fn make_read_only(path: &Path) -> Result<()> {
    for child in fs::read_dir(path).map_err(|e| io("Read snapshot", path, e))? {
        let child = child.map_err(|e| io("Read snapshot", path, e))?;
        let child_path = child.path();
        if child_path.is_dir() {
            make_read_only(&child_path)?;
        }
        let mut permissions = fs::metadata(&child_path)
            .map_err(|e| io("Read snapshot permissions", &child_path, e))?
            .permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&child_path, permissions)
            .map_err(|e| io("Protect snapshot", &child_path, e))?;
    }
    let mut permissions = fs::metadata(path)
        .map_err(|e| io("Read snapshot permissions", path, e))?
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).map_err(|e| io("Protect snapshot", path, e))
}

#[cfg(test)]
mod tests;
