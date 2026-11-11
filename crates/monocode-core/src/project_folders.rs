use crate::{provider::Kind, store::Store};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFolders {
    pub additional: Vec<PathBuf>,
}
impl ProjectFolders {
    pub fn supported(provider: Kind, project: &str) -> bool {
        provider == Kind::Codex && !project.starts_with("remote://")
    }
    pub fn validate_support(&self, provider: Kind, project: &str) -> Result<(), String> {
        if !self.additional.is_empty() && !Self::supported(provider, project) {
            return Err("Additional folders are supported only by local Codex conversations. Remove them in Project settings to use this provider.".into());
        }
        Ok(())
    }
    pub fn validate(&self, primary: &Path) -> Result<(), String> {
        let mut seen = vec![directory(primary)?];
        for path in &self.additional {
            let normalized = directory(path)?;
            if &normalized != path {
                return Err(format!(
                    "Folder target changed. Remove and add this folder again: {}",
                    path.display()
                ));
            }
            if seen.contains(&normalized) {
                return Err(format!("Folder is already included: {}", path.display()));
            }
            seen.push(normalized);
        }
        Ok(())
    }
}
fn directory(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() || !path.is_dir() {
        return Err(format!(
            "Choose an existing absolute folder path: {}",
            path.display()
        ));
    }
    path.canonicalize()
        .map_err(|e| format!("Cannot open folder {}: {e}", path.display()))
}
impl Store {
    pub fn project_folders(&self, project: &str) -> Result<ProjectFolders, String> {
        Ok(self.project_folder_snapshot(project)?.unwrap_or_default())
    }
    pub fn project_folder_snapshot(&self, project: &str) -> Result<Option<ProjectFolders>, String> {
        self.setting(&format!("project-folders:{project}"))?
            .map(|raw| {
                serde_json::from_str(&raw)
                    .map_err(|e| format!("Cannot read additional folders: {e}"))
            })
            .transpose()
    }
    pub fn add_project_folder(&self, project: &str, path: &Path) -> Result<ProjectFolders, String> {
        if project.starts_with("remote://") {
            return Err("Additional folders are not supported for remote projects.".into());
        }
        let mut folders = self.project_folders(project)?;
        folders.additional.push(directory(path)?);
        folders.validate(Path::new(project))?;
        self.save_setting(
            &format!("project-folders:{project}"),
            &serde_json::to_string(&folders).map_err(|e| e.to_string())?,
        )?;
        Ok(folders)
    }
    pub fn remove_project_folder(
        &self,
        project: &str,
        path: &Path,
    ) -> Result<ProjectFolders, String> {
        let mut folders = self.project_folders(project)?;
        folders.additional.retain(|saved| saved != path);
        self.save_setting(
            &format!("project-folders:{project}"),
            &serde_json::to_string(&folders).map_err(|e| e.to_string())?,
        )?;
        Ok(folders)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folders_persist_per_project_and_reject_invalid_or_duplicate_paths() {
        let dir = tempfile::tempdir().unwrap();
        let primary = dir.path().join("primary");
        let other = dir.path().join("other");
        let extra = dir.path().join("extra");
        for path in [&primary, &other, &extra] {
            std::fs::create_dir(path).unwrap();
        }
        let primary = primary.to_str().unwrap();
        let db = dir.path().join("store.db");
        let store = Store::open(&db).unwrap();
        assert!(store
            .add_project_folder(primary, Path::new("relative"))
            .is_err());
        assert!(store.add_project_folder(primary, &db).is_err());
        assert!(store
            .add_project_folder(primary, Path::new(primary))
            .is_err());
        let saved = store.add_project_folder(primary, &extra).unwrap();
        assert!(store.add_project_folder(primary, &extra).is_err());
        #[cfg(unix)]
        {
            let alias = dir.path().join("alias");
            std::os::unix::fs::symlink(&extra, &alias).unwrap();
            assert!(store.add_project_folder(primary, &alias).is_err());
        }
        drop(store);
        let store = Store::open(&db).unwrap();
        assert_eq!(store.project_folders(primary).unwrap(), saved);
        assert!(store
            .project_folders(other.to_str().unwrap())
            .unwrap()
            .additional
            .is_empty());
        std::fs::remove_dir(&extra).unwrap();
        assert!(saved.validate(Path::new(primary)).is_err());
        assert!(store
            .remove_project_folder(primary, &saved.additional[0])
            .unwrap()
            .additional
            .is_empty());
        store
            .save_setting(&format!("project-folders:{primary}"), "invalid")
            .unwrap();
        assert!(store.project_folders(primary).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn changed_symlink_target_requires_reconnection_but_can_be_removed() {
        let dir = tempfile::tempdir().unwrap();
        let primary = dir.path().canonicalize().unwrap();
        let extra = primary.join("extra");
        let replacement = primary.join("replacement");
        std::fs::create_dir(&extra).unwrap();
        std::fs::create_dir(&replacement).unwrap();
        let store = Store::open(&primary.join("store.db")).unwrap();
        let folders = store
            .add_project_folder(primary.to_str().unwrap(), &extra)
            .unwrap();
        std::fs::remove_dir(&extra).unwrap();
        std::os::unix::fs::symlink(&replacement, &extra).unwrap();
        assert!(folders
            .validate(&primary)
            .unwrap_err()
            .contains("target changed"));
        assert!(store
            .remove_project_folder(primary.to_str().unwrap(), &extra)
            .unwrap()
            .additional
            .is_empty());
    }
    #[test]
    fn unsupported_requests_and_persisted_execution_snapshots_are_rejected() {
        let folders = ProjectFolders {
            additional: vec!["/extra".into()],
        };
        assert!(folders.validate_support(Kind::Codex, "/primary").is_ok());
        assert!(folders.validate_support(Kind::Claude, "/primary").is_err());
        assert!(folders
            .validate_support(Kind::Codex, "remote://host/project")
            .is_err());
        let settings = crate::agent::Settings {
            additional_folders: Some(folders.additional),
            ..Default::default()
        };
        let encoded = serde_json::to_string(&settings).unwrap();
        assert!(!encoded.contains("additional_folders"));
        let decoded: crate::agent::Settings = serde_json::from_str(&encoded).unwrap();
        assert!(decoded.additional_folders.is_none());
    }
}
