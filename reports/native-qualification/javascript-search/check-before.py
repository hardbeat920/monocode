#!/usr/bin/env python3
"""Run the owned search regressions against unchanged production dependencies."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

NAMES = [
    "javascript_regex_search_accepts_lookaround_and_backreferences",
    "javascript_regex_replacement_keeps_lookbehind_and_capture_groups",
    "javascript_regex_classes_keep_word_digit_and_whitespace_rules",
    "javascript_preview_regex_accepts_lookaround_and_backreferences",
    "javascript_preview_regex_keeps_unicode_character_class_rules",
    "codemirror_literal_search_normalizes_canonical_and_compatibility_characters",
    "codemirror_literal_replacement_skips_partial_normalized_characters",
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    source = args.source.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    paths = ["crates/editor/src/search.rs", "crates/view-files/src/preview_search_tests.rs", "Cargo.toml", "Cargo.lock", "crates/editor/Cargo.toml", "crates/view-files/Cargo.toml"]
    hashes = {path: hashlib.sha256((source / path).read_bytes()).hexdigest() for path in paths}
    (output / "before-source-sha256.json").write_text(json.dumps(hashes, indent=2, sort_keys=True) + "\n")
    command = ["cargo", "test", "--workspace", "--lib", "--bins", "--all-features", "--locked", "--offline", "-j", "2", "--no-run", "--message-format=json"]
    with (output / "before-compile.log").open("w") as stream:
        result = subprocess.run(command, cwd=source, stdout=stream, stderr=subprocess.STDOUT)
    if result.returncode:
        raise RuntimeError("The common workspace graph did not compile")
    binaries = []
    for line in (output / "before-compile.log").read_text().splitlines():
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            continue
        executable = item.get("executable")
        if item.get("reason") != "compiler-artifact" or not item.get("profile", {}).get("test") or not executable:
            continue
        binary = Path(executable)
        if binary.name.startswith(("monocode_editor-", "monocode_view_files-")) and binary not in binaries:
            binaries.append(binary)
    assert len(binaries) == 2, binaries
    inventory = {}
    for binary in binaries:
        result = subprocess.run([binary, "--list"], cwd=source, capture_output=True, text=True, check=True)
        inventory[str(binary)] = [line[:-6] for line in result.stdout.splitlines() if line.endswith(": test")]
    (output / "before-test-inventory.json").write_text(json.dumps(inventory, indent=2) + "\n")
    summary = {"compile_command": command, "source_sha256": hashes, "tests": []}
    with (output / "before.log").open("w") as combined:
        for name in NAMES:
            registered = [(binary, full) for binary, names in inventory.items() for full in names if full.rsplit("::", 1)[-1] == name]
            assert len(registered) == 1, (name, registered)
            binary, full = registered[0]
            command = [binary, full, "--exact", "--nocapture"]
            result = subprocess.run(command, cwd=source, capture_output=True, text=True)
            log = result.stdout + result.stderr
            (output / (name + ".log")).write_text(log)
            combined.write("COMMAND " + json.dumps(command) + "\n" + log + "\n")
            passed_before = "test result: FAILED. 0 passed; 1 failed;" in log
            summary["tests"].append({"name": full, "command": command, "exit_code": result.returncode, "intended_failure": passed_before})
            print(name + ": exit " + str(result.returncode), flush=True)
        summary["complete"] = all(row["intended_failure"] for row in summary["tests"])
    summary["test_binary_sha256"] = {str(binary): hashlib.sha256(binary.read_bytes()).hexdigest() for binary in binaries}
    (output / "before-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    assert summary["complete"], summary["tests"]
    print("All seven intended search failures reproduced", flush=True)


if __name__ == "__main__":
    main()
