#!/usr/bin/env python3
"""Inspect an owned ad hoc arm64 package without installing it or starting its GUI."""

import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import subprocess
import tarfile
import tempfile


def sha256(path):
    checksum = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            checksum.update(chunk)
    return checksum.hexdigest()


def run(*arguments):
    result = subprocess.run([str(value) for value in arguments], check=True,
                            text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, timeout=180)
    return result.stdout.strip()


def verify(output, binaries, version):
    target = "aarch64-apple-darwin"
    app = output / "MonoCode.app"
    run("codesign", "--verify", "--deep", "--strict", app)
    signature = run("codesign", "--display", "--verbose=4", app)
    assert "Signature=adhoc" in signature, signature
    run("plutil", "-lint", app / "Contents/Info.plist")
    with (app / "Contents/Info.plist").open("rb") as stream:
        plist = plistlib.load(stream)
    assert plist["CFBundleIdentifier"] == "com.monocode.desktop"
    assert plist["CFBundleExecutable"] == "monocode-app"
    assert plist["CFBundleVersion"] == version
    assert plist["CFBundleShortVersionString"] == version
    assert plist["CFBundleURLTypes"][0]["CFBundleURLSchemes"] == ["monocode"]
    uuids = {}
    for name in ("monocode-app", "monocode-host"):
        executable = app / "Contents/MacOS" / name
        assert run("lipo", "-archs", executable) == "arm64"
        actual_uuid = run("dwarfdump", "--uuid", executable).split()[1]
        input_uuid = run("dwarfdump", "--uuid", binaries / name).split()[1]
        assert actual_uuid == input_uuid
        uuids[name] = actual_uuid
    assert run(app / "Contents/MacOS/monocode-host", "--version") == version
    views = run(app / "Contents/MacOS/monocode-app", "--list-views")
    assert "shell" in views and "widgets" in views
    for name in ("icon.icns", "Assets.car"):
        assert (app / "Contents/Resources" / name).stat().st_size > 0

    host_archive = output / f"monocode-host_{version}_{target}.tar.gz"
    with tarfile.open(host_archive) as archive:
        entries = archive.getmembers()
        assert len(entries) == 1 and entries[0].isfile()
        assert entries[0].name == "monocode-host"
        assert hashlib.sha256(archive.extractfile(entries[0]).read()).hexdigest() == sha256(binaries / "monocode-host")
    app_archive = output / f"MonoCode_{version}_{target}.app.tar.gz"
    with tarfile.open(app_archive) as archive:
        for relative in ("Contents/Info.plist", "Contents/MacOS/monocode-app",
                         "Contents/MacOS/monocode-host", "Contents/Resources/icon.icns",
                         "Contents/Resources/Assets.car"):
            entry = archive.getmember(f"MonoCode.app/{relative}")
            assert entry.isfile()
            assert hashlib.sha256(archive.extractfile(entry).read()).hexdigest() == sha256(app / relative)

    image = output / f"MonoCode_{version}_{target}.dmg"
    run("hdiutil", "verify", image)
    with tempfile.TemporaryDirectory(prefix="monocode-package-mount-") as directory:
        mount = Path(directory) / "volume"
        mount.mkdir()
        run("hdiutil", "attach", "-nobrowse", "-readonly", "-mountpoint", mount, image)
        try:
            assert (mount / "Applications").is_symlink()
            assert (mount / "Applications").readlink() == Path("/Applications")
            mounted = mount / "MonoCode.app"
            run("codesign", "--verify", "--deep", "--strict", mounted)
            for relative in ("Contents/Info.plist", "Contents/MacOS/monocode-app",
                             "Contents/MacOS/monocode-host"):
                assert sha256(mounted / relative) == sha256(app / relative)
        finally:
            run("hdiutil", "detach", mount)

    recorded = {}
    for line in (output / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        expected, name = line.split("  ", 1)
        assert name == Path(name).name and name not in recorded
        assert sha256(output / name) == expected
        recorded[name] = expected
    expected_names = {path.name for path in output.iterdir()
                      if path.is_file() and path.name not in ("SHA256SUMS", "latest.json")}
    assert set(recorded) == expected_names
    return {"output": str(output), "version": version, "target": target,
            "signing": "ad hoc", "notarization": "not performed",
            "profile": "qualified development binaries, no optimized release claim",
            "app_cli": "list-views passed", "uuid": uuids,
            "input_sha256": {name: sha256(binaries / name) for name in
                             ("monocode-app", "monocode-host", "monocode-package")},
            "artifact_sha256": {**recorded, "SHA256SUMS": sha256(output / "SHA256SUMS")}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--binaries", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--summary", type=Path)
    options = parser.parse_args()
    result = verify(options.output.resolve(), options.binaries.resolve(), options.version)
    if options.summary:
        options.summary.write_bytes((json.dumps(result, indent=2) + "\n").encode("utf-8"))
    print("test native_macos_bundle_signature_plist_architecture_versions_archives_checksums_and_dmg ... ok")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
