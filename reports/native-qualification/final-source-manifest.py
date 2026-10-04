#!/usr/bin/env python3
"""Create or verify byte hashes for the explicit native qualification inputs.

Examples:
  python3 final-source-manifest.py create --root SOURCE --manifest snapshot.json
  python3 final-source-manifest.py verify --root COPY --manifest snapshot.json

The manifest has no timestamps or absolute source paths. It includes untracked
files, skips every target and .git directory, and rejects symlinks and special
files so a build cannot read unrecorded external inputs through those entries.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import sys


INCLUDES = (
    "apps",
    "crates",
    "vendor",
    ".cargo",
    "packaging",
    "Cargo.toml",
    "Cargo.lock",
    "CHANGELOG.md",
    ".github/workflows/native-ci.yml",
    ".github/workflows/native-release.yml",
    "scripts/install-native-host.ps1",
    "scripts/install-native-host.sh",
    "scripts/install-native-linux-deps.sh",
    "scripts/test-remote-ssh.py",
)
EXCLUDED_DIRECTORIES = (".git", "target")
FORMAT_VERSION = 1


def canonical_bytes(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True,
                      separators=(",", ":")).encode("utf-8")


def digest(value):
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def in_scope(relative):
    parts = PurePosixPath(relative).parts
    return (relative == PurePosixPath(relative).as_posix()
            and bool(parts) and not PurePosixPath(relative).is_absolute()
            and ".." not in parts
            and not any(part in EXCLUDED_DIRECTORIES for part in parts)
            and any(relative == included or relative.startswith(included + "/")
                    for included in INCLUDES))


def scan(root, require_roots):
    paths = set()
    for included in INCLUDES:
        start = root / included
        if start.is_symlink():
            raise ValueError(f"Symlink is not a frozen input: {included}")
        if not start.exists():
            if require_roots:
                raise ValueError(f"Required source input is missing: {included}")
            continue
        if start.is_file():
            paths.add(start)
            continue
        if not start.is_dir():
            raise ValueError(f"Source input is not a regular file or directory: {included}")

        def walk_error(error):
            raise error

        for directory, directories, files in os.walk(start, onerror=walk_error,
                                                     followlinks=False):
            directories[:] = sorted(name for name in directories
                                    if name not in EXCLUDED_DIRECTORIES)
            for name in directories:
                path = Path(directory) / name
                if path.is_symlink():
                    raise ValueError(f"Symlink is not a frozen input: {path.relative_to(root)}")
            paths.update(Path(directory) / name for name in files)

    entries = []
    for path in sorted(paths, key=lambda item: item.relative_to(root).as_posix()):
        relative = path.relative_to(root).as_posix()
        before = path.lstat()
        if not stat.S_ISREG(before.st_mode):
            raise ValueError(f"Source input is not a regular file: {relative}")
        checksum = hashlib.sha256()
        size = 0
        with path.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                checksum.update(chunk)
                size += len(chunk)
        after = path.lstat()
        if (before.st_size != size or before.st_size != after.st_size
                or before.st_mtime_ns != after.st_mtime_ns
                or before.st_ino != after.st_ino):
            raise ValueError(f"Source changed while hashing: {relative}")
        entries.append({"path": relative, "size": size,
                        "sha256": checksum.hexdigest()})
    return entries


def payload(entries):
    return {"version": FORMAT_VERSION, "includes": list(INCLUDES),
            "excluded_directory_names": list(EXCLUDED_DIRECTORIES),
            "files": entries}


def read_manifest(path):
    manifest = json.loads(path.read_text(encoding="utf-8"))
    keys = {"version", "includes", "excluded_directory_names", "files", "sha256"}
    if not isinstance(manifest, dict) or set(manifest) != keys:
        raise ValueError("Manifest has unexpected fields")
    if (manifest["version"] != FORMAT_VERSION
            or manifest["includes"] != list(INCLUDES)
            or manifest["excluded_directory_names"] != list(EXCLUDED_DIRECTORIES)):
        raise ValueError("Manifest scope or version differs from this helper")
    entries = manifest["files"]
    if not isinstance(entries, list):
        raise ValueError("Manifest files must be a list")
    paths = []
    for entry in entries:
        if not isinstance(entry, dict) or set(entry) != {"path", "size", "sha256"}:
            raise ValueError("Manifest file entry has unexpected fields")
        path = entry["path"]
        if not isinstance(path, str) or not in_scope(path):
            raise ValueError(f"Manifest path is outside the native scope: {path!r}")
        if type(entry["size"]) is not int or entry["size"] < 0:
            raise ValueError(f"Manifest file size is invalid: {path}")
        if not isinstance(entry["sha256"], str) or not re.fullmatch(r"[0-9a-f]{64}", entry["sha256"]):
            raise ValueError(f"Manifest file hash is invalid: {path}")
        paths.append(path)
    if paths != sorted(set(paths)):
        raise ValueError("Manifest file paths must be unique and sorted")
    if manifest["sha256"] != digest(payload(entries)):
        raise ValueError("Manifest digest does not match its recorded entries")
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("create", "verify"))
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    options = parser.parse_args()
    root = options.root.resolve(strict=True)
    if not root.is_dir():
        raise ValueError("Source root must be a directory")
    manifest_path = options.manifest.resolve()
    if manifest_path.is_relative_to(root) and in_scope(manifest_path.relative_to(root).as_posix()):
        raise ValueError("Write the manifest outside the recorded source inputs")

    if options.command == "create":
        entries = scan(root, require_roots=True)
        document = payload(entries)
        document["sha256"] = digest(document)
        manifest_path.parent.mkdir(parents=True, exist_ok=True)
        manifest_path.write_bytes((json.dumps(document, ensure_ascii=False, sort_keys=True,
                                            indent=2) + "\n").encode("utf-8"))
        print(f"files={len(entries)} sha256={document['sha256']}")
        return 0

    manifest = read_manifest(manifest_path)
    entries = scan(root, require_roots=False)
    expected = {entry["path"]: entry for entry in manifest["files"]}
    actual = {entry["path"]: entry for entry in entries}
    missing = sorted(expected.keys() - actual.keys())
    extra = sorted(actual.keys() - expected.keys())
    changed = sorted(path for path in expected.keys() & actual.keys()
                     if expected[path] != actual[path])
    for label, paths in (("changed", changed), ("missing", missing), ("extra", extra)):
        for path in paths:
            print(f"{label}: {path}")
    print(f"files={len(entries)} sha256={digest(payload(entries))} "
          f"changed={len(changed)} missing={len(missing)} extra={len(extra)}")
    return int(bool(changed or missing or extra))


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        sys.exit(2)
