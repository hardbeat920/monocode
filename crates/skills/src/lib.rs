//! A local skill library with editable sources and owned provider exports.
//!
//! Export status describes files on this host. It does not prove that a provider
//! process loaded a skill or that its external tools are available.

mod bundle;
mod manager;
mod metadata;
mod persistence;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub use manager::{default_targets, SkillManager};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{action} at {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Invalid skill bundle: {0}")]
    InvalidBundle(String),
    #[error("Skill name '{name}' already has different content. Existing skill ID: {existing_id}")]
    NameConflict { name: String, existing_id: String },
    #[error("Unknown skill ID: {0}")]
    UnknownSkill(String),
    #[error("Cannot read the skill registry at {path}: {detail}. The file was preserved.")]
    InvalidRegistry { path: PathBuf, detail: String },
    #[error("Cannot recover the skill operation at {path}: {detail}. The journal was preserved.")]
    InvalidJournal { path: PathBuf, detail: String },
    #[error("Invalid export target: {0}")]
    InvalidTarget(String),
    #[error("The skill library has reached its {0} limit")]
    LimitReached(&'static str),
}

pub(crate) fn io(action: &'static str, path: impl Into<PathBuf>, source: std::io::Error) -> Error {
    Error::Io {
        action,
        path: path.into(),
        source,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExportState {
    Exported,
    Pending,
    Conflict,
    Disabled,
    Unsupported,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExportStatus {
    pub target_key: String,
    pub providers: Vec<String>,
    pub path: PathBuf,
    pub state: ExportState,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub digest: String,
    pub revision: u64,
    pub shared: bool,
    /// The editable managed directory. Applied snapshots are separate.
    pub source_path: PathBuf,
    /// The immutable directory for this entry's captured applied revision.
    pub applied_path: PathBuf,
    /// Original directories imported into this entry.
    pub origins: Vec<PathBuf>,
    pub statuses: Vec<ExportStatus>,
    pub warnings: Vec<String>,
}

/// A coherent library view read under one registry lock.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillSnapshot {
    pub generation: u64,
    pub entries: Vec<SkillEntry>,
    pub targets: Vec<ExportTarget>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillExportStatus {
    pub skill_id: String,
    pub export: ExportStatus,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ReconcileReport {
    pub generation: u64,
    pub statuses: Vec<SkillExportStatus>,
    /// Destinations installed, replaced, or removed by this call.
    pub changed_paths: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImportResult {
    pub entry: SkillEntry,
    pub already_present: bool,
    pub report: ReconcileReport,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportTarget {
    pub key: String,
    pub root: PathBuf,
    pub providers: Vec<String>,
}

impl ExportTarget {
    /// Supply an already resolved account or configuration skills directory.
    pub fn new(key: impl Into<String>, root: impl Into<PathBuf>, providers: Vec<String>) -> Self {
        Self {
            key: key.into(),
            root: root.into(),
            providers,
        }
    }
}
