#!/usr/bin/env python3
"""Package a completed frozen Mac qualification using owned artifact inputs.

Run only after the qualification coordinator releases the Mac Cargo slot.
The updater fixture uses disposable keys and replaces only its temporary app.
The renderer writes its own GPUI scene, without desktop capture or input.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import time


def sha256(path):
    checksum = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            checksum.update(chunk)
    return checksum.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--qualification-summary", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output-root", type=Path, required=True)
    options = parser.parse_args()
    if sys.platform != "darwin" or platform.machine() != "arm64":
        raise RuntimeError("This runner requires an arm64 Mac")
    source = options.source.resolve()
    helpers = Path(__file__).resolve().parent
    qualified = json.loads(options.qualification_summary.read_text(encoding="utf-8"))
    if not qualified.get("complete") or qualified.get("platform") != "darwin":
        raise RuntimeError("A completed Mac qualification summary is required")
    if Path(qualified["source"]).resolve() != source:
        raise RuntimeError("Qualification summary belongs to a different source copy")
    manifest = options.manifest.resolve()
    frozen = json.loads(manifest.read_text(encoding="utf-8"))
    options.output_root.mkdir(parents=True, exist_ok=True)
    artifact = Path(tempfile.mkdtemp(prefix="final-macos-package-", dir=options.output_root.resolve()))
    inputs = artifact / "inputs"
    inputs.mkdir()
    logs = artifact / "logs"
    logs.mkdir()
    packages = artifact / "packages"
    summary = {"complete": False, "artifact_root": str(artifact), "source": str(source),
               "source_manifest_sha256": frozen["sha256"],
               "signing": "ad hoc", "notarization": "not performed",
               "profile": "qualified development binaries, no optimized release claim",
               "commands": {}}

    def save_summary():
        (artifact / "summary.json").write_bytes((json.dumps(summary, indent=2) + "\n").encode("utf-8"))

    def run(name, arguments, *, env=None, timeout=900):
        started = time.monotonic()
        log = logs / f"{name}.log"
        print(f"{name}: starting", flush=True)
        with log.open("wb") as stream:
            result = subprocess.run([str(value) for value in arguments], cwd=source,
                                    env=env, stdout=stream, stderr=subprocess.STDOUT,
                                    timeout=timeout)
        summary["commands"][name] = {"arguments": [str(value) for value in arguments],
                                     "exit_code": result.returncode,
                                     "seconds": round(time.monotonic() - started, 3),
                                     "log": str(log.relative_to(artifact))}
        save_summary()
        print(f"{name}: exit {result.returncode}", flush=True)
        if result.returncode:
            raise RuntimeError(f"{name} failed, see {log}")
        return log

    verify_source = [sys.executable, helpers / "final-source-manifest.py", "verify",
                     "--root", source, "--manifest", manifest]
    run("source-before", verify_source)
    shutil.copy2(manifest, artifact / "source-manifest.json")
    shutil.copy2(options.qualification_summary, artifact / "qualification-summary.json")
    target = Path(os.environ.get("CARGO_TARGET_DIR", source / "target"))
    if not target.is_absolute():
        target = source / target
    expected = qualified["binary_sha256"]
    input_hashes = {}
    for name in ("monocode-app", "monocode-host", "monocode-package"):
        original = target / "debug" / name
        if sha256(original) != expected[name]:
            raise RuntimeError(f"Qualified binary changed before packaging: {name}")
        shutil.copy2(original, inputs / name)
        input_hashes[name] = sha256(inputs / name)
        if input_hashes[name] != expected[name]:
            raise RuntimeError(f"Copied input differs from qualified binary: {name}")
    summary["input_sha256"] = input_hashes
    save_summary()

    version_log = run("host-version", [inputs / "monocode-host", "--version"])
    version = version_log.read_text(encoding="utf-8").strip()
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?", version):
        raise RuntimeError(f"Unexpected host version: {version!r}")
    summary["version"] = version
    run("bundle", [inputs / "monocode-package", "bundle", "--target", "aarch64-apple-darwin",
                   "--binaries", inputs, "--output", packages, "--signing-identity", "-"])
    run("checksums", [inputs / "monocode-package", "checksums", "--directory", packages])
    run("verify-package", [sys.executable, helpers / "verify-native-macos-package.py",
                           "--output", packages, "--binaries", inputs, "--version", version,
                           "--summary", artifact / "package-verification.json"])
    summary["package_verification"] = json.loads((artifact / "package-verification.json").read_text(encoding="utf-8"))

    run("build-updater-fixtures", ["cargo", "build", "-p", "monocode-updater",
                                   "--features", "sign", "--bin", "monocode-updater-sign",
                                   "--example", "apply_update", "--locked", "--offline", "-j", "2"])
    for name, relative in (("monocode-updater-sign", "debug/monocode-updater-sign"),
                           ("apply_update", "debug/examples/apply_update")):
        shutil.copy2(target / relative, inputs / name)
        summary["input_sha256"][name] = sha256(inputs / name)
    run("private-updater", [sys.executable, helpers / "macos-package-update.py", artifact,
                            packages / f"MonoCode_{version}_aarch64-apple-darwin.app.tar.gz",
                            "--signer", inputs / "monocode-updater-sign",
                            "--updater", inputs / "apply_update"], timeout=300)

    profile = artifact / "render-profile"
    profile.mkdir()
    image = artifact / "packaged-widgets.png"
    render_env = os.environ.copy()
    render_env["MONOCODE_RUN_SCHEDULES"] = "0"
    run("packaged-renderer", [packages / "MonoCode.app/Contents/MacOS/monocode-app",
                              "--view", "widgets", "--data-dir", profile,
                              "--theme", "dark", "--size", "1280x800",
                              "--settle-ms", "2500", "--screenshot", image], env=render_env, timeout=90)
    header = image.read_bytes()[:24]
    if header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR":
        raise RuntimeError("Packaged renderer did not write a PNG")
    width, height = struct.unpack(">II", header[16:24])
    if width < 1280 or height < 800 or width * 800 != height * 1280:
        raise RuntimeError(f"Unexpected renderer dimensions: {width}x{height}")
    summary["renderer"] = {"image": image.name, "sha256": sha256(image),
                           "dimensions": [width, height], "view": "widgets",
                           "engine_booted": False, "settings_imported": False,
                           "scheduled_agents_enabled": False,
                           "expected_widgets": ["Buttons", "Icon buttons", "Text fields",
                                                "Switch and segmented control", "Menu, popover, tooltip"],
                           "visual_review": "pending coordinator image inspection",
                           "scope": "owned GPUI renderer only, no desktop capture or GUI input"}
    for name, expected_hash in summary["input_sha256"].items():
        if sha256(inputs / name) != expected_hash:
            raise RuntimeError(f"Packaging changed an owned input: {name}")
    run("source-after", verify_source)
    summary["complete"] = True
    save_summary()
    print(json.dumps(summary, indent=2), flush=True)


if __name__ == "__main__":
    main()
