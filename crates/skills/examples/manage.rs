//! Headless administration for explicit local data and skill home directories.

use monocode_skills::SkillManager;
use serde::Serialize;
use serde_json::Value;

const USAGE: &str = "Usage: manage <absolute-data-dir> <absolute-skill-home> <command>\nCommands: import <folder>, list, apply <id>, share <id> <on|off>, repair";

enum Action<'a> {
    Import(&'a str),
    List,
    Apply(&'a str),
    Share(&'a str, bool),
    Repair,
}

fn run(args: &[String]) -> Result<Value, String> {
    let [data_dir, skill_home, command, rest @ ..] = args else {
        return Err(USAGE.into());
    };
    let action = match (command.as_str(), rest) {
        ("import", [folder]) => Action::Import(folder),
        ("list", []) => Action::List,
        ("apply", [id]) => Action::Apply(id),
        ("share", [id, value]) if value == "on" => Action::Share(id, true),
        ("share", [id, value]) if value == "off" => Action::Share(id, false),
        ("repair", []) => Action::Repair,
        _ => return Err(USAGE.into()),
    };
    let manager = SkillManager::open(data_dir, skill_home).map_err(|error| error.to_string())?;
    match action {
        Action::Import(folder) => json(manager.import(folder).map_err(|error| error.to_string())?),
        Action::List => json(manager.entries().map_err(|error| error.to_string())?),
        Action::Apply(id) => json(manager.apply(id).map_err(|error| error.to_string())?),
        Action::Share(id, shared) => json(
            manager
                .set_shared(id, shared)
                .map_err(|error| error.to_string())?,
        ),
        Action::Repair => json(manager.reconcile(&[]).map_err(|error| error.to_string())?),
    }
}

fn json(value: impl Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| error.to_string())
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match run(&args)
        .and_then(|value| serde_json::to_string_pretty(&value).map_err(|error| error.to_string()))
    {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("{}", serde_json::json!({ "error": error }));
            std::process::exit(1);
        }
    }
}
