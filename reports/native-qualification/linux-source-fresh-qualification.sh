#!/bin/bash
set -euo pipefail
qualification_dir=/home/niost/monocode-gpui-qualification
cd "$qualification_dir"
node_bin=$(find "$qualification_dir/tools/node24" -path '*/bin/node' -type f | sort | tail -n1)
[[ -n "$node_bin" ]]
export PATH="$(dirname "$node_bin"):/home/niost/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$qualification_dir/target"
export CARGO_BUILD_JOBS=2
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
python3 - <<'PY'
from pathlib import Path
import os,hashlib
files=[]
for root in ['apps','crates']:
    for p in Path(root).rglob('*'):
        if p.is_file() and (p.suffix == '.rs' or p.name == 'Cargo.toml'):
            os.utime(p,None)
for root in ['apps','crates','packaging','.cargo']:
    files.extend(p for p in Path(root).rglob('*') if p.is_file())
files.extend([Path('Cargo.toml'),Path('Cargo.lock')])
Path('linux-source-fresh-hashes.txt').write_text(''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p}\n' for p in sorted(files)))
PY
cargo test --workspace --lib --bins --all-features --locked -j2 --no-run > linux-source-fresh-compile.log 2>&1
remote_test=$(sed -n 's/.*(\(target\/debug\/deps\/monocode_remote-[0-9a-f]*\)).*/\1/p' linux-source-fresh-compile.log | head -n1)
app_test=$(sed -n 's/.*(\(target\/debug\/deps\/monocode_app-[0-9a-f]*\)).*/\1/p' linux-source-fresh-compile.log | tail -n1)
[[ -n "$remote_test" && -n "$app_test" ]]
"$remote_test" host::http::tests::early_rejection --nocapture > linux-source-fresh-http.log 2>&1
"$app_test" --exact panes::workspace::remote_tests::remote_workspace_shortcut_preserves_selection_and_creates_the_host_worktree_before_sending --nocapture > linux-source-fresh-shortcut.log 2>&1
grep -q '2 passed; 0 failed' linux-source-fresh-http.log
grep -q '1 passed; 0 failed' linux-source-fresh-shortcut.log
cargo test --workspace --lib --bins --all-features --locked -j2 > linux-source-fresh-workspace.log 2>&1
cargo clippy --workspace --all-targets --all-features --locked -j2 -- -D warnings > linux-source-fresh-clippy.log 2>&1
cargo test -p monocode-core --tests --locked -j2 > linux-source-fresh-core.log 2>&1
cargo test -p monocode-host --test remote_recovery --locked -j2 > linux-source-fresh-recovery.log 2>&1
cargo build -p monocode-app -p monocode-host -p monocode-package --bins --features monocode-app/screenshot --locked -j2 > linux-source-fresh-build.log 2>&1
python3 - <<'PY'
from pathlib import Path
import hashlib,json,re
for line in Path('linux-source-fresh-hashes.txt').read_text().splitlines():
    expected,path=line.split('  ',1)
    assert hashlib.sha256(Path(path).read_bytes()).hexdigest() == expected,path
summary={}
for name in ['workspace','core','recovery','http','shortcut']:
    logfile=f'linux-source-fresh-{name}.log'
    counts=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',Path(logfile).read_text())
    summary[logfile]={'targets':len(counts),'passed':sum(int(c[0]) for c in counts),'failed':sum(int(c[1]) for c in counts),'ignored':sum(int(c[2]) for c in counts)}
Path('linux-source-fresh-summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps(summary,indent=2))
PY
printf 'test source_fresh_linux_workspace_named_http_shortcut_and_native_build ... ok\n'
