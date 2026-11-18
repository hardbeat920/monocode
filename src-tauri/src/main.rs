#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = monocode_lib::ssh_askpass::maybe_run() {
        std::process::exit(code);
    }
    if let Some(code) = run_cli(std::env::args().skip(1)) {
        std::process::exit(code);
    }
    monocode_lib::run()
}

fn run_cli(mut args: impl ExactSizeIterator<Item = String>) -> Option<i32> {
    match args.next().as_deref() {
        Some("--help" | "-h") if args.len() == 0 => {
            let exe = monocode_lib::control_cli::quoted_executable();
            println!(
                "Usage: {exe} [app|control]\n\n\
                Run without arguments to open the desktop app.\n\n\
                For command details, run either\n  {exe} app --help\n  {exe} control --help"
            );
            Some(0)
        }
        Some("app") => Some(monocode_lib::control_cli::run_app(args.collect())),
        Some("control") => Some(monocode_lib::control_cli::run(args.collect())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::run_cli;

    #[test]
    fn only_standalone_help_exits_before_desktop_startup() {
        for flag in ["--help", "-h"] {
            assert_eq!(run_cli(std::iter::once(flag.to_owned())), Some(0));
            for args in [[flag, "project"], ["project", flag]] {
                assert_eq!(
                    run_cli(args.into_iter().map(String::from)),
                    None,
                    "{args:?}"
                );
            }
        }
        assert_eq!(run_cli(std::iter::empty()), None);
    }
}
