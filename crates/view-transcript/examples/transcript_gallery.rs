//! Renders a transcript in a window, from a session in a monocode.db copy or
//! from a built-in synthetic session.
//!
//! ```sh
//! cargo run -p monocode-view-transcript --example transcript_gallery -- --list
//! cargo run -p monocode-view-transcript --example transcript_gallery -- --pick 3
//! cargo run -p monocode-view-transcript --example transcript_gallery -- \
//!     --synthetic --busy --screenshot /tmp/transcript.png
//! ```
//!
//! The database opens read-only. `--screenshot` draws the window offscreen
//! with `Window::render_to_image`, writes a PNG at the display's scale, and
//! exits.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    App, AppContext as _, AsyncApp, Bounds, Context, Entity, IntoElement, ParentElement as _,
    Render, Styled as _, Window, WindowBounds, WindowOptions, div, px, size,
};
use gpui_component::Root;
use monocode_core::block::{AgentStepKind, ToolPreviewLineKind};
use monocode_core::transcript::fixtures::*;
use monocode_core::{Block, BlockRole, HarnessId, Session};
use monocode_ui::{AppearanceSettings, Theme, ThemePreference};
use monocode_view_transcript::transcript::{
    ChangedFile, TranscriptConfig, TranscriptEvent, TranscriptView,
};

const USAGE: &str = "\
usage: transcript_gallery [--db <path>] [--session <id> | --pick <n> | --synthetic]
                          [--list] [--busy] [--top] [--turn <n>] [--open-work] [--theme dark|light]
                          [--size WxH] [--layout chat|full] [--changes]
                          [--screenshot <out.png>]";

#[derive(Debug, Clone)]
struct Args {
    db: PathBuf,
    session: Option<String>,
    pick: Option<usize>,
    synthetic: bool,
    list: bool,
    busy: bool,
    top: bool,
    turn: Option<usize>,
    open_work: bool,
    changes: bool,
    light: bool,
    full: bool,
    size: (f32, f32),
    screenshot: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        db: PathBuf::from("/tmp/mc/golden.db"),
        session: None,
        pick: None,
        synthetic: false,
        list: false,
        busy: false,
        top: false,
        turn: None,
        open_work: false,
        changes: false,
        light: false,
        full: false,
        size: (900., 1000.),
        screenshot: None,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| iter.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--db" => args.db = PathBuf::from(value("--db")?),
            "--session" => args.session = Some(value("--session")?),
            "--pick" => {
                args.pick = Some(
                    value("--pick")?
                        .parse()
                        .map_err(|_| "--pick takes a number")?,
                )
            }
            "--synthetic" => args.synthetic = true,
            "--list" => args.list = true,
            "--busy" => args.busy = true,
            "--top" => args.top = true,
            "--turn" => {
                args.turn = Some(
                    value("--turn")?
                        .parse()
                        .map_err(|_| "--turn takes a number")?,
                )
            }
            "--open-work" => args.open_work = true,
            "--changes" => args.changes = true,
            "--theme" => args.light = value("--theme")? == "light",
            "--layout" => args.full = value("--layout")? == "full",
            "--size" => {
                let raw = value("--size")?;
                let (w, h) = raw.split_once('x').ok_or("--size takes WxH")?;
                args.size = (
                    w.parse().map_err(|_| "--size width")?,
                    h.parse().map_err(|_| "--size height")?,
                );
            }
            "--screenshot" => args.screenshot = Some(PathBuf::from(value("--screenshot")?)),
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown flag {other}\n{USAGE}")),
        }
    }
    Ok(args)
}

/// Sessions in the database, largest first: id, harness, title, blocks bytes.
fn list_sessions(db: &Path) -> rusqlite::Result<Vec<(String, String, String, i64)>> {
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn.prepare(
        "select id, harness, title, length(blocks_json) from sessions where archived = 0 or archived = 1 \
         order by length(blocks_json) desc",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })?;
    rows.collect()
}

/// One session row, read-only.
fn load_session(db: &Path, id: &str) -> Result<Session, String> {
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|err| format!("open {}: {err}", db.display()))?;
    let (harness, model, cwd, title, blocks_json): (String, String, String, String, String) = conn
        .query_row(
            "select harness, model, cwd, title, blocks_json from sessions where id = ?1",
            [id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|err| format!("session {id}: {err}"))?;
    let harness = HarnessId::parse(&harness).unwrap_or(HarnessId::Claude);
    let mut session = Session::blank(id, harness, model, cwd);
    session.title = title;
    session.blocks =
        serde_json::from_str(&blocks_json).map_err(|err| format!("blocks_json: {err}"))?;
    Ok(session)
}

/// A session that shows every kind of row, with made-up content.
fn synthetic_session(busy: bool) -> Session {
    let mut session = Session::blank(
        "synthetic",
        HarnessId::Claude,
        "claude:opus-4.6",
        "/Users/dev/arcade",
    );
    let mut blocks: Vec<Block> = Vec::new();
    let mut first = timed_user(
        "u1",
        "Make each arcade game finish with a WIN or LOSE screen.",
        1_700_000_000_000,
        159_000,
    );
    first.turn_model = Some(monocode_core::block::TurnModel {
        harness: HarnessId::Claude,
        id: "claude:opus-4.6".into(),
        name: "Claude Opus 4.6".into(),
        extra: Default::default(),
    });
    blocks.push(first);
    blocks.push(thought(
        "r1",
        "**Mapping the game loop**\n\nThe attract loop cycles games on a timer.",
    ));
    blocks.push(note(
        "a1",
        "I'll start by finding where each game decides it is over.",
    ));
    blocks.push(search("s1", "isGameOver"));
    blocks.push(read("t1", "/Users/dev/arcade/src/surfaces/gridArcade.ts"));
    blocks.push(read("t2", "/Users/dev/arcade/src/surfaces/speechBubble.ts"));
    blocks.push(note("a2", "Now the scene type and the finish conditions."));
    blocks.push(edit_with_diff(
        "e1",
        "/Users/dev/arcade/src/surfaces/gridArcade.ts",
        &[
            (
                ToolPreviewLineKind::Del,
                1,
                "export type ArcadeResult = \"win\" | \"lose\";",
            ),
            (ToolPreviewLineKind::Add, 1, "export type ArcadeScene = {"),
            (ToolPreviewLineKind::Add, 2, "  durationMs: number;"),
            (
                ToolPreviewLineKind::Add,
                3,
                "  start(cols: number, rows: number): void;",
            ),
            (ToolPreviewLineKind::Add, 4, "  step(dt: number): void;"),
        ],
    ));
    blocks.push(command("c1", "npm test -- --run src/surfaces", "completed"));
    let mut failed = command("c2", "npm run lint", "failed");
    failed.tool.as_mut().unwrap().detail =
        Some("src/surfaces/gridArcade.ts:12  'dt' is defined but never used".into());
    blocks.push(failed);
    blocks.push(note(
        "a3",
        "Each game now plays all the way through. It ends in a **WIN** or **LOSE**, that word shows in the center, \
         then a different game starts at random.\n\n- Snake, invaders, pong, and breakout all have finish conditions.\n- \
         The attract loop waits for the result screen before moving on.",
    ));
    blocks.push(user("u2", "Run two reviews on the change, please."));
    blocks.push(note(
        "a4",
        "I'll run a correctness review and a quality review in parallel.",
    ));
    blocks.push(agent_run(
        "ag1",
        "Correctness review",
        if busy { "in_progress" } else { "completed" },
        vec![
            step(
                "ag1s1",
                AgentStepKind::Message,
                "Reading the diff first.",
                None,
            ),
            step(
                "ag1s2",
                AgentStepKind::Tool,
                "Read src/surfaces/gridArcade.ts",
                Some("completed"),
            ),
            step("ag1s3", AgentStepKind::Tool, "npm test", Some("failed")),
        ],
    ));
    blocks.push(agent(
        "ag2",
        "Quality review of the arcade loop and its timers",
        if busy { "in_progress" } else { "completed" },
    ));
    let mut tasks = tasks_block("tasks");
    tasks.text = "[x] inspect\n[~] implement".into();
    blocks.push(tasks);
    if busy {
        blocks.push(note(
            "a6",
            "Checking the scene files while the reviews run.",
        ));
        blocks.push(read("t3", "/Users/dev/arcade/src/surfaces/gridArcade.ts"));
        blocks.push(read("t4", "/Users/dev/arcade/src/surfaces/attractLoop.ts"));
        blocks.push(edit_with_diff(
            "e2",
            "/Users/dev/arcade/src/surfaces/attractLoop.ts",
            &[
                (
                    ToolPreviewLineKind::Context,
                    41,
                    "  const scene = pick(scenes);",
                ),
                (
                    ToolPreviewLineKind::Del,
                    42,
                    "  setTimeout(next, scene.durationMs);",
                ),
                (
                    ToolPreviewLineKind::Add,
                    42,
                    "  scene.onFinish(() => showResult(scene, next));",
                ),
            ],
        ));
        let mut lint = command("c4", "npm run lint -- src/surfaces", "failed");
        lint.tool.as_mut().unwrap().detail = Some("1 problem (1 error, 0 warnings)".into());
        blocks.push(lint);
        blocks.push(with_approval(
            command("c3", "rm -rf dist && npm run build", "pending"),
            7,
        ));
    } else {
        blocks.push(note("a5", "Both reviews agree the change is correct."));
    }
    session.blocks = blocks;
    if busy {
        session.busy = Some(true);
        session.blocks[11].started_at = Some(super_now() - 42_000);
    } else {
        session.blocks[11].started_at = Some(1_700_000_200_000);
        session.blocks[11].duration_ms = Some(55_000);
    }
    let _ = BlockRole::User;
    session
}

fn super_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

struct Gallery {
    transcript: Entity<TranscriptView>,
}

impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        div()
            .size_full()
            .bg(theme.colors.background_base)
            .text_color(theme.colors.content)
            .child(self.transcript.clone())
    }
}

fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    if args.list {
        match list_sessions(&args.db) {
            Ok(rows) => {
                for (index, (id, harness, title, bytes)) in rows.iter().enumerate() {
                    println!("{index:>3}  {id}  {harness:<10} {bytes:>9}  {title}");
                }
            }
            Err(err) => eprintln!("{err}"),
        }
        return;
    }
    let session = if args.synthetic {
        synthetic_session(args.busy)
    } else {
        let id = match (&args.session, args.pick) {
            (Some(id), _) => id.clone(),
            (None, pick) => {
                let rows = list_sessions(&args.db).unwrap_or_else(|err| {
                    eprintln!("{err}");
                    std::process::exit(1);
                });
                let index = pick.unwrap_or(0).min(rows.len().saturating_sub(1));
                rows.get(index).map(|row| row.0.clone()).unwrap_or_default()
            }
        };
        let mut session = load_session(&args.db, &id).unwrap_or_else(|err| {
            eprintln!("{err}");
            std::process::exit(1);
        });
        session.busy = Some(args.busy);
        session
    };
    let session = Arc::new(session);
    eprintln!("{} blocks in {}", session.blocks.len(), session.title);

    gpui_platform::application()
        .with_assets(monocode_ui::Assets)
        .run(move |cx: &mut App| {
            gpui_component::init(cx);
            let appearance = AppearanceSettings {
                theme_preference: if args.light {
                    ThemePreference::Light
                } else {
                    ThemePreference::Dark
                },
                ..Default::default()
            };
            monocode_ui::init(appearance, cx);
            monocode_view_transcript::transcript::init(cx);
            let (width, height) = args.size;
            let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            };
            let args_for_window = args.clone();
            let session_for_window = session.clone();
            let window = cx
                .open_window(options, move |window, cx| {
                    monocode_ui::sync_window(window, cx);
                    window.set_background_appearance(gpui::WindowBackgroundAppearance::Opaque);
                    let transcript = cx.new(|cx| {
                        let mut view = TranscriptView::new(cx);
                        view.set_config(
                            TranscriptConfig {
                                layout: if args_for_window.full {
                                    monocode_core::appearance::TranscriptLayout::Full
                                } else {
                                    monocode_core::appearance::TranscriptLayout::Chat
                                },
                                can_save_notes: true,
                                can_second_opinion: true,
                                can_handoff: true,
                                can_open_plans: true,
                                can_build_plans: true,
                                can_send_drafts: true,
                                can_edit_last_turn: true,
                                ..Default::default()
                            },
                            cx,
                        );
                        let started = std::time::Instant::now();
                        view.set_session(session_for_window.clone(), cx);
                        eprintln!(
                            "laid out {} rows in {:?}",
                            view.rows().len(),
                            started.elapsed()
                        );
                        if args_for_window.changes {
                            view.set_changes(sample_changes(), false, cx);
                        }
                        if args_for_window.open_work {
                            view.open_all_work(cx);
                        }
                        if args_for_window.top {
                            view.scroll_to_top(cx);
                        }
                        if let Some(turn) = args_for_window.turn {
                            view.scroll_to_turn(turn, cx);
                        }
                        view
                    });
                    cx.subscribe(&transcript, |_, event: &TranscriptEvent, _| {
                        eprintln!("event: {event:?}");
                    })
                    .detach();
                    let gallery = cx.new(|_| Gallery { transcript });
                    cx.new(|cx| Root::new(gallery, window, cx))
                })
                .expect("open the gallery window");
            if let Some(out) = args.screenshot.clone() {
                capture_and_quit(window.into(), out, cx);
            }
            cx.activate(true);
        });
}

fn sample_changes() -> Vec<ChangedFile> {
    [
        ("src/surfaces/gridArcade.ts", 100, 217),
        ("src/surfaces/speechBubble.ts", 12, 3),
    ]
    .into_iter()
    .map(|(relative, additions, deletions)| ChangedFile {
        path: format!("/Users/dev/arcade/{relative}"),
        relative: relative.into(),
        status: "M".into(),
        additions,
        deletions,
        exact: true,
        undoable: true,
    })
    .collect()
}

/// Redraw until images and fonts settle, then write the frame as a PNG.
fn capture_and_quit(window: gpui::AnyWindowHandle, out: PathBuf, cx: &mut App) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        for _ in 0..20 {
            cx.background_executor()
                .timer(Duration::from_millis(60))
                .await;
            let _ = window.update(cx, |_, window, cx| {
                window.dispatch_event(
                    gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                        position: gpui::point(px(-1000.), px(-1000.)),
                        ..Default::default()
                    }),
                    cx,
                );
                window.refresh();
            });
        }
        let result = window.update(cx, |_, window, cx| {
            window.draw(cx).clear();
            window.render_to_image()
        });
        let code = match result {
            Ok(Ok(image)) => {
                if let Some(parent) = out.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match image.save(&out) {
                    Ok(()) => {
                        eprintln!(
                            "wrote {} ({}x{})",
                            out.display(),
                            image.width(),
                            image.height()
                        );
                        0
                    }
                    Err(err) => {
                        eprintln!("screenshot failed: {err}");
                        1
                    }
                }
            }
            Ok(Err(err)) => {
                eprintln!("screenshot failed: {err:#}");
                1
            }
            Err(err) => {
                eprintln!("screenshot failed: {err:#}");
                1
            }
        };
        cx.update(|cx| cx.quit());
        std::process::exit(code);
    })
    .detach();
}
