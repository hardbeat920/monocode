//! `monocode-updater-sign`: generates updater keys, signs release packages,
//! and checks signatures. It replaces `tauri signer` in the release scripts
//! and keeps its key and signature formats.
//!
//! ```text
//! monocode-updater-sign generate <dir> [--password <password>] [--unencrypted]
//!     Writes <dir>/updater.key (TAURI_SIGNING_PRIVATE_KEY) and
//!     <dir>/updater.key.pub (TAURI_UPDATER_PUBKEY).
//! monocode-updater-sign sign <file>...
//!     Writes <file>.sig with the key in TAURI_SIGNING_PRIVATE_KEY (the key
//!     text or a path to it) and TAURI_SIGNING_PRIVATE_KEY_PASSWORD.
//! monocode-updater-sign verify <file> [--pubkey <key>]
//!     Checks <file>.sig with --pubkey or TAURI_UPDATER_PUBKEY, the same way
//!     the updater does.
//! ```

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context as _, Result, bail};
use monocode_updater::sign::{decode_secret_key, generate_keys, sign_file};

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("monocode-updater-sign: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<()> {
    let Some((command, rest)) = args.split_first() else {
        bail!("usage: monocode-updater-sign <generate|sign|verify> ...");
    };
    match command.as_str() {
        "generate" => generate(rest),
        "sign" => sign(rest),
        "verify" => verify(rest),
        other => bail!("unknown command {other:?}"),
    }
}

/// Positional arguments, then `(--name, value)` options.
type Parsed<'a> = (Vec<&'a str>, Vec<(&'a str, &'a str)>);

/// Splits `--name value` options from positional arguments. `flags` take no
/// value.
fn parse<'a>(args: &'a [String], flags: &[&str]) -> Result<Parsed<'a>> {
    let mut positional = Vec::new();
    let mut options = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if flags.contains(&arg.as_str()) {
            options.push((arg.as_str(), ""));
        } else if let Some(name) = arg.strip_prefix("--") {
            let value = iter
                .next()
                .with_context(|| format!("--{name} needs a value"))?;
            options.push((arg.as_str(), value.as_str()));
        } else {
            positional.push(arg.as_str());
        }
    }
    Ok((positional, options))
}

fn option<'a>(options: &[(&'a str, &'a str)], name: &str) -> Option<&'a str> {
    options
        .iter()
        .find(|(option, _)| *option == name)
        .map(|(_, value)| *value)
}

fn generate(args: &[String]) -> Result<()> {
    let (positional, options) = parse(args, &["--unencrypted"])?;
    let [dir] = positional.as_slice() else {
        bail!(
            "usage: monocode-updater-sign generate <dir> [--password <password>] [--unencrypted]"
        );
    };
    let password = if option(&options, "--unencrypted").is_some() {
        None
    } else {
        Some(option(&options, "--password").unwrap_or_default())
    };
    let keys = generate_keys(password)?;
    let dir = Path::new(dir);
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let private_path = dir.join("updater.key");
    let public_path = dir.join("updater.key.pub");
    std::fs::write(&private_path, &keys.private_key)?;
    std::fs::write(&public_path, &keys.public_key)?;
    println!("{}", private_path.display());
    println!("{}", public_path.display());
    Ok(())
}

fn sign(args: &[String]) -> Result<()> {
    let (files, _) = parse(args, &[])?;
    if files.is_empty() {
        bail!("usage: monocode-updater-sign sign <file>...");
    }
    let key = std::env::var("TAURI_SIGNING_PRIVATE_KEY")
        .context("TAURI_SIGNING_PRIVATE_KEY is not set")?;
    let password = std::env::var("TAURI_SIGNING_PRIVATE_KEY_PASSWORD").ok();
    let sk = decode_secret_key(&key, password.as_deref())?;
    for file in files {
        let sig = sign_file(&sk, Path::new(file))?;
        println!("{}", sig.display());
    }
    Ok(())
}

fn verify(args: &[String]) -> Result<()> {
    let (files, options) = parse(args, &[])?;
    let [file] = files.as_slice() else {
        bail!("usage: monocode-updater-sign verify <file> [--pubkey <key>]");
    };
    let pubkey = match option(&options, "--pubkey") {
        Some(pubkey) => pubkey.to_string(),
        None => std::env::var("TAURI_UPDATER_PUBKEY")
            .context("pass --pubkey or set TAURI_UPDATER_PUBKEY")?,
    };
    let pubkey = read_value(&pubkey)?;
    let data = std::fs::read(file).with_context(|| format!("reading {file}"))?;
    let mut sig_path = PathBuf::from(file).into_os_string();
    sig_path.push(".sig");
    let signature = std::fs::read_to_string(&sig_path)
        .with_context(|| format!("reading {}", PathBuf::from(&sig_path).display()))?;
    monocode_updater::verify_signature(&data, &signature, &pubkey)
        .with_context(|| format!("{file} does not match its signature"))?;
    println!("{file}: signature OK");
    Ok(())
}

/// A key given as its text or as a path to a file holding it.
fn read_value(value: &str) -> Result<String> {
    let path = Path::new(value.trim());
    if path.is_file() {
        return Ok(std::fs::read_to_string(path)?.trim().to_string());
    }
    Ok(value.trim().to_string())
}
