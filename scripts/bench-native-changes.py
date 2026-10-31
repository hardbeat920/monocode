#!/usr/bin/env python3
"""Time the production Changes tree builder without Git, GPUI, or database I/O."""
import argparse
import pathlib
import subprocess
import tempfile

root = pathlib.Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--baseline", type=pathlib.Path, help="Compare against an earlier changes.rs")
args = parser.parse_args()

def program(source):
    start = source.index("#[derive(Default)]\nstruct ChangeDir")
    end = source.index("fn dirname(", start)
    return r'''
#[allow(unused_imports)]
use std::{collections::HashMap, time::Instant};
struct ChangedFile { relative: String, status: String }
''' + source[start:end] + r'''
fn count(dir: &ChangeDir) -> usize {
    dir.files.len() + dir.dirs.iter().map(count).sum::<usize>()
}
fn main() {
    let files: Vec<_> = (0..20_000).map(|i| ChangedFile {
        relative: format!("dir-{}/nested/file-{i}.rs", i % 2_000),
        status: "modified".into(),
    }).collect();
    let members: Vec<_> = (0..files.len()).collect();
    for trial in 0..1 {
        let start = Instant::now();
        let tree = std::hint::black_box(build_tree(&files, &members));
        let elapsed = start.elapsed();
        assert_eq!(count(&tree), 20_000);
        assert_eq!(tree.dirs.len(), 2_000);
        println!("changes-tree trial={trial} files=20000 elapsed_ms={:.3}", elapsed.as_secs_f64() * 1000.);
    }
}
'''
with tempfile.TemporaryDirectory(prefix="monocode-changes-") as directory:
    path = pathlib.Path(directory)
    sources = {"after": root / "crates/monocode-gpui/src/changes.rs"}
    if args.baseline:
        sources = {"before": args.baseline, **sources}
    for label, source in sources.items():
        rust = path / f"{label}.rs"
        rust.write_text(program(source.read_text()))
        subprocess.run(["rustc", "--edition=2024", "-O", str(rust), "-o", str(path / label)], check=True)
    for trial in range(5):
        for label in sources:
            result = subprocess.run([str(path / label)], check=True, capture_output=True, text=True)
            print(f"{label} run={trial} {result.stdout.strip()}", flush=True)
