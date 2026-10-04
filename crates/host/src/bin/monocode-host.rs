//! Standalone host executable. It shares the engine and service commands
//! with `monocode-app host` and does not load the desktop or require Node.

use monocode_remote::host::connect::RuntimeSource;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match RuntimeSource::current_exe(&[]) {
        Ok(runtime) => monocode_host::cli::run_host_cli_with(&args, &runtime),
        Err(error) => {
            eprintln!("{error}");
            1
        }
    };
    std::process::exit(result);
}
