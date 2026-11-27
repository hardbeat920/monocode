//! Opt-in real-provider check. Uses existing CLI authentication; makes paid/subscription requests.
use monocode_core::provider::{self, Event, Kind, Request};
use std::sync::{atomic::AtomicBool, Arc};
fn main() -> Result<(), String> {
    let name = std::env::args().nth(1).unwrap_or_else(|| "codex".into());
    let kind = Kind::parse(&name).ok_or_else(|| format!("Unknown provider: {name}"))?;
    let binary = provider::resolve(kind).ok_or("CLI not installed")?;
    if std::env::args().nth(2).as_deref() == Some("--models") {
        for model in monocode_core::catalog::models(kind)? {
            println!(
                "{} | {} | {:?} | fast={}",
                model.id, model.name, model.efforts, model.fast
            );
        }
        return Ok(());
    }
    if std::env::args().nth(2).as_deref() == Some("--inspect") {
        return inspect(kind, binary);
    }
    if std::env::args().nth(2).as_deref() == Some("--approval") {
        return approval(kind, binary);
    }
    if std::env::args().nth(2).as_deref() == Some("--readonly") {
        return read_only(kind, binary);
    }
    let mut id = None;
    for (ix, prompt) in [
        "Remember the word plum. Reply with exactly: NATIVE_STREAM_OK",
        "What word did I ask you to remember? Reply with that word only.",
    ]
    .into_iter()
    .enumerate()
    {
        let mut text = String::new();
        let mut batches = 0;
        let mut done = false;
        provider::run(
            Request {
                kind,
                binary: binary.clone(),
                cwd: std::env::temp_dir(),
                prompt: prompt.into(),
                provider_id: id.clone(),
                model: std::env::var(format!("MONOCODE_{}_MODEL", kind.id().to_uppercase())).ok(),
                settings: Default::default(),
            },
            Arc::new(AtomicBool::new(false)),
            |events| {
                batches += 1;
                for event in events {
                    match event {
                        Event::Text(delta) => {
                            print!("{delta}");
                            text.push_str(&delta);
                        }
                        Event::SessionId(value) => id = Some(value),
                        Event::Done => done = true,
                        _ => {}
                    }
                }
                Ok(())
            },
        )?;
        println!(
            "\n{} turn {}: {batches} batches; completed={done}; context_id={}",
            kind.label(),
            ix + 1,
            id.is_some()
        );
        if !done
            || (ix == 0 && !text.contains("NATIVE_STREAM_OK"))
            || (ix == 1 && !text.to_lowercase().contains("plum"))
        {
            return Err("Provider smoke response did not satisfy the check".into());
        }
    }
    Ok(())
}

fn inspect(kind: Kind, binary: std::path::PathBuf) -> Result<(), String> {
    let started = std::time::Instant::now();
    let mut tools = std::collections::HashSet::new();
    let mut text_batches = 0;
    let mut first_text = None;
    let mut last_text = None;
    let mut done = false;
    let mut messages = 0;
    provider::run(Request {
        kind, binary, cwd: std::env::current_dir().map_err(|e| e.to_string())?,
        prompt: "Inspect this repository using git status --short and git diff --stat. Do not change any files. Send one brief progress message before running the tools, then report the current changes in Markdown with two headings, bullets, inline code, and a small fenced Rust example. Keep the final explanation between 150 and 220 words. Use the actual tools; do not ask me to paste output.".into(),
        provider_id: None, model: None, settings: monocode_core::agent::Settings { access: monocode_core::agent::AccessMode::ReadOnly, ..Default::default() },
    }, Arc::new(AtomicBool::new(false)), |events| {
        for event in events {
            match event {
                Event::BeginAssistant => { messages += 1; println!("\n[{:.2}s] assistant message {messages}", started.elapsed().as_secs_f32()); },
                Event::Text(delta) => {
                    text_batches += 1;
                    first_text.get_or_insert(started.elapsed()); last_text = Some(started.elapsed());
                    print!("{delta}");
                },
                Event::Tool(tool) => { if tools.insert(tool.id.clone()) { println!("\n[{:.2}s] TOOL {} {}", started.elapsed().as_secs_f32(), tool.name, tool.input); } },
                Event::Done => done = true,
                _ => {},
            }
        }
        Ok(())
    })?;
    println!("\nLIVE: provider={} completed={done} tools={} messages={messages} text_batches={text_batches} first_text={first_text:?} last_text={last_text:?} elapsed={:?}", kind.label(), tools.len(), started.elapsed());
    if !done || tools.is_empty() || text_batches < 2 || messages < 2 {
        return Err(
            "Live repository request did not exercise tools and incremental messages".into(),
        );
    }
    Ok(())
}

fn read_only(kind: Kind, binary: std::path::PathBuf) -> Result<(), String> {
    let cwd = std::env::temp_dir().join(format!(
        "monocode-readonly-{}-{}",
        kind.id(),
        std::process::id()
    ));
    std::fs::create_dir_all(&cwd).map_err(|e| e.to_string())?;
    let mut done = false;
    provider::run(Request {
        kind,binary,cwd:cwd.clone(),provider_id:None,
        model:std::env::var(format!("MONOCODE_{}_MODEL",kind.id().to_uppercase())).ok(),
        prompt:"Try to create permission-check.txt with the word WRITE in the current directory using a file tool. If your current permission mode prohibits edits, explain that briefly and finish without requesting a mode change. Do not run shell commands.".into(),
        settings:monocode_core::agent::Settings {access:monocode_core::agent::AccessMode::ReadOnly,..Default::default()},
    },Arc::new(AtomicBool::new(false)),|events| {
        for event in events { match event { Event::Text(text)=>print!("{text}"),Event::Done=>done=true,_=>{} } }
        Ok(())
    })?;
    if !done || cwd.join("permission-check.txt").exists() {
        return Err("Read-only mode did not complete without writes".into());
    }
    println!(
        "\nread-only: provider={} completed={done} no_file_written=true",
        kind.id()
    );
    Ok(())
}
fn approval(kind: Kind, binary: std::path::PathBuf) -> Result<(), String> {
    use monocode_core::agent::{AccessMode, Control, Decision, Settings};
    let cwd = std::env::temp_dir().join(format!(
        "monocode-permissions-{}-{}",
        kind.id(),
        std::process::id()
    ));
    std::fs::create_dir_all(&cwd).map_err(|e| e.to_string())?;
    let (tx, rx) = std::sync::mpsc::channel();
    let mut asked = 0;
    let mut completed = false;
    let models = monocode_core::catalog::models(kind)?;
    let model = std::env::var(format!("MONOCODE_{}_MODEL", kind.id().to_uppercase()))
        .ok()
        .or_else(|| {
            models
                .iter()
                .find(|m| kind == Kind::Claude && m.id == "haiku")
                .or_else(|| models.first())
                .map(|m| m.id.clone())
        });
    provider::run_with_controls(Request {
        kind, binary, cwd: cwd.clone(), model,
        prompt: "Create permission-check.txt in the current directory with exactly NATIVE_APPROVAL_OK followed by a newline. Use the file edit/write tool, request approval when needed, and do not run shell commands or change anything else. Then reply DONE.".into(),
        provider_id: None,
        settings: Settings { access:AccessMode::Supervised, effort:Some("low".into()), ..Default::default() },
    }, Arc::new(AtomicBool::new(false)), Some(rx), |events| {
        for event in events { match event {
            Event::Approval(request) => { asked += 1; println!("APPROVAL {}: {}", request.title, request.detail); tx.send(Control {id:request.id, decision:Decision::Allow}).map_err(|e| e.to_string())?; },
            Event::Tool(tool) => println!("TOOL {} {:?}", tool.name, tool.state),
            Event::Text(text) => print!("{text}"),
            Event::Done => completed = true,
            _ => {},
        }}
        Ok(())
    })?;
    let result =
        std::fs::read_to_string(cwd.join("permission-check.txt")).map_err(|e| e.to_string())?;
    println!(
        "\npermissions: provider={} asked={asked} completed={completed} file={}",
        kind.id(),
        cwd.display()
    );
    if asked == 0 || !completed || result.trim() != "NATIVE_APPROVAL_OK" {
        return Err("Approval was not requested or the approved write did not complete".into());
    }
    Ok(())
}
