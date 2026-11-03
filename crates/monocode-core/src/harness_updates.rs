//! Launch-only harness update detection. All I/O runs on a worker; installation
//! occurs only when the user chooses Update. Nothing is persisted.
use crate::{
    catalog, cli,
    provider::{self, Kind},
};
use std::{
    cmp::Ordering,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering as AtomicOrdering},
    time::Duration,
};

static CLAIMED: AtomicBool = AtomicBool::new(false);
pub fn claim_launch_check() -> bool {
    !CLAIMED.swap(true, AtomicOrdering::SeqCst)
}
#[derive(Clone, Debug)]
pub struct Update {
    pub harness: Kind,
    pub installed: String,
    pub latest: String,
}
fn package(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::Claude => Some("@anthropic-ai/claude-code"),
        Kind::Codex => Some("@openai/codex"),
        Kind::OpenCode => Some("opencode-ai"),
        Kind::Pi => Some("@earendil-works/pi-coding-agent"),
        _ => None,
    }
}
pub fn parse_version(text: &str) -> Option<String> {
    regex::Regex::new(r"\d+\.\d+\.\d+")
        .ok()?
        .find(text)
        .map(|m| m.as_str().to_owned())
}
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let parts = |s: &str| {
        s.split('.')
            .map(|n| n.parse::<u64>().unwrap_or(0))
            .take(3)
            .collect::<Vec<_>>()
    };
    parts(a).cmp(&parts(b))
}
fn cwd() -> PathBuf {
    crate::accounts::home().unwrap_or_else(|| PathBuf::from("/"))
}
fn installed(kind: Kind) -> Result<String, String> {
    let path = provider::resolve(kind).ok_or("CLI could not be resolved")?;
    let output = cli::output(
        &path,
        &["--version".into()],
        &cwd(),
        Duration::from_secs(10),
    )?;
    parse_version(&output).ok_or("CLI returned no version".into())
}
pub fn latest_version(kind: Kind) -> Result<String, String> {
    let package = package(kind).ok_or("No update feed for harness")?;
    let text = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build()
        .get(&format!("https://registry.npmjs.org/{package}/latest"))
        .set("User-Agent", "MonoCode")
        .set("Accept", "application/json")
        .call()
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    let body: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    parse_version(body["version"].as_str().unwrap_or(""))
        .ok_or("npm registry returned no version".into())
}
/// Hidden and unresolved providers are excluded; failures are deliberately silent.
pub fn find_updates(hidden: &[String]) -> Vec<Update> {
    std::thread::scope(|scope| {
        let jobs = Kind::ALL
            .into_iter()
            .filter(|k| package(*k).is_some() && !hidden.iter().any(|id| id == k.id()))
            .map(|harness| {
                scope.spawn(move || {
                    let installed = installed(harness).ok()?;
                    let latest = latest_version(harness).ok()?;
                    (compare_versions(&latest, &installed).is_gt()).then_some(Update {
                        harness,
                        installed,
                        latest,
                    })
                })
            })
            .collect::<Vec<_>>();
        jobs.into_iter()
            .filter_map(|job| job.join().ok().flatten())
            .collect()
    })
}
/// Use exactly the resolved CLI's own updater, then verify its actual version
/// and refresh models before reporting success (exit status alone is insufficient).
pub fn run_update(update: &Update) -> Result<String, String> {
    let args: &[&str] = match update.harness {
        Kind::Claude | Kind::Codex => &["update"],
        Kind::OpenCode => &["upgrade"],
        Kind::Pi => &["update", "--self"],
        _ => return Err("No updater for harness".into()),
    };
    let path = provider::resolve(update.harness).ok_or("CLI could not be resolved")?;
    cli::output(
        &path,
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        &cwd(),
        Duration::from_secs(300),
    )
    .map_err(|error| {
        error
            .lines()
            .rfind(|l| !l.trim().is_empty())
            .unwrap_or("Update failed")
            .to_owned()
    })?;
    let version = installed(update.harness).unwrap_or_else(|_| update.installed.clone());
    if compare_versions(&version, &update.latest).is_lt() {
        return Err(format!("Still on {version} after updating."));
    }
    catalog::invalidate(update.harness, &crate::accounts::Profile::default());
    catalog::models(update.harness)?;
    Ok(version)
}
