//! App.tsx / handoffTurn.ts's bounded recap from the outgoing bound provider.
use super::*;
use monocode_core::agent::{AccessMode, Decision, Handoff, ProviderSwitch};
use std::{path::Path, sync::mpsc, time::Instant};

pub(super) fn prepare(
    session: i64,
    source: &ProviderSwitch,
    cwd: &str,
    user_request: &str,
    rows: &[Row],
    root: Option<std::path::PathBuf>,
    user_cancel: Arc<AtomicBool>,
) -> Handoff {
    let kind = Kind::from_id(&source.from);
    let cwd = if source.cwd.is_empty() {
        cwd
    } else {
        &source.cwd
    };
    let mut brief = String::new();
    let mut binding = source.provider_id.clone();
    let has_edits = rows.iter().filter(|row| row.role == "tool").any(|row| {
        serde_json::from_str::<monocode_core::tool::ToolCall>(&row.text)
            .ok()
            .is_some_and(|tool| {
                matches!(
                    tool.summary().0.as_str(),
                    "Edit" | "Write" | "MultiEdit" | "NotebookEdit"
                )
            })
    });
    if !source.skip_outgoing_recap
        && source.provider_id.is_some()
        && has_edits
        && !user_cancel.load(Ordering::Relaxed)
        && let Some(binary) = provider::resolve(kind)
    {
        let cancel = Arc::new(AtomicBool::new(false));
        let timer_cancel = cancel.clone();
        let (settled, settlement) = mpsc::channel();
        let timer = std::thread::spawn(move || {
            let started = Instant::now();
            loop {
                if user_cancel.load(Ordering::Relaxed)
                    || started.elapsed() >= Duration::from_secs(45)
                {
                    timer_cancel.store(true, Ordering::Relaxed);
                    break;
                }
                match settlement.recv_timeout(Duration::from_millis(25)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        });
        let mut account = source.account.clone();
        if account.id != monocode_core::accounts::DEFAULT && account.root.is_none() {
            account.root = root;
        }
        let request = Request {
            kind,
            binary,
            cwd: cwd.into(),
            prompt: monocode_core::response::outgoing_handoff_prompt(user_request),
            provider_id: source.provider_id.clone(),
            model: source.model.clone(),
            settings: Settings {
                account,
                model: source.model.clone(),
                model_settings: source.model_settings.clone(),
                effort: source.effort.clone(),
                fast: source.fast,
                service_tier: source.service_tier.clone(),
                thinking: source.thinking,
                access: AccessMode::Supervised,
                ..Default::default()
            },
        };
        let (controls, control_rx) = mpsc::channel();
        let _ = monocode_core::process::owned_by(session, || {
            provider::run_with_controls(request, cancel, Some(control_rx), |events| {
                for event in events {
                    match event {
                        Event::Text(text) => {
                            // The retained packet is at most 1,800 characters.
                            // Ignore an unhelpful oversized recap without
                            // introducing another provider failure condition.
                            if brief.len() < 64 * 1024 {
                                let mut end = text.len().min(64 * 1024 - brief.len());
                                while !text.is_char_boundary(end) {
                                    end -= 1;
                                }
                                brief.push_str(&text[..end]);
                            }
                        }
                        Event::SessionId(id) => binding = Some(id),
                        Event::Approval(request) => {
                            let _ = controls.send(Control {
                                id: request.id,
                                decision: Decision::Deny,
                            });
                        }
                        Event::Question(request) => {
                            let _ = controls.send(Control {
                                id: request.id,
                                decision: Decision::Deny,
                            });
                        }
                        _ => {}
                    }
                }
                Ok(())
            })
        });
        let _ = settled.send(());
        let _ = timer.join();
    }
    if let Some(id) = &source.provider_id {
        provider::forget_session(kind, id, &source.account.id, Path::new(cwd));
    }
    if let Some(id) = binding.filter(|id| Some(id) != source.provider_id.as_ref()) {
        provider::forget_session(kind, &id, &source.account.id, Path::new(cwd));
    }
    Handoff {
        from: source.from.clone(),
        brief: monocode_core::response::choose_handoff_brief(&brief, rows),
    }
}
