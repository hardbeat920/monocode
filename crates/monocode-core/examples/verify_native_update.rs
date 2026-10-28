//! Offline publication check with the actual client verifier and embedded key.
use monocode_core::app_update::{verify_feed, Config};
use std::{env, fs, path::PathBuf};

fn verify() -> Result<(), String> {
    let directory = PathBuf::from(
        env::args_os()
            .nth(1)
            .ok_or("Expected an artifact directory")?,
    );
    let key = env::var("TAURI_UPDATER_PUBKEY").map_err(|_| "Missing update public key")?;
    let expected =
        env::var("MONOCODE_NATIVE_VERSION").map_err(|_| "Missing native release version")?;
    let config = Config::new(
        "https://offline.example.test/native-preview/latest.json",
        &key,
        "0.0.0-0",
    )?;
    let feed = fs::read(directory.join("latest.json")).map_err(|e| e.to_string())?;
    let mut verified = 0;
    for file in fs::read_dir(&directory).map_err(|e| e.to_string())? {
        let file = file.map_err(|e| e.to_string())?;
        let name = file.file_name().to_string_lossy().to_string();
        if !name.starts_with("artifacts-") || !name.ends_with(".json") {
            continue;
        }
        let records: Vec<serde_json::Value> =
            serde_json::from_slice(&fs::read(file.path()).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        for record in records {
            let Some(target) = record["target"].as_str() else {
                continue;
            };
            let filename = record["filename"]
                .as_str()
                .ok_or("Missing package filename")?;
            if std::path::Path::new(filename)
                .file_name()
                .and_then(|n| n.to_str())
                != Some(filename)
                || filename.contains('\\')
            {
                return Err("Invalid package filename".into());
            }
            let update = verify_feed(&feed, &config, target)?
                .ok_or("No native preview update in the signed feed")?;
            if update.version.to_string() != expected {
                return Err("Signed feed version does not match the native release".into());
            }
            update.verify_file(&directory.join(filename))?;
            verified += 1;
        }
    }
    if verified != 6 {
        return Err(format!(
            "Expected six signed native update packages, verified {verified}"
        ));
    }
    println!("Verified the native feed and all six updater packages with the client verifier.");
    Ok(())
}
fn main() {
    if let Err(error) = verify() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
