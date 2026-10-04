import datetime, hashlib, json, os, pathlib, signal, subprocess, sys, time
binary = pathlib.Path(sys.argv[1]).resolve()
provider = sys.argv[2]
test = sys.argv[3]
base = pathlib.Path(__file__).resolve().parent
log = base / (provider + '.log')
metadata = {'provider': provider, 'test': test, 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(), 'started_at': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'outer_timeout_seconds': 240}
command = [str(binary), '--ignored', '--exact', test, '--nocapture', '--test-threads=1']
started = time.monotonic()
with log.open('w') as output:
    output.write(json.dumps(metadata, sort_keys=True) + '\n')
    output.flush()
    child = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT, start_new_session=True, cwd=base)
    try:
        metadata['exit'] = child.wait(timeout=240)
    except subprocess.TimeoutExpired:
        metadata['outer_timeout'] = True
        tree = subprocess.check_output(['ps', '-axo', 'pid=,ppid='], text=True)
        parents = {int(line.split()[0]): int(line.split()[1]) for line in tree.splitlines() if len(line.split()) == 2}
        owned = {child.pid}
        while True:
            additions = {pid for pid, parent in parents.items() if parent in owned} - owned
            if not additions:
                break
            owned.update(additions)
        for pid in sorted(owned, reverse=True):
            try:
                os.kill(pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
        try:
            metadata['exit'] = child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            metadata['exit'] = child.wait(timeout=5)
metadata['elapsed_seconds'] = round(time.monotonic() - started, 3)
metadata['log'] = str(log)
(base / (provider + '.json')).write_text(json.dumps(metadata, indent=2) + '\n')
print(json.dumps(metadata, sort_keys=True))
