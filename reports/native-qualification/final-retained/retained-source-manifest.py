import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import sys

parser = argparse.ArgumentParser()
parser.add_argument("command", choices=["create", "verify"])
parser.add_argument("--root", type=Path, required=True)
parser.add_argument("--manifest", type=Path, required=True)
args = parser.parse_args()
root = args.root.resolve()
entries = {}
for directory, directories, files in os.walk(root / "src-tauri", followlinks=False):
    directories[:] = sorted(name for name in directories if name not in {"target", ".git"})
    for name in directories:
        path = Path(directory) / name
        if path.is_symlink():
            raise SystemExit(f"Retained source directory is a symlink: {path.relative_to(root)}")
    for name in sorted(files):
        if name == ".DS_Store":
            continue
        path = Path(directory) / name
        if not stat.S_ISREG(path.lstat().st_mode):
            raise SystemExit(f"Retained source is not a regular file: {path.relative_to(root)}")
        entries[path.relative_to(root).as_posix()] = {
            "size": path.stat().st_size,
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        }
if args.command == "create":
    args.manifest.parent.mkdir(parents=True, exist_ok=True)
    args.manifest.write_text(json.dumps(entries, indent=2, sort_keys=True) + "\n")
    print(f"Retained source files={len(entries)}")
    raise SystemExit(0)
expected = json.loads(args.manifest.read_text())
missing = sorted(expected.keys() - entries.keys())
extra = sorted(entries.keys() - expected.keys())
changed = sorted(name for name in expected.keys() & entries.keys() if expected[name] != entries[name])
for label, paths in [("changed", changed), ("missing", missing), ("extra", extra)]:
    for path in paths:
        print(f"{label}: {path}")
print(f"Retained source files={len(entries)} changed={len(changed)} missing={len(missing)} extra={len(extra)}")
raise SystemExit(bool(changed or missing or extra))
