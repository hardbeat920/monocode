//! Labels and project selections use the same records as the desktop client.
use super::*;
use serde_json::Value;

/// Only explicit native edits override desktop metadata. Saving a selection
/// must not freeze all desktop account labels in a stale copy of the book.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct DesktopOverrides {
    accounts: Vec<Account>,
    removed: Vec<(String, String)>,
    selections: HashMap<String, HashMap<String, String>>,
}

impl Book {
    pub(crate) fn from_desktop(accounts: &Value, selections: &Value) -> Option<Self> {
        let stored = accounts.as_object()?;
        let mut book = Self::default();
        for kind in PROVIDERS {
            for value in stored
                .get(kind.id())
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(id) = value["id"].as_str().filter(|id| validate_id(id).is_ok()) else {
                    continue;
                };
                let label = clean_label(value["label"].as_str().unwrap_or_default());
                if label.is_empty()
                    || (id != DEFAULT && !supports_profiles(kind))
                    || book
                        .accounts
                        .iter()
                        .any(|a| a.provider == kind.id() && a.id == id)
                {
                    continue;
                }
                book.accounts.push(Account {
                    id: id.into(),
                    provider: kind.id().into(),
                    label,
                });
            }
        }
        if let Some(selections) = selections.as_object() {
            for (project, selected) in selections {
                for kind in PROVIDERS {
                    if let Some(id) = selected[kind.id()]
                        .as_str()
                        .filter(|id| book.exists(kind, id))
                    {
                        // This also applies the desktop path-key normalization.
                        let _ = book.select(kind, project, id);
                    }
                }
            }
        }
        Some(book)
    }

    pub(crate) fn desktop_overrides(&self, source: &Self) -> DesktopOverrides {
        let mut changes = DesktopOverrides::default();
        for kind in PROVIDERS {
            let desired = self.list(kind);
            let previous = source.list(kind);
            for account in &desired {
                if previous.iter().find(|a| a.id == account.id) != Some(account) {
                    changes.accounts.push(account.clone());
                }
            }
            for account in &previous {
                if !account.is_default() && !desired.iter().any(|a| a.id == account.id) {
                    changes
                        .removed
                        .push((account.provider.clone(), account.id.clone()));
                }
            }
            for project in source.selections.keys().chain(self.selections.keys()) {
                let id = self.selected(kind, project);
                if id != source.selected(kind, project) {
                    changes
                        .selections
                        .entry(project_key(project))
                        .or_default()
                        .insert(kind.id().into(), id);
                }
            }
        }
        changes
    }

    pub(crate) fn apply_desktop_overrides(&mut self, changes: DesktopOverrides) {
        self.accounts.retain(|a| {
            !changes
                .removed
                .iter()
                .any(|(p, id)| &a.provider == p && &a.id == id)
        });
        for account in changes.accounts {
            let Some(kind) = Kind::parse(&account.provider).filter(|k| supports_accounts(*k))
            else {
                continue;
            };
            if validate_id(&account.id).is_err()
                || (account.id != DEFAULT && !supports_profiles(kind))
            {
                continue;
            }
            let label = clean_label(&account.label);
            if label.is_empty() {
                continue;
            }
            if let Some(old) = self
                .accounts
                .iter_mut()
                .find(|a| a.provider == account.provider && a.id == account.id)
            {
                old.label = label;
            } else {
                self.accounts.push(Account { label, ..account });
            }
        }
        for (project, selected) in changes.selections {
            for (provider, id) in selected {
                if let Some(kind) = Kind::parse(&provider).filter(|k| supports_accounts(*k)) {
                    let _ = self.select(kind, &project, &id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_desktop_labels_defaults_and_project_selections() {
        let book = Book::from_desktop(&json!({
            "codex":[{"id":"default","label":" Personal "},{"id":"account-work","label":"Work account"},
                {"id":"account-work","label":"Duplicate"},{"id":"../bad","label":"Invalid"}],
            "claude":[{"id":"account-work","label":"Claude Work"}],
            "devin":[{"id":"unavailable-profile","label":"Ignore"}]
        }), &json!({"/project/":{"codex":"account-work","claude":"missing"}})).unwrap();
        assert_eq!(
            book.list(Kind::Codex)
                .iter()
                .map(|a| a.label.as_str())
                .collect::<Vec<_>>(),
            ["Personal", "Work account"]
        );
        assert_eq!(book.selected(Kind::Codex, "/project"), "account-work");
        assert_eq!(book.selected(Kind::Claude, "/project"), DEFAULT);
        assert_eq!(book.list(Kind::Claude)[1].label, "Claude Work");
        assert_eq!(book.list(Kind::Devin).len(), 1);
    }

    #[test]
    fn saving_a_selection_does_not_freeze_desktop_names_or_resurrect_removed_accounts() {
        let accounts = json!({"codex":[{"id":"account-work","label":"Work"},{"id":"account-old","label":"Old"}]});
        let source = Book::from_desktop(&accounts, &json!({})).unwrap();
        let mut native = source.clone();
        native
            .select(Kind::Codex, "/project", "account-work")
            .unwrap();
        native.remove(Kind::Codex, "account-old").unwrap();
        let edits = native.desktop_overrides(&source);
        let mut fresh = Book::from_desktop(
            &json!({"codex":[{"id":"account-work","label":"Renamed in desktop"},
            {"id":"account-old","label":"Old"}]}),
            &json!({}),
        )
        .unwrap();
        fresh.apply_desktop_overrides(edits);
        assert_eq!(fresh.list(Kind::Codex)[1].label, "Renamed in desktop");
        assert_eq!(fresh.selected(Kind::Codex, "/project"), "account-work");
        assert!(!fresh.exists(Kind::Codex, "account-old"));
    }

    #[test]
    fn explicit_native_renames_and_additions_survive_a_reload() {
        let source = Book::from_desktop(
            &json!({"codex":[{"id":"account-work","label":"Work"}]}),
            &json!({}),
        )
        .unwrap();
        let mut native = source.clone();
        native
            .rename(Kind::Codex, "account-work", "Native name")
            .unwrap();
        let added = native.add(Kind::Claude, "Extra").unwrap();
        let mut reloaded = source.clone();
        reloaded.apply_desktop_overrides(native.desktop_overrides(&source));
        assert_eq!(reloaded.list(Kind::Codex)[1].label, "Native name");
        assert_eq!(reloaded.list(Kind::Claude)[1], added);
    }
}
