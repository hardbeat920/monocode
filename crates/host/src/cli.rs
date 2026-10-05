//! The `monocode-app host ...` entry point: `monocode_remote::host::cli`
//! with this crate's engine as the backend `serve` runs.

use std::sync::Arc;

use monocode_remote::host::HostStore;
use monocode_remote::host::connect::RuntimeSource;

use crate::backend::HostEngineOptions;
use crate::engine::HostEngine;

/// The version a host reports as `hostVersion`.
pub const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Builds the engine `serve` runs, once this process owns the data
/// directory.
pub fn host_engine(store: Arc<HostStore>) -> Result<HostEngine, String> {
    HostEngine::start(store, HostEngineOptions::default())
}

/// Runs one host command, such as `["serve"]` or `["connect", "--json"]`,
/// with `runtime` naming the program that `start`, `connect`, and the login
/// service run. Returns the exit code.
pub fn run_host_cli_with(args: &[String], runtime: &RuntimeSource) -> i32 {
    monocode_remote::host::cli::main(args, runtime, HOST_VERSION, host_engine)
}

/// `monocode-app host <args>`: `args` are the arguments after `host`.
/// Returns the exit code.
pub fn run_host_cli(args: &[String]) -> i32 {
    match RuntimeSource::current_exe(&["host"]) {
        Ok(runtime) => run_host_cli_with(args, &runtime),
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_version_help_and_unknown_commands() {
        assert_eq!(run_host_cli(&["--version".into()]), 0);
        assert_eq!(run_host_cli(&["help".into()]), 0);
        let directory = tempfile::tempdir().unwrap();
        let data_dir = directory.path().join("data").to_string_lossy().into_owned();
        assert_eq!(
            run_host_cli(&["devices".into(), "--data-dir".into(), data_dir.clone()]),
            0
        );
        assert_eq!(
            run_host_cli(&["nonsense".into(), "--data-dir".into(), data_dir]),
            1
        );
    }
}
