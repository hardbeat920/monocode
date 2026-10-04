import hashlib
import json
import pathlib
import platform
import subprocess
import sys

source = pathlib.Path(sys.argv[1])
output = pathlib.Path(sys.argv[2])
artifacts = []
for line in pathlib.Path(sys.argv[3]).read_text().splitlines():
    try:
        message = json.loads(line)
    except json.JSONDecodeError:
        continue
    if (
        message.get("reason") == "compiler-artifact"
        and message.get("target", {}).get("name") == "monocode_engine"
        and message.get("profile", {}).get("test")
        and message.get("executable")
    ):
        artifacts.append(message["executable"])
if len(artifacts) != 1:
    raise SystemExit(f"Expected one workspace engine test executable, found {len(artifacts)}")
paths = sorted(path for path in (source / "crates/engine").rglob("*") if path.is_file())
paths += [source / "Cargo.toml", source / "Cargo.lock"]
snapshot = {
    "source_root": str(source),
    "compiler": subprocess.check_output(["rustc", "--version"], text=True).strip(),
    "architecture": platform.machine(),
    "executable": artifacts[0],
    "executable_sha256": hashlib.sha256(pathlib.Path(artifacts[0]).read_bytes()).hexdigest(),
    "source_sha256": {
        str(path.relative_to(source)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in paths
    },
}
output.write_text(json.dumps(snapshot, indent=2, sort_keys=True) + "\n")
print(f"Captured {len(paths)} engine/workspace files and the compiled workspace test executable")
