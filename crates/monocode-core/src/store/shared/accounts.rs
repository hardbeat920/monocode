use super::*;
use crate::accounts::{Account, Book, DesktopOverrides, DEFAULT};

impl Store {
    fn shared_desktop_accounts(&self) -> Option<Book> {
        let preferences = self.desktop_preferences()?;
        if !preferences.is_available() {
            return None;
        }
        Book::from_desktop(
            &preferences
                .json("monocode.providerAccounts.v1")
                .unwrap_or(json!({})),
            &preferences
                .json("monocode.providerAccountSelections.v1")
                .unwrap_or(json!({})),
        )
    }

    pub(in crate::store) fn shared_accounts(&self) -> Result<Book, String> {
        if let Some(mut desktop) = self.shared_desktop_accounts() {
            *self.shared.as_ref().unwrap().account_source.borrow_mut() = Some(desktop.clone());
            if let Some(raw) = self.shared_setting("provider-account-overrides")? {
                desktop.apply_desktop_overrides(parse::<DesktopOverrides>(&raw)?);
            }
            // The desktop registry is authoritative. Old profile folders can
            // survive removing an account and must not recreate it in the UI.
            return Ok(desktop);
        }

        let mut book: Book = self
            .shared_setting("provider-accounts")?
            .map(|raw| parse(&raw))
            .transpose()?
            .unwrap_or_default();
        if let Some(directory) = self.data_dir() {
            for provider in ["codex", "claude"] {
                let root = directory.join("provider-accounts").join(provider);
                if !root.is_dir() {
                    continue;
                }
                let mut ids = Vec::new();
                for entry in std::fs::read_dir(root).map_err(|e| e.to_string())? {
                    let entry = entry.map_err(|e| e.to_string())?;
                    if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                        ids.push(entry.file_name().to_string_lossy().into_owned());
                    }
                }
                ids.sort();
                for id in ids {
                    if crate::accounts::validate_id(&id).is_ok()
                        && id != DEFAULT
                        && !book
                            .accounts
                            .iter()
                            .any(|a| a.provider == provider && a.id == id)
                    {
                        let ordinal = book
                            .accounts
                            .iter()
                            .filter(|a| a.provider == provider && a.id != DEFAULT)
                            .count()
                            + 2;
                        book.accounts.push(Account {
                            id,
                            provider: provider.into(),
                            label: format!("Account {ordinal}"),
                        });
                    }
                }
            }
        }
        Ok(book)
    }

    pub(in crate::store) fn shared_save_accounts(&self, book: &Book) -> Result<(), String> {
        let source = self
            .shared
            .as_ref()
            .unwrap()
            .account_source
            .borrow()
            .clone()
            .or_else(|| self.shared_desktop_accounts());
        if let Some(source) = source {
            self.shared_save_setting(
                "provider-account-overrides",
                &encode(&book.desktop_overrides(&source))?,
            )
        } else {
            self.shared_save_setting("provider-accounts", &encode(book)?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Kind;

    #[test]
    fn isolated_account_names_and_selections_survive_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("monocode.db");
        let store = Store::open_monocode(&path).unwrap();
        assert!(store.shared_desktop_accounts().is_none());
        let mut book = Book::default();
        let account = book.add(Kind::Codex, "My development account").unwrap();
        book.select(Kind::Codex, "/project", &account.id).unwrap();
        store.save_provider_accounts(&book).unwrap();
        drop(store);
        let store = Store::open_monocode(&path).unwrap();
        let book = store.provider_accounts().unwrap();
        assert_eq!(book.list(Kind::Codex)[1].label, "My development account");
        assert_eq!(book.selected(Kind::Codex, "/project"), account.id);
    }
}
