#!/usr/bin/env python3
"""Run repaired search regressions and both affected crate test suites."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

import runpy

NAMES = runpy.run_path(str(Path(__file__).with_name("check-before.py")))["NAMES"]
PATHS = ["Cargo.toml", "Cargo.lock", "crates/editor/Cargo.toml", "crates/editor/src/search.rs", "crates/view-files/Cargo.toml", "crates/view-files/src/preview_search.rs", "crates/view-files/src/preview_search_tests.rs"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--compile-log", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    hashes = {path: hashlib.sha256((args.source / path).read_bytes()).hexdigest() for path in PATHS}
    binaries = set()
    for line in args.compile_log.read_text().splitlines():
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            continue
        executable = item.get("executable")
        if item.get("reason") == "compiler-artifact" and item.get("profile", {}).get("test") and executable:
            if Path(executable).name.startswith(("monocode_editor-", "monocode_view_files-")):
                binaries.add(executable)
    assert len(binaries) == 2, binaries
    inventory = {}
    for binary in sorted(binaries):
        result = subprocess.run([binary, "--list"], cwd=args.source, capture_output=True, text=True, check=True)
        inventory[binary] = [line[:-6] for line in result.stdout.splitlines() if line.endswith(": test")]
    summary = {"source_sha256": hashes, "tests": [], "suites": [], "test_binary_sha256": {binary: hashlib.sha256(Path(binary).read_bytes()).hexdigest() for binary in binaries}}
    for name in NAMES:
        registered = [(binary, full) for binary, names in inventory.items() for full in names if full.rsplit("::", 1)[-1] == name]
        assert len(registered) == 1, (name, registered)
        binary, full = registered[0]
        command = [binary, full, "--exact", "--nocapture"]
        result = subprocess.run(command, cwd=args.source, capture_output=True, text=True)
        log = result.stdout + result.stderr
        (args.output / (name + ".log")).write_text(log)
        summary["tests"].append({"name": full, "command": command, "exit_code": result.returncode, "passed": "test result: ok. 1 passed; 0 failed;" in log})
        print(name + ": exit " + str(result.returncode), flush=True)
    for binary in sorted(binaries):
        command = [binary]
        result = subprocess.run(command, cwd=args.source, capture_output=True, text=True)
        label = "editor" if Path(binary).name.startswith("monocode_editor-") else "view-files"
        (args.output / (label + "-suite.log")).write_text(result.stdout + result.stderr)
        summary["suites"].append({"name": label, "command": command, "exit_code": result.returncode})
        print(label + " suite: exit " + str(result.returncode), flush=True)
    assert hashes == {path: hashlib.sha256((args.source / path).read_bytes()).hexdigest() for path in PATHS}
    summary["complete"] = all(item["passed"] for item in summary["tests"]) and all(item["exit_code"] == 0 for item in summary["suites"])
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    (args.output / "test-inventory.json").write_text(json.dumps(inventory, indent=2) + "\n")
    assert summary["complete"], summary


if __name__ == "__main__":
    main()
