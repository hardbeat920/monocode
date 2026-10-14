//! The remaining actions (spec 9.1, 9.4, 10.1, 10.2, 8 `screenshot` and
//! `read`): `navigate`, `back`/`forward`, `select`, `upload`, `dialog`,
//! `wait`, `screenshot`, `read`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::super::link::{CdpError, EventFilter, Method};
use super::super::policy::{check_upload_path, check_url};
use super::super::protocol::{
    ActionResult, DialogAction, DialogParams, DialogType, HistoryParams, ImageFormat,
    NavigateParams, Observed, ReadFormat, ReadParams, Reason, ScreenshotParams, SelectParams,
    UploadParams, WaitFor, WaitParams, WaitUntil,
};
use super::super::targets::{self, TabError};
use super::element::{self, Needs, PAGE_JS, READ_JS, SELECT_JS};
use super::observe::{self, Watch};
use super::{cursor, describe, refs, Fail, Run};

/// How long `upload` waits for a file chooser after the click (spec 10.2).
const CHOOSER_WAIT: Duration = Duration::from_secs(5);
/// Polling interval of `wait` (spec 9.4).
const POLL: Duration = Duration::from_millis(100);
/// `wait {for: "time"}` cap (spec 9.4).
const MAX_SLEEP: Duration = Duration::from_secs(10);
/// `read` slice size (spec 8: up to 50 KB per call).
const READ_LIMIT: u64 = 50_000;
/// Tallest full-page screenshot, in CSS pixels.
const MAX_FULL_PAGE_HEIGHT: f64 = 16_384.0;

fn tab_error(error: TabError) -> Fail {
    match error {
        TabError::Reason(reason, message) => Fail::Reason(reason, message),
        TabError::Cdp(error) => Fail::Cdp(error, "Page.navigate"),
    }
}

fn with_summary(what: String, observed: &Observed) -> String {
    let summary = observe::summary(observed);
    if summary.is_empty() {
        what
    } else {
        format!("{what} {summary}")
    }
}

/// `navigate` (spec 9.4): URL policy first (no command on `url_denied`),
/// then `Page.navigate` and the wait. A timeout of the wait is still `done`
/// with `load_state: "timeout"`; the page is often usable without `load`.
pub fn navigate(run: &Run<'_>, p: NavigateParams) -> Result<ActionResult, Fail> {
    let url = check_url(&p.url).map_err(Fail::reason)?;
    let observed = targets::navigate_and_wait(
        run.link,
        run.ctx,
        run.handle,
        &url,
        p.wait_until.unwrap_or_default(),
        run.ctx.deadline,
    )
    .map_err(tab_error)?;
    refs::clear(&run.key());
    let shown = observed.url.clone().unwrap_or(url);
    Ok(ActionResult::done(
        run.id(),
        with_summary(
            format!("Opened {shown}."),
            &Observed {
                navigated: false,
                ..observed.clone()
            },
        ),
    )
    .with_observed(observed))
}

/// `back` (`delta = -1`) and `forward` (`delta = 1`): history entry, URL
/// policy, `Page.navigateToHistoryEntry`, wait for load.
pub fn history(run: &Run<'_>, p: HistoryParams, delta: i64) -> Result<ActionResult, Fail> {
    let _ = &p.tab;
    let history = run.send(run.main(), Method::PageGetNavigationHistory, json!({}))?;
    let current = history["currentIndex"].as_i64().unwrap_or(0);
    let entries = history["entries"].as_array().cloned().unwrap_or_default();
    let target = current + delta;
    let Some(entry) = usize::try_from(target).ok().and_then(|i| entries.get(i)) else {
        return Err(Fail::detail(
            Reason::NavigationFailed,
            if delta < 0 {
                "There is no previous page in this tab's history."
            } else {
                "There is no next page in this tab's history."
            },
        ));
    };
    let url = entry["url"].as_str().unwrap_or("");
    check_url(url).map_err(Fail::reason)?;
    let entry_id = entry["id"].clone();
    let subscription = run.link.subscribe(EventFilter::session(run.main()));
    run.send_mut(
        run.main(),
        Method::PageNavigateToHistoryEntry,
        json!({ "entryId": entry_id }),
    )?;
    let (load_state, dialog) = targets::wait_for_load_tracked(
        run.link,
        &subscription,
        None,
        Default::default(),
        run.ctx.deadline,
    );
    refs::clear(&run.key());
    if let Some(lease) = run.link.policy().write().lease_mut(run.tab()) {
        lease.snapshot_version += 1;
    }
    let mut observed = run.observed_now();
    observed.navigated = true;
    observed.load_state = Some(load_state);
    observed.snapshot_stale = true;
    observed.dialog = dialog;
    if observed.url.as_deref().is_none_or(str::is_empty) {
        observed.url = Some(url.to_string());
    }
    let what = format!(
        "Went {} to {url}.",
        if delta < 0 { "back" } else { "forward" }
    );
    Ok(ActionResult::done(
        run.id(),
        with_summary(
            what,
            &Observed {
                navigated: false,
                ..observed.clone()
            },
        ),
    )
    .with_observed(observed))
}

/// `select` (spec 9.4): option values first, then labels; all or nothing.
pub fn select(run: &Run<'_>, p: SelectParams) -> Result<ActionResult, Fail> {
    if p.values.is_empty() {
        return Err(Fail::detail(
            Reason::InvalidRequest,
            "values must name at least one option.",
        ));
    }
    let el = element::locate(run, &p.element)?;
    let prepared = element::prepare(
        run,
        &el,
        Needs {
            visible: true,
            enabled: true,
            scroll: true,
            ..Needs::default()
        },
    )?;
    if !prepared.facts.select {
        return Err(Fail::detail(
            Reason::OptionNotFound,
            "Element is not a <select>; open custom dropdowns with browser_click and pick the option the same way.",
        ));
    }
    let mut watch = Watch::start(run);
    watch.count_mutations(run);
    let session = el.node.frame.session.clone();
    let answer = watch.input(
        run,
        &session,
        Method::RuntimeCallFunctionOn,
        element::element_call(&el, SELECT_JS, &[element::arg(json!(p.values))]),
    )?;
    let label = describe(p.element.trim(), &el.node.role, &el.node.name);
    let selected = match answer {
        Some(answer) => {
            let value = element::call_value(answer)?;
            match value["status"].as_str().unwrap_or("") {
                "ok" => value["selected"].clone(),
                "missing" => {
                    let available: Vec<String> = value["available"]
                        .as_array()
                        .map(|options| {
                            options
                                .iter()
                                .map(|o| {
                                    format!(
                                        "{} ({})",
                                        o["value"].as_str().unwrap_or(""),
                                        o["label"].as_str().unwrap_or("")
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    return Err(Fail::detail(
                        Reason::OptionNotFound,
                        format!(
                            "No option matches {}. Options: {}.",
                            value["missing"],
                            available.join(", ")
                        ),
                    ));
                }
                "single" => {
                    return Err(Fail::detail(
                        Reason::InvalidRequest,
                        "This <select> takes one value; pass exactly one.",
                    ))
                }
                "disabled" => return Err(Fail::reason(Reason::NotEnabled)),
                _ => {
                    return Err(Fail::detail(
                        Reason::OptionNotFound,
                        "Element is not a <select>.",
                    ))
                }
            }
        }
        None => Value::Null,
    };
    let observed = watch.finish(run, observe::WINDOW)?;
    Ok(ActionResult::done(
        run.id(),
        with_summary(format!("Selected {} in {label}.", selected), &observed),
    )
    .with_data(json!({ "selected": selected }))
    .with_observed(observed))
}

/// Allowed upload roots: the session's working directory, then the ones the
/// user added in Settings (spec 10.2).
fn upload_roots(run: &Run<'_>) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(auth) = run.link.policy().read().session(&run.ctx.session) {
        if !auth.cwd.is_empty() {
            roots.push(PathBuf::from(&auth.cwd));
        }
    }
    roots.extend(run.settings.upload_roots.iter().map(PathBuf::from));
    roots
}

fn base_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `upload` (spec 10.2): paths are checked before any command; a file input
/// gets `DOM.setFileInputFiles` directly, anything else is clicked with the
/// file chooser intercepted for exactly this action.
pub fn upload(run: &Run<'_>, p: UploadParams) -> Result<ActionResult, Fail> {
    if p.paths.is_empty() {
        return Err(Fail::detail(
            Reason::InvalidRequest,
            "paths must name at least one file.",
        ));
    }
    let roots = upload_roots(run);
    let mut files = Vec::with_capacity(p.paths.len());
    for raw in &p.paths {
        let path =
            check_upload_path(raw, &roots, run.settings.allow_dotfiles).map_err(|denied| {
                Fail::detail(Reason::PathDenied, format!("{raw}: {}", denied.message))
            })?;
        files.push(path);
    }
    let names: Vec<String> = files.iter().map(|f| base_name(f)).collect();
    let file_list: Vec<String> = files
        .iter()
        .map(|f| f.to_string_lossy().into_owned())
        .collect();
    let el = element::locate(run, &p.element)?;
    let label = describe(p.element.trim(), &el.node.role, &el.node.name);
    let facts = element::facts(run, &el)?;
    if !facts.connected {
        return Err(Fail::reason(Reason::Detached));
    }
    if facts.file_input {
        if !facts.enabled {
            return Err(Fail::reason(Reason::NotEnabled));
        }
        if !facts.multiple && files.len() > 1 {
            return Err(Fail::detail(
                Reason::InvalidRequest,
                "This file input takes one file; pass exactly one path.",
            ));
        }
        let mut watch = Watch::start(run);
        watch.count_mutations(run);
        let session = el.node.frame.session.clone();
        watch.input(
            run,
            &session,
            Method::DomSetFileInputFiles,
            json!({ "files": file_list, "backendNodeId": el.node.backend }),
        )?;
        let observed = watch.finish(run, observe::WINDOW)?;
        return Ok(uploaded(run, &names, &label, observed));
    }
    if facts.text_like || facts.select {
        return Err(Fail::reason(Reason::NotFileInput));
    }
    if el.node.frame.is_oopif() {
        return Err(Fail::detail(
            Reason::UnsupportedFrame,
            "Uploading through a button inside a cross-site frame is not supported; use the frame's file input ref if the snapshot shows one.",
        ));
    }
    let prepared = element::prepare(
        run,
        &el,
        Needs {
            visible: true,
            enabled: true,
            scroll: true,
            stable: true,
            point: true,
            hit: true,
            ..Needs::default()
        },
    )?;
    let point = prepared
        .point
        .ok_or_else(|| Fail::reason(Reason::NotVisible))?;
    let chooser = run.link.subscribe(EventFilter {
        session: Some(run.main().clone()),
        methods: vec!["Page.fileChooserOpened"],
    });
    run.send(
        run.main(),
        Method::PageSetInterceptFileChooserDialog,
        json!({ "enabled": true }),
    )?;
    let outcome = upload_via_chooser(run, &chooser, point, &file_list);
    // Always switch interception off again, whatever happened (spec 10.2).
    let _ = run.link.send(
        run.ctx,
        super::super::link::Command::in_session(
            run.main(),
            Method::PageSetInterceptFileChooserDialog,
            json!({ "enabled": false }),
        ),
    );
    let observed = outcome?;
    Ok(uploaded(run, &names, &label, observed))
}

fn upload_via_chooser(
    run: &Run<'_>,
    chooser: &super::super::link::Subscription,
    point: element::Point,
    files: &[String],
) -> Result<Observed, Fail> {
    let mut watch = Watch::start(run);
    for (kind, buttons) in [("mouseMoved", 0), ("mousePressed", 1), ("mouseReleased", 0)] {
        let mut event = json!({ "type": kind, "x": point.x, "y": point.y, "modifiers": 0 });
        if kind != "mouseMoved" {
            event["button"] = json!("left");
            event["buttons"] = json!(buttons);
            event["clickCount"] = json!(1);
        }
        if watch
            .input(run, run.main(), Method::InputDispatchMouseEvent, event)?
            .is_none()
        {
            return watch.finish(run, observe::SHORT_WINDOW);
        }
    }
    let until = Instant::now() + CHOOSER_WAIT;
    let opened = loop {
        let remaining = until
            .min(run.ctx.deadline)
            .saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break None;
        }
        if let Some(event) = chooser.recv_timeout(remaining.min(Duration::from_millis(50))) {
            break Some(event.params);
        }
    };
    let Some(opened) = opened else {
        return Err(Fail::reason(Reason::ChooserTimeout));
    };
    if opened["mode"] == "selectSingle" && files.len() > 1 {
        return Err(Fail::detail(
            Reason::InvalidRequest,
            "The file chooser takes one file; pass exactly one path.",
        ));
    }
    let backend = opened["backendNodeId"]
        .as_i64()
        .ok_or_else(|| Fail::reason(Reason::NotFileInput))?;
    watch.input(
        run,
        run.main(),
        Method::DomSetFileInputFiles,
        json!({ "files": files, "backendNodeId": backend }),
    )?;
    watch.finish(run, observe::WINDOW)
}

fn uploaded(run: &Run<'_>, names: &[String], label: &str, observed: Observed) -> ActionResult {
    ActionResult::done(
        run.id(),
        with_summary(
            format!("Attached {} to {label}.", names.join(", ")),
            &observed,
        ),
    )
    .with_data(json!({ "files": names }))
    .with_observed(observed)
}

/// `dialog` (spec 10.1): answer the dialog waiting on the tab. Nothing is
/// ever answered without this explicit request.
pub fn dialog(run: &Run<'_>, p: DialogParams) -> Result<ActionResult, Fail> {
    let pending = run.lease(|l| l.pending_dialog.clone())?;
    let Some(pending) = pending else {
        return Err(Fail::reason(Reason::NoDialog));
    };
    let accept = p.action == DialogAction::Accept;
    let mut params = json!({ "accept": accept });
    if let (true, DialogType::Prompt, Some(text)) = (accept, &pending.kind, p.text.as_ref()) {
        params["promptText"] = json!(text);
    }
    let watch = Watch::start(run);
    let mut sessions = vec![run.main().clone()];
    sessions.extend(run.lease(|l| {
        l.frames
            .values()
            .map(|f| f.cdp_session.clone())
            .collect::<Vec<_>>()
    })?);
    let mut answered = false;
    for session in sessions {
        match run.send_mut(&session, Method::PageHandleJavaScriptDialog, params.clone()) {
            Ok(_) => {
                answered = true;
                break;
            }
            Err(fail) if fail.protocol_message().is_some() => continue,
            Err(fail) => return Err(fail),
        }
    }
    if let Some(lease) = run.link.policy().write().lease_mut(run.tab()) {
        lease.pending_dialog = None;
    }
    if !answered {
        // The user (or the page) closed it first; the state was stale.
        return Err(Fail::detail(
            Reason::NoDialog,
            "The dialog is no longer open; it may have been closed by the user.",
        ));
    }
    let observed = watch.finish(run, observe::WINDOW)?;
    let kind = match pending.kind {
        DialogType::Alert => "alert",
        DialogType::Confirm => "confirm",
        DialogType::Prompt => "prompt",
        DialogType::Beforeunload => "beforeunload",
    };
    let verb = if accept { "Accepted" } else { "Dismissed" };
    Ok(ActionResult::done(
        run.id(),
        with_summary(format!("{verb} the {kind} dialog."), &observed),
    )
    .with_data(json!({
        "type": kind,
        "message": pending.message,
        "action": if accept { "accept" } else { "dismiss" },
    }))
    .with_observed(observed))
}

/// Glob with `*` (any run) and `?` (one character).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// `wait {for: "url"}`: a glob when the value has `*` or `?`, else a prefix.
pub fn url_matches(pattern: &str, url: &str) -> bool {
    if pattern.contains('*') || pattern.contains('?') {
        glob_match(pattern, url)
    } else {
        url.starts_with(pattern)
    }
}

fn value_str(p: &WaitParams) -> Result<String, Fail> {
    match &p.value {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        _ => Err(Fail::detail(
            Reason::InvalidRequest,
            "This wait needs a non-empty string `value`.",
        )),
    }
}

/// `wait` (spec 9.4). A condition not met in time is `failed` / `timeout`.
pub fn wait(run: &Run<'_>, p: WaitParams) -> Result<ActionResult, Fail> {
    let started = Instant::now();
    let name = match p.condition {
        WaitFor::Time => "time",
        WaitFor::Text => "text",
        WaitFor::Ref => "ref",
        WaitFor::Url => "url",
        WaitFor::Load => "load",
    };
    let timed_out = |what: String| {
        Fail::detail(
            Reason::Timeout,
            format!(
                "{what} within {} ms.",
                run.ctx.deadline.duration_since(run.ctx.started).as_millis()
            ),
        )
    };
    let what = match p.condition {
        WaitFor::Time => {
            let ms = match &p.value {
                Some(Value::Number(n)) => n.as_u64(),
                Some(Value::String(s)) => s.trim().parse().ok(),
                _ => None,
            }
            .ok_or_else(|| {
                Fail::detail(
                    Reason::InvalidRequest,
                    "wait for time needs `value` in milliseconds.",
                )
            })?;
            let wanted = Duration::from_millis(ms).min(MAX_SLEEP);
            run.pause(wanted);
            format!("Waited {} ms.", started.elapsed().as_millis())
        }
        WaitFor::Text => {
            let text = value_str(&p)?;
            let main = element::main_frame(run)?;
            loop {
                let found = element::call_frame(
                    run,
                    &main,
                    PAGE_JS,
                    &[element::arg(json!("text")), element::arg(json!(text))],
                    false,
                );
                match found {
                    Ok(value) if value["found"] == json!(true) => break,
                    Ok(_) => {}
                    // The page navigated mid-poll: keep waiting on the new document.
                    Err(fail) if fail.protocol_message().is_some() => {}
                    Err(Fail::Reason(Reason::Detached, _)) => {}
                    Err(Fail::Reason(Reason::Timeout, _)) => {
                        return Err(timed_out("The text did not appear".into()))
                    }
                    Err(fail) => return Err(fail),
                }
                if run.ctx.remaining() <= POLL {
                    return Err(timed_out("The text did not appear".into()));
                }
                run.pause(POLL);
            }
            "The text appeared.".to_string()
        }
        WaitFor::Ref => {
            let raw = value_str(&p)?;
            let el = element::locate(run, &raw)?;
            loop {
                let facts = match element::facts(run, &el) {
                    Ok(facts) => facts,
                    Err(Fail::Reason(Reason::Timeout, _)) => {
                        return Err(timed_out(format!("{raw} did not become visible")))
                    }
                    Err(fail) => return Err(fail),
                };
                if !facts.connected {
                    return Err(Fail::detail(
                        Reason::Detached,
                        format!("{raw} left the document; take a new snapshot."),
                    ));
                }
                if facts.visible {
                    break;
                }
                if run.ctx.remaining() <= POLL {
                    return Err(timed_out(format!("{raw} did not become visible")));
                }
                run.pause(POLL);
            }
            format!("{} is visible.", raw.trim())
        }
        WaitFor::Url => {
            let pattern = value_str(&p)?;
            loop {
                let url = run.lease(|l| l.url.clone())?;
                if url_matches(&pattern, &url) {
                    break;
                }
                if run.ctx.remaining() <= POLL {
                    return Err(timed_out(format!(
                        "The URL did not match {pattern} (it is {url})"
                    )));
                }
                run.pause(POLL);
            }
            "The URL matches.".to_string()
        }
        WaitFor::Load => {
            let subscription = run.link.subscribe(EventFilter::session(run.main()));
            let main = element::main_frame(run)?;
            let ready =
                element::call_frame(run, &main, PAGE_JS, &[element::arg(json!("ready"))], false)
                    .ok()
                    .and_then(|v| v["state"].as_str().map(str::to_string));
            if ready.as_deref() != Some("complete") {
                loop {
                    let (state, _) = targets::wait_for_load(
                        &subscription,
                        None,
                        WaitUntil::Load,
                        (Instant::now() + Duration::from_millis(250)).min(run.ctx.deadline),
                    );
                    if state == super::super::protocol::LoadState::Load {
                        break;
                    }
                    let ready = element::call_frame(
                        run,
                        &main,
                        PAGE_JS,
                        &[element::arg(json!("ready"))],
                        false,
                    )
                    .ok()
                    .and_then(|v| v["state"].as_str().map(str::to_string));
                    if ready.as_deref() == Some("complete") {
                        break;
                    }
                    if run.ctx.expired() {
                        return Err(timed_out("The page did not finish loading".into()));
                    }
                }
            }
            "The page is loaded.".to_string()
        }
    };
    Ok(ActionResult::done(run.id(), what)
        .with_data(json!({ "for": name }))
        .with_observed(run.observed_now()))
}

/// `screenshot` (spec 8, 9.4). A background tab that renders nothing within
/// the time limit is `screenshot_unavailable_background`; `Page.bringToFront`
/// is never sent from here.
pub fn screenshot(run: &Run<'_>, p: ScreenshotParams) -> Result<ActionResult, Fail> {
    let format = p.format.unwrap_or_default();
    let mut params = json!({
        "format": match format { ImageFormat::Png => "png", ImageFormat::Jpeg => "jpeg" },
        "captureBeyondViewport": false,
        "fromSurface": true,
    });
    if format == ImageFormat::Jpeg {
        params["quality"] = json!(p.quality.unwrap_or(80).min(100));
    }
    if let Some(raw) = p.element.as_deref() {
        let el = element::locate(run, raw)?;
        element::prepare(
            run,
            &el,
            Needs {
                visible: true,
                scroll: true,
                ..Needs::default()
            },
        )?;
        let model = run
            .send(
                &el.node.frame.session,
                Method::DomGetBoxModel,
                json!({ "backendNodeId": el.node.backend }),
            )
            .map_err(|fail| match fail.protocol_message() {
                Some(_) => Fail::reason(Reason::NotVisible),
                None => fail,
            })?;
        let border: Vec<f64> =
            serde_json::from_value(model["model"]["border"].clone()).unwrap_or_default();
        if border.len() < 8 {
            return Err(Fail::reason(Reason::NotVisible));
        }
        let (dx, dy) = element::frame_offset(run, &el.node.frame)?;
        let (page_x, page_y, _, _) = element::viewport(run)?;
        let xs = [border[0], border[2], border[4], border[6]];
        let ys = [border[1], border[3], border[5], border[7]];
        let left = xs.iter().copied().fold(f64::INFINITY, f64::min);
        let right = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let top = ys.iter().copied().fold(f64::INFINITY, f64::min);
        let bottom = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if right - left < 1.0 || bottom - top < 1.0 {
            return Err(Fail::reason(Reason::NotVisible));
        }
        params["clip"] = json!({
            "x": left + dx + page_x,
            "y": top + dy + page_y,
            "width": right - left,
            "height": bottom - top,
            "scale": 1,
        });
    } else if p.full_page.unwrap_or(false) {
        let metrics = run.send(run.main(), Method::PageGetLayoutMetrics, json!({}))?;
        let size = if metrics["cssContentSize"].is_object() {
            &metrics["cssContentSize"]
        } else {
            &metrics["contentSize"]
        };
        let width = size["width"].as_f64().unwrap_or(0.0).ceil();
        let height = size["height"].as_f64().unwrap_or(0.0).ceil();
        if width >= 1.0 && height >= 1.0 {
            params["captureBeyondViewport"] = json!(true);
            params["clip"] = json!({
                "x": 0, "y": 0, "width": width, "height": height.min(MAX_FULL_PAGE_HEIGHT), "scale": 1,
            });
        }
    }
    // The agent gets the page without MonoCode's own cursor on it.
    let concealed = cursor::conceal(run);
    let shot = run.send(run.main(), Method::PageCaptureScreenshot, params);
    if concealed {
        cursor::reveal(run);
    }
    let shot = shot.map_err(|fail| match fail {
        Fail::Cdp(CdpError::Timeout, _) | Fail::Reason(Reason::Timeout, _) => {
            Fail::reason(Reason::ScreenshotUnavailableBackground)
        }
        other => other,
    })?;
    let data = shot["data"].as_str().unwrap_or("");
    if data.is_empty() {
        return Err(Fail::reason(Reason::ScreenshotUnavailableBackground));
    }
    let bytes = data.len() / 4 * 3;
    let mime = match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
    };
    Ok(ActionResult::done(
        run.id(),
        format!("Captured a {mime} screenshot (~{} KB).", bytes / 1024),
    )
    .with_data(json!({ "mime_type": mime, "base64": data, "bytes": bytes }))
    .with_observed(run.observed_now()))
}

/// `read` (spec 8): text of a ref or of the main document, 50 KB per call,
/// continued with `offset`.
pub fn read(run: &Run<'_>, p: ReadParams) -> Result<ActionResult, Fail> {
    let format = match p.format.unwrap_or_default() {
        ReadFormat::Text => "text",
        ReadFormat::Markdown => "markdown",
    };
    let offset = p.offset.unwrap_or(0);
    let args = [
        element::arg(json!(format)),
        element::arg(json!(offset)),
        element::arg(json!(READ_LIMIT)),
    ];
    let value = match p.element.as_deref() {
        Some(raw) => {
            let el = element::locate(run, raw)?;
            element::call(run, &el, READ_JS, &args)?
        }
        None => {
            let main = element::main_frame(run)?;
            element::call_frame(run, &main, READ_JS, &args, false)?
        }
    };
    let text = value["text"].as_str().unwrap_or("").to_string();
    let total = value["total"].as_u64().unwrap_or(0);
    let returned = text.encode_utf16().count() as u64;
    let end = offset.min(total) + returned;
    let mut data = json!({ "text": text, "offset": offset, "total": total });
    let message = if end < total {
        data["next_offset"] = json!(end);
        format!("Read characters {offset}-{end} of {total}; call again with offset {end} for more.")
    } else if total == 0 {
        "The element has no text.".to_string()
    } else {
        format!("Read characters {offset}-{end} of {total}.")
    };
    Ok(ActionResult::done(run.id(), message)
        .with_data(data)
        .with_observed(run.observed_now()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_patterns_are_prefixes_or_globs() {
        assert!(url_matches("https://x.com/a", "https://x.com/a/b?c"));
        assert!(!url_matches("https://x.com/b", "https://x.com/a"));
        assert!(url_matches(
            "*/checkout/*",
            "https://shop.example/checkout/42"
        ));
        assert!(url_matches("https://?.com/*", "https://x.com/"));
        assert!(!url_matches(
            "*/checkout",
            "https://shop.example/checkout/42"
        ));
        assert!(glob_match("*", ""));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"));
    }
}
