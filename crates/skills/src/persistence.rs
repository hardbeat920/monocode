use crate::{bundle::valid_name, io, Error, ExportTarget, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub(crate) const SCHEMA_VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Registry {
    pub schema_version: u32,
    pub generation: u64,
    pub skill_home: PathBuf,
    pub entries: Vec<StoredSkill>,
    pub targets: Vec<ExportTarget>,
    pub owned: BTreeMap<PathBuf, OwnedExport>,
    /// Tombstones let recovery discard account operations after durable retirement.
    #[serde(default)]
    pub retired_roots: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct StoredSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub digest: String,
    pub revision: u64,
    pub shared: bool,
    pub origins: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct OwnedExport {
    pub skill_id: String,
    pub target: ExportTarget,
    pub digest: String,
    pub generation: u64,
}

/// One recoverable directory replacement. Paths must be siblings on one volume.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Journal {
    pub schema_version: u32,
    pub destination: PathBuf,
    pub stage: PathBuf,
    pub backup: PathBuf,
    pub previous: Option<OwnedExport>,
    pub next: Option<OwnedExport>,
}

pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let metadata = fs::symlink_metadata(path).map_err(|e| io("Read manifest", path, e))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(Error::InvalidRegistry {
            path: path.into(),
            detail: "Manifest is not a regular file".into(),
        });
    }
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Err(Error::InvalidRegistry {
            path: path.into(),
            detail: "Manifest exceeds 8 MiB".into(),
        });
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| io("Read manifest", path, e))?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io("Read manifest", path, e))?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(Error::InvalidRegistry {
            path: path.into(),
            detail: "Manifest exceeds 8 MiB".into(),
        });
    }
    serde_json::from_slice(&bytes).map_err(|e| Error::InvalidRegistry {
        path: path.into(),
        detail: e.to_string(),
    })
}

pub(crate) fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::InvalidTarget("Manifest has no parent directory".into()))?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| Error::InvalidRegistry {
        path: path.into(),
        detail: e.to_string(),
    })?;
    if bytes.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(Error::InvalidRegistry {
            path: path.into(),
            detail: "Manifest exceeds 8 MiB".into(),
        });
    }
    let temp = parent.join(format!(".manifest-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = File::create_new(&temp).map_err(|e| io("Create manifest", &temp, e))?;
    file.write_all(&bytes)
        .map_err(|e| io("Write manifest", &temp, e))?;
    file.sync_all()
        .map_err(|e| io("Flush manifest", &temp, e))?;
    drop(file);
    fs::rename(&temp, path).map_err(|e| io("Replace manifest", path, e))?;
    sync_directory(parent)
}

pub(crate) fn validate_registry(registry: &Registry, path: &Path, home: &Path) -> Result<()> {
    let invalid = |detail: String| Error::InvalidRegistry {
        path: path.into(),
        detail,
    };
    if registry.schema_version != SCHEMA_VERSION {
        return Err(invalid(format!(
            "Unsupported schema version {}",
            registry.schema_version
        )));
    }
    if registry.skill_home != home {
        return Err(invalid("Registry belongs to a different skill home".into()));
    }
    if registry.retired_roots.iter().any(|root| {
        !root.is_absolute()
            || root
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
    }) {
        return Err(invalid("Invalid retired account root".into()));
    }
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    for entry in &registry.entries {
        if uuid::Uuid::parse_str(&entry.id).is_err() || !ids.insert(entry.id.clone()) {
            return Err(invalid("Invalid or duplicate skill ID".into()));
        }
        if !valid_name(&entry.name) || !names.insert(entry.name.clone()) {
            return Err(invalid("Invalid or duplicate skill name".into()));
        }
        if !valid_digest(&entry.digest)
            || entry.revision == 0
            || entry.description.trim().is_empty()
        {
            return Err(invalid("Invalid applied skill revision".into()));
        }
    }
    let mut target_keys = BTreeSet::new();
    for target in &registry.targets {
        validate_target(target).map_err(|e| invalid(e.to_string()))?;
        if !target_keys.insert(&target.key) {
            return Err(invalid("Duplicate export target key".into()));
        }
    }
    for (destination, owned) in &registry.owned {
        validate_target(&owned.target).map_err(|e| invalid(e.to_string()))?;
        let Some(entry) = registry
            .entries
            .iter()
            .find(|entry| entry.id == owned.skill_id)
        else {
            return Err(invalid("Export refers to an unknown skill".into()));
        };
        if !destination.is_absolute()
            || destination.file_name().and_then(|n| n.to_str()) != Some(&entry.name)
            || destination.parent() != Some(owned.target.root.as_path())
            || !valid_digest(&owned.digest)
            || owned.generation > registry.generation
        {
            return Err(invalid("Invalid owned export path or revision".into()));
        }
    }
    Ok(())
}

pub(crate) fn validate_target(target: &ExportTarget) -> Result<()> {
    if target.key.is_empty() || target.key.len() > 256 || target.key.chars().any(char::is_control) {
        return Err(Error::InvalidTarget(
            "Target key must contain 1 to 256 printable characters".into(),
        ));
    }
    if !target.root.is_absolute()
        || target
            .root
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(Error::InvalidTarget(
            "Target root must be an absolute path without '..'".into(),
        ));
    }
    if target.providers.is_empty()
        || target
            .providers
            .iter()
            .any(|p| p.is_empty() || p.len() > 128)
    {
        return Err(Error::InvalidTarget(
            "Target must name its providers".into(),
        ));
    }
    Ok(())
}

pub(crate) fn valid_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

pub(crate) fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| io("Flush directory", path, e))?;
    // Windows does not permit opening directories with File::open.
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
