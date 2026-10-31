#!/usr/bin/env python3
"""Create a fresh synthetic native-app profile; never opens an existing database."""
import argparse
import json
from pathlib import Path
import plistlib
import subprocess
import tempfile

SEED = r'''
use monocode_core::{store::Store, transcript::Row};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let path = std::path::Path::new(&args[1]);
    assert!(!path.exists(), "fixture database must be new");
    let mut store = Store::open_monocode(path).unwrap();
    let row = |role: &str, text: String| Row { role: role.into(), text, first: true, sent_at: None };
    for i in 0..2999 {
        let session = store.create("codex", &args[2]).unwrap();
        store.title(session.id, &format!("Synthetic session {i:04}")).unwrap();
        store.write_rows(session.id, 0, &[row("user", format!("Synthetic prompt {i}"))]).unwrap();
    }
    let session = store.create("codex", &args[2]).unwrap();
    store.title(session.id, "Performance transcript").unwrap();
    let rows: Vec<_> = (0..2000).map(|i| match i % 4 {
        0 => row("user", format!("Synthetic request {i}: inspect module and explain the changes.")),
        1 => row("reasoning", format!("Checking module {i} and its dependent functions.")),
        2 => row("tool", format!(r#"{{"id":"call-{i}","name":"read_file","input":"src/module.rs","output":"pub fn sample() {{}}","state":"completed"}}"#)),
        _ => row("assistant", format!("## Module {i}\n\nThe implementation returns the expected result.\n\n```rust\npub fn sample() -> usize {{ {i} }}\n```")),
    }).collect();
    store.write_rows(session.id, 0, &rows).unwrap();
    store.save_ui_state(&serde_json::json!({
        "project": args[2], "projects": [&args[2]], "tabs": [session.id],
        "active": session.id, "title_order": [{"Session": session.id}],
        "sidebar_visible": true, "right_panel_open": false
    }).to_string()).unwrap();
    assert_eq!(store.sessions().unwrap().len(), 3000);
    assert_eq!(store.load(session.id).unwrap().rows.len(), 2000);
    println!("{}", session.id);
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--deps", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    deps = args.deps.resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix="monocode-live-performance-", dir="/private/tmp"))
    repo = root / "synthetic-repo"
    repo.mkdir()
    for i in range(20000):
        path = repo / f"src/group-{i // 10:04}/module-{i:05}.rs"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f"pub fn module_{i}() -> usize {{ {i} }}\n")
    def git(*command):
        subprocess.run(["git", "-C", str(repo), *command], check=True, capture_output=True)
    git("init", "-q")
    git("add", ".")
    git("-c", "user.name=Performance fixture", "-c", "user.email=fixture@example.invalid",
        "-c", "commit.gpgsign=false", "commit", "-qm", "Synthetic baseline")
    for i in range(2000):
        path = repo / f"src/group-{i // 10:04}/module-{i:05}.rs"
        with path.open("a") as output:
            output.writelines(f"// Synthetic added line {line} in module {i}\n" for line in range(250))
    source = root / "seed.rs"
    source.write_text(SEED)
    newest = lambda pattern: max(deps.glob(pattern), key=lambda path: path.stat().st_mtime)
    subprocess.run(["rustc", "--edition=2021", str(source), "-o", str(root / "seed"),
                    "-L", f"dependency={deps}", "--extern", f"monocode_core={newest('libmonocode_core-*.rlib')}",
                    "--extern", f"serde_json={newest('libserde_json-*.rlib')}"], check=True)
    data = root / "data"
    data.mkdir()
    seeded = subprocess.check_output([str(root / "seed"), str(data / "monocode.db"), str(repo)], text=True)
    app = root / "Monocode Performance.app"
    executable = app / "Contents/MacOS/monocode-gpui"
    executable.parent.mkdir(parents=True)
    subprocess.run(["cp", "-c", str(binary), str(executable)], check=True)
    bundle_id = "dev.usemono.performance." + root.name.rsplit("-", 1)[-1]
    environment = {"MONOCODE_NATIVE_DATA_DIR": str(data), "MONOCODE_PERF_LOG": str(root / "trace.jsonl")}
    with (app / "Contents/Info.plist").open("wb") as output:
        plistlib.dump({"CFBundleIdentifier": bundle_id, "CFBundleName": "Monocode Performance",
                      "CFBundleExecutable": "monocode-gpui", "CFBundlePackageType": "APPL",
                      "MonocodeDataMode": "dev-data", "LSEnvironment": environment}, output)
    manifest = dict(root=str(root), repo=str(repo), data=str(data), app=str(app),
                    executable=str(executable), bundle_id=bundle_id, session=int(seeded.strip()),
                    environment=environment)
    (root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
