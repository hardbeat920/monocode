//! Name a new checkout without delaying or joining the user's provider turn.
use super::*;
use monocode_core::git::{self, naming};

impl Conversation {
    pub(super) fn generate_worktree_branch(&mut self, message: &str, cx: &mut Context<Self>) {
        if message.trim().is_empty() {
            return;
        }
        let kind = Kind::from_id(&self.summary.provider);
        let account = self.settings.account.clone();
        let message = message.to_owned();
        self.start_worktree_naming(cx, move |cwd, cancel| {
            if cfg!(test) {
                None
            } else {
                naming::generate(kind, cwd, &message, account, cancel)
            }
        });
    }

    fn start_worktree_naming(
        &mut self,
        cx: &mut Context<Self>,
        generate: impl FnOnce(&Path, Arc<AtomicBool>) -> Option<String> + Send + 'static,
    ) {
        if self.is_remote() || self.worktree_name_cancel.is_some() {
            return;
        }
        let Some(cwd) = self.settings.worktree_cwd.clone() else {
            return;
        };
        let id = self.summary.id;
        let Some(expected) = naming::temporary_branch(Path::new(&cwd), id) else {
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.worktree_name_cancel = Some(cancel.clone());
        let flag = cancel.clone();
        let root = self.summary.cwd.clone();
        let store = self.store.clone();
        let work_cwd = cwd.clone();
        let work = cx.background_executor().spawn(async move {
            let path = Path::new(&work_cwd);
            if git::snapshot(path).ok()?.current.as_deref() != Some(&expected) {
                return None;
            }
            let branch = generate(path, flag.clone())?;
            if flag.load(Ordering::Relaxed) {
                return None;
            }
            // A deleted session or a checkout selected while generation was
            // running must not receive a late name.
            let retained = locked(&store).ok()?.session_worktrees().ok()?;
            if retained.get(&id).and_then(Option::as_deref) != Some(work_cwd.as_str()) {
                return None;
            }
            naming::rename(Path::new(&root), path, &expected, &branch).ok()
        });
        cx.spawn(async move |this, cx| {
            let named = work.await;
            let _ = this.update(cx, |this, cx| {
                this.worktree_name_cancel = None;
                if cancel.load(Ordering::Relaxed) || this.execution_cwd() != cwd {
                    return;
                }
                if let Some(git) = named {
                    crate::git_cache::remember(&cwd, &git);
                    this.branch = git.current.clone();
                    this.git = Some(Arc::new(git));
                    this.refresh_git(cx);
                    cx.emit(SessionChanged);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::store::Store;
    use std::sync::Mutex;

    #[gpui::test]
    fn new_worktrees_receive_names_and_deleted_sessions_reject_late_names(
        cx: &mut gpui::TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!(
            "monocode-worktree-naming-{}-{}",
            std::process::id(),
            monocode_core::mono::new_id(),
        ));
        std::fs::create_dir_all(&root).unwrap();
        let project = root.join("project");
        std::fs::create_dir(&project).unwrap();
        let run = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(&project)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        run(&["init", "-b", "main"]);
        run(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            "Base",
        ]);
        let store = Arc::new(Mutex::new(Store::open(Path::new(":memory:")).unwrap()));

        for deleted in [false, true] {
            let summary = locked(&store)
                .unwrap()
                .create("codex", project.to_str().unwrap())
                .unwrap();
            let id = summary.id;
            let path = git::create_worktree(&project, "HEAD", id).unwrap();
            let temporary = naming::temporary_branch(&path, id).unwrap();
            let settings = Settings {
                worktree_cwd: Some(path.to_string_lossy().into_owned()),
                ..Default::default()
            };
            locked(&store)
                .unwrap()
                .save_agent_settings(id, &settings)
                .unwrap();
            let view = cx.new(|cx| {
                let mut view = Conversation::new(summary, store.clone(), cx);
                view.settings = settings;
                view
            });
            let worker_store = store.clone();
            let generated = format!("mc/descriptive-worktree-name-{id}");
            let name = generated.clone();
            view.update(cx, |view, cx| {
                view.start_worktree_naming(cx, move |_, _| {
                    if deleted {
                        locked(&worker_store).unwrap().delete(id).unwrap();
                    }
                    Some(name)
                });
            });
            cx.run_until_parked();
            let expected = if deleted {
                temporary.as_str()
            } else {
                generated.as_str()
            };
            assert_eq!(
                git::snapshot(&path).unwrap().current.as_deref(),
                Some(expected)
            );
            if !deleted {
                assert_eq!(
                    view.read_with(cx, |view, _| view.branch.clone()).as_deref(),
                    Some(expected)
                );
                assert_eq!(
                    crate::git_cache::get(path.to_str().unwrap())
                        .unwrap()
                        .current
                        .as_deref(),
                    Some(expected)
                );
                assert!(locked(&store).unwrap().load(id).unwrap().rows.is_empty());
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
