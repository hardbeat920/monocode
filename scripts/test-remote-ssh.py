#!/usr/bin/env python3
"""Run native SSH transport tests against disposable loopback services.

Requires OpenSSH sshd, built native host and desktop executables, and either
Cargo or a compiled monocode-remote test executable.
Does not modify HOME, SSH configuration, authorized_keys or OS services.
"""
import argparse
import json
import os
import pathlib
import pwd
import shutil
import socket
import subprocess
import tempfile
import threading
import time

ROOT = pathlib.Path(__file__).resolve().parent.parent
TEST = "remote_ssh::tests::loopback_transport_preserves_host_and_reconnects"


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", type=pathlib.Path, default=ROOT / "target/debug/monocode-host")
    parser.add_argument("--desktop", type=pathlib.Path, default=ROOT / "target/debug/monocode-app")
    parser.add_argument("--test-binary", type=pathlib.Path)
    parser.add_argument("--sshd", default=shutil.which("sshd") or "/usr/sbin/sshd")
    args = parser.parse_args()
    for path in (args.host, args.desktop, args.test_binary):
        if path is not None and not path.is_file():
            raise RuntimeError(f"Required executable is missing: {path}")
    host_command = [str(args.host.resolve())]
    if args.host.suffix == ".mjs":
        host_command.insert(0, shutil.which("node") or "node")
    sshd = args.sshd
    with tempfile.TemporaryDirectory(prefix="monocode-ssh-integration-") as directory:
        folder = pathlib.Path(directory)
        for key in ("host", "client"):
            subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(folder / key)], check=True)
        ssh_port, host_port = free_port(), free_port()
        config = folder / "sshd_config"
        config.write_text(f"Port {ssh_port}\nListenAddress 127.0.0.1\nHostKey {folder}/host\nPidFile {folder}/pid\nAuthorizedKeysFile {folder}/client.pub\nStrictModes no\nUsePAM no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nLogLevel ERROR\n")
        (folder / "known_hosts").write_text(f"[127.0.0.1]:{ssh_port} " + (folder / "host.pub").read_text())
        daemon = subprocess.Popen([sshd, "-D", "-e", "-f", str(config)], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        def host(*args):
            return subprocess.check_output([*host_command, *args, "--data-dir", str(folder / "data"), "--port", str(host_port)], text=True, timeout=15)
        try:
            time.sleep(0.3)
            if daemon.poll() is not None:
                raise RuntimeError(daemon.stderr.read().decode())
            # Exercise the actual desktop executable's non-GUI askpass mode.
            # Only trust the fingerprint of the disposable key created above.
            desktop = args.desktop.resolve()
            fingerprint = subprocess.check_output(["ssh-keygen", "-lf", str(folder / "host.pub")], text=True).split()[1]
            with socket.socket() as prompts:
                prompts.bind(("127.0.0.1", 0))
                prompts.listen()
                prompts.settimeout(20)
                secret = os.urandom(32).hex()
                prompt_results = []
                def answer_prompt():
                    try:
                        stream, _ = prompts.accept()
                        with stream:
                            stream.settimeout(10)
                            request = json.loads(stream.makefile("rb").readline(16384))
                            valid = request["secret"] == secret and request["confirm"] and fingerprint in request["prompt"]
                            prompt_results.append(True if valid else {"confirm": request["confirm"], "prompt": request["prompt"], "expectedFingerprint": fingerprint})
                            stream.sendall((json.dumps("yes" if valid else None) + "\n").encode())
                    except Exception as error:
                        prompt_results.append(str(error))
                worker = threading.Thread(target=answer_prompt, daemon=True)
                worker.start()
                result = subprocess.run(["ssh", "-F", "/dev/null", "-i", str(folder / "client"), "-p", str(ssh_port), "-o", "IdentitiesOnly=yes", "-o", "StrictHostKeyChecking=ask", "-o", f"UserKnownHostsFile={folder}/prompt_known_hosts", f"{pwd.getpwuid(os.getuid()).pw_name}@127.0.0.1", "printf", "askpass-ok"],
                    env={**os.environ, "SSH_ASKPASS": str(desktop), "SSH_ASKPASS_REQUIRE": "force", "DISPLAY": "monocode:0", "MONOCODE_SSH_ASKPASS_ADDRESS": f"127.0.0.1:{prompts.getsockname()[1]}", "MONOCODE_SSH_ASKPASS_SECRET": secret}, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=25)
                worker.join(timeout=1)
                assert prompt_results == [True], prompt_results
                assert result.returncode == 0 and result.stdout == "askpass-ok", result.stderr
                print("Native askpass verified the disposable SSH host fingerprint.")
            host("start")
            device = json.loads(host("pair", "--name", "Loopback test", "--json"))
            env = {**os.environ, "MONOCODE_TEST_SSH_TARGET": f"{pwd.getpwuid(os.getuid()).pw_name}@127.0.0.1", "MONOCODE_TEST_SSH_PORT": str(ssh_port), "MONOCODE_TEST_HOST_PORT": str(host_port), "MONOCODE_TEST_SSH_KEY": str(folder / "client"), "MONOCODE_TEST_KNOWN_HOSTS": str(folder / "known_hosts"), "MONOCODE_TEST_TOKEN": device["token"], "MONOCODE_TEST_ENVIRONMENT": device["environmentId"]}
            if args.test_binary is None:
                test_command = ["cargo", "test", "-p", "monocode-remote", "--lib", "--locked", "--"]
            else:
                test_command = [str(args.test_binary.resolve())]
            subprocess.run([*test_command, "--exact", TEST, "--ignored", "--nocapture", "--test-threads=1"], cwd=ROOT, env=env, check=True, timeout=90)
            assert json.loads(host("connection-info"))["port"] == host_port
            print("Native host survived both SSH tunnel closures.")
        finally:
            try:
                if (folder / "data/running.json").exists():
                    host("stop")
                    for _ in range(50):
                        if not (folder / "data/running.json").exists():
                            break
                        time.sleep(0.1)
            finally:
                daemon.terminate()
                try:
                    daemon.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    daemon.kill()
                    daemon.wait()


if __name__ == "__main__":
    main()
