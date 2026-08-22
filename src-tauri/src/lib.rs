use tauri::Manager;

mod checkpoint;
mod cursor_store;
mod fs;
mod harness;
#[cfg(target_os = "macos")]
mod macos;
mod menu;
mod project_logo;
mod pty;
mod search;
mod session_store;
mod skills;
mod window;
mod window_transfer;

// Phase 1 seam: spawn / kill harness children per MonoCode thread.
// Adapters own the protocol; this host only supervises processes.

/// Project directory for new sessions — prefer cwd, else home.
#[tauri::command]
fn default_cwd() -> String {
    if let Ok(cwd) = std::env::current_dir() {
        return cwd.to_string_lossy().into_owned();
    }
    dirs_home().unwrap_or_else(|| "~".into())
}

#[tauri::command]
fn home_dir() -> String {
    dirs_home().unwrap_or_else(|| "~".into())
}

pub(crate) fn dirs_home() -> Option<String> {
    std::env::var_os("HOME").map(|h| h.to_string_lossy().into_owned())
}

#[tauri::command]
fn set_traffic_lights_visible(
    #[allow(unused_variables)] window: tauri::WebviewWindow,
    #[allow(unused_variables)] visible: bool,
) {
    #[cfg(target_os = "macos")]
    macos::set_visible(&window, visible);
}

#[tauri::command]
fn set_window_background_blur(
    #[allow(unused_variables)] window: tauri::WebviewWindow,
    #[allow(unused_variables)] radius: u8,
) {
    #[cfg(target_os = "macos")]
    macos::set_background_blur_radius(&window, radius);
}

#[tauri::command]
fn set_dock_badge(
    #[allow(unused_variables)] window: tauri::WebviewWindow,
    #[allow(unused_variables)] count: u32,
) {
    #[cfg(target_os = "macos")]
    macos::set_window_badge(&window, count);
}

#[tauri::command]
fn open_new_window(app: tauri::AppHandle) -> Result<(), String> {
    window::open_new_window(&app)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(harness::HarnessHost::new())
        .manage(pty::PtyHost::new())
        .manage(window_transfer::WindowTransferState::new())
        .setup(|app| {
            session_store::init(app.handle())?;
            checkpoint::init(app.handle())?;
            menu::install(app.handle())?;
            #[cfg(target_os = "macos")]
            {
                macos::install_dock_menu(app.handle());
                if let Some(window) = app.get_webview_window("main") {
                    macos::install(&window);
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_decorations(false);
                    let _ = window.set_shadow(true);
                }
            }
            Ok(())
        })
        .on_menu_event(|app, event| {
            menu::dispatch(app, event.id().as_ref());
        })
        .invoke_handler(tauri::generate_handler![
            default_cwd,
            home_dir,
            fs::list_dir,
            fs::list_project_files,
            fs::git_diff_stats,
            fs::git_diff_index,
            fs::git_file_diff,
            fs::git_stage_file,
            fs::git_stage_contents,
            fs::git_unstage_file,
            fs::git_discard_file,
            fs::git_stage_all,
            fs::git_unstage_all,
            fs::git_commit,
            fs::git_staged_context,
            fs::git_push,
            fs::git_pull,
            fs::git_sync,
            fs::git_range_context,
            fs::git_pr_status,
            fs::git_pr_create,
            fs::git_branches,
            fs::git_checkout,
            fs::git_create_branch,
            fs::create_path,
            fs::rename_path,
            fs::delete_path,
            fs::copy_path,
            fs::move_path,
            fs::reveal_path,
            fs::clone_repo,
            fs::read_file_preview,
            fs::stat_files,
            fs::inspect_paths,
            fs::read_file_base64,
            fs::write_attachment,
            fs::read_text_file,
            fs::write_text_file,
            skills::list_skills,
            search::search_project,
            cursor_store::cursor_tool_calls,
            harness::harness_resolve_cursor,
            harness::harness_resolve_codex,
            harness::harness_resolve_opencode,
            harness::harness_resolve_claude,
            harness::harness_resolve_pi,
            harness::harness_free_port,
            harness::harness_spawn,
            harness::harness_write,
            harness::harness_kill,
            harness::harness_kill_all,
            harness::harness_http,
            harness::harness_sse_open,
            harness::harness_sse_close,
            harness::harness_exec,
            pty::pty_spawn,
            pty::pty_write,
            pty::pty_resize,
            pty::pty_status,
            pty::pty_kill,
            pty::pty_kill_all,
            session_store::session_upsert,
            session_store::session_list_by_project,
            session_store::session_get,
            session_store::session_delete,
            session_store::session_set_in_flight,
            session_store::session_list_in_flight,
            session_store::session_take_in_flight,
            session_store::workspace_set_snapshot,
            session_store::workspace_get_snapshot,
            checkpoint::session_checkpoint_ensure,
            checkpoint::session_checkpoint_capture,
            checkpoint::session_checkpoint_sync,
            checkpoint::session_checkpoint_status,
            checkpoint::session_checkpoint_stats,
            checkpoint::session_checkpoint_undo,
            checkpoint::session_checkpoint_keep,
            set_traffic_lights_visible,
            set_window_background_blur,
            set_dock_badge,
            open_new_window,
            window::hide_window,
            window::destroy_window,
            window::confirm_quit,
            window_transfer::stage_window_transfer,
            window_transfer::take_window_transfer,
            project_logo::save_project_logo,
            project_logo::remove_project_logo,
        ])
        .build(tauri::generate_context!())
        .expect("error while building MonoCode");

    app.run(|handle, event| match event {
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } => {
            let _ = window::show_hidden_or_open_new(handle);
        }
        tauri::RunEvent::Ready => {
            #[cfg(target_os = "macos")]
            {
                macos::request_badge_authorization();
                #[cfg(debug_assertions)]
                macos::prefer_bundle_dock_icon();
            }
            window::ensure_launch_window_visible(handle);
        }
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::Destroyed,
            ..
        } => {
            let other_window = handle.webview_windows().keys().any(|name| name != &label);
            if !other_window {
                reap_harness_children(handle);
            }
        }
        tauri::RunEvent::ExitRequested { api, code, .. } => {
            if window::allow_exit() {
                return;
            }
            api.prevent_exit();
            // Last window destroyed (red button). Stay in the dock; ⌘Q is a
            // separate menu handler and arrives with an exit code.
            if code.is_none() {
                return;
            }
            window::request_quit(handle);
        }
        tauri::RunEvent::Exit => {
            reap_harness_children(handle);
        }
        _ => {}
    });
}

fn reap_harness_children(handle: &tauri::AppHandle) {
    if let Some(host) = handle.try_state::<harness::HarnessHost>() {
        host.kill_all();
    }
    if let Some(host) = handle.try_state::<pty::PtyHost>() {
        host.kill_all();
    }
}

#[cfg(all(debug_assertions, target_os = "macos"))]
pub fn ensure_macos_dev_bundle() {
    macos::ensure_dev_bundle();
}
