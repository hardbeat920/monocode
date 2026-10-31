#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod accounts;
mod activity;
mod app_menu;
mod arcade;
mod assets;
mod automations;
mod changes;
mod composer_completion;
mod composer_motion;
mod composer_runner;
mod context_meter;
mod conversation;
#[cfg(debug_assertions)]
mod dev_reload;
mod diff_view;
mod editor_analysis;
mod editor_scrollbar;
mod editor_syntax;
mod explorer;
mod file_editor;
mod file_icons;
mod fixtures;
mod fonts;
mod fs_watch;
mod git_cache;
mod git_graph;
mod git_picker;
mod git_status;
mod gpu_trim;
#[cfg(target_os = "macos")]
mod hang_watchdog;
mod history;
mod image_lightbox;
mod image_memory;
mod inbox;
mod ink_field;
mod input;
mod markdown;
mod markdown_model;
#[cfg(target_os = "macos")]
mod memory_log;
#[cfg(all(test, target_os = "macos"))]
mod metal_tests;
mod mobile;
mod modal;
mod monos;
mod notes;
mod notifications;
mod pane_layout;
mod pane_motion;
mod particle_text;
mod pickers;
mod popover;
mod project_background;
mod project_search;
mod project_settings;
mod project_tabs;
mod project_theme;
mod pulse;
mod questions;
mod quick_composer;
mod quick_open;
mod reflow_motion;
mod resource_pressure;
mod response_actions;
mod reveal;
mod review;
mod right_panel;
mod runtime;
mod search;
mod session_mark;
mod session_review;
mod settings;
mod shell;
mod sound;
mod syntax;
mod tab_label;
mod tab_motion;
mod terminal;
mod theme;
#[cfg(target_os = "macos")]
mod traffic_lights;
mod transcript;
mod usage_footer;
mod visuals;
mod window_state;
mod workspace;
mod worktree_switcher;

use gpui::{App, KeyBinding, TitlebarOptions, WindowOptions, actions, point, prelude::*, px, size};
use monocode_core::{store::Store, transcript::Transcript};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};
/// libmalloc keeps freed large spans dirty, so startup decodes and image churn
/// stayed in the footprint. mimalloc 2 returns them; 3 retains churn too.
#[cfg(target_os = "macos")]
#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

actions!(application, [Quit]);
fn quit(cx: &mut App) {
    if notifications::updater::defer_quit(cx) {
        return;
    }
    let shutdown = cx
        .background_executor()
        .spawn(async { monocode_core::process::shutdown_all() });
    cx.spawn(async move |cx| {
        shutdown.await;
        cx.update(|cx| cx.quit());
    })
    .detach();
}

/// Quit, then relaunch the same app once this process has exited.
fn restart(cx: &mut App) {
    let shutdown = cx
        .background_executor()
        .spawn(async { monocode_core::process::shutdown_all() });
    cx.spawn(async move |cx| {
        shutdown.await;
        cx.update(|cx| cx.restart());
    })
    .detach();
}

fn data_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("MONOCODE_NATIVE_DATA_DIR") {
        return path.into();
    }
    #[cfg(target_os = "macos")]
    let base = PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join("Library/Application Support");
    #[cfg(windows)]
    let base = PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default());
    #[cfg(not(any(target_os = "macos", windows)))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    base.join("Monocode Development")
}
// The packaged app runs this binary directly: a launcher that execs it changes the
// process's pid version, and macOS then refuses its menu bar item. The bundle
// records its data mode in Info.plist instead of passing a flag.
fn bundle_uses_monocode_data() -> Option<bool> {
    let exe = std::env::current_exe().ok()?;
    let plist = std::fs::read_to_string(exe.parent()?.parent()?.join("Info.plist")).ok()?;
    plist_data_mode(&plist)
}
fn plist_data_mode(plist: &str) -> Option<bool> {
    let rest = plist.split("<key>MonocodeDataMode</key>").nth(1)?;
    let mode = rest
        .trim_start()
        .strip_prefix("<string>")?
        .split('<')
        .next()?;
    match mode {
        "monocode-data" => Some(true),
        "dev-data" => Some(false),
        _ => None,
    }
}
fn main() {
    if let Some(code) = monocode_core::remote::ssh_askpass::maybe_run() {
        std::process::exit(code);
    }
    let cli_args: Vec<_> = std::env::args().skip(1).collect();
    if cli_args
        .first()
        .is_some_and(|arg| arg == "--version" || arg == "-V")
    {
        println!("Monocode Native {}", notifications::updater::version());
        return;
    }
    if cli_args.first().is_some_and(|a| a == "visual-mcp") {
        if let Err(error) = monocode_core::visual_bridge::mcp_stdio() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if cli_args.first().is_some_and(|a| a == "visual") {
        match monocode_core::visual_bridge::client(&cli_args[1..]) {
            Ok(value) => {
                println!("{value}");
                if value.get("ok") == Some(&serde_json::Value::Bool(false)) {
                    std::process::exit(1);
                }
            }
            Err(error) => {
                println!("{}", serde_json::json!({"ok":false,"error":error}));
                std::process::exit(1);
            }
        }
        return;
    }
    if cli_args.first().is_some_and(|a| a == "app") {
        match monocode_core::mono::bridge::client(&cli_args[1..]) {
            Ok(value) => {
                println!("{value}");
                if value.get("ok") == Some(&serde_json::Value::Bool(false)) {
                    std::process::exit(1);
                }
            }
            Err(error) => {
                println!("{}", serde_json::json!({"ok":false,"error":error}));
                std::process::exit(1);
            }
        }
        return;
    }
    let mut args = std::env::args().skip(1);
    let mut fixture = false;
    let mut chat_fixture = false;
    let mut visual_fixture = false;
    // Development stays separate; release builds use the installed app's data.
    let mut use_monocode = bundle_uses_monocode_data().unwrap_or(!cfg!(debug_assertions));
    let mut monocode_db = monocode_core::store::monocode_database();
    let mut check_data = false;
    let mut explicit_cwd = false;
    let mut cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--fixture" => fixture = true,
            "--chat-fixture" => chat_fixture = true,
            "--visual-fixture" => visual_fixture = true,
            "--dev-data" => use_monocode = false,
            "--monocode-data" => use_monocode = true,
            "--monocode-db" => {
                use_monocode = true;
                monocode_db = PathBuf::from(args.next().expect("--monocode-db needs a path"));
            }
            "--check-data" => check_data = true,
            "--cwd" => {
                explicit_cwd = true;
                cwd = PathBuf::from(args.next().expect("--cwd needs a path"));
            }
            "--help" | "-h" => {
                println!(
                    "monocode-gpui [--cwd PATH] [--dev-data | --monocode-data | --monocode-db PATH]\nDevelopment builds use a separate database; release builds use the installed MonoCode database.\n--dev-data opens an isolated development database using Monocode's existing schema.\n--monocode-data opens the existing desktop monocode.db directly.\n--monocode-db PATH opens a specific desktop database.\n--fixture, --chat-fixture and --visual-fixture add local samples to isolated development data only.\nvisual docs prints the native visual SDK reference.\nvisual check/show --input FILE validates or publishes native session visuals.\nvisual-mcp runs the session visual tool server over standard input/output.\n--check-data validates and lists the selected store read-only, then exits."
                );
                return;
            }
            _ => {
                eprintln!("Unknown option: {arg}");
                std::process::exit(2);
            }
        }
    }
    if use_monocode && (fixture || chat_fixture || visual_fixture) {
        eprintln!(
            "Fixtures require --dev-data; they cannot be written into your Monocode database."
        );
        std::process::exit(2);
    }
    let cwd = cwd
        .canonicalize()
        .unwrap_or(cwd)
        .to_string_lossy()
        .to_string();
    let database = if use_monocode {
        monocode_db
    } else {
        data_dir().join("monocode.db")
    };
    if !use_monocode
        && database
            .canonicalize()
            .ok()
            .zip(
                monocode_core::store::monocode_database()
                    .canonicalize()
                    .ok(),
            )
            .is_some_and(|(development, desktop)| development == desktop)
    {
        eprintln!(
            "The isolated development directory points at the desktop database. Choose a separate MONOCODE_NATIVE_DATA_DIR."
        );
        std::process::exit(2);
    }
    let mut db = if check_data {
        Store::read_monocode(&database)
    } else {
        Store::open_monocode(&database)
    }
    .unwrap_or_else(|error| {
        eprintln!("Cannot open {}: {error}", database.display());
        std::process::exit(1);
    });
    if check_data {
        match db.sessions() {
            Ok(sessions) => eprintln!(
                "Database: {}\n{} saved sessions; existing Monocode schema; no import or migration.",
                database.display(),
                sessions.len()
            ),
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
        return;
    }
    monocode_core::checkpoint::init(
        database
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("checkpoints"),
    );
    let saved = db.ui_state().unwrap_or_else(|e| {
        eprintln!("Cannot read workspace settings: {e}");
        None
    });
    let existing_project = if use_monocode {
        saved
            .as_deref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
            .and_then(|state| {
                state["project"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
            })
            .or_else(|| {
                db.sessions()
                    .ok()
                    .and_then(|sessions| sessions.into_iter().find(|s| !s.archived).map(|s| s.cwd))
            })
    } else {
        None
    };
    // Opened from the Dock or Finder, macOS starts the app in `/`. Resume the
    // saved project instead of deriving a project from an internal chat path.
    let cwd = if let Some(project) = existing_project.filter(|_| !explicit_cwd) {
        project
    } else if explicit_cwd || std::path::Path::new(&cwd).parent().is_some() {
        cwd
    } else {
        db.sessions()
            .ok()
            .and_then(|sessions| sessions.into_iter().map(|s| s.cwd).find(|c| c != &cwd))
            .unwrap_or_else(|| std::env::var("HOME").unwrap_or(cwd))
    };
    if fixture {
        let s = db.create("codex", &cwd).expect("create fixture");
        db.title(s.id, "Scroll lab · 10,000 messages").unwrap();
        let mut t = Transcript::default();
        let started = Instant::now();
        for i in 0..10_000 {
            t.begin(if i % 2 == 0 { "user" } else { "assistant" });
            t.append(&format!("Message {i:05} · Native transcript stress fixture.\nUnicode stays intact: 🙂 漢字 café. Scroll up while another session streams.\nThis message is local fixture data; sending resumes a new real provider conversation."));
        }
        db.write_rows(s.id, 0, &t.rows).unwrap();
        eprintln!("Seeded {} rows in {:?}", t.rows.len(), started.elapsed());
    }
    if chat_fixture {
        let s = db.create("codex", &cwd).expect("create chat fixture");
        db.title(s.id, "Native chat · Markdown and tools").unwrap();
        db.write_rows(s.id, 0, &fixtures::chat().rows).unwrap();
    }
    if visual_fixture {
        let s = db.create("codex", &cwd).expect("create visual fixture");
        db.title(s.id, "Native visuals · Controls, charts and 3D")
            .unwrap();
        db.write_rows(s.id, 0, &fixtures::visuals().rows).unwrap();
        let s = db
            .create("claude", &cwd)
            .expect("create visual story fixture");
        db.title(s.id, "Native visuals · Charts inside an answer")
            .unwrap();
        db.write_rows(s.id, 0, &fixtures::visual_story().rows)
            .unwrap();
    }
    let mut sessions = db.sessions().expect("read native sessions");
    if sessions.is_empty() {
        sessions.push(db.create("codex", &cwd).expect("create initial session"));
    }
    // Gather compact sidebar state before entering the platform event loop.
    // The shell does not query thousands of saved agents on the UI thread.
    let metadata = sessions
        .iter()
        .map(|s| (s.id, s.cwd.clone()))
        .collect::<Vec<_>>();
    let workspace = workspace::Workspace::restore(saved.as_deref(), &metadata, &cwd);
    let mut projects = workspace.projects;
    if !projects.contains(&workspace.project) {
        projects.push(workspace.project);
    }
    let sidebar_metadata = db.session_sidebar_metadata(&projects);
    let db = Arc::new(Mutex::new(db));
    let platform = gpui_platform::current_platform(false);
    let title_raster = particle_text::GlyphRaster(platform.text_system());
    gpui::Application::with_platform(platform)
        .with_assets(assets::Assets)
        .run(move |cx: &mut App| {
            #[cfg(debug_assertions)]
            dev_reload::install(cx);
            cx.set_global(title_raster);
            image_memory::init(cx);
            #[cfg(target_os = "macos")]
            memory_log::start(cx);
            gpu_trim::start(cx);
            #[cfg(target_os = "macos")]
            hang_watchdog::start(database.with_file_name("gpui-hangs"), cx);
            input::bind(cx);
            image_lightbox::bind(cx);
            composer_completion::bind(cx);
            file_editor::bind(cx);
            popover::bind(cx);
            cx.set_prompt_builder(modal::prompt_builder);
            explorer::bind(cx);
            history::bind(cx);
            diff_view::bind(cx);
            fonts::init(cx);
            terminal::bind(cx);
            let modifier = if cfg!(target_os = "macos") {
                "cmd"
            } else {
                "ctrl"
            };
            cx.bind_keys([KeyBinding::new(&format!("{modifier}-q"), Quit, None)]);
            shell::bind_keys(cx);
            app_menu::bind(cx);
            usage_footer::bind(cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    quit(cx)
                }
            })
            .detach();
            let bounds = workspace.window.bounds(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(bounds),
                    window_min_size: Some(size(px(820.), px(520.))),
                    window_decorations: workspace
                        .preferences
                        .wayland_decorations(cx.compositor_name()),
                    // Linux desktops match this to the .desktop file's
                    // StartupWMClass to show the app icon.
                    app_id: Some("dev.usemono.gpui".into()),
                    window_background: theme::window_backdrop(true),
                    // GPUI owns the custom chrome. AppKit must not reinterpret
                    // its controls as native titlebar double-click/drag gestures.
                    app_owns_titlebar_drag: cfg!(target_os = "macos"),
                    titlebar: Some(TitlebarOptions {
                        title: Some("Monocode Native".into()),
                        // Windows also drops its caption: the app's drag areas
                        // hit-test as the caption and the frame still resizes.
                        appears_transparent: cfg!(any(target_os = "macos", target_os = "windows")),
                        traffic_light_position: if cfg!(target_os = "macos") {
                            // Original macOS chrome: 12 pt left inset, with
                            // 14 pt native buttons centered in the 40 pt bar.
                            Some(point(px(12.), px((theme::TITLE_BAR_HEIGHT - 14.) / 2.)))
                        } else {
                            None
                        },
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    // Install the menu one frame later, after the first frame presents.
                    window.on_next_frame(|window, _| {
                        window.on_next_frame(|_, cx| app_menu::ready(cx));
                    });
                    // Off the launch path: it lists every conversation folder.
                    let cleanup = db.clone();
                    window.on_next_frame(move |_, cx| {
                        cx.background_executor()
                            .spawn(async move {
                                let result = cleanup
                                    .lock()
                                    .map_err(|error| error.to_string())
                                    .and_then(|store| store.sweep_thread_dirs());
                                if let Err(error) = result {
                                    eprintln!(
                                        "Cannot clean up deleted conversations' files: {error}"
                                    );
                                }
                            })
                            .detach();
                    });
                    let shell = cx.new(|cx| {
                        shell::Shell::new_with_metadata(
                            db,
                            sessions,
                            cwd,
                            saved,
                            sidebar_metadata,
                            cx,
                        )
                    });
                    shell.update(cx, |shell, cx| shell.focus_active(window, cx));
                    #[cfg(target_os = "macos")]
                    {
                        traffic_lights::pin(window, theme::TITLE_BAR_HEIGHT);
                        // AppKit rebuilds the buttons on some titlebar changes.
                        shell.update(cx, |_, cx| {
                            cx.observe_window_activation(window, |_, window, _| {
                                traffic_lights::pin(window, theme::TITLE_BAR_HEIGHT)
                            })
                            .detach();
                        });
                    }
                    window.activate_window();
                    shell
                },
            )
            .expect("open GPUI window");
            cx.activate(true);
        });
}

#[cfg(test)]
mod native_tests {
    use super::*;
    use crate::{input, transcript::TranscriptView};
    use gpui::{Bounds, Context, Entity, Render, StyleRefinement, Window, WindowBounds, div};
    use gpui::{EntityInputHandler, ListOffset, TestAppContext, point};
    use monocode_core::transcript::Row;
    struct Surface {
        transcript: Entity<TranscriptView>,
        input: Entity<input::Input>,
    }
    impl Render for Surface {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .flex()
                .flex_col()
                .size_full()
                .child(
                    div().flex_1().min_h_0().child(
                        self.transcript
                            .clone()
                            .cached(StyleRefinement::default().size_full()),
                    ),
                )
                .child(
                    self.input
                        .clone()
                        .cached(StyleRefinement::default().w_full().h(px(76.))),
                )
        }
    }
    #[test]
    fn bundle_plist_selects_the_data_mode() {
        let plist = |mode: &str| {
            format!("<dict>\n<key>MonocodeDataMode</key><string>{mode}</string>\n</dict>")
        };
        assert_eq!(plist_data_mode(&plist("monocode-data")), Some(true));
        assert_eq!(plist_data_mode(&plist("dev-data")), Some(false));
        assert_eq!(plist_data_mode(&plist("other")), None);
        assert_eq!(plist_data_mode("<dict></dict>"), None);
    }
    #[gpui::test]
    fn long_history_virtualizes_and_typing_does_not_rebuild_it(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(900.), px(700.)),
                    ))),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|cx| {
                        let transcript = cx.new(|_| TranscriptView::new());
                        let mut history = Transcript::default();
                        for _ in 0..10_000 {
                            history.begin("assistant");
                            history.append("A bounded block of text.\nA second line.");
                        }
                        transcript.update(cx, |v, cx| v.patch(0, history.rows, cx));
                        Surface {
                            transcript,
                            input: cx.new(input::Input::new),
                        }
                    })
                },
            )
            .unwrap()
        });
        cx.refresh().unwrap();
        cx.run_until_parked();
        let root = window.root(cx).unwrap();
        let transcript = root.read_with(cx, |v, _| v.transcript.clone());
        let (passes, rendered) = transcript.read_with(cx, |t, _| (t.render_passes, t.rendered));
        assert!(
            rendered > 0 && rendered < 100,
            "expected viewport-sized work, rendered {rendered} rows"
        );
        window
            .update(cx, |v, w, cx| {
                v.input.update(cx, |i, cx| {
                    i.replace_text_in_range(None, "Typing 🙂 stays local", w, cx)
                })
            })
            .unwrap();
        cx.run_until_parked();
        assert_eq!(
            transcript.read_with(cx, |t, _| t.render_passes),
            passes,
            "typing must not rerender the transcript entity"
        );
        window
            .update(cx, |v, _, cx| {
                v.transcript.update(cx, |t, cx| {
                    t.list.pause_following_tail();
                    t.list.scroll_to(ListOffset {
                        item_ix: 5000,
                        offset_in_item: px(7.),
                    });
                    let anchor = t.list.logical_scroll_top();
                    t.patch(
                        9999,
                        vec![Row {
                            role: "assistant".into(),
                            text: "Changed streaming tail".into(),
                            first: true,
                            sent_at: None,
                        }],
                        cx,
                    );
                    assert_eq!(t.list.logical_scroll_top().item_ix, anchor.item_ix);
                    assert_eq!(
                        t.list.logical_scroll_top().offset_in_item,
                        anchor.offset_in_item
                    );
                });
            })
            .unwrap();
    }
}
