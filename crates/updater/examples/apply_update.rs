//! Runs one update the way the app would, for manual end to end checks:
//!
//! ```text
//! cargo run -p monocode-updater --example apply_update -- \
//!     --endpoint http://127.0.0.1:8000/latest.json \
//!     --pubkey "$(cat updater.key.pub)" \
//!     --current 0.5.0 \
//!     --executable /tmp/install/MonoCode.app/Contents/MacOS/MonoCode \
//!     [--check-only]
//! ```
//!
//! It checks the feed, downloads with progress, verifies the signature, and
//! installs over the app that `--executable` belongs to.

use std::path::PathBuf;

use anyhow::{Context as _, Result, bail};
use monocode_updater::{DownloadEvent, Updater, UpdaterConfig};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut config = UpdaterConfig::new(env!("CARGO_PKG_VERSION"));
    let mut check_only = false;
    while let Some(arg) = args.next() {
        let mut value = || args.next().with_context(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--endpoint" => config.endpoints.push(value()?),
            "--pubkey" => config.pubkey = value()?,
            "--current" => config.current_version = value()?,
            "--executable" => config.executable_path = Some(PathBuf::from(value()?)),
            "--check-only" => check_only = true,
            other => bail!("unknown argument {other}"),
        }
    }
    config.dangerous_insecure_transport_protocol = true;
    config.no_proxy = true;

    let updater = Updater::new(config)?;
    println!("replaces {}", updater.extract_path().display());
    let Some(update) = updater.check()? else {
        println!("{} is current", updater.current_version());
        return Ok(());
    };
    println!(
        "update {} -> {} from {}",
        update.current_version, update.version, update.download_url
    );
    if check_only {
        return Ok(());
    }

    let mut total = None;
    let mut downloaded = 0usize;
    let bytes = update.download(|event| match event {
        DownloadEvent::Started { content_length } => total = content_length,
        DownloadEvent::Progress { chunk_length } => downloaded += chunk_length,
        DownloadEvent::Finished => println!("downloaded {downloaded} of {total:?} bytes"),
    })?;
    println!("signature verified");
    update.install(bytes)?;
    println!("installed {}", update.version);
    Ok(())
}
