import json
import pathlib
import subprocess
import sys

artifacts = []
for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
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
print(f"Running workspace engine test executable {artifacts[0]}", flush=True)
raise SystemExit(subprocess.run([artifacts[0]], check=False).returncode)
