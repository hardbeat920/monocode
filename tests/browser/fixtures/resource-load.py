#!/usr/bin/python3
"""Disposable native benchmark agent; children exit after 45 seconds.

Invoked only from the isolated release fixture's explicit Start load control.
Four roots cover the CPU cores, one does bounded temporary-file I/O, and one
holds 1 GiB of touched memory. No real provider, user files, or network is used.
"""
import hashlib
import math
import os
import subprocess
import sys
import tempfile
import time


def child(mode, deadline):
    if mode == "cpu":
        data = os.urandom(65536)
        while time.monotonic() < deadline:
            hashlib.sha256(data).digest()
    elif mode == "disk":
        data = os.urandom(4 * 1024 * 1024)
        with tempfile.TemporaryFile(prefix="monocode-load-") as output:
            while time.monotonic() < deadline:
                output.seek(0)
                for _ in range(16):
                    output.write(data)
                output.flush()
                os.fsync(output.fileno())
                time.sleep(0.02)
    elif mode == "memory":
        data = bytearray(os.urandom(1024 * 1024)) * 1024
        while time.monotonic() < deadline:
            for offset in range(0, len(data), 65536):
                data[offset] = (data[offset] + 1) % 256
            time.sleep(0.05)


if __name__ == "__main__":
    if sys.argv[1] == "--version":
        # The isolated fixture is copied to a disposable `claude` executable
        # and goes through the normal configured-CLI validation path.
        print("Claude benchmark fixture 0.0.0")
    elif sys.argv[1] == "--child":
        child(sys.argv[2], float(sys.argv[3]))
    else:
        index = int(sys.argv[1])
        mode = "cpu" if index < 4 else "disk" if index == 4 else "memory"
        count = max(1, math.ceil((os.cpu_count() or 4) / 4)) if mode == "cpu" else 1
        deadline = time.monotonic() + 45
        children = [subprocess.Popen([sys.executable, __file__, "--child", mode, str(deadline)]) for _ in range(count)]
        # Stopping a child leaves this agent root alive for the remainder of the
        # fixture, making conversation-preserving process control observable.
        while time.monotonic() < deadline:
            time.sleep(0.1)
        for process in children:
            process.wait()
