# What the shell chrome needs from boot, main, and the panes

Written by the shell agent. The lead relays these to the agent that owns
`boot.rs`, `main.rs`, `lib.rs`, `slots.rs`, and `view_data.rs`.

## Boot (`src/boot.rs`, `src/lib.rs`)

1. Initialize the history and projects packages after `Engine::init`:
   - `HistoryPackage::init(HistoryConfig::new(kv, notes, cx), cx)` with
     `notes = Arc::new(StoreNotesBackend::new(store, data_dir, executor))`.
   - `ProjectsGlobal::init_native(kv, data_dir, store, terminals_in_use, in_use, cx)`.
   Until boot does this, `shell::packages::ensure` initializes both the same
   way when the first shell window opens. It does nothing once the globals
   exist, so boot can take this over without touching the shell.
2. The shell owns `History::set_host` and `ProjectsGlobal::set_hooks`
   (`shell/hosts.rs`). Both mostly call the focused window's `Workspace`
   and show dialogs, so the shell installs them for the active window
   (on attach and on window activation). Please do not install another
   `HistoryHost` or `ProjectsHooks` in boot. For the calls that reach other
   packages (orchestration `run_lead_for_session`, `stop_active_run`,
   `delete_session`, `live_runs`; inbox `reveal_linked_update`; submit
   `finish_preparing_handoff`, `rebase_ci_repairs`, the handoff builders;
   remote), add the bodies to `shell/hosts.rs` directly or list the calls
   here and the shell agent adds them.
3. `restore_workspace` calls `crate::projects::last_project_path`. Use
   `monocode_engine::projects::recents::last_project_path(&kv)` instead.
   The shell deletes `src/projects.rs` and `src/history.rs`; drop
   `pub mod history; pub mod projects;` from `lib.rs`.

## Main (`src/main.rs`)

4. `view_data.rs` is no longer used: the shell reads the engine through
   `shell/data.rs`. Delete `view_data.rs` and its `mod view_data;` line.
   It imports `monocode_app::history` and `monocode_app::projects`, which
   go away.
5. The keymap, menus, and windows live in `src/keymap.rs`, `src/menus.rs`,
   and `src/windows.rs`. `shell/mod.rs` declares them with `#[path]`, so
   do not add `mod keymap;`, `mod menus;`, or `mod windows;` to `main.rs`
   (a second copy would register every action twice). In `main.rs`:
   - Call `shell::init(cx)` once after `monocode_ui::init`. It binds the
     keymap with the user's overrides, installs the app-level actions
     (quit, new window, zoom), and sets the macOS menu bar and dock menu.
   - Remove the `Quit` action, its `cmd-q` binding, the `cmd-t` binding,
     and `cx.on_action(|_: &Quit, ..)`. The keymap owns them now.
   - Build the main window's options with `shell::windows::window_options(size, cx)`
     so new windows (File > New Window) match the first one.
