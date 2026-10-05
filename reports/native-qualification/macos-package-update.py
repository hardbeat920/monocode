#!/usr/bin/env python3
"""Qualify an actual package through the native updater in an owned temporary bundle."""

import argparse
import hashlib
import http.server
import json
import os
from pathlib import Path
import plistlib
import subprocess
import tarfile
import tempfile
import threading


def run(arguments, *, env=None, success=True):
    result = subprocess.run(
        [str(value) for value in arguments],
        env=env,
        capture_output=True,
        text=True,
        timeout=120,
    )
    if success and result.returncode:
        raise RuntimeError(result.stdout + result.stderr)
    return result


def qualify(task_root, package, *, signer=None, updater=None):
    signer = signer or task_root / "source/target/debug/monocode-updater-sign"
    updater = updater or task_root / "source/target/debug/examples/apply_update"
    payload = package.read_bytes()
    package_hash = hashlib.sha256(payload).hexdigest()
    with tarfile.open(package) as archive:
        names = archive.getnames()
        plist_name = next(name for name in names if name.endswith("/Contents/Info.plist"))
        plist = plistlib.loads(archive.extractfile(plist_name).read())
        executable_name = plist["CFBundleExecutable"]
        bundle_name = plist_name.split("/")[0]
        executable_entry = f"{bundle_name}/Contents/MacOS/{executable_name}"
        executable_hash = hashlib.sha256(archive.extractfile(executable_entry).read()).hexdigest()

    with tempfile.TemporaryDirectory(prefix="monocode-package-update-", dir=task_root) as scratch:
        scratch = Path(scratch)
        run([signer, "generate", scratch / "keys", "--unencrypted"])
        private_key = scratch / "keys/updater.key"
        private_key.chmod(0o600)
        signed_package = scratch / package.name
        signed_package.write_bytes(payload)
        signing_env = os.environ.copy()
        signing_env["TAURI_SIGNING_PRIVATE_KEY"] = str(private_key)
        signing_env.pop("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", None)
        run([signer, "sign", signed_package], env=signing_env)
        public_key = (scratch / "keys/updater.key.pub").read_text().strip()
        run([signer, "verify", signed_package, "--pubkey", public_key])
        signature = Path(str(signed_package) + ".sig").read_text().strip()

        installed = scratch / "Applications/Owned qualification.app"
        old_executable = installed / "Contents/MacOS/old-test-executable"
        old_executable.parent.mkdir(parents=True)
        old_executable.write_bytes(b"owned old executable")
        old_executable.chmod(0o755)
        old_plist = installed / "Contents/Info.plist"
        old_plist.write_bytes(b"owned old plist")
        marker = installed / "original-marker"
        marker.write_bytes(b"preserve before trusted install")

        state = {"tampered": True, "hits": []}
        changed = bytearray(payload)
        changed[len(changed) // 2] ^= 1
        tampered_payload = bytes(changed)

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                state["hits"].append(self.path)
                if self.path == "/latest.json":
                    body = json.dumps(
                        {
                            "version": plist["CFBundleShortVersionString"],
                            "notes": "Owned package qualification",
                            "platforms": {
                                "darwin-aarch64": {
                                    "url": f"http://127.0.0.1:{self.server.server_port}/package",
                                    "signature": signature,
                                }
                            },
                        }
                    ).encode()
                elif self.path == "/package":
                    body = tampered_payload if state["tampered"] else payload
                else:
                    self.send_error(404)
                    return
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *_):
                pass

        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        arguments = [
            updater,
            "--endpoint",
            f"http://127.0.0.1:{server.server_port}/latest.json",
            "--pubkey",
            public_key,
            "--current",
            "0.5.0",
            "--executable",
            old_executable,
        ]
        try:
            rejected = run(arguments, success=False)
            assert rejected.returncode != 0, "tampered package was accepted"
            assert "signature" in rejected.stderr.lower(), rejected.stderr
            assert marker.read_bytes() == b"preserve before trusted install"
            assert old_executable.read_bytes() == b"owned old executable"
            assert old_plist.read_bytes() == b"owned old plist"
            state["tampered"] = False
            accepted = run(arguments)
            assert "signature verified" in accepted.stdout
            assert f"installed {plist['CFBundleShortVersionString']}" in accepted.stdout
        finally:
            server.shutdown()
            server.server_close()
            worker.join(timeout=2)
            assert not worker.is_alive()

        assert not marker.exists(), "old bundle was not replaced"
        actual_executable = installed / "Contents/MacOS" / executable_name
        actual_hash = hashlib.sha256(actual_executable.read_bytes()).hexdigest()
        assert actual_hash == executable_hash, "installed native executable differs from the archive"
        actual_plist = plistlib.loads((installed / "Contents/Info.plist").read_bytes())
        assert actual_plist == plist, "installed bundle metadata differs from the archive"
        assert "usage: monocode-app" in run([actual_executable, "--help"]).stdout
        views = run([actual_executable, "--list-views"]).stdout
        assert "shell" in views
        run(["/usr/bin/codesign", "--verify", "--deep", "--strict", installed])
        hosts = list((installed / "Contents").rglob("monocode-host"))
        assert len(hosts) == 1, "expected one bundled native host"
        assert run([hosts[0], "--version"]).stdout.strip() == plist["CFBundleShortVersionString"]
        assert state["hits"] == ["/latest.json", "/package", "/latest.json", "/package"]
        assert hashlib.sha256(package.read_bytes()).hexdigest() == package_hash
        print(
            json.dumps(
                {
                    "package": str(package),
                    "package_sha256": package_hash,
                    "installed_app_sha256": actual_hash,
                    "version": plist["CFBundleShortVersionString"],
                    "tampered_package_rejected": True,
                    "original_bundle_preserved_on_rejection": True,
                    "signed_package_installed": True,
                    "installed_cli_help_and_views": True,
                    "bundled_host_version": True,
                    "installed_bundle_signature_verified": True,
                    "source_package_unchanged": True,
                    "http_requests": state["hits"],
                    "keys": "disposable fixture keys, removed with owned scratch directory",
                    "scope": "owned temporary bundle only, no GUI launch or relaunch",
                },
                indent=2,
            )
        )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("task_root", type=Path)
    parser.add_argument("package", type=Path)
    parser.add_argument("--signer", type=Path)
    parser.add_argument("--updater", type=Path)
    options = parser.parse_args()
    qualify(options.task_root.resolve(), options.package.resolve(),
            signer=options.signer.resolve() if options.signer else None,
            updater=options.updater.resolve() if options.updater else None)
