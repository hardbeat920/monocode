use std::io::{self, Read};
fn main() {
    let arguments: Vec<_> = std::env::args().collect();
    monocode_isolation::set_publication_helper(std::env::current_exe().expect("executable path"));
    if arguments.get(1).map(String::as_str) == Some("--cow-git-hook") {
        let mut input = String::new();
        if arguments.get(2).map(String::as_str) == Some("pre-push") {
            io::stdin()
                .read_to_string(&mut input)
                .expect("Git push input");
        }
        if let Err(error) = monocode_isolation::publication_hook(
            arguments.get(2).map_or("", String::as_str),
            &input,
        ) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    let result = (|| {
        if arguments.len() != 3 || arguments[1] != "--store" {
            return Err("Usage: monocode-isolation --store <registry-root>".to_string());
        }
        let mut input = String::new();
        io::stdin()
            .read_to_string(&mut input)
            .map_err(|e| e.to_string())?;
        let request = serde_json::from_str(&input).map_err(|e| e.to_string())?;
        monocode_isolation::dispatch(std::path::Path::new(&arguments[2]), request)
    })();
    match result {
        Ok(value) => println!("{}", serde_json::json!({"ok":true,"result":value})),
        Err(error) => {
            println!("{}", serde_json::json!({"ok":false,"error":error}));
            std::process::exit(1);
        }
    }
}
