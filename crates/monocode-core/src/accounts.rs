//! Provider-owned sign-ins. Metadata never contains credentials.
mod desktop;
mod identity;
pub(crate) use desktop::DesktopOverrides;
pub(crate) mod login;
mod transfer;
pub use identity::Identity;
pub use login::arguments as login_arguments;
pub use login::{authenticate, AuthAction};
mod processes;
#[cfg(test)]
mod tests;
pub use processes::stop_processes;
pub(crate) use processes::{track, Tracked};

use crate::provider::Kind;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
};

pub const DEFAULT: &str = "default";
pub const PROVIDERS: [Kind; 3] = [Kind::Claude, Kind::Codex, Kind::Devin];
fn default_id() -> String {
    DEFAULT.into()
}
pub(crate) fn request_id() -> String {
    let mut bytes: [u8; 16] = rand::random();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex = format!("{:032x}", u128::from_be_bytes(bytes));
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(default)]
pub struct Profile {
    pub id: String,
    /// Supplied by the native store, never accepted from persisted settings.
    #[serde(skip)]
    pub root: Option<PathBuf>,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            id: default_id(),
            root: None,
        }
    }
}
impl Profile {
    pub fn new(id: &str, root: &Path) -> Self {
        if id == DEFAULT {
            return Self::default();
        }
        Self {
            id: id.into(),
            root: Some(root.into()),
        }
    }
    pub fn directory(&self, kind: Kind) -> Result<Option<PathBuf>, String> {
        if self.id == DEFAULT {
            return Ok(None);
        }
        if !supports_profiles(kind) {
            return Err("Provider account profiles are not supported for this provider".into());
        }
        validate_id(&self.id)?;
        let root = self
            .root
            .as_ref()
            .ok_or("Provider account storage is unavailable")?;
        Ok(Some(
            root.join("provider-accounts")
                .join(kind.id())
                .join(&self.id),
        ))
    }
    pub fn home(&self, kind: Kind) -> Result<PathBuf, String> {
        if let Some(dir) = self.directory(kind)? {
            return Ok(dir);
        }
        let (var, dir) = match kind {
            Kind::Codex => ("CODEX_HOME", ".codex"),
            Kind::Claude => ("CLAUDE_CONFIG_DIR", ".claude"),
            _ => {
                return Err("Native transcript transfer is not supported for this provider".into())
            }
        };
        std::env::var_os(var)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| home().map(|p| p.join(dir)))
            .ok_or_else(|| format!("Could not find the {} home directory", kind.label()))
    }
    pub fn apply(&self, kind: Kind, command: &mut Command) -> Result<(), String> {
        let Some(dir) = self.directory(kind)? else {
            return Ok(());
        };
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Could not create the account directory: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        match kind {
            Kind::Claude => {
                command
                    .env("CLAUDE_CONFIG_DIR", &dir)
                    .env("CLAUDE_SECURESTORAGE_CONFIG_DIR", &dir)
                    .env_remove("ANTHROPIC_API_KEY")
                    .env_remove("ANTHROPIC_AUTH_TOKEN")
                    .env_remove("CLAUDE_CODE_OAUTH_TOKEN");
            }
            Kind::Codex => {
                command
                    .env("CODEX_HOME", &dir)
                    .env_remove("OPENAI_API_KEY")
                    .env_remove("CODEX_API_KEY")
                    .env_remove("CODEX_ACCESS_TOKEN");
            }
            _ => unreachable!(),
        }
        Ok(())
    }
    pub fn identity(&self, kind: Kind) -> Option<Identity> {
        identity::read(self, kind)
    }
    pub fn transfer(&self, to: &Self, kind: Kind, thread: &str) -> Result<(), String> {
        transfer::transfer(self, to, kind, thread)
    }
    pub fn remove_credentials(&self, kind: Kind) -> Result<(), String> {
        let dir = self
            .directory(kind)?
            .ok_or("The default provider account cannot be removed")?;
        #[cfg(target_os = "macos")]
        if kind == Kind::Claude {
            crate::usage::claude::delete_keychain(&dir)?;
        }
        match std::fs::symlink_metadata(&dir) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                std::fs::remove_dir_all(dir).map_err(|e| e.to_string())
            }
            Ok(_) => std::fs::remove_file(dir).map_err(|e| e.to_string()),
        }
    }
}

pub fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}
pub fn supports_profiles(kind: Kind) -> bool {
    matches!(kind, Kind::Codex | Kind::Claude)
}
pub fn supports_accounts(kind: Kind) -> bool {
    PROVIDERS.contains(&kind)
}
pub fn validate_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 80
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        Err("Invalid provider account id".into())
    } else {
        Ok(())
    }
}
fn clean_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(48)
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Account {
    pub id: String,
    pub provider: String,
    pub label: String,
}
impl Account {
    pub fn is_default(&self) -> bool {
        self.id == DEFAULT
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Book {
    pub accounts: Vec<Account>,
    pub selections: HashMap<String, HashMap<String, String>>,
}
impl Book {
    pub fn list(&self, kind: Kind) -> Vec<Account> {
        if !supports_accounts(kind) {
            return Vec::new();
        }
        let label = self
            .accounts
            .iter()
            .find(|a| a.provider == kind.id() && a.is_default())
            .map(|a| clean_label(&a.label))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Default account".into());
        let mut rows = vec![Account {
            id: DEFAULT.into(),
            provider: kind.id().into(),
            label,
        }];
        if supports_profiles(kind) {
            for a in self
                .accounts
                .iter()
                .filter(|a| a.provider == kind.id() && !a.is_default())
            {
                let label = clean_label(&a.label);
                if validate_id(&a.id).is_ok()
                    && !label.is_empty()
                    && !rows.iter().any(|row| row.id == a.id)
                {
                    rows.push(Account { label, ..a.clone() });
                }
            }
        }
        rows
    }
    pub fn add(&mut self, kind: Kind, label: &str) -> Result<Account, String> {
        if !supports_profiles(kind) {
            return Err("This provider supports only its default account".into());
        }
        let label = clean_label(label);
        let account = Account {
            id: format!("account-{}", request_id()),
            provider: kind.id().into(),
            label: if label.is_empty() {
                format!("Account {}", self.list(kind).len() + 1)
            } else {
                label
            },
        };
        self.accounts.push(account.clone());
        Ok(account)
    }
    pub fn rename(&mut self, kind: Kind, id: &str, label: &str) -> Result<(), String> {
        let label = clean_label(label);
        if label.is_empty() || !self.exists(kind, id) {
            return Err("Account name is empty or account no longer exists".into());
        }
        if let Some(account) = self
            .accounts
            .iter_mut()
            .find(|a| a.provider == kind.id() && a.id == id)
        {
            account.label = label;
        } else {
            self.accounts.push(Account {
                id: id.into(),
                provider: kind.id().into(),
                label,
            });
        }
        Ok(())
    }
    pub fn exists(&self, kind: Kind, id: &str) -> bool {
        self.list(kind).iter().any(|a| a.id == id)
    }
    pub fn remove(&mut self, kind: Kind, id: &str) -> Result<(), String> {
        if id == DEFAULT || !self.exists(kind, id) {
            return Err("This account cannot be removed".into());
        }
        self.accounts
            .retain(|a| a.provider != kind.id() || a.id != id);
        for selections in self.selections.values_mut() {
            if selections.get(kind.id()).is_some_and(|v| v == id) {
                selections.remove(kind.id());
            }
        }
        Ok(())
    }
    pub fn selected(&self, kind: Kind, project: &str) -> String {
        self.selections
            .get(&project_key(project))
            .and_then(|s| s.get(kind.id()))
            .filter(|id| self.exists(kind, id))
            .cloned()
            .unwrap_or_else(default_id)
    }
    pub fn select(&mut self, kind: Kind, project: &str, id: &str) -> Result<(), String> {
        if !self.exists(kind, id) {
            return Err("Provider account no longer exists".into());
        }
        self.selections
            .entry(project_key(project))
            .or_default()
            .insert(kind.id().into(), id.into());
        Ok(())
    }
}
fn project_key(project: &str) -> String {
    let key = project.trim().replace('\\', "/");
    let key = key.trim_end_matches('/');
    let key = if key.is_empty() { "~" } else { key };
    if cfg!(windows) {
        key.to_lowercase()
    } else {
        key.into()
    }
}
