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

fn run_cli(mut args: impl Iterator<Item = String>) -> Option<i32> {
    match args.next().as_deref() {
        Some("--help" | "-h") => {
            println!(
                "Usage: monocode [app|control]\n\n\
                Run monocode without arguments to open the desktop app.\n\
                Run monocode app --help or monocode control --help for command details."
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
    fn help_exits_before_desktop_startup() {
        for flag in ["--help", "-h"] {
            assert_eq!(run_cli(std::iter::once(flag.to_owned())), Some(0));
        }
        assert_eq!(run_cli(std::iter::empty()), None);
        assert_eq!(
            run_cli(["project", "--help"].into_iter().map(String::from)),
            None
        );
    }
}
