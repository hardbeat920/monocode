//! Boots Engine and Submit with a real provider in a temporary project.
//! The test approves a file write through attention's `Approvals`, waits
//! for the turn to finish, and starts another process. The second process
//! loads the session from the store and resumes it with another prompt.
//!
//! It spawns a real provider CLI, so it only runs when asked. The provider
//! defaults to Claude. Set MONOCODE_LIVE_HARNESS=codex to test Codex.
//!
//! ```sh
//! MONOCODE_DATA_DIR=/tmp/mc/appdata \
//!   cargo test -p monocode-app --test live_engine -- --ignored
//! ```
//!
//! GPUI's run loop needs the main thread, so this file has its own `main`.
//! Cargo sets `harness = false`. The executable reports itself ignored
//! unless the caller passes `--ignored` or `--include-ignored`.

use std::path::PathBuf;
use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow, bail, ensure};
use gpui::{App, AsyncApp};
use monocode_app::boot::{self, AppServices, BootOptions};
use monocode_app::data_dir::{self, DataDir, DataDirSource};
use monocode_core::block::ApprovalDecided;
use monocode_core::harness_event::ApprovalDecision;
use monocode_core::{BlockRole, HarnessId, RuntimeMode, Session};
use monocode_engine::attention::Approvals;
use monocode_engine::runtime::Engine;
use monocode_engine::submit::{Submit, SubmitOptions};
use monocode_engine::workspace::SessionFactory as _;

const TEST_NAME: &str = "live_provider_turn_survives_restart";
const PROJECT_DIR_ENV: &str = "MONOCODE_LIVE_PROJECT_DIR";
const FIRST_PROMPT: &str = "Create a file named hello.txt in the current directory containing exactly the word hi. Use a file editing tool and request approval if necessary. Do nothing else.";
const SECOND_PROMPT: &str =
    "What is the name of the file you just created? Reply with the file name only.";
const TURN_TIMEOUT: Duration = Duration::from_secs(240);
const POLL: Duration = Duration::from_millis(200);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--list") {
        println!("{TEST_NAME}: test");
        return ExitCode::SUCCESS;
    }
    if let Some(phase) = args.iter().position(|arg| arg == "--phase") {
        let phase = args.get(phase + 1).cloned().unwrap_or_default();
        let session = args
            .iter()
            .position(|arg| arg == "--session")
            .and_then(|index| args.get(index + 1).cloned());
        return run_phase(&phase, session);
    }
    let run = args
        .iter()
        .any(|arg| arg == "--ignored" || arg == "--include-ignored");
    println!("\nrunning 1 test");
    if !run {
        println!("test {TEST_NAME} ... ignored\n\ntest result: ok. 0 passed; 0 failed; 1 ignored");
        return ExitCode::SUCCESS;
    }
    match orchestrate() {
        Ok(()) => {
            println!("test {TEST_NAME} ... ok\n\ntest result: ok. 1 passed; 0 failed");
            ExitCode::SUCCESS
        }
        Err(error) => {
            println!(
                "test {TEST_NAME} ... FAILED\n\n{error:#}\n\ntest result: FAILED. 0 passed; 1 failed"
            );
            ExitCode::FAILURE
        }
    }
}

/// The copied data dir from `MONOCODE_DATA_DIR`. The test never runs on the
/// user's real data dir.
fn test_data_dir() -> Result<DataDir> {
    let dir = data_dir::resolve(None)?;
    ensure!(
        dir.source == DataDirSource::Env,
        "set MONOCODE_DATA_DIR to a copy of the data dir (see the module docs)"
    );
    let real = data_dir::default_data_dir()?;
    ensure!(
        dir.path.canonicalize().ok() != real.canonicalize().ok(),
        "MONOCODE_DATA_DIR points at the real data dir {}",
        real.display()
    );
    Ok(dir)
}

fn project_dir() -> Result<PathBuf> {
    std::env::var_os(PROJECT_DIR_ENV)
        .map(PathBuf::from)
        .context("the live test phase has no isolated project directory")
}

fn harness() -> Result<HarnessId> {
    let id = std::env::var("MONOCODE_LIVE_HARNESS").unwrap_or_else(|_| "claude".into());
    match HarnessId::parse(&id) {
        Some(id @ (HarnessId::Claude | HarnessId::Codex)) => Ok(id),
        _ => bail!("MONOCODE_LIVE_HARNESS must be claude or codex"),
    }
}

/// Run both phases as child processes, so the second one starts the
/// engine from nothing, the way a relaunch does.
fn orchestrate() -> Result<()> {
    let data_dir = test_data_dir()?;
    let _ = harness()?;
    let project = std::env::temp_dir().join(format!("monocode-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&project)?;
    let exe = std::env::current_exe()?;

    let first = Command::new(&exe)
        .args(["--phase", "turn"])
        .env(data_dir::DATA_DIR_ENV, &data_dir.path)
        .env(PROJECT_DIR_ENV, &project)
        .output()
        .context("running the first phase")?;
    let stdout = String::from_utf8_lossy(&first.stdout);
    eprint!("{}", String::from_utf8_lossy(&first.stderr));
    print!("{stdout}");
    ensure!(first.status.success(), "the first phase failed");
    let session_id = stdout
        .lines()
        .find_map(|line| line.strip_prefix("SESSION "))
        .map(str::to_string)
        .ok_or_else(|| anyhow!("the first phase printed no session id"))?;

    let created = std::fs::read_to_string(project.join("hello.txt"))
        .context("reading hello.txt, which the approved Write should have created")?;
    ensure!(created.trim() == "hi", "hello.txt holds {created:?}");

    let second = Command::new(&exe)
        .args(["--phase", "resume", "--session", &session_id])
        .env(data_dir::DATA_DIR_ENV, &data_dir.path)
        .env(PROJECT_DIR_ENV, &project)
        .output()
        .context("running the second phase")?;
    eprint!("{}", String::from_utf8_lossy(&second.stderr));
    print!("{}", String::from_utf8_lossy(&second.stdout));
    ensure!(second.status.success(), "the second phase failed");
    if let Some(parent) = data_dir.path.parent() {
        let _ = std::fs::write(parent.join("live-session-id"), format!("{session_id}\n"));
    }
    Ok(())
}

/// One phase inside a headless GPUI app. Exits the process with its result.
fn run_phase(phase: &str, session: Option<String>) -> ExitCode {
    let phase = phase.to_string();
    gpui_platform::headless().run(move |cx: &mut App| {
        let result = test_data_dir().and_then(|data_dir| {
            boot::boot(
                BootOptions {
                    data_dir,
                    import_webkit: false,
                    sounds: false,
                    reap_orphans: false,
                    run_schedules: false,
                    control_server: true,
                },
                cx,
            )
        });
        if let Err(error) = result {
            eprintln!("boot failed: {error:#}");
            std::process::exit(1);
        }
        cx.spawn(async move |cx| {
            let result = match phase.as_str() {
                "turn" => first_turn(cx).await,
                "resume" => match session {
                    Some(id) => resume(&id, cx).await,
                    None => Err(anyhow!("--session is missing")),
                },
                other => Err(anyhow!("unknown phase {other}")),
            };
            // Stop the CLI children before leaving.
            let stop = cx.update(|cx| {
                let host = AppServices::global(cx).host.clone();
                let flush = boot::flush(cx);
                (host, flush)
            });
            let (host, flush) = stop;
            flush.await;
            smol::unblock(move || host.kill_all()).await;
            match result {
                Ok(()) => std::process::exit(0),
                Err(error) => {
                    eprintln!("{error:#}");
                    std::process::exit(1);
                }
            }
        })
        .detach();
    });
    ExitCode::FAILURE
}

fn session(id: &str, cx: &mut AsyncApp) -> Option<Session> {
    cx.update(|cx| Engine::sessions(cx).read(cx).get(id).cloned())
}

/// Wait until `check` holds for the session, or fail after `timeout`.
async fn wait_for(
    id: &str,
    what: &str,
    timeout: Duration,
    cx: &mut AsyncApp,
    check: impl Fn(&Session) -> bool,
) -> Result<Session> {
    let started = Instant::now();
    loop {
        if let Some(session) = session(id, cx)
            && check(&session)
        {
            return Ok(session);
        }
        if started.elapsed() > timeout {
            let blocks = session(id, cx)
                .map(|session| describe(&session))
                .unwrap_or_default();
            bail!("timed out waiting for {what}\n{blocks}");
        }
        cx.background_executor().timer(POLL).await;
    }
}

fn describe(session: &Session) -> String {
    session
        .blocks
        .iter()
        .map(|block| {
            let text: String = block.text.chars().take(500).collect();
            format!("  {:?} {:?}", block.role, text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn pending_approval(session: &Session) -> Option<i64> {
    session.blocks.iter().find_map(|block| {
        block
            .approval
            .as_ref()
            .filter(|approval| approval.decided.is_none())
            .map(|approval| approval.request_id)
    })
}

fn submit(id: &str, text: &str, cx: &mut AsyncApp) -> Result<()> {
    let accepted = cx.update(|cx| {
        Submit::global(cx).update(cx, |submit, cx| {
            submit.on_submit(id, text, Vec::new(), SubmitOptions::default(), cx)
        })
    });
    ensure!(accepted, "the submit pipeline did not take the prompt");
    Ok(())
}

fn last_assistant_text(session: &Session) -> String {
    session
        .blocks
        .iter()
        .rev()
        .find(|block| block.role == BlockRole::Assistant)
        .map(|block| block.text.clone())
        .unwrap_or_default()
}

/// Phase one runs a supervised turn that needs an approval.
async fn first_turn(cx: &mut AsyncApp) -> Result<()> {
    let project = project_dir()?;
    let harness = harness()?;
    let (registry, catalog) = cx.update(|cx| {
        let services = AppServices::global(cx);
        (services.registry.clone(), services.catalog.clone())
    });
    registry
        .refresh_harness_catalogs([harness], false, |id| catalog.has_live_catalog(id))
        .await;
    ensure!(
        harness != HarnessId::Codex || !catalog.read().default_model_id(harness).is_empty(),
        "Codex did not return a default model from its live catalog"
    );
    let cwd = project.to_string_lossy().to_string();
    let id = cx.update(|cx| {
        let factory = AppServices::global(cx).factory.clone();
        let session = factory.new_session(harness, &cwd, None, Some(RuntimeMode::Supervised), None);
        let id = session.id.clone();
        Engine::sessions(cx).update(cx, |sessions, cx| {
            sessions.insert(session, cx);
        });
        id
    });
    println!("SESSION {id}");
    eprintln!("[live] session {id} in {cwd}");

    submit(&id, FIRST_PROMPT, cx)?;
    let waiting = wait_for(&id, "an approval request", TURN_TIMEOUT, cx, |session| {
        pending_approval(session).is_some() || (!session.is_busy() && session.blocks.len() > 1)
    })
    .await?;
    let request_id = pending_approval(&waiting).ok_or_else(|| {
        anyhow!(
            "the turn ended without asking for approval\n{}",
            describe(&waiting)
        )
    })?;
    eprintln!("[live] approving request {request_id}");
    cx.update(|cx| Approvals::approve(&id, request_id, ApprovalDecision::Allow, cx));

    let started = Instant::now();
    let done = loop {
        cx.background_executor().timer(POLL).await;
        let current = session(&id, cx).context("the live session disappeared")?;
        if let Some(request_id) = pending_approval(&current) {
            eprintln!("[live] approving follow-up request {request_id}");
            cx.update(|cx| Approvals::approve(&id, request_id, ApprovalDecision::Allow, cx));
        } else if !current.is_busy() {
            break current;
        }
        ensure!(
            started.elapsed() < TURN_TIMEOUT,
            "timed out waiting for the turn to finish\n{}",
            describe(&current)
        );
    };
    let decided = done
        .blocks
        .iter()
        .filter_map(|block| block.approval.as_ref())
        .any(|approval| approval.decided == Some(ApprovalDecided::Allow));
    ensure!(
        decided,
        "no approval block was marked allowed\n{}",
        describe(&done)
    );
    ensure!(
        done.provider_session_id
            .as_deref()
            .is_some_and(|id| !id.is_empty()),
        "the session has no provider session id"
    );
    eprintln!("[live] turn finished with {} blocks", done.blocks.len());
    eprintln!("{}", describe(&done));

    // Save now and wait for the write.
    cx.update(|cx| {
        Engine::sessions(cx).update(cx, |sessions, cx| sessions.persist(&id, cx));
    });
    let flush = cx.update(|cx| boot::flush(cx));
    flush.await;
    Ok(())
}

/// Phase two: a fresh engine loads the session from the store and resumes
/// the provider conversation.
async fn resume(id: &str, cx: &mut AsyncApp) -> Result<()> {
    let opening = cx
        .update(|cx| Engine::sessions(cx).update(cx, |sessions, cx| sessions.ensure_open(id, cx)));
    let loaded = opening
        .await
        .ok_or_else(|| anyhow!("session {id} did not load from the store"))?;
    eprintln!("[live] loaded {} blocks", loaded.blocks.len());
    let roles: Vec<BlockRole> = loaded.blocks.iter().map(|block| block.role).collect();
    ensure!(
        roles.contains(&BlockRole::User) && roles.contains(&BlockRole::Assistant),
        "the stored session lacks its turn\n{}",
        describe(&loaded)
    );
    ensure!(
        loaded
            .blocks
            .iter()
            .filter_map(|block| block.approval.as_ref())
            .any(|approval| approval.decided == Some(ApprovalDecided::Allow)),
        "the stored session lost its approval\n{}",
        describe(&loaded)
    );
    let provider = loaded
        .provider_session_id
        .clone()
        .ok_or_else(|| anyhow!("the stored session has no provider session id"))?;
    ensure!(!loaded.is_busy(), "the stored session is still busy");

    let before = loaded.blocks.len();
    submit(id, SECOND_PROMPT, cx)?;
    let done = wait_for(
        id,
        "the resumed turn to finish",
        TURN_TIMEOUT,
        cx,
        |session| session.blocks.len() > before + 1 && !session.is_busy(),
    )
    .await?;
    let reply = last_assistant_text(&done);
    eprintln!("[live] resumed reply: {reply:?}");
    ensure!(
        reply.to_lowercase().contains("hello.txt"),
        "the resumed turn did not remember the file: {reply:?}\n{}",
        describe(&done)
    );
    ensure!(
        done.provider_session_id.as_deref() == Some(provider.as_str()),
        "the resumed turn started a new provider session ({:?} became {:?})",
        provider,
        done.provider_session_id
    );
    cx.update(|cx| {
        Engine::sessions(cx).update(cx, |sessions, cx| sessions.persist(id, cx));
    });
    let flush = cx.update(|cx| boot::flush(cx));
    flush.await;
    Ok(())
}
